//! Storage adapters around the lifecycle core; contracts live in `docs/reference/`.
pub use axon_core::{declaration, lifecycle, read};
pub mod declaration_file;
mod error;
pub use error::{Error, PREFIX_RULE, Result, validate_prefix};

pub mod file;
pub mod location;

/// The context of a change made now: the current time and the recorder that the inherited
/// environment names, the same for every adapter that writes records.
pub fn context_now() -> lifecycle::Context {
    lifecycle::Context {
        at: chrono::Utc::now(),
        recorder: axon_recorder::detect().map(|recorder| lifecycle::Recorder {
            actor: recorder.actor,
            data: recorder
                .data
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }),
    }
}
