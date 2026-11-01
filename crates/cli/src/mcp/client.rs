//! The agent API client: `GET /api/v1/agent/…` on the control with an agent token.

use super::Backend;
use protocol::{
    agent::{AgentIdentity, AgentUpdates},
    link::{validate_control_url, ControlUrlPolicy},
    ControlProjectionSnapshot, Secret,
};
use serde::de::DeserializeOwned;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Talks to one control as one agent.
pub struct AgentClient {
    http: reqwest::Client,
    origin: String,
    token: Secret,
}

impl AgentClient {
    /// `control_url` must be the control's `https://….ts.net` origin; `development`
    /// also allows `http://` to loopback or a Tailscale IP.
    pub fn new(control_url: &str, token: Secret, development: bool) -> Result<Self, String> {
        let policy = if development {
            ControlUrlPolicy::Development
        } else {
            ControlUrlPolicy::Production
        };
        let origin =
            validate_control_url(control_url, policy).map_err(|error| error.to_string())?;
        if !token
            .expose()
            .starts_with(protocol::agent::AGENT_TOKEN_PREFIX)
        {
            return Err("this is not an agent token; create one in Settings → Agents".into());
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(AgentClient {
            http,
            origin,
            token,
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, wait: Duration) -> Result<T, String> {
        let response = self
            .http
            .get(format!("{}{path}", self.origin))
            .bearer_auth(self.token.expose())
            .timeout(REQUEST_TIMEOUT + wait)
            .send()
            .await
            .map_err(|error| format!("could not reach the control: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            let message = body["message"].as_str().unwrap_or("request refused");
            return Err(format!("{} {message}", status.as_u16()));
        }
        response
            .json()
            .await
            .map_err(|error| format!("unexpected reply: {error}"))
    }
}

impl Backend for AgentClient {
    async fn whoami(&self) -> Result<AgentIdentity, String> {
        self.get("/api/v1/agent/whoami", Duration::ZERO).await
    }

    async fn snapshot(&self) -> Result<ControlProjectionSnapshot, String> {
        self.get("/api/v1/agent/snapshot", Duration::ZERO).await
    }

    async fn updates(&self, after: u64, wait: Duration) -> Result<AgentUpdates, String> {
        let path = format!(
            "/api/v1/agent/updates?after={after}&wait_ms={}",
            wait.as_millis()
        );
        self.get(&path, wait).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Answer one HTTP request with `status` and `body`; returns the request head.
    async fn one_reply(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buffer[..read]).to_string()
        });
        (origin, task)
    }

    fn token() -> Secret {
        Secret::new(format!("sat_agt_{}_{}", "0".repeat(16), "f".repeat(64)))
    }

    #[tokio::test]
    async fn requests_carry_the_agent_token_and_the_wait() {
        let (origin, server) = one_reply("200 OK", r#"{"messages":[]}"#).await;
        let client = AgentClient::new(&origin, token(), true).unwrap();
        let updates = client
            .updates(41, Duration::from_millis(1500))
            .await
            .unwrap();
        assert!(updates.messages.is_empty());
        let head = server.await.unwrap();
        assert!(
            head.starts_with("GET /api/v1/agent/updates?after=41&wait_ms=1500 HTTP/1.1"),
            "{head}"
        );
        assert!(head
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {}", token().expose())));
    }

    #[tokio::test]
    async fn a_refusal_reports_the_controls_message() {
        let (origin, server) = one_reply(
            "403 Forbidden",
            r#"{"code":"UNAUTHORIZED","message":"this agent is level 0; x needs level 2"}"#,
        )
        .await;
        let client = AgentClient::new(&origin, token(), true).unwrap();
        let error = client.whoami().await.unwrap_err();
        assert_eq!(error, "403 this agent is level 0; x needs level 2");
        server.await.unwrap();
    }

    #[test]
    fn only_a_tailnet_control_and_an_agent_token_are_accepted() {
        let token = || Secret::new(format!("sat_agt_{}_{}", "0".repeat(16), "f".repeat(64)));
        assert!(AgentClient::new("https://control.example-tailnet.ts.net", token(), false).is_ok());
        assert!(AgentClient::new("https://evil.example.com", token(), false).is_err());
        assert!(AgentClient::new("http://127.0.0.1:7420", token(), false).is_err());
        assert!(AgentClient::new("http://127.0.0.1:7420", token(), true).is_ok());
        let machine = Secret::new("a".repeat(64));
        let error = AgentClient::new("https://control.example-tailnet.ts.net", machine, false)
            .err()
            .unwrap();
        assert!(error.contains("not an agent token"));
    }
}
