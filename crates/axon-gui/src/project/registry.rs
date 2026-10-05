//! The list of projects and the rules for changing it, as values. Nothing here touches the
//! filesystem: [`super::data`] reads and writes the file this module encodes.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The version of the registry file this build reads and writes. A file of another version is
/// refused rather than rewritten, so a newer application's list is never lost.
pub const FORMAT: u32 = 1;
/// The longest project name, in characters.
pub const NAME_LIMIT: usize = 80;
/// The number of lowercase hexadecimal digits in a [`ProjectId`].
pub const ID_LENGTH: usize = 12;

/// The stable identifier of a project. It names the project's directory, so the display name
/// can be any text and is free to change without moving data.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectId(String);

impl ProjectId {
    /// The identifier spelled by the lowest [`ID_LENGTH`] hexadecimal digits of `bits`; the
    /// shell supplies the randomness.
    pub fn from_bits(bits: u64) -> Self {
        Self(format!(
            "{:0width$x}",
            bits & ((1 << (4 * ID_LENGTH)) - 1),
            width = ID_LENGTH
        ))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ProjectId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.len() == ID_LENGTH
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err(format!(
                "invalid project ID {value:?}: expected {ID_LENGTH} lowercase hexadecimal digits"
            ))
        }
    }
}
impl From<ProjectId> for String {
    fn from(id: ProjectId) -> Self {
        id.0
    }
}
impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether the project's store is known to exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Registered, but the store may not be initialized yet: creation was interrupted or
    /// failed after the project was recorded. Finishing it initializes the store if absent.
    Creating,
    /// The store was initialized. A missing or unreadable store is then an error, never a
    /// reason to initialize an empty one in its place.
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub status: Status,
}

/// The projects in creation order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    projects: Vec<Project>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    projects: Vec<Project>,
}

/// Why a registry file could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A format this build does not know.
    Format(u32),
    /// Not a registry: unreadable JSON, a missing field, an invalid ID or name.
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

/// Why a name was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    ControlCharacter,
    Duplicate,
}

/// Why a change to the registry was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeError {
    Name(NameError),
    /// The ID is already registered.
    DuplicateId,
    /// No project with that ID.
    Unknown,
}

/// Control, line and paragraph separator, format characters (zero-width and bidirectional
/// controls among them) and blank fillers: they break a name across lines, hide it, or make it
/// display as another. Variation selectors stay allowed for kanji variants, and the joiners and
/// tag characters that emoji sequences are built from stay allowed too.
fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{0600}'..='\u{0605}'
                | '\u{061C}'
                | '\u{06DD}'
                | '\u{070F}'
                | '\u{180E}'
                | '\u{200B}'
                | '\u{200E}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{034F}'
                | '\u{0890}'..='\u{0891}'
                | '\u{08E2}'
                | '\u{115F}'..='\u{1160}'
                | '\u{3164}'
                | '\u{FFA0}'
                | '\u{110BD}'
                | '\u{110CD}'
                | '\u{13430}'..='\u{1343F}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E001F}'
        )
}

