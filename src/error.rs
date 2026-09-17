//! Errors shared by storage adapters and their callers.
use crate::lifecycle;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    PublicationUnknown(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("Result unknown: SQLite commit failed: {0}; inspect saved state before retrying")]
    Commit(rusqlite::Error),
    #[error(transparent)]
    Core(#[from] lifecycle::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

pub fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Err(invalid("Entity prefix must not be empty"));
    }
    Ok(())
}
