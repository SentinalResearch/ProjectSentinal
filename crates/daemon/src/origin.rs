//! Exact browser-Origin allow-list for every HTTP route and WebSocket upgrade.
//!
//! A request carrying `Origin` must match one configured origin exactly
//! (scheme and host compared case-insensitively, default ports normalized).
//! Requests without `Origin` — the CLI, worker links, curl — carry no ambient
//! browser credential and are allowed. An empty list therefore admits no
//! browser at all, which is the secure default.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use protocol::link::{LinkFailure, LinkFailureCode};

/// Why an `Origin` header was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OriginRejection {
    /// `Origin: null` (sandboxed frames, file URLs, some redirects).
    #[error("origin 'null' is never allowed")]
    Null,
    /// The header was not a parseable `scheme://host[:port]`.
    #[error("origin is malformed")]
    Malformed,
    /// The origin is well-formed but not configured.
    #[error("origin is not allowed")]
    NotAllowed,
}

/// Why a configured origin was refused at load time.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid allowed origin '{value}': {reason}")]
pub struct OriginConfigError {
    /// Configured value.
    pub value: String,
    /// Which rule failed.
    pub reason: &'static str,
}

/// Configured exact origins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginPolicy {
    allowed: Vec<String>,
}

impl OriginPolicy {
    /// Validate configured origins; plain HTTP is accepted only in development.
    pub fn new(values: &[String], development: bool) -> Result<Self, OriginConfigError> {
        let mut allowed = Vec::with_capacity(values.len());
        for value in values {
            let invalid = |reason| OriginConfigError {
                value: value.clone(),
                reason,
            };
            if value.contains('*') {
                return Err(invalid("wildcards are not allowed"));
            }
            let canonical = canonical(value).map_err(|_| {
                invalid("expected scheme://host[:port] with no path, query, or credentials")
            })?;
            if value.ends_with('/') {
                return Err(invalid("must not end with '/'"));
            }
            if canonical.starts_with("http://") && !development {
                return Err(invalid(
                    "plain HTTP origins are allowed only with development = true",
                ));
            }
            allowed.push(canonical);
        }
        Ok(OriginPolicy { allowed })
    }

    /// Check one request's `Origin` header value.
    pub fn check(&self, origin: Option<&str>) -> Result<(), OriginRejection> {
        let Some(origin) = origin else {
            return Ok(());
        };
        if origin.trim() == "null" {
            return Err(OriginRejection::Null);
        }
        let canonical = canonical(origin)?;
        if self.allowed.contains(&canonical) {
            Ok(())
        } else {
            Err(OriginRejection::NotAllowed)
        }
    }
}

/// Normalize `scheme://host[:port]`; anything else is malformed.
fn canonical(value: &str) -> Result<String, OriginRejection> {
    let lower = value.to_ascii_lowercase();
    let (scheme, rest) = lower.split_once("://").ok_or(OriginRejection::Malformed)?;
    if !matches!(scheme, "http" | "https")
        || rest.is_empty()
        || rest.contains(['/', '?', '#', '@', '\\', ' '])
    {
        return Err(OriginRejection::Malformed);
    }
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (
            host,
            Some(
                port.parse::<u16>()
                    .map_err(|_| OriginRejection::Malformed)?,
            ),
        ),
        None => (rest, None),
    };
    // Hostnames and IPv4 only; browsers here never use IPv6 literal origins.
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(OriginRejection::Malformed);
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    Ok(match port {
        Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// Axum middleware refusing disallowed browser origins with `403 ORIGIN_REJECTED`.
pub async fn enforce(State(policy): State<OriginPolicy>, request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .map(|value| value.to_str().unwrap_or("\u{0}"));
    match policy.check(origin) {
        Ok(()) => next.run(request).await,
        Err(rejection) => {
            tracing::warn!(reason = %rejection, path = %request.uri().path(), "rejected browser origin");
            (
                StatusCode::FORBIDDEN,
                Json(LinkFailure {
                    code: LinkFailureCode::OriginRejected,
                    message: rejection.to_string(),
                }),
            )
                .into_response()
        }
    }
}
