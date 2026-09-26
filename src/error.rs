//! Errors shared by storage adapters and their callers.
use crate::lifecycle;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    PublicationUnknown(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Core(#[from] lifecycle::Error),
    /// The Git index of `worktree` has unmerged `paths` (relative to it) under `.axon/`.
    #[error(
        "unmerged Git index under .axon/ in the worktree {}; resolve and stage these paths before normal operations{}",
        .worktree.display(),
        unmerged_lines(.paths)
    )]
    Unmerged {
        worktree: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// A `.axon` holding `file` but no header stopped discovery at `root`.
    #[error(
        "{} is not a store: no {} beside {}; an earlier format or a foreign file is not converted",
        .root.join(".axon").display(),
        crate::file::HEADER_FILE,
        .file.display()
    )]
    NotAStore { root: PathBuf, file: PathBuf },
}
/// One line per unmerged path; a newline in a path cannot start a line of its own.
fn unmerged_lines(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| {
            format!(
                "\nUnmerged: {}",
                path.display().to_string().replace('\n', "\\n")
            )
        })
        .collect()
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

/// The characters an ID prefix may use, for diagnostics that reject one.
pub const PREFIX_RULE: &str =
    "ASCII lowercase letters, digits and hyphens, starting and ending with a letter or digit";

pub fn validate_prefix(prefix: &str) -> Result<()> {
    let allowed = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-';
    if prefix.is_empty()
        || prefix.starts_with('-')
        || prefix.ends_with('-')
        || !prefix.as_bytes().iter().all(allowed)
    {
        return Err(invalid(format!(
            "invalid ID prefix {prefix:?}: expected {PREFIX_RULE}"
        )));
    }
    Ok(())
}
