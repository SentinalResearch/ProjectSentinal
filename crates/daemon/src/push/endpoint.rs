//! Validation for browser-supplied Push endpoints and resolved destinations.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A shape-validated endpoint whose DNS answers still require validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPushEndpoint {
    /// Original bounded HTTPS endpoint including its opaque provider path.
    pub url: String,
    /// ASCII DNS hostname used for resolution and TLS SNI.
    pub host: String,
    /// Provider port, always 443 in production.
    pub port: u16,
}

/// Why an untrusted Push endpoint or DNS result was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PushEndpointError {
    /// URL shape is outside the production policy.
    #[error("push endpoint {0}")]
    Invalid(&'static str),
    /// DNS returned no usable addresses.
    #[error("push endpoint DNS returned no addresses")]
    EmptyDns,
    /// At least one DNS answer was not globally routable.
    #[error("push endpoint resolved to a non-public address")]
    NonPublic,
}

/// Validate URL shape without performing DNS or network I/O.
pub fn validate_push_endpoint_shape(
    value: &str,
) -> Result<ValidatedPushEndpoint, PushEndpointError> {
    if value.is_empty() || value.len() > protocol::notification::MAX_PUSH_ENDPOINT_BYTES {
        return Err(PushEndpointError::Invalid("must be 1 to 2048 bytes"));
    }
    if value.contains('\\') || value.chars().any(char::is_control) {
        return Err(PushEndpointError::Invalid(
            "must not contain backslashes or control characters",
        ));
    }
    if value
        .strip_prefix("https://")
        .is_some_and(|authority| authority.starts_with('/'))
    {
        return Err(PushEndpointError::Invalid("must contain a DNS name"));
    }
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| PushEndpointError::Invalid("must be an absolute HTTPS URL"))?;
    if parsed.scheme() != "https" {
        return Err(PushEndpointError::Invalid("must use HTTPS"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(PushEndpointError::Invalid("must not contain credentials"));
    }
    if parsed.fragment().is_some() {
        return Err(PushEndpointError::Invalid("must not contain a fragment"));
    }
    if parsed.port_or_known_default() != Some(443) {
        return Err(PushEndpointError::Invalid("must use port 443"));
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty() && host.len() <= 253)
        .ok_or(PushEndpointError::Invalid(
            "must contain a bounded DNS name",
        ))?;
    if host.parse::<IpAddr>().is_ok()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(PushEndpointError::Invalid(
            "must use a DNS name rather than an IP literal",
        ));
    }
    Ok(ValidatedPushEndpoint {
        url: value.to_string(),
        host: host.to_ascii_lowercase(),
        port: 443,
    })
}

/// Require every DNS answer to be globally routable before pinning a request.
pub fn validate_resolved_addresses(addresses: &[IpAddr]) -> Result<(), PushEndpointError> {
    if addresses.is_empty() {
        return Err(PushEndpointError::EmptyDns);
    }
    if addresses.iter().copied().all(is_public) {
        Ok(())
    } else {
        Err(PushEndpointError::NonPublic)
    }
}

fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    if address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_multicast()
        || address.is_unspecified()
    {
        return false;
    }
    !matches!(
        octets,
        [0, ..]
            | [100, 64..=127, ..]
            | [192, 0, 0, ..]
            | [192, 0, 2, ..]
            | [192, 88, 99, ..]
            | [198, 18..=19, ..]
            | [198, 51, 100, ..]
            | [203, 0, 113, ..]
            | [240..=255, ..]
    )
}

fn public_v6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return public_v4(mapped);
    }
    let segments = address.segments();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || segments[..2] == [0x2001, 0x0db8])
}