impl Registry {
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }
    pub fn get(&self, id: &ProjectId) -> Option<&Project> {
        self.projects.iter().find(|project| &project.id == id)
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
        for project in file.projects {
            if registry.get(&project.id).is_some() {
                return Err(DecodeError::Invalid(format!(
                    "project ID {} appears twice",
                    project.id
                )));
            }
            // Stored names are taken as written: rules for new names may tighten later, and a
            // name they would refuse must not make the whole list unreadable.
            if project.name.is_empty() {
                return Err(DecodeError::Invalid(format!(
                    "project {} has an empty name",
                    project.id
                )));
            }
            registry.projects.push(project);
        }
        Ok(registry)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(&File {
            format: FORMAT,
            projects: self.projects.clone(),
        })
        .expect("a registry always serializes");
        bytes.push(b'\n');
        bytes
    }

    /// The name a project would be stored under: `raw` without surrounding whitespace, when it
    /// is not empty, fits [`NAME_LIMIT`], has no control characters and no other project has
    /// it.
    pub fn check_name(&self, raw: &str) -> Result<String, NameError> {
        let name = raw.trim();
        // Joiners, tag characters and variation selectors are allowed only beside something
        // visible.
        if name.chars().all(|c| {
            matches!(
                c,
                '\u{200C}'..='\u{200D}'
                    | '\u{E0020}'..='\u{E007F}'
                    | '\u{FE00}'..='\u{FE0F}'
                    | '\u{E0100}'..='\u{E01EF}'
            )
        }) {
            return Err(NameError::Empty);
        }
        if name.chars().count() > NAME_LIMIT {
            return Err(NameError::TooLong);
        }
        if name.chars().any(invisible) {
            return Err(NameError::ControlCharacter);
        }
        if self.projects.iter().any(|project| project.name == name) {
            return Err(NameError::Duplicate);
        }
        Ok(name.to_owned())
    }

    /// The registry with a new project, in [`Status::Creating`], appended.
    pub fn with_creating(&self, id: ProjectId, raw_name: &str) -> Result<Self, ChangeError> {
        let name = self.check_name(raw_name).map_err(ChangeError::Name)?;
        if self.get(&id).is_some() {
            return Err(ChangeError::DuplicateId);
        }
        let mut next = self.clone();
        next.projects.push(Project {
            id,
            name,
            status: Status::Creating,
        });
        Ok(next)
    }

    /// The registry with the project marked [`Status::Ready`].
    pub fn with_ready(&self, id: &ProjectId) -> Result<Self, ChangeError> {
        let mut next = self.clone();
        let project = next
            .projects
            .iter_mut()
            .find(|project| &project.id == id)
            .ok_or(ChangeError::Unknown)?;
        project.status = Status::Ready;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(bits: u64) -> ProjectId {
        ProjectId::from_bits(bits)
    }

    #[test]
    fn ids_are_fixed_width_lowercase_hex() {
        assert_eq!(id(0).as_str(), "000000000000");
        assert_eq!(id(u64::MAX).as_str(), "ffffffffffff");
        assert_eq!(id(0xab).as_str(), "0000000000ab");
        for invalid in [
            "",
            "00000000000",
            "0000000000000",
            "00000000000G",
            "00000000000A",
            "../../etc/pw",
        ] {
            assert!(
                ProjectId::try_from(invalid.to_owned()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn names_are_trimmed_bounded_and_unique() {
        let registry = Registry::default();
        assert_eq!(registry.check_name("  読書会 "), Ok("読書会".into()));
        for blank in [" \t", "\u{200D}", "\u{E0061}\u{FE0F}"] {
            assert_eq!(
                registry.check_name(blank),
                Err(NameError::Empty),
                "{blank:?}"
            );
        }
        for name in [
            "改\n行",
            "改\u{2028}行",
            "\u{200B}",
            "名\u{202E}前",
            "読書会\u{E0001}",
            "\u{3164}",
        ] {
            assert_eq!(
                registry.check_name(name),
                Err(NameError::ControlCharacter),
                "{name:?}"
            );
        }
        for name in [
            "👨\u{200D}💻 開発",
            "🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
            "葛\u{E0100}飾",
        ] {
            assert_eq!(registry.check_name(name), Ok(name.to_owned()), "{name:?}");
        }
        assert_eq!(
            registry.check_name(&"あ".repeat(NAME_LIMIT)),
            Ok("あ".repeat(NAME_LIMIT))
        );
        assert_eq!(
            registry.check_name(&"あ".repeat(NAME_LIMIT + 1)),
            Err(NameError::TooLong)
        );
        let registry = registry.with_creating(id(1), "読書会").unwrap();
        assert_eq!(registry.check_name("読書会 "), Err(NameError::Duplicate));
        assert_eq!(
            registry.with_creating(id(2), "読書会"),
            Err(ChangeError::Name(NameError::Duplicate))
        );
        assert_eq!(
            registry.with_creating(id(1), "家計"),
            Err(ChangeError::DuplicateId)
        );
    }

    #[test]
    fn creation_then_ready_round_trips_through_the_file() {
        let registry = Registry::default()
            .with_creating(id(1), "読書会")
            .unwrap()
            .with_creating(id(2), "家計")
            .unwrap()
            .with_ready(&id(1))
            .unwrap();
        assert_eq!(
            registry.projects(),
            [
                Project {
                    id: id(1),
                    name: "読書会".into(),
                    status: Status::Ready
                },
                Project {
                    id: id(2),
                    name: "家計".into(),
                    status: Status::Creating
                },
            ]
        );
        assert_eq!(Registry::decode(&registry.encode()), Ok(registry.clone()));
        assert_eq!(registry.with_ready(&id(3)), Err(ChangeError::Unknown));
    }

    #[test]
    fn foreign_or_damaged_files_are_refused() {
        assert_eq!(
            Registry::decode(br#"{"format":2,"projects":[]}"#),
            Err(DecodeError::Format(2))
        );
        for invalid in [
            &b""[..],
            b"{",
            b"[]",
            br#"{"format":1}"#,
            br#"{"format":1,"projects":[],"extra":0}"#,
            br#"{"format":1,"projects":[{"id":"x","name":"a","status":"ready"}]}"#,
            br#"{"format":1,"projects":[{"id":"000000000001","name":"","status":"ready"}]}"#,
            br#"{"format":1,"projects":[{"id":"000000000001","name":"a","status":"gone"}]}"#,
            br#"{"format":1,"projects":[{"id":"000000000001","name":"a","status":"ready"},{"id":"000000000001","name":"b","status":"ready"}]}"#,
        ] {
            assert!(
                matches!(Registry::decode(invalid), Err(DecodeError::Invalid(_))),
                "{}",
                String::from_utf8_lossy(invalid)
            );
        }
        assert_eq!(
            Registry::decode(br#"{"format":1,"projects":[]}"#),
            Ok(Registry::default())
        );
        // A stored name a newer rule would refuse is still read.
        let stored = Registry::decode(
            br#"{"format":1,"projects":[{"id":"000000000001","name":" a\u200b","status":"ready"}]}"#,
        )
        .unwrap();
        assert_eq!(stored.projects()[0].name, " a\u{200b}");
    }
}
