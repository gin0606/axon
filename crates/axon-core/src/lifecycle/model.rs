use super::{Result, invalid};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;
use std::fmt;

/// Entity IDs appear on command lines, so they stay free of characters a shell would quote.
fn entity_id_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}
fn store_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}
macro_rules! identifier {
    ($name:ident, $valid:path, $expected:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }
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
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
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
identifier!(StoreId, store_id_byte, "ASCII letters, digits and hyphens");

impl StoreId {
    pub fn generate() -> Self {
        Self(format!("store-{:032x}", rand::random::<u128>()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub(crate) fn next_as(self, kind: Kind, before: Lifecycle) -> Option<Lifecycle> {
        use Lifecycle::*;
        match (kind, self, before) {
            (Kind::Group, Self::Start | Self::Release, _) => None,
            (Kind::Group, Self::Complete, NotStarted) => Some(Completed),
            (Kind::Group, Self::Complete | Self::Cancel, InProgress) => None,
            (_, Self::Accept, Undecided) => Some(NotStarted),
            (_, Self::Withdraw, NotStarted) => Some(Undecided),
            (_, Self::Start, NotStarted) => Some(InProgress),
            (_, Self::Release, InProgress) => Some(NotStarted),
            (_, Self::Complete, InProgress) => Some(Completed),
            (_, Self::Cancel, Undecided | NotStarted | InProgress) => Some(Cancelled),
            (_, Self::Reconsider, Cancelled) => Some(Undecided),
            (_, Self::Reopen, Completed) => Some(NotStarted),
            _ => None,
        }
    }
    /// A Group is never started or released: its InProgress is derived from the Issues below
    /// it, so it completes from NotStarted and is cancelled from Undecided or NotStarted.
    pub fn apply_as(self, kind: Kind, before: Lifecycle) -> Result<Lifecycle> {
        self.next_as(kind, before).ok_or_else(|| match (kind, self, before) {
            (Kind::Group, Self::Start, _) => invalid(
                "a Group is not started directly: it is InProgress while a direct child is InProgress or Completed, and its saved lifecycle does not change",
            ),
            (Kind::Group, Self::Release, _) => invalid(
                "a Group is not released directly: it stops being InProgress when no direct child is InProgress or Completed, and its saved lifecycle does not change",
            ),
            (Kind::Group, Self::Complete | Self::Cancel, Lifecycle::InProgress) => {
                invalid(format!("cannot {self:?} a Group from {before:?}"))
            }
            _ => invalid(format!("cannot {self:?} from {before:?}")),
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Issue,
    Group,
}
/// The kind of work an Entity represents, from a fixed set. Every Entity has exactly one; it
/// affects no lifecycle rule, structure, candidate set or situation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    Bug,
    Feat,
    Chore,
    Docs,
    Test,
    Refactor,
    Spike,
}
impl Label {
    /// Every label in the order help and documentation list them.
    pub const ALL: [Label; 7] = [
        Label::Bug,
        Label::Feat,
        Label::Chore,
        Label::Docs,
        Label::Test,
        Label::Refactor,
        Label::Spike,
    ];
    /// The one spelling records, the CLI and declarations use.
    pub fn name(self) -> &'static str {
        match self {
            Label::Bug => "bug",
            Label::Feat => "feat",
            Label::Chore => "chore",
            Label::Docs => "docs",
            Label::Test => "test",
            Label::Refactor => "refactor",
            Label::Spike => "spike",
        }
    }
    /// The label spelled exactly `name`; anything outside the set is rejected.
    pub fn from_name(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|label| label.name() == name)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|label| label.name()).collect();
                invalid(format!(
                    "unknown label {name:?}: expected one of {}",
                    names.join(", ")
                ))
            })
    }
}
impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorder {
    pub actor: String,
    #[serde(deserialize_with = "deserialize_recorder_data")]
    pub data: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub at: DateTime<Utc>,
    pub recorder: Option<Recorder>,
}

/// Recorder metadata is kept as the literal JSON it was written with, so numbers of any
/// precision survive a decode and encode unchanged.
fn deserialize_recorder_data<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries = BTreeMap::<String, Box<RawValue>>::deserialize(deserializer)?;
    entries
        .into_iter()
        .map(|(key, raw)| {
            literal_json(&raw)
                .map(|value| (key, value))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

fn literal_json(raw: &RawValue) -> serde_json::Result<Value> {
    let text = raw.get();
    match text.as_bytes()[0] {
        b'{' => {
            let entries: BTreeMap<String, &RawValue> = serde_json::from_str(text)?;
            entries
                .into_iter()
                .map(|(key, value)| literal_json(value).map(|v| (key, v)))
                .collect::<serde_json::Result<serde_json::Map<_, _>>>()
                .map(Value::Object)
        }
        b'[' => {
            let entries: Vec<&RawValue> = serde_json::from_str(text)?;
            entries
                .into_iter()
                .map(literal_json)
                .collect::<serde_json::Result<Vec<_>>>()
                .map(Value::Array)
        }
        b'"' => serde_json::from_str(text).map(Value::String),
        b't' | b'f' => serde_json::from_str(text).map(Value::Bool),
        b'n' => Ok(Value::Null),
        _ => serde_json::from_str(text).map(Value::Number),
    }
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
