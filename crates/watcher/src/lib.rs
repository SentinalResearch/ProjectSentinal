//! Pure deterministic conversion of Session observations into Events and Health.

mod adapter;
pub mod agent_activity;
mod agent_cli;
pub mod agent_transcript;
mod bursts;
mod cargo_adapter;
mod declarative;
mod engine;
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
