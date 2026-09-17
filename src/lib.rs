//! Single-lifecycle core and adapters for `spec/lifecycle_proposal.md`.
pub mod declaration;
pub mod declaration_file;
mod error;
pub use error::{Error, Result, validate_prefix};

pub mod file;
pub mod file_merge;
pub mod lifecycle;
pub mod location;
pub mod sqlite;
