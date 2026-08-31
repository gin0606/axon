//! 状態モデルの型。`docs/axes.md` の A / B / C と依存に対応する。

use chrono::{DateTime, NaiveDate, Utc};
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("不正な進行状態: {0}")]
    Progress(String),
    #[error("不正な採否: {0}")]
    Commitment(String),
    #[error("不正な再浮上条件: {0}")]
    Condition(String),
    #[error("着手中の issue に claim がない")]
    MissingClaim,
    #[error("着手していない issue に claim がある")]
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
            Progress::NotStarted => "未着手",
            Progress::InProgress(_) => "着手中",
            Progress::Ended => "終了",
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
pub enum Commitment {
    Undecided,
    Accepted,
    Rejected,
}

impl Commitment {
    pub fn as_db(&self) -> &'static str {
        match self {
            Commitment::Undecided => "undecided",
            Commitment::Accepted => "accepted",
            Commitment::Rejected => "rejected",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Commitment::Undecided => "未判断",
            Commitment::Accepted => "採用",
            Commitment::Rejected => "不採用",
        }
    }

    pub fn from_db(s: &str) -> Result<Self, ParseError> {
        match s {
            "undecided" => Ok(Commitment::Undecided),
            "accepted" => Ok(Commitment::Accepted),
            "rejected" => Ok(Commitment::Rejected),
            other => Err(ParseError::Commitment(other.to_string())),
        }
    }
}

/// C: 時期。いつ再び意識に上げるかの条件で、状態ではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    At(NaiveDate),
    AfterIssue(IssueId),
}

impl Condition {
    pub fn kind_db(&self) -> &'static str {
        match self {
            Condition::At(_) => "date",
            Condition::AfterIssue(_) => "after_issue",
        }
    }

    pub fn from_db(kind: &str, date: Option<&str>, reference: Option<&str>) -> Result<Self, ParseError> {
        match (kind, date, reference) {
            ("date", Some(d), None) => d
                .parse::<NaiveDate>()
                .map(Condition::At)
                .map_err(|_| ParseError::Condition(format!("日付として読めない: {d}"))),
            ("after_issue", None, Some(r)) => Ok(Condition::AfterIssue(IssueId::from_stored(r))),
            _ => Err(ParseError::Condition(format!(
                "kind={kind} date={date:?} ref={reference:?}"
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

impl IssueId {
    pub fn generate(prefix: &str) -> Self {
        let suffix: String = (0..ID_LEN)
            .map(|_| ID_ALPHABET[rand::random_range(0..ID_ALPHABET.len())] as char)
            .collect();
        IssueId(format!("{prefix}-{suffix}"))
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

#[derive(Debug, Clone)]
pub struct Issue {
    pub id: IssueId,
    pub title: String,
    pub description: Option<String>,
    pub progress: Progress,
    pub commitment: Commitment,
    pub condition: Option<Condition>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Issue {
    /// 終端 = これ以上成果物が増えない。依存の解除判定に使う。
    pub fn is_terminal(&self) -> bool {
        matches!(self.progress, Progress::Ended) || self.commitment == Commitment::Rejected
    }
}
