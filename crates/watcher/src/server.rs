//! vLLM and llama.cpp server readiness recognition.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, EventKind};

/// Current structured server state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerState {
    /// Model name when reported by the server.
    pub model: Option<String>,
    /// Listening port when known.
    pub port: Option<u16>,
    /// Listening URL when known.
    pub url: Option<String>,
    /// Whether a readiness line was observed.
    pub started: bool,
}

/// Adapter for vLLM and llama.cpp server Commands.
pub struct ServerAdapter {
    state: ServerState,
}

impl ServerAdapter {
    pub(crate) fn new() -> Self {
        ServerAdapter {
            state: ServerState {
                model: None,
                port: None,
                url: None,
                started: false,
            },
        }
    }
}

impl WatcherAdapter for ServerAdapter {
    fn name(&self) -> &'static str {
        "server"
    }

    fn matches(&self, command: &Command) -> bool {
        let lower = command.text.to_ascii_lowercase();
        lower.contains("vllm")
            || lower.contains("llama-server")
            || lower.contains("llama_server")
            || lower.contains("llama.cpp")
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::OutputLine(line) = &observation.kind else {
            return Vec::new();
        };
        let lower = line.to_ascii_lowercase();
        if let Some(model) = value_after(line, &lower, "loading model ")
            .or_else(|| value_after(line, &lower, "model.name = "))
        {
            self.state.model = Some(model);
        }
        let ready = lower.contains("uvicorn running on")
            || lower.contains("server is listening on")
            || lower.contains("listening at");
        if !ready || self.state.started {
            return Vec::new();
        }
        let url = find_url(line);
        let port = url
            .as_deref()
            .and_then(port_from_url)
            .or_else(|| find_port(line));
        self.state.started = true;
        self.state.url.clone_from(&url);
        self.state.port = port;
        vec![EventKind::ServerStarted { port, url }]
    }

    fn state(&self) -> AdapterState {
        AdapterState::Server(self.state.clone())
    }
}

fn value_after(line: &str, lower: &str, marker: &str) -> Option<String> {
    let index = lower.find(marker)? + marker.len();
    let value = line[index..].trim().trim_matches(['\'', '"']);
    (!value.is_empty()).then(|| value.to_string())
}

fn find_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
        .map(|part| {
            part.trim_end_matches(|value: char| ",.;)]}".contains(value))
                .to_string()
        })
}

fn port_from_url(url: &str) -> Option<u16> {
    let authority = url.split("//").nth(1)?.split('/').next()?;
    authority.rsplit_once(':')?.1.parse().ok()
}

fn find_port(line: &str) -> Option<u16> {
    let lower = line.to_ascii_lowercase();
    let index = lower.find("port ")? + 5;
    line[index..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}
