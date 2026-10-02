//! Same-user, bounded Unix-socket ingress for external Session observations.

mod audit;
pub mod path;

pub use audit::SecurityAudit;
pub use path::default_socket_path;

use protocol::{Envelope, LocalErrorCode, LocalRequest, LocalResponse, MAX_LOCAL_FRAME_BYTES};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::watch,
};

/// Maximum human-readable text included in a local error response.
pub const MAX_ERROR_MESSAGE_BYTES: usize = 1_024;
const MAX_SOCKET_PATH_BYTES: usize = 100;

/// Errors while preparing or serving the private local socket.
#[derive(Debug, thiserror::Error)]
pub enum LocalSocketError {
    /// A socket path had no usable parent.
    #[error("local socket path '{}' has no parent directory", .0.display())]
    MissingParent(PathBuf),
    /// A path failed a type, ownership, or symlink safety check.
    #[error("refusing unsafe local socket path '{}': {reason}", path.display())]
    UnsafePath {
        /// Path that failed validation.
        path: PathBuf,
        /// Validation failure.
        reason: String,
    },
    /// A configured path cannot fit a portable Unix socket address.
    #[error("local socket path '{}' is {} bytes; maximum is {MAX_SOCKET_PATH_BYTES}", path.display(), path.as_os_str().as_encoded_bytes().len())]
    PathTooLong {
        /// Path that exceeded the limit.
        path: PathBuf,
    },
    /// A filesystem or socket operation failed.
    #[error("failed to {operation} local socket '{}': {source}", path.display())]
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Operating-system error.
        source: std::io::Error,
    },
    /// A security audit record could not be encoded.
    #[error("failed to encode local security audit record: {0}")]
    AuditEncode(#[from] serde_json::Error),
    /// A shared security-audit lock was poisoned.
    #[error("failed to lock local security audit log")]
    AuditLock,
}

/// Bound daemon-owned Unix listener with verified cleanup identity.
#[derive(Debug)]
pub struct UnixSocketServer {
    listener: UnixListener,
    path: PathBuf,
    expected_uid: u32,
    device: u64,
    inode: u64,
    audit: SecurityAudit,
}

impl UnixSocketServer {
    /// Bind for the daemon's own OS user; see [`UnixSocketServer::bind_for_uid`].
    pub fn bind(path: &Path, audit: SecurityAudit) -> Result<Self, LocalSocketError> {
        Self::bind_for_uid(path, audit, current_uid(path)?)
    }

    /// Prepare the owner-only parent, safely replace a stale socket, and bind
    /// a listener that only serves peers running as `expected_uid`.
    pub fn bind_for_uid(
        path: &Path,
        audit: SecurityAudit,
        expected_uid: u32,
    ) -> Result<Self, LocalSocketError> {
        path::validate_socket_length(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| LocalSocketError::MissingParent(path.to_path_buf()))?;
        path::prepare_private_parent(parent, expected_uid)?;
        path::inspect_existing_socket(path, expected_uid)?;
        let listener = UnixListener::bind(path).map_err(|source| LocalSocketError::Io {
            operation: "bind",
            path: path.to_path_buf(),
            source,
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            LocalSocketError::Io {
                operation: "set mode 0600 on",
                path: path.to_path_buf(),
                source,
            }
        })?;
        let metadata = fs::symlink_metadata(path).map_err(|source| LocalSocketError::Io {
            operation: "inspect newly bound",
            path: path.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_socket() || metadata.uid() != expected_uid {
            return Err(LocalSocketError::UnsafePath {
                path: path.to_path_buf(),
                reason: "new socket type or owner changed during bind".into(),
            });
        }
        Ok(UnixSocketServer {
            listener,
            path: path.to_path_buf(),
            expected_uid,
            device: metadata.dev(),
            inode: metadata.ino(),
            audit,
        })
    }

    /// Accept same-user clients until shutdown is requested, answering each
    /// valid request with `handler`.
    pub async fn serve(
        self,
        mut shutdown: watch::Receiver<bool>,
        handler: LocalHandler,
    ) -> Result<(), LocalSocketError> {
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                accepted = self.listener.accept() => {
                    let stream = match accepted {
                        Ok((stream, _)) => stream,
                        Err(error) => {
                            // One failed accept (for example EMFILE) must not stop the listener.
                            tracing::warn!(path = %self.path.display(), %error, "local accept failed");
                            continue;
                        }
                    };
                    let peer_uid = peer_uid(&stream, &self.path);
                    let audit = self.audit.clone();
                    let path = self.path.clone();
                    let expected_uid = self.expected_uid;
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(
                            stream,
                            expected_uid,
                            peer_uid,
                            audit,
                            &path,
                            &handler,
                        ).await {
                            tracing::warn!(path = %path.display(), %error, "local ingress connection failed");
                        }
                    });
                }
            }
        }
    }
}

impl Drop for UnixSocketServer {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.uid() == self.expected_uid
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            if let Err(error) = fs::remove_file(&self.path) {
                tracing::warn!(path = %self.path.display(), %error, "failed to remove owned local socket");
            }
        } else {
            tracing::warn!(path = %self.path.display(), "refusing to remove replaced local socket path");
        }
    }
}

