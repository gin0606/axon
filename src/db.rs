//! SQLite への永続化。DB は git 管理外に置き、worktree 間で共有する。

use crate::domain::*;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("git リポジトリの中ではありません")]
    NotInRepo,
    #[error("axon が初期化されていません。`axon init` を実行してください")]
    NotInitialized,
    #[error("すでに初期化されています: {0}")]
    AlreadyInitialized(PathBuf),
    #[error("保存された値を読めません: {0}")]
    Parse(#[from] ParseError),
    #[error("{0} は存在しません")]
    NoSuchIssue(String),
    #[error("{id} は複数の issue に一致します: {candidates}")]
    AmbiguousId { id: String, candidates: String },
    #[error("{id} は着手できません ({reason})")]
    CannotClaim { id: String, reason: String },
}

pub type Result<T> = std::result::Result<T, DbError>;

const SCHEMA: &str = r#"
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE issues (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  description TEXT,

  progress    TEXT NOT NULL CHECK (progress   IN ('not_started','in_progress','ended')),
  commitment  TEXT NOT NULL CHECK (commitment IN ('undecided','accepted','rejected')),

  cond_kind   TEXT CHECK (cond_kind IN ('date','after_issue')),
  cond_date   TEXT,
  cond_ref    TEXT,

  claimed_actor   TEXT,
  claimed_session TEXT,
  claimed_pid     INTEGER,
  claimed_at      TEXT,

  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,

  CHECK (
    (progress =  'in_progress' AND claimed_session IS NOT NULL) OR
    (progress <> 'in_progress' AND claimed_session IS NULL)
  ),
  CHECK (
    (cond_kind IS NULL         AND cond_date IS NULL     AND cond_ref IS NULL) OR
    (cond_kind =  'date'       AND cond_date IS NOT NULL AND cond_ref IS NULL) OR
    (cond_kind =  'after_issue' AND cond_ref IS NOT NULL AND cond_date IS NULL)
  )
);

CREATE TABLE issue_deps (
  issue_id      TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  PRIMARY KEY (issue_id, depends_on_id),
  CHECK (issue_id <> depends_on_id)
);
"#;

/// worktree から呼ばれても同じ DB を指すよう、git の共通ディレクトリを基準にする。
fn repo_root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|_| DbError::NotInRepo)?;
    if !out.status.success() {
        return Err(DbError::NotInRepo);
    }
    let git_dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    git_dir.parent().map(Path::to_path_buf).ok_or(DbError::NotInRepo)
}

