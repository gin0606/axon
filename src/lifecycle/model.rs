use super::{Result, invalid};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! identifier {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn generate() -> Self {
                Self(format!("{}-{:032x}", $prefix, rand::random::<u128>()))
            }
        }
        impl TryFrom<String> for $name {
            type Error = super::Error;
            fn try_from(value: String) -> Result<Self> {
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(invalid(concat!("invalid ", stringify!($name))));
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
identifier!(EntityId, "entity");
identifier!(RecordId, "record");
identifier!(StoreId, "store");

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
}
impl Operation {
    pub fn apply(self, before: Lifecycle) -> Result<Lifecycle> {
        use Lifecycle::*;
        match (self, before) {
            (Self::Accept, Undecided) => Ok(NotStarted),
            (Self::Withdraw, NotStarted) => Ok(Undecided),
            (Self::Start, NotStarted) => Ok(InProgress),
            (Self::Release, InProgress) => Ok(NotStarted),
            (Self::Complete, InProgress) => Ok(Completed),
            (Self::Cancel, Undecided | NotStarted | InProgress) => Ok(Cancelled),
            (Self::Reconsider, Cancelled) => Ok(Undecided),
            _ => Err(invalid(format!("cannot {self:?} from {before:?}"))),
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
impl Current {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(invalid("empty title"));
        }
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
