//! Common adapter contract and structured adapter state.

use crate::Observation;
use protocol::{Command, EventKind};

/// Structured state exposed by one selected adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterState {
    /// Generic prompt/error/stall state.
    Generic(crate::GenericState),
    /// Training progress and scalar state.
    Training(crate::training::TrainingState),
    /// pytest collection and result state.
    Pytest(crate::pytest::PytestState),
    /// Model-server listening state.
    Server(crate::server::ServerState),
    /// Cargo compile/test state.
    Cargo(crate::cargo_adapter::CargoState),
    /// Declarative rule match count.
    Declarative(crate::declarative::DeclarativeState),
    /// AI coding agent and its approval prompt (6.1).
    AgentCli(crate::agent_cli::AgentCliState),
}

/// Pure deterministic adapter selected from a structured Command.
pub trait WatcherAdapter: Send {
    /// Stable adapter name.
    fn name(&self) -> &'static str;
    /// Whether this adapter handles the Command.
    fn matches(&self, command: &Command) -> bool;
    /// Convert one observation into zero or more Event kinds.
    fn observe(&mut self, observation: &Observation) -> Vec<EventKind>;
    /// Return current structured parser state.
    fn state(&self) -> AdapterState;
    /// Whether the program is waiting for a person right now.
    fn waiting(&self) -> bool {
        false
    }
    /// Whether this adapter reports prompts itself, so the generic prompt
    /// guess (which has no answers) is not reported as well.
    fn owns_prompts(&self) -> bool {
        false
    }
}