fn db_path() -> Result<PathBuf> {
    Ok(repo_root()?.join(".axon").join("axon.db"))
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn init(prefix: &str) -> Result<PathBuf> {
        let path = db_path()?;
        if path.exists() {
            return Err(DbError::AlreadyInitialized(path));
        }
        std::fs::create_dir_all(path.parent().expect("親ディレクトリがある"))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA)?;
        conn.execute("INSERT INTO meta (key, value) VALUES ('prefix', ?1)", params![prefix])?;
        Ok(path)
    }

    pub fn open() -> Result<Self> {
        let path = db_path()?;
        if !path.exists() {
            return Err(DbError::NotInitialized);
        }
        let conn = Connection::open(&path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(Store { conn })
    }

    pub fn prefix(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'prefix'", [], |r| r.get(0))?)
    }

    pub fn insert(&self, issue: &Issue) -> Result<()> {
        let claim = issue.progress.claim();
        self.conn.execute(
            "INSERT INTO issues (id, title, description, progress, commitment,
                                 cond_kind, cond_date, cond_ref,
                                 claimed_actor, claimed_session, claimed_pid, claimed_at,
                                 created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                issue.id.as_str(),
                issue.title,
                issue.description,
                issue.progress.as_db(),
                issue.commitment.as_db(),
                issue.condition.as_ref().map(|c| c.kind_db()),
                cond_date(&issue.condition),
                cond_ref(&issue.condition),
                claim.map(|c| c.actor.clone()),
                claim.map(|c| c.session.clone()),
                claim.map(|c| c.pid),
                claim.map(|c| c.at.to_rfc3339()),
                issue.created_at.to_rfc3339(),
                issue.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<Issue>> {
        read_all(&self.conn)
    }

    pub fn deps(&self) -> Result<Vec<(IssueId, IssueId)>> {
        read_deps(&self.conn)
    }

    /// 前方一致で 1 件に定まるときだけ解決する。曖昧なら候補を返して失敗する。
    pub fn resolve_id(&self, input: &str) -> Result<IssueId> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM issues WHERE id = ?1 OR id LIKE '%-' || ?1")?;
        let ids: Vec<String> = stmt
            .query_map(params![input], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match ids.len() {
            0 => Err(DbError::NoSuchIssue(input.to_string())),
            1 => Ok(IssueId::from_stored(&ids[0])),
            _ => Err(DbError::AmbiguousId {
                id: input.to_string(),
                candidates: ids.join(", "),
            }),
        }
    }

    pub fn get(&self, id: &IssueId) -> Result<Issue> {
        let sql = format!("{SELECT_ISSUE} WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let raw = stmt
            .query_row(params![id.as_str()], |r| Ok(RawIssue::from_row(r)))
            .optional()?
            .ok_or_else(|| DbError::NoSuchIssue(id.to_string()))?;
        Ok(raw.into_issue()?)
    }

    pub fn add_dep(&self, issue: &IssueId, depends_on: &IssueId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO issue_deps (issue_id, depends_on_id) VALUES (?1, ?2)",
            params![issue.as_str(), depends_on.as_str()],
        )?;
        Ok(())
    }

    pub fn remove_dep(&self, issue: &IssueId, depends_on: &IssueId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM issue_deps WHERE issue_id = ?1 AND depends_on_id = ?2",
            params![issue.as_str(), depends_on.as_str()],
        )?;
        Ok(())
    }

    /// 状態更新の唯一の経路。履歴を持たせるときは、ここにログ出力を挟む。
    pub fn apply(&self, id: &IssueId, change: Change) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        match change {
            Change::Claim(c) => {
                let n = self.conn.execute(
                    "UPDATE issues
                        SET progress = 'in_progress',
                            claimed_actor = ?2, claimed_session = ?3,
                            claimed_pid = ?4, claimed_at = ?5, updated_at = ?6
                      WHERE id = ?1 AND progress = 'not_started'",
                    params![id.as_str(), c.actor, c.session, c.pid, c.at.to_rfc3339(), now],
                )?;
                if n == 0 {
                    let current = self.get(id)?;
                    let reason = match current.progress.claim() {
                        Some(cl) => format!("{} が着手中", cl.actor),
                        None => format!("状態が{}", current.progress.label()),
                    };
                    return Err(DbError::CannotClaim { id: id.to_string(), reason });
                }
            }
            Change::End => {
                self.conn.execute(
                    "UPDATE issues
                        SET progress = 'ended',
                            claimed_actor = NULL, claimed_session = NULL,
                            claimed_pid = NULL, claimed_at = NULL, updated_at = ?2
                      WHERE id = ?1",
                    params![id.as_str(), now],
                )?;
            }
            Change::Decide(c) => {
                self.conn.execute(
                    "UPDATE issues SET commitment = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id.as_str(), c.as_db(), now],
                )?;
            }
            Change::SetCondition(cond) => {
                self.conn.execute(
                    "UPDATE issues SET cond_kind = ?2, cond_date = ?3, cond_ref = ?4, updated_at = ?5
                      WHERE id = ?1",
                    params![
                        id.as_str(),
                        cond.as_ref().map(|c| c.kind_db()),
                        cond_date(&cond),
                        cond_ref(&cond),
                        now
                    ],
                )?;
            }
        }
        Ok(())
    }

    /// ready から 1 件取って着手するまでを 1 トランザクションで行う。
    pub fn claim_next(&mut self, claim: Claim) -> Result<Option<Issue>> {
        use rusqlite::TransactionBehavior;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let issues = read_all(&tx)?;
        let deps = read_deps(&tx)?;
        let view = crate::derived::View::new(issues, deps);
        let picked = view.ready().first().map(|i| (*i).clone());

        if let Some(ref issue) = picked {
            tx.execute(
                "UPDATE issues
                    SET progress = 'in_progress',
                        claimed_actor = ?2, claimed_session = ?3,
                        claimed_pid = ?4, claimed_at = ?5, updated_at = ?5
                  WHERE id = ?1",
                params![
                    issue.id.as_str(),
                    claim.actor,
                    claim.session,
                    claim.pid,
                    claim.at.to_rfc3339()
                ],
            )?;
        }
        tx.commit()?;
        Ok(picked)
    }
}

