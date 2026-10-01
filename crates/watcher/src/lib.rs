//! Pure deterministic conversion of Session observations into Events and Health.

mod adapter;
mod cargo_adapter;
mod declarative;
mod engine;
mod framing;
mod generic;
mod health;
mod observation;
mod pytest;
mod server;
mod training;

pub use adapter::{AdapterState, WatcherAdapter};
pub use declarative::{DeclarativeAdapter, DeclarativeError, DeclarativeState};
pub use engine::{EngineUpdate, EventEngine};
pub use framing::LineFramer;
pub use generic::{GenericAdapter, GenericState};
pub use health::HealthTracker;
pub use observation::{MetricsObservation, Observation, ObservationKind, ProcessObservation};
