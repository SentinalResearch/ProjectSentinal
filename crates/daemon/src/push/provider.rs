//! Closed safe payloads and RFC Web Push request construction.

use super::{endpoint::validate_push_endpoint_shape, key_store::VapidKeyStore};
use base64ct::{Base64UrlUnpadded, Encoding as _};
use protocol::{NotificationCategory, Timestamp};
use serde::{Deserialize, Serialize};
use web_push_native::{p256::PublicKey, Auth, WebPushBuilder};

const MAX_PLAINTEXT_BYTES: usize = 2_048;
const MAX_TITLE_BYTES: usize = 100;
const MAX_BODY_BYTES: usize = 240;
const MAX_ROUTE_BYTES: usize = 2_048;

/// Coarse urgency hint interpreted by the Sentinal service worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PushPriority {
    /// Time-sensitive failure or owner-input state.
    High,
    /// Ordinary completion or health state.
    Default,
    /// Informational checkpoint or progress state.
    Low,
}

/// The only plaintext schema that may be encrypted for a browser Push service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafePushPayload {
    /// Payload schema version.
    pub v: u32,
    /// Bounded safe title.
    pub title: String,
    /// Bounded safe body rendered from structured facts.
    pub body: String,
    /// Stable Event-derived category.
    pub category: NotificationCategory,
    /// Coarse urgency hint.
    pub priority: PushPriority,
    /// Event timestamp.
    pub at: Timestamp,
    /// Validated same-origin internal Session route.
    pub route: String,
}

impl SafePushPayload {
    /// Build and validate a closed privacy-safe encrypted payload.
    pub fn new(
        title: &str,
        body: &str,
        category: NotificationCategory,
        priority: PushPriority,
        at: Timestamp,
        route: &str,
    ) -> Result<Self, PushBuildError> {
        validate_text("title", title, MAX_TITLE_BYTES)?;
        validate_text("body", body, MAX_BODY_BYTES)?;
        validate_route(route)?;
        let payload = Self {
            v: 1,
            title: title.to_string(),
            body: body.to_string(),
            category,
            priority,
            at,
            route: route.to_string(),
        };
        let bytes = serde_json::to_vec(&payload)
            .map_err(|_| PushBuildError::Invalid("payload could not be encoded"))?;
        if bytes.len() > MAX_PLAINTEXT_BYTES {
            return Err(PushBuildError::Invalid(
                "payload exceeds 2048 plaintext bytes",
            ));
        }
        Ok(payload)
    }

    fn encode(&self) -> Result<Vec<u8>, PushBuildError> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| PushBuildError::Invalid("payload could not be encoded"))?;
        if bytes.len() > MAX_PLAINTEXT_BYTES {
            return Err(PushBuildError::Invalid(
                "payload exceeds 2048 plaintext bytes",
            ));
        }
        Ok(bytes)
    }
}

/// Provider-ready encrypted request without exposing dependency HTTP types.
#[derive(Clone, PartialEq, Eq)]
pub struct BuiltPushRequest {
    url: String,
    host: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl std::fmt::Debug for BuiltPushRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BuiltPushRequest")
            .field("url", &"[redacted]")
            .field("headers", &"[redacted]")
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl BuiltPushRequest {
    /// Total bounded provider request bytes, excluding transport framing.
    pub fn encoded_size(&self) -> u32 {
        let headers = self
            .headers
            .iter()
            .map(|(name, value)| name.len().saturating_add(value.len()))
            .sum::<usize>();
        u32::try_from(headers.saturating_add(self.body.len())).unwrap_or(u32::MAX)
    }

    /// Generated HTTP headers, including VAPID authorization.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Encrypted request body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Destination DNS host for tests and pinned transport preparation.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn into_parts(self) -> (String, Vec<(String, String)>, Vec<u8>) {
        (self.url, self.headers, self.body)
    }
}

/// Pure request builder for the Web Push provider.
pub struct WebPushProvider;

impl WebPushProvider {
    /// Encrypt one safe payload and add VAPID headers for one subscription.
    pub fn build_request(
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        payload: &SafePushPayload,
        vapid: &VapidKeyStore,
    ) -> Result<BuiltPushRequest, PushBuildError> {
        let validated = validate_push_endpoint_shape(endpoint)
            .map_err(|_| PushBuildError::Invalid("endpoint failed HTTPS validation"))?;
        let public_bytes = Base64UrlUnpadded::decode_vec(p256dh)
            .map_err(|_| PushBuildError::Invalid("p256dh is not valid base64url"))?;
        let ua_public = PublicKey::from_sec1_bytes(&public_bytes)
            .map_err(|_| PushBuildError::Invalid("p256dh is not a P-256 public key"))?;
        let auth_bytes = Base64UrlUnpadded::decode_vec(auth)
            .map_err(|_| PushBuildError::Invalid("auth is not valid base64url"))?;
        let ua_auth: [u8; 16] = auth_bytes
            .try_into()
            .map_err(|_| PushBuildError::Invalid("auth must contain 16 bytes"))?;
        let endpoint_uri = endpoint
            .parse()
            .map_err(|_| PushBuildError::Invalid("endpoint is not an HTTP URI"))?;
        let signing_key = vapid
            .signing_key()
            .map_err(|_| PushBuildError::Invalid("VAPID signing key is invalid"))?;
        let request =
            WebPushBuilder::new(endpoint_uri, ua_public, Auth::clone_from_slice(&ua_auth))
                .with_vapid(&signing_key, vapid.subject())
                .build(payload.encode()?)
                .map_err(|_| PushBuildError::Invalid("Web Push encryption failed"))?;
        let (parts, body) = request.into_parts();
        let mut headers = Vec::with_capacity(parts.headers.len());
        for (name, value) in &parts.headers {
            let text = value
                .to_str()
                .map_err(|_| PushBuildError::Invalid("generated header was not text"))?;
            headers.push((name.as_str().to_string(), text.to_string()));
        }
        Ok(BuiltPushRequest {
            url: endpoint.to_string(),
            host: validated.host,
            headers,
            body,
        })
    }
}

/// Safe request-construction failure with no endpoint or key material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PushBuildError {
    /// One closed input failed validation or encryption.
    #[error("invalid Web Push request: {0}")]
    Invalid(&'static str),
}

fn validate_text(field: &'static str, value: &str, maximum: usize) -> Result<(), PushBuildError> {
    if value.is_empty() {
        return Err(PushBuildError::Invalid(if field == "title" {
            "title must not be empty"
        } else {
            "body must not be empty"
        }));
    }
    if value.len() > maximum {
        return Err(PushBuildError::Invalid(if field == "title" {
            "title exceeds 100 bytes"
        } else {
            "body exceeds 240 bytes"
        }));
    }
    if value.chars().any(|character| {
        character.is_control()
            || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        return Err(PushBuildError::Invalid(if field == "title" {
            "title contains unsafe controls"
        } else {
            "body contains unsafe controls"
        }));
    }
    Ok(())
}

fn validate_route(route: &str) -> Result<(), PushBuildError> {
    if route.len() > MAX_ROUTE_BYTES
        || !route.starts_with("/app/sessions/")
        || route.starts_with("//")
        || route.contains(['\\', '#', '?'])
        || route.chars().any(char::is_control)
    {
        return Err(PushBuildError::Invalid(
            "route must be one bounded internal Session path",
        ));
    }
    Ok(())
}