/// Effective UID of this process, read from a connected socketpair's OS peer
/// credentials so no `unsafe` libc call or new dependency is needed.
pub fn current_uid(path: &Path) -> Result<u32, LocalSocketError> {
    let (stream, _peer) = UnixStream::pair().map_err(|source| LocalSocketError::Io {
        operation: "create identity socketpair for",
        path: path.to_path_buf(),
        source,
    })?;
    stream
        .peer_cred()
        .map(|credentials| credentials.uid())
        .map_err(|source| LocalSocketError::Io {
            operation: "read own peer credentials for",
            path: path.to_path_buf(),
            source,
        })
}

/// OS-reported UID of a connected peer, or `None` when the OS cannot say.
pub fn peer_uid(stream: &UnixStream, path: &Path) -> Option<u32> {
    match stream.peer_cred() {
        Ok(credentials) => Some(credentials.uid()),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "local peer credentials unavailable");
            None
        }
    }
}

/// Future answering one local request.
pub type LocalReply = std::pin::Pin<Box<dyn std::future::Future<Output = LocalResponse> + Send>>;

/// Answers one validated local request; never attaches, resizes, or sends input.
pub type LocalHandler = std::sync::Arc<dyn Fn(LocalRequest) -> LocalReply + Send + Sync>;

/// Wrap a synchronous answer as a local handler.
pub fn sync_handler(
    answer: impl Fn(LocalRequest) -> LocalResponse + Send + Sync + 'static,
) -> LocalHandler {
    std::sync::Arc::new(move |request| {
        let response = answer(request);
        Box::pin(async move { response })
    })
}

/// A handler for listeners with no request support (every request is `UNAVAILABLE`).
pub fn unavailable_handler() -> LocalHandler {
    sync_handler(|_| {
        local_error(
            LocalErrorCode::Unavailable,
            "this local operation is not available",
        )
    })
}

/// Process one framed request after applying the injected OS peer identity.
pub async fn handle_connection(
    mut stream: UnixStream,
    expected_uid: u32,
    peer_uid: Option<u32>,
    audit: SecurityAudit,
    socket_path: &Path,
    handler: &LocalHandler,
) -> Result<(), LocalSocketError> {
    if peer_uid != Some(expected_uid) {
        let reason = match peer_uid {
            Some(actual) => format!("peer uid {actual} does not match daemon uid {expected_uid}"),
            None => "OS peer uid is unavailable".into(),
        };
        audit.append("WRONG_PEER_UID", peer_uid, socket_path, &reason)?;
        return write_response(
            &mut stream,
            local_error(LocalErrorCode::Forbidden, reason),
            socket_path,
        )
        .await;
    }

    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .await
        .map_err(|source| LocalSocketError::Io {
            operation: "read frame length from",
            path: socket_path.to_path_buf(),
            source,
        })?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_LOCAL_FRAME_BYTES {
        return write_response(
            &mut stream,
            local_error(
                LocalErrorCode::InvalidRequest,
                format!("local frame length {length} is outside 1..={MAX_LOCAL_FRAME_BYTES} bytes"),
            ),
            socket_path,
        )
        .await;
    }

    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|source| LocalSocketError::Io {
            operation: "read framed request from",
            path: socket_path.to_path_buf(),
            source,
        })?;
    let response = match std::str::from_utf8(&body) {
        Ok(json) => match Envelope::<LocalRequest>::decode(json) {
            Ok(envelope) => match envelope.payload.validate() {
                Ok(()) => match handler(envelope.payload).await {
                    LocalResponse::Error { code, message } => local_error(code, message),
                    response => response,
                },
                Err(error) => local_error(LocalErrorCode::InvalidRequest, error.to_string()),
            },
            Err(error) => local_error(LocalErrorCode::InvalidRequest, error.to_string()),
        },
        Err(error) => local_error(
            LocalErrorCode::InvalidRequest,
            format!("local request is not UTF-8: {error}"),
        ),
    };
    write_response(&mut stream, response, socket_path).await
}

async fn write_response(
    stream: &mut UnixStream,
    response: LocalResponse,
    path: &Path,
) -> Result<(), LocalSocketError> {
    let encoded =
        Envelope::new(response)
            .encode()
            .map_err(|error| LocalSocketError::UnsafePath {
                path: path.to_path_buf(),
                reason: format!("failed to encode bounded local response: {error}"),
            })?;
    let length = u32::try_from(encoded.len()).map_err(|_| LocalSocketError::UnsafePath {
        path: path.to_path_buf(),
        reason: "encoded local response exceeded u32 length".into(),
    })?;
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|source| LocalSocketError::Io {
            operation: "write response length to",
            path: path.to_path_buf(),
            source,
        })?;
    stream
        .write_all(encoded.as_bytes())
        .await
        .map_err(|source| LocalSocketError::Io {
            operation: "write framed response to",
            path: path.to_path_buf(),
            source,
        })
}

fn local_error(code: LocalErrorCode, message: impl Into<String>) -> LocalResponse {
    let mut message = message.into();
    if message.len() > MAX_ERROR_MESSAGE_BYTES {
        let mut boundary = MAX_ERROR_MESSAGE_BYTES;
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
    }
    LocalResponse::Error { code, message }
}
