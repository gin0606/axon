//! The registered management roots and the rules for changing the list, as values. Nothing here
//! touches the filesystem: [`super::data`] reads and writes the file this module encodes.

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// The version of the registry file this build reads and writes. A file of another version is
/// refused rather than rewritten, so a newer application's list is never lost.
pub const FORMAT: u32 = 1;

/// A registered management root: the absolute path of the directory holding `.axon`. It
/// identifies the registration, so each worktree of one repository is a registration of its own.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    /// The registration of `path`, which must be absolute and UTF-8 so that the registry file
    /// can hold it. The caller normalizes it first.
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let Some(text) = path.to_str() else {
            return Err(format!("{} is not valid UTF-8", path.display()));
        };
        Self::try_from(text.to_owned())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    /// The name shown for the registration: the directory's own name, or the whole path for a
    /// root that has none.
    pub fn name(&self) -> String {
        match self.0.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self.0.display().to_string(),
        }
    }
}
impl TryFrom<String> for ProjectRoot {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            Ok(Self(path))
        } else {
            Err(format!(
                "invalid management root {}: expected an absolute path",
                path.display()
            ))
        }
    }
}
impl From<ProjectRoot> for String {
    fn from(root: ProjectRoot) -> Self {
        // Only UTF-8 paths are ever constructed.
        root.0.to_string_lossy().into_owned()
    }
}
impl fmt::Display for ProjectRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// The registered roots in registration order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    roots: Vec<ProjectRoot>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    roots: Vec<ProjectRoot>,
}

/// Why a registry file could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A format this build does not know.
    Format(u32),
    /// Not a registry: unreadable JSON, a missing field, a relative or repeated root.
    Invalid(String),
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(format) => write!(
                f,
                "unsupported registry format {format}; this build reads format {FORMAT}"
            ),
            Self::Invalid(reason) => write!(f, "invalid registry: {reason}"),
        }
    }
}

/// Why a change to the registry was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeError {
    /// The root is already registered.
    Duplicate,
    /// The root is not registered.
    Unknown,
}

impl Registry {
    pub fn roots(&self) -> &[ProjectRoot] {
        &self.roots
    }
    pub fn contains(&self, root: &ProjectRoot) -> bool {
        self.roots.contains(root)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        #[derive(Deserialize)]
        struct Version {
            format: u32,
        }
        let version: Version = serde_json::from_slice(bytes)
            .map_err(|error| DecodeError::Invalid(error.to_string()))?;
        if version.format != FORMAT {
            return Err(DecodeError::Format(version.format));
        }
        let file: File = serde_json::from_slice(bytes)
            .map_err(|error| DecodeError::Invalid(error.to_string()))?;
        let mut registry = Self::default();
        for root in file.roots {
            if registry.contains(&root) {
                return Err(DecodeError::Invalid(format!("{root} appears twice")));
            }
            registry.roots.push(root);
        }
        Ok(registry)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(&File {
            format: FORMAT,
            roots: self.roots.clone(),
        })
        .expect("a registry always serializes");
        bytes.push(b'\n');
        bytes
    }

    /// The registry with `root` appended.
    pub fn with(&self, root: ProjectRoot) -> Result<Self, ChangeError> {
        if self.contains(&root) {
            return Err(ChangeError::Duplicate);
        }
        let mut next = self.clone();
        next.roots.push(root);
        Ok(next)
    }

    /// The registry without `root`.
    pub fn without(&self, root: &ProjectRoot) -> Result<Self, ChangeError> {
        if !self.contains(root) {
            return Err(ChangeError::Unknown);
        }
        let mut next = self.clone();
        next.roots.retain(|registered| registered != root);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registration of `path` under an absolute directory of this platform.
    fn root(path: &str) -> ProjectRoot {
        ProjectRoot::new(std::env::temp_dir().join(path)).unwrap()
    }

    #[test]
    fn roots_are_absolute_and_named_after_their_directory() {
        assert!(ProjectRoot::new(PathBuf::from("relative/repo")).is_err());
        assert!(ProjectRoot::new(PathBuf::new()).is_err());
        assert_eq!(root("work/読書会").name(), "読書会");
        assert_eq!(root("work/axon-worktrees/gui").name(), "gui");
        let top = std::env::temp_dir()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            ProjectRoot::new(top.clone()).unwrap().name(),
            top.display().to_string()
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"/work/\xff".to_vec()));
            assert!(ProjectRoot::new(invalid).is_err());
        }
    }

    #[test]
    fn registering_and_unregistering_keep_the_order_and_refuse_duplicates() {
        let registry = Registry::default()
            .with(root("work/a"))
            .unwrap()
            .with(root("work/b"))
            .unwrap()
            .with(root("work/c"))
            .unwrap();
        assert_eq!(registry.with(root("work/b")), Err(ChangeError::Duplicate));
        let registry = registry.without(&root("work/b")).unwrap();
        assert_eq!(registry.roots(), [root("work/a"), root("work/c")]);
        assert_eq!(registry.without(&root("work/b")), Err(ChangeError::Unknown));
        assert_eq!(Registry::decode(&registry.encode()), Ok(registry));
    }

    #[test]
    fn foreign_or_damaged_files_are_refused() {
        assert_eq!(
            Registry::decode(br#"{"format":2,"roots":[]}"#),
            Err(DecodeError::Format(2))
        );
        for invalid in [
            &b""[..],
            b"{",
            b"[]",
            br#"{"format":1}"#,
            br#"{"format":1,"roots":[],"extra":0}"#,
            br#"{"format":1,"roots":["relative"]}"#,
            br#"{"format":1,"roots":[1]}"#,
            // The list of the application's own projects is another file.
            br#"{"format":1,"projects":[]}"#,
        ] {
            assert!(
                matches!(Registry::decode(invalid), Err(DecodeError::Invalid(_))),
                "{}",
                String::from_utf8_lossy(invalid)
            );
        }
        let twice = serde_json::to_vec(&serde_json::json!({
            "format": 1,
            "roots": [root("work/a"), root("work/a")],
        }))
        .unwrap();
        assert!(
            matches!(Registry::decode(&twice), Err(DecodeError::Invalid(reason)) if reason.contains("twice"))
        );
        assert_eq!(
            Registry::decode(br#"{"format":1,"roots":[]}"#),
            Ok(Registry::default())
        );
    }
}
