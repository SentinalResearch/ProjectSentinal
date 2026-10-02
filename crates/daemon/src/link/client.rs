//! HTTPS client for worker → control link requests.

use protocol::link::{LinkFailure, LinkFailureCode};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a control request failed.
#[derive(Debug)]
pub(super) enum CallError {
    /// No HTTP answer (DNS, TCP, TLS, timeout).
    Unreachable(String),
    /// The control answered with a structured failure.
    Refused { failure: LinkFailure },
    /// The control answered something that is not a Sentinal control.
    Unexpected(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Unreachable(detail) => write!(formatter, "control unreachable: {detail}"),
            CallError::Refused { failure, .. } => {
                write!(
                    formatter,
                    "control refused ({:?}): {}",
                    failure.code, failure.message
                )
            }
            CallError::Unexpected(detail) => {
                write!(formatter, "unexpected control answer: {detail}")
            }
        }
    }
}

impl CallError {
    /// The failure code, when the control sent one.
    pub(super) fn code(&self) -> Option<LinkFailureCode> {
        match self {
            CallError::Refused { failure, .. } => Some(failure.code),
            _ => None,
        }
    }
}

/// Minimal JSON-over-HTTPS client: no redirects, short timeouts.
#[derive(Clone)]
pub(super) struct ControlClient {
    http: reqwest::Client,
}

impl ControlClient {
    /// Plain HTTP is allowed only in development.
    pub(super) fn new(development: bool) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .https_only(!development)
            .user_agent(concat!("sentinald/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        ControlClient { http }
    }

    /// POST `body` to `<control_url><path>`, with an optional bearer.
    pub(super) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        control_url: &str,
        path: &str,
        bearer: Option<&str>,
        body: &B,
    ) -> Result<Option<T>, CallError> {
        let mut request = self.http.post(format!("{control_url}{path}")).json(body);
        if let Some(bearer) = bearer {
            request = request.bearer_auth(bearer);
        }
        // `without_url`: errors must not echo URLs into logs or replies.
        let response = request
            .send()
            .await
            .map_err(|error| CallError::Unreachable(error.without_url().to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| CallError::Unreachable(error.without_url().to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes).map(Some).map_err(|_| {
                CallError::Unexpected(format!("HTTP {status} with an unrecognized body"))
            });
        }
        match serde_json::from_slice::<LinkFailure>(&bytes) {
            Ok(failure) => Err(CallError::Refused { failure }),
            Err(_) => Err(CallError::Unexpected(format!(
                "HTTP {status}; is this a Sentinal control?"
            ))),
        }
    }
}
