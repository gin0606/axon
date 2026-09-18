//! Single-lifecycle core and adapters for `spec/lifecycle_proposal.md`.
pub use axon_core::{declaration, lifecycle, read};
pub mod declaration_file;
mod error;
pub use error::{Error, PREFIX_RULE, Result, validate_prefix};

pub mod file;
pub mod file_merge;
pub mod location;
pub mod sqlite;
