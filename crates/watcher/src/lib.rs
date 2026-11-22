//! Pure deterministic conversion of Session observations into Events and Health.

mod adapter;
pub mod agent_activity;
mod agent_cli;
pub mod agent_transcript;
mod bursts;
mod cargo_adapter;
mod declarative;
mod engine;
pub mod estimate;
mod framing;
mod generic;
mod health;
mod observation;
mod progress;
mod pytest;
pub mod redact;
mod screen_prompt;
mod server;
pub mod training;

pub use adapter::{AdapterState, WatcherAdapter};
pub use agent_cli::{agent_of, coding_agent, AgentCliAdapter, AgentCliState};
pub use declarative::{DeclarativeAdapter, DeclarativeError, DeclarativeState};
pub use engine::{EngineUpdate, EventEngine};
pub use framing::LineFramer;
pub use generic::{GenericAdapter, GenericState};
pub use health::HealthTracker;
pub use observation::{MetricsObservation, Observation, ObservationKind, ProcessObservation};
pub use progress::AgentLine;

/// The menu on a screen, read the way the watcher reads questions: its
/// options and the one the selector is on (T3.4). Terminal text is data.
pub fn menu_on_screen(rows: &[String]) -> Option<(Vec<protocol::PromptOption>, Option<u8>)> {
    screen_prompt::find_question(rows).map(|question| (question.options, question.marked))
}
