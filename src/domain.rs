//! 状態モデルの型。`docs/axes.md` の A / B / C と依存に対応する。

use chrono::{DateTime, NaiveDate, Utc};
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid Progress value: {0}")]
    Progress(String),
    #[error("invalid Disposition value: {0}")]
    Disposition(String),
    #[error("invalid resurface condition: {0}")]
    ResurfaceCondition(String),
    #[error("an InProgress issue has no claim")]
    MissingClaim,
    #[error("an issue that is not InProgress has a claim")]
    UnexpectedClaim,
}

/// 誰がその issue を握っているか。`InProgress` のときのみ存在する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub actor: String,
    pub session: String,
    pub pid: i32,
    pub at: DateTime<Utc>,
}

/// A: 進行。終端は「やり切った」ではなく「もう進めない」を意味する。
// InProgress は軸の語彙そのもので、Progress を外した名前は意味が変わる。
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
            Progress::NotStarted => "not_started",
            Progress::InProgress(_) => "in_progress",
            Progress::Ended => "ended",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Progress::NotStarted => "NotStarted",
            Progress::InProgress(_) => "InProgress",
            Progress::Ended => "Ended",
        }
    }

    pub fn claim(&self) -> Option<&Claim> {
        match self {
            Progress::InProgress(c) => Some(c),
            _ => None,
        }
    }

    /// DB から読んだ値を型に戻す。claim の有無が状態と食い違っていたら弾く。
    pub fn from_db(s: &str, claim: Option<Claim>) -> Result<Self, ParseError> {
        match (s, claim) {
            ("not_started", None) => Ok(Progress::NotStarted),
            ("ended", None) => Ok(Progress::Ended),
            ("in_progress", Some(c)) => Ok(Progress::InProgress(c)),
            ("in_progress", None) => Err(ParseError::MissingClaim),
            ("not_started" | "ended", Some(_)) => Err(ParseError::UnexpectedClaim),
            (other, _) => Err(ParseError::Progress(other.to_string())),
        }
    }
}

/// B: 採否。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Undecided,
    Accepted,
    Rejected,
}

impl Disposition {
    pub fn as_db(&self) -> &'static str {
        match self {
            Disposition::Undecided => "undecided",
            Disposition::Accepted => "accepted",
            Disposition::Rejected => "rejected",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Disposition::Undecided => "Undecided",
            Disposition::Accepted => "Accepted",
            Disposition::Rejected => "Rejected",
        }
    }

    pub fn from_db(s: &str) -> Result<Self, ParseError> {
        match s {
            "undecided" => Ok(Disposition::Undecided),
            "accepted" => Ok(Disposition::Accepted),
            "rejected" => Ok(Disposition::Rejected),
            other => Err(ParseError::Disposition(other.to_string())),
        }
    }
}

/// C: 時期。いつ再び意識に上げるかの条件で、状態ではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResurfaceCondition {
    Always,
    AtDate(NaiveDate),
    AfterIssue(IssueId),
}

impl ResurfaceCondition {
    pub fn kind_db(&self) -> Option<&'static str> {
        match self {
            ResurfaceCondition::Always => None,
            ResurfaceCondition::AtDate(_) => Some("date"),
            ResurfaceCondition::AfterIssue(_) => Some("after_issue"),
        }
    }

    pub fn from_db(
        kind: Option<&str>,
        date: Option<&str>,
        reference: Option<&str>,
    ) -> Result<Self, ParseError> {
        match (kind, date, reference) {
            (None, None, None) => Ok(ResurfaceCondition::Always),
            (Some("date"), Some(d), None) => d
                .parse::<NaiveDate>()
                .map(ResurfaceCondition::AtDate)
                .map_err(|_| ParseError::ResurfaceCondition(format!("invalid date: {d}"))),
            (Some("after_issue"), None, Some(r)) => {
                Ok(ResurfaceCondition::AfterIssue(IssueId::from_stored(r)))
            }
            _ => Err(ParseError::ResurfaceCondition(format!(
                "kind={kind:?} date={date:?} ref={reference:?}"
            ))),
        }
    }
}

/// `<prefix>-<ランダム 6 文字>`。連番にしないのは、番号から順序を推測させないため。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IssueId(String);

/// Crockford Base32 から紛らわしい I / L / O / U を除いたもの。
const ID_ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
const ID_LEN: usize = 6;

fn random_suffix() -> String {
    (0..ID_LEN)
        .map(|_| ID_ALPHABET[rand::random_range(0..ID_ALPHABET.len())] as char)
        .collect()
}

impl IssueId {
    pub fn generate(prefix: &str) -> Self {
        IssueId(format!("{prefix}-{}", random_suffix()))
    }

