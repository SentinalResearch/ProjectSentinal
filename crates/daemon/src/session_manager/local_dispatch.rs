//! Answering validated private local-socket requests.

use super::{SessionManager, SessionManagerError};
use protocol::{LocalErrorCode, LocalRequest, LocalResponse};

impl SessionManager {
    /// Dispatch one verified local-socket request.
    pub fn handle_local(&self, request: LocalRequest) -> LocalResponse {
        // The socket layer validates too; this keeps direct callers safe.
        if let Err(error) = request.validate() {
            return LocalResponse::Error {
                code: LocalErrorCode::InvalidRequest,
                message: error.to_string(),
            };
        }
        let result = match request {
            LocalRequest::RegisterShell {
                pid,
                shell,
                cwd,
                title,
            } => self
                .register_shell(pid, &shell, &cwd, title)
                .map(|session| LocalResponse::Session { session }),
            LocalRequest::ShellCommandStarted {
                session_id,
                pid,
                sequence,
                text,
                cwd,
                started_at,
            } => self
                .report_shell_command_started(session_id, pid, sequence, &text, &cwd, started_at)
                .and_then(|started| match started {
                    Some(command) => Ok(LocalResponse::Command { command }),
                    None => self
                        .session(session_id)
                        .map(|session| LocalResponse::Session { session }),
                }),
            LocalRequest::ShellCommandFinished {
                session_id,
                pid,
                sequence,
                exit_code,
                finished_at,
            } => self
                .shell_command_finished(session_id, pid, sequence, exit_code, finished_at)
                .and_then(|finished| match finished {
                    Some(command) => Ok(LocalResponse::Command { command }),
                    None => self
                        .session(session_id)
                        .map(|session| LocalResponse::Session { session }),
                }),
            LocalRequest::AdoptProcess {
                pid,
                title,
                log_path,
            } => self
                .adopt_process(pid, title, log_path)
                .map(|session| LocalResponse::Session { session }),
            // Machine identity and control links belong to the link manager.
            LocalRequest::GetIdentity
            | LocalRequest::ConnectControl { .. }
            | LocalRequest::ConnectionStatus
            | LocalRequest::DisconnectControl
            | LocalRequest::RequestApproval { .. }
            | LocalRequest::ClaimApproval
            | LocalRequest::CancelApproval
            | LocalRequest::HandoffWorkers { .. }
            | LocalRequest::ResetIdentity => {
                return LocalResponse::Error {
                    code: LocalErrorCode::Unavailable,
                    message: "Machine links are handled by the link manager".into(),
                }
            }
        };
        result.unwrap_or_else(|error| LocalResponse::Error {
            code: error_code(&error),
            message: error.to_string(),
        })
    }
}

fn error_code(error: &SessionManagerError) -> LocalErrorCode {
    match error {
        SessionManagerError::UnknownSession(_) | SessionManagerError::ProcessNotFound(_) => {
            LocalErrorCode::NotFound
        }
        SessionManagerError::ProcessNotOwned { .. }
        | SessionManagerError::NotShellHook(_)
        | SessionManagerError::ShellPidMismatch { .. } => LocalErrorCode::Forbidden,
        SessionManagerError::SequenceConflict { .. }
        | SessionManagerError::AlreadyAdopted { .. } => LocalErrorCode::Conflict,
        SessionManagerError::AdoptSelf(_) | SessionManagerError::InvalidLog { .. } => {
            LocalErrorCode::InvalidRequest
        }
        SessionManagerError::ProcessProbeUnavailable => LocalErrorCode::Unavailable,
        SessionManagerError::Protocol(_) => LocalErrorCode::InvalidRequest,
        _ => LocalErrorCode::Internal,
    }
}
