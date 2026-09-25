use super::{Result, invalid};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Entity IDs appear on command lines, so they stay free of characters a shell would quote.
fn entity_id_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}
fn record_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}
macro_rules! identifier {
    ($name:ident, $valid:path, $expected:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = super::Error;
            fn try_from(value: String) -> Result<Self> {
                if value.is_empty() || !value.bytes().all($valid) {
                    return Err(invalid(format!(
                        concat!("invalid ", stringify!($name), " {:?}: expected ", $expected),
                        value
                    )));
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
identifier!(
    EntityId,
    entity_id_byte,
    "ASCII lowercase letters, digits and hyphens"
);
identifier!(
    RecordId,
    record_id_byte,
    "ASCII letters, digits and hyphens"
);
identifier!(StoreId, record_id_byte, "ASCII letters, digits and hyphens");

impl EntityId {
    pub fn generate(prefix: &str) -> Self {
        const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
        let suffix: String = (0..6)
            .map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char)
            .collect();
        Self(format!("{prefix}-{suffix}"))
    }
}
impl RecordId {
    pub fn generate() -> Self {
        Self(format!("record-{:032x}", rand::random::<u128>()))
    }
}
impl StoreId {
    pub fn generate() -> Self {
        Self(format!("store-{:032x}", rand::random::<u128>()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lifecycle {
    Undecided,
    NotStarted,
    InProgress,
    Completed,
    Cancelled,
}
impl Lifecycle {
    pub fn editable(self) -> bool {
        matches!(self, Self::Undecided | Self::NotStarted | Self::InProgress)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    Accept,
    Withdraw,
    Start,
    Release,
    Complete,
    Cancel,
    Reconsider,
    Reopen,
}
impl Operation {
    /// The basic transition of an Issue. Groups share the states but not every operation;
    /// `apply_as` adds the kind-specific rules and is the entry point for callers.
    pub(crate) fn apply(self, before: Lifecycle) -> Result<Lifecycle> {
        use Lifecycle::*;
        match (self, before) {
            (Self::Accept, Undecided) => Ok(NotStarted),
            (Self::Withdraw, NotStarted) => Ok(Undecided),
            (Self::Start, NotStarted) => Ok(InProgress),
            (Self::Release, InProgress) => Ok(NotStarted),
            (Self::Complete, InProgress) => Ok(Completed),
            (Self::Cancel, Undecided | NotStarted | InProgress) => Ok(Cancelled),
            (Self::Reconsider, Cancelled) => Ok(Undecided),
            (Self::Reopen, Completed) => Ok(NotStarted),
            _ => Err(invalid(format!("cannot {self:?} from {before:?}"))),
        }
    }
    /// A Group is never started or released: its InProgress is derived from the Issues below
    /// it, so it completes from NotStarted and is cancelled from Undecided or NotStarted.
    pub fn apply_as(self, kind: Kind, before: Lifecycle) -> Result<Lifecycle> {
        use Lifecycle::*;
        match (kind, self, before) {
            (Kind::Issue, _, _) => self.apply(before),
            (Kind::Group, Self::Start, _) => Err(invalid(
                "a Group is not started directly: it is InProgress while a direct child is InProgress or Completed, and its saved lifecycle does not change",
            )),
            (Kind::Group, Self::Release, _) => Err(invalid(
                "a Group is not released directly: it stops being InProgress when no direct child is InProgress or Completed, and its saved lifecycle does not change",
            )),
            (Kind::Group, Self::Complete, NotStarted) => Ok(Completed),
            (Kind::Group, Self::Complete | Self::Cancel, InProgress) => {
                Err(invalid(format!("cannot {self:?} a Group from {before:?}")))
            }
            (Kind::Group, _, _) => self.apply(before),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Issue,
    Group,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorder {
    pub actor: String,
    #[serde(deserialize_with = "super::codec::deserialize_recorder_data")]
    pub data: BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub at: DateTime<Utc>,
    pub recorder: Option<Recorder>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Current {
    pub title: String,
    pub description: String,
    pub lifecycle: Lifecycle,
    /// None means no condition. The core never executes a stored command.
    pub condition: Option<String>,
    pub parent: Option<EntityId>,
    pub dependencies: BTreeSet<EntityId>,
}
/// Titles and reasons are shown inside one line. A value that needs line breaks, control
/// characters or this much room belongs in the description or a Note, and accepting it here
/// would store a caller's mistake instead of reporting it.
pub const TITLE_LIMIT: usize = 200;
pub const REASON_LIMIT: usize = 500;
pub(crate) fn validate_line(field: &str, value: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("empty {field}")));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} contains a line break or control character"
        )));
    }
    let length = value.chars().count();
    if length > limit {
        return Err(invalid(format!(
            "{field} has {length} characters; the limit is {limit}"
        )));
    }
    Ok(())
}
pub(crate) fn validate_reason(reason: &Option<String>) -> Result<()> {
    reason
        .as_deref()
        .map_or(Ok(()), |text| validate_line("reason", text, REASON_LIMIT))
}
impl Current {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_line("title", &self.title, TITLE_LIMIT)?;
        if self.condition.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(invalid("empty condition command"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: EntityId,
    pub kind: Kind,
    pub created_at: DateTime<Utc>,
    pub root: RecordId,
    pub head: RecordId,
    pub current: Current,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub head: RecordId,
    pub current: Current,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StateEvent {
    Created {
        kind: Kind,
        initial: Lifecycle,
    },
    Transition {
        operation: Operation,
        before: Lifecycle,
        after: Lifecycle,
        reason: Option<String>,
    },
    /// Each input is a current Entity value, not a replay of its branch's edits.
    Integration {
        inputs: Vec<Candidate>,
        selected: usize,
        reason: Option<String>,
    },
}
impl StateEvent {
    pub fn result(&self) -> Result<Lifecycle> {
        match self {
            Self::Created { initial, .. } => Ok(*initial),
            Self::Transition { after, .. } => Ok(*after),
            Self::Integration {
                inputs, selected, ..
            } => inputs
                .get(*selected)
                .map(|c| c.current.lifecycle)
                .ok_or_else(|| invalid("missing integration selection")),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateRecord {
    pub id: RecordId,
    pub entity: EntityId,
    pub parents: BTreeSet<RecordId>,
    pub context: Context,
    pub event: StateEvent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub id: RecordId,
    pub entity: EntityId,
    pub parents: BTreeSet<RecordId>,
    pub context: Context,
    pub body: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}