/// 状態を変える操作。`Store::apply` 以外から状態を書き換えない。
pub enum Change {
    Claim(Claim),
    End,
    Decide(Commitment),
    SetCondition(Option<Condition>),
}

const SELECT_ISSUE: &str = "SELECT id, title, description, progress, commitment,
                                   cond_kind, cond_date, cond_ref,
                                   claimed_actor, claimed_session, claimed_pid, claimed_at,
                                   created_at, updated_at
                            FROM issues";

fn read_all(conn: &Connection) -> Result<Vec<Issue>> {
    let sql = format!("{SELECT_ISSUE} ORDER BY created_at");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok(RawIssue::from_row(r)))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?.into_issue()?);
    }
    Ok(out)
}

fn read_deps(conn: &Connection) -> Result<Vec<(IssueId, IssueId)>> {
    let mut stmt = conn.prepare("SELECT issue_id, depends_on_id FROM issue_deps")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            IssueId::from_stored(&r.get::<_, String>(0)?),
            IssueId::from_stored(&r.get::<_, String>(1)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn cond_date(c: &Option<Condition>) -> Option<String> {
    match c {
        Some(Condition::At(d)) => Some(d.to_string()),
        _ => None,
    }
}

fn cond_ref(c: &Option<Condition>) -> Option<String> {
    match c {
        Some(Condition::AfterIssue(id)) => Some(id.as_str().to_string()),
        _ => None,
    }
}

/// SQLite の行をそのまま受けた形。ここが型への境界になる。
struct RawIssue {
    id: String,
    title: String,
    description: Option<String>,
    progress: String,
    commitment: String,
    cond_kind: Option<String>,
    cond_date: Option<String>,
    cond_ref: Option<String>,
    claimed_actor: Option<String>,
    claimed_session: Option<String>,
    claimed_pid: Option<i32>,
    claimed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl RawIssue {
    fn from_row(r: &rusqlite::Row<'_>) -> Self {
        RawIssue {
            id: r.get_unwrap(0),
            title: r.get_unwrap(1),
            description: r.get_unwrap(2),
            progress: r.get_unwrap(3),
            commitment: r.get_unwrap(4),
            cond_kind: r.get_unwrap(5),
            cond_date: r.get_unwrap(6),
            cond_ref: r.get_unwrap(7),
            claimed_actor: r.get_unwrap(8),
            claimed_session: r.get_unwrap(9),
            claimed_pid: r.get_unwrap(10),
            claimed_at: r.get_unwrap(11),
            created_at: r.get_unwrap(12),
            updated_at: r.get_unwrap(13),
        }
    }

    fn into_issue(self) -> std::result::Result<Issue, ParseError> {
        let claim = match (self.claimed_actor, self.claimed_session, self.claimed_pid, self.claimed_at) {
            (Some(actor), Some(session), Some(pid), Some(at)) => Some(Claim {
                actor,
                session,
                pid,
                at: parse_ts(&at),
            }),
            _ => None,
        };
        let condition = match self.cond_kind {
            Some(kind) => Some(Condition::from_db(
                &kind,
                self.cond_date.as_deref(),
                self.cond_ref.as_deref(),
            )?),
            None => None,
        };
        Ok(Issue {
            id: IssueId::from_stored(&self.id),
            title: self.title,
            description: self.description,
            progress: Progress::from_db(&self.progress, claim)?,
            commitment: Commitment::from_db(&self.commitment)?,
            condition,
            created_at: parse_ts(&self.created_at),
            updated_at: parse_ts(&self.updated_at),
        })
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
