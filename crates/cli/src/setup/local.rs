//! Talking to this Machine's own daemon: the private local socket and plain
//! HTTP on loopback. Nothing goes on a command line, so the Machine token and
//! the owner password never appear in `ps`.

use super::config::API_BIND;
use std::time::Duration;

/// One request on the daemon's private local socket (same-user only).
pub(super) fn local_socket(
    request: protocol::LocalRequest,
) -> Result<protocol::LocalResponse, String> {
    use std::io::{Read, Write};
    let path = crate::connect_command::default_socket().ok_or("HOME is not set")?;
    let mut stream = std::os::unix::net::UnixStream::connect(&path)
        .map_err(|error| format!("cannot reach sentinald at {}: {error}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(300))).ok();
    let text = protocol::Envelope::new(request)
        .encode()
        .map_err(|error| error.to_string())?;
    let length = u32::try_from(text.len()).map_err(|_| "request too large".to_string())?;
    stream
        .write_all(&length.to_be_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .write_all(text.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let length = u32::from_be_bytes(header) as usize;
    if length > 1024 * 1024 {
        return Err("reply too large".into());
    }
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(body).map_err(|error| error.to_string())?;
    protocol::Envelope::<protocol::LocalResponse>::decode(&text)
        .map(|envelope| envelope.payload)
        .map_err(|error| error.to_string())
}

/// One HTTP/1.1 request to the daemon on loopback. Nothing goes on a command
/// line, so the token and password never appear in `ps`.
pub(super) fn local_http(
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    http_to(API_BIND, method, path, bearer, body)
}

fn http_to(
    address: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(
        &address.parse().map_err(|_| "bad API address".to_string())?,
        Duration::from_secs(3),
    )
    .map_err(|error| format!("cannot reach sentinald: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let body = body.unwrap_or("");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(bearer) = bearer {
        request.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    if !body.is_empty() {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut reply = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut reply)
        .map_err(|error| error.to_string())?;
    let reply = String::from_utf8_lossy(&reply);
    let status = reply
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("sentinald sent no HTTP status")?;
    let body = reply
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::http_to;
    use std::io::{Read, Write};

    #[test]
    fn the_loopback_request_carries_the_bearer_and_json_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n\r\n{\"message\":\"password must be between 12 and 1024 bytes\"}")
                .unwrap();
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        let (status, body) = http_to(
            &address,
            "POST",
            "/api/v1/auth/setup",
            Some("tok"),
            Some("{\"password\":\"x\"}"),
        )
        .unwrap();
        let request = server.join().unwrap();
        assert_eq!(status, 400);
        assert!(body.contains("between 12"), "{body}");
        assert!(
            request.starts_with("POST /api/v1/auth/setup HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.contains("Authorization: Bearer tok\r\n"),
            "{request}"
        );
        assert!(request.contains("Content-Length: 16\r\n"), "{request}");
        assert!(
            request.ends_with("\r\n\r\n{\"password\":\"x\"}"),
            "{request}"
        );
    }
}
