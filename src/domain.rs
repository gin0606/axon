use chrono::{DateTime, NaiveDate, Utc};
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid Entity kind: {0}")]
    Kind(String),
    #[error("invalid Progress value: {0}")]
    Progress(String),
    #[error("invalid Disposition value: {0}")]
    Disposition(String),
    #[error("invalid resurface condition: {0}")]
    ResurfaceCondition(String),
    #[error("an InProgress entity has no claim")]
    MissingClaim,
    #[error("an entity that is not InProgress has a claim")]
    UnexpectedClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Issue,
    Group,
}

impl EntityKind {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Group => "group",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Issue => "Issue",
            Self::Group => "Group",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, ParseError> {
        match value {
            "issue" => Ok(Self::Issue),
            "group" => Ok(Self::Group),
            other => Err(ParseError::Kind(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub actor: String,
    pub worktree: String,
    pub at: DateTime<Utc>,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    NotStarted,
    InProgress(Claim),
    Ended,
}

impl Progress {
    pub fn as_db(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress(_) => "in_progress",
            Self::Ended => "ended",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::NotStarted => "NotStarted",
            Self::InProgress(_) => "InProgress",
            Self::Ended => "Ended",
        }
    }

    pub fn claim(&self) -> Option<&Claim> {
        match self {
            Self::InProgress(claim) => Some(claim),
            _ => None,
        }
    }

    pub fn from_db(value: &str, claim: Option<Claim>) -> Result<Self, ParseError> {
        match (value, claim) {
            ("not_started", None) => Ok(Self::NotStarted),
            ("in_progress", Some(claim)) => Ok(Self::InProgress(claim)),
            ("ended", None) => Ok(Self::Ended),
            ("in_progress", None) => Err(ParseError::MissingClaim),
            ("not_started" | "ended", Some(_)) => Err(ParseError::UnexpectedClaim),
            (other, _) => Err(ParseError::Progress(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Undecided,
    Accepted,
    Rejected,
}

impl Disposition {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Undecided => "undecided",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Undecided => "Undecided",
            Self::Accepted => "Accepted",
            Self::Rejected => "Rejected",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, ParseError> {
        match value {
            "undecided" => Ok(Self::Undecided),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            other => Err(ParseError::Disposition(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResurfaceCondition {
    Always,
    AtDate(NaiveDate),
    AfterEntity(EntityId),
}

impl ResurfaceCondition {
    pub fn kind_db(&self) -> Option<&'static str> {
        match self {
            Self::Always => None,
            Self::AtDate(_) => Some("date"),
            Self::AfterEntity(_) => Some("after_entity"),
        }
    }

    pub fn from_db(
        kind: Option<&str>,
        date: Option<&str>,
        reference: Option<&str>,
    ) -> Result<Self, ParseError> {
        match (kind, date, reference) {
            (None, None, None) => Ok(Self::Always),
            (Some("date"), Some(date), None) => date
                .parse::<NaiveDate>()
                .map(Self::AtDate)
                .map_err(|_| ParseError::ResurfaceCondition(format!("invalid date: {date}"))),
            (Some("after_entity"), None, Some(reference)) => {
                Ok(Self::AfterEntity(EntityId::from_stored(reference)))
            }
            values => Err(ParseError::ResurfaceCondition(format!(
                "kind={:?} date={:?} ref={:?}",
                values.0, values.1, values.2
            ))),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Always => "Always".to_string(),
            Self::AtDate(date) => format!("AtDate({date})"),
            Self::AfterEntity(id) => format!("AfterEntity({id})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(String);

const ID_ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
const ID_LEN: usize = 6;

impl EntityId {
    pub fn generate(prefix: &str) -> Self {
        let suffix: String = (0..ID_LEN)
            .map(|_| ID_ALPHABET[rand::random_range(0..ID_ALPHABET.len())] as char)
            .collect();
        Self(format!("{prefix}-{suffix}"))
    }

    pub fn from_stored(value: &str) -> Self {
        Self(value.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub title: String,
    pub description: Option<String>,
    pub progress: Progress,
    pub disposition: Disposition,
    pub resurface_condition: ResurfaceCondition,
    pub parent: Option<EntityId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Entity {
    pub fn is_terminal(&self) -> bool {
        matches!(self.progress, Progress::Ended) || self.disposition == Disposition::Rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim() -> Claim {
        Claim {
            actor: "tester".to_string(),
            worktree: "/worktree".to_string(),
            at: Utc::now(),
        }
    }

    #[test]
    fn progress_requires_claim_to_match_state() {
        assert!(matches!(
            Progress::from_db("not_started", None),
            Ok(Progress::NotStarted)
        ));
        assert!(matches!(
            Progress::from_db("in_progress", Some(claim())),
            Ok(Progress::InProgress(_))
        ));
        assert!(matches!(
            Progress::from_db("in_progress", None),
            Err(ParseError::MissingClaim)
        ));
        assert!(matches!(
            Progress::from_db("ended", Some(claim())),
            Err(ParseError::UnexpectedClaim)
        ));
    }

    #[test]
    fn generated_ids_share_one_restricted_namespace() {
        for _ in 0..100 {
            let id = EntityId::generate("axon");
            let suffix = id.as_str().strip_prefix("axon-").unwrap();
            assert_eq!(suffix.len(), ID_LEN);
            assert!(suffix.bytes().all(|byte| ID_ALPHABET.contains(&byte)));
        }
    }

    #[test]
    fn resurface_condition_round_trips() {
        let after = ResurfaceCondition::from_db(Some("after_entity"), None, Some("t-1")).unwrap();
        assert_eq!(
            after,
            ResurfaceCondition::AfterEntity(EntityId::from_stored("t-1"))
        );
        assert_eq!(after.kind_db(), Some("after_entity"));
        assert!(ResurfaceCondition::from_db(Some("date"), None, None).is_err());
    }
}