    /// DB に入っている値をそのまま型に載せる。
    pub fn from_stored(s: &str) -> Self {
        IssueId(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IssueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// グループの内部識別子。人間は slug で参照するため、これは表に出さない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(String);

impl GroupId {
    pub fn generate() -> Self {
        GroupId(format!("g-{}", random_suffix()))
    }

    pub fn from_stored(s: &str) -> Self {
        GroupId(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 機能群。状態を持たず、進捗も完了も子から導出する。
#[derive(Debug, Clone)]
pub struct Group {
    pub id: GroupId,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub parent: Option<GroupId>,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub id: IssueId,
    pub title: String,
    pub description: Option<String>,
    pub progress: Progress,
    pub disposition: Disposition,
    pub resurface_condition: ResurfaceCondition,
    pub group: Option<GroupId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Issue {
    /// 終端 = これ以上成果物が増えない。依存の解除判定に使う。
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
            session: "s".to_string(),
            pid: 1,
            at: Utc::now(),
        }
    }

    fn issue(progress: Progress, disposition: Disposition) -> Issue {
        let now = Utc::now();
        Issue {
            id: IssueId::from_stored("t-1"),
            title: "t".to_string(),
            description: None,
            progress,
            disposition,
            resurface_condition: ResurfaceCondition::Always,
            group: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// 「着手中なら claim がある」は型で表せているので、外から来た値を載せる境界だけが検査点。
    #[test]
    fn progress_from_db_requires_claim_to_match_state() {
        assert!(matches!(
            Progress::from_db("not_started", None),
            Ok(Progress::NotStarted)
        ));
        assert!(matches!(
            Progress::from_db("ended", None),
            Ok(Progress::Ended)
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
            Progress::from_db("not_started", Some(claim())),
            Err(ParseError::UnexpectedClaim)
        ));
        assert!(matches!(
            Progress::from_db("ended", Some(claim())),
            Err(ParseError::UnexpectedClaim)
        ));
        assert!(matches!(
            Progress::from_db("bogus", None),
            Err(ParseError::Progress(_))
        ));
    }

    #[test]
    fn progress_round_trips_through_db_representation() {
        for p in [
            Progress::NotStarted,
            Progress::Ended,
            Progress::InProgress(claim()),
        ] {
            assert_eq!(Progress::from_db(p.as_db(), p.claim().cloned()).unwrap(), p);
        }
    }

    #[test]
    fn disposition_round_trips_and_rejects_unknown() {
        for c in [
            Disposition::Undecided,
            Disposition::Accepted,
            Disposition::Rejected,
        ] {
            assert_eq!(Disposition::from_db(c.as_db()).unwrap(), c);
        }
        assert!(matches!(
            Disposition::from_db("bogus"),
            Err(ParseError::Disposition(_))
        ));
    }

    #[test]
    fn condition_from_db_needs_columns_matching_its_kind() {
        let always = ResurfaceCondition::from_db(None, None, None).unwrap();
        assert_eq!(always, ResurfaceCondition::Always);
        assert_eq!(always.kind_db(), None);

        let date = ResurfaceCondition::from_db(Some("date"), Some("2026-01-31"), None).unwrap();
        assert_eq!(
            date,
            ResurfaceCondition::AtDate("2026-01-31".parse().unwrap())
        );
        assert_eq!(date.kind_db(), Some("date"));

        let after = ResurfaceCondition::from_db(Some("after_issue"), None, Some("t-1")).unwrap();
        assert_eq!(
            after,
            ResurfaceCondition::AfterIssue(IssueId::from_stored("t-1"))
        );
        assert_eq!(after.kind_db(), Some("after_issue"));

        assert!(ResurfaceCondition::from_db(Some("date"), None, Some("t-1")).is_err());
        assert!(ResurfaceCondition::from_db(Some("date"), None, None).is_err());
        assert!(
            ResurfaceCondition::from_db(Some("after_issue"), Some("2026-01-31"), None).is_err()
        );
        assert!(ResurfaceCondition::from_db(Some("date"), Some("not-a-date"), None).is_err());
        assert!(ResurfaceCondition::from_db(Some("bogus"), None, None).is_err());
    }

    #[test]
    fn generated_ids_use_the_restricted_alphabet() {
        for _ in 0..200 {
            let id = IssueId::generate("axon");
            let suffix = id.as_str().strip_prefix("axon-").expect("接頭辞が付く");
            assert_eq!(suffix.len(), ID_LEN);
            assert!(suffix.bytes().all(|b| ID_ALPHABET.contains(&b)), "{id}");
            assert!(!suffix.contains(['i', 'l', 'o', 'u']));
        }
        let g = GroupId::generate();
        assert_eq!(
            g.as_str().strip_prefix("g-").expect("接頭辞が付く").len(),
            ID_LEN
        );
    }

    #[test]
    fn terminal_covers_both_axes() {
        assert!(issue(Progress::Ended, Disposition::Accepted).is_terminal());
        assert!(issue(Progress::NotStarted, Disposition::Rejected).is_terminal());
        assert!(issue(Progress::Ended, Disposition::Rejected).is_terminal());
        assert!(!issue(Progress::NotStarted, Disposition::Accepted).is_terminal());
        assert!(!issue(Progress::NotStarted, Disposition::Undecided).is_terminal());
        assert!(!issue(Progress::InProgress(claim()), Disposition::Accepted).is_terminal());
    }
}
