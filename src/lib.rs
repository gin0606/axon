//! Storage adapters around the lifecycle core; contracts live in `docs/reference/`.
pub use axon_core::{declaration, lifecycle, read};
pub mod declaration_file;
mod error;
pub use error::{Error, PREFIX_RULE, Result, validate_prefix};

pub mod file;
pub mod location;
