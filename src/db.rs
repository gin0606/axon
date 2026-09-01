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
    #[error("グループ {0} は存在しません")]
    NoSuchGroup(String),
    #[error("{id} は複数の issue に一致します: {candidates}")]
    AmbiguousId { id: String, candidates: String },
    #[error("{id} は着手できません ({reason})")]
    CannotClaim { id: String, reason: String },
}

pub type Result<T> = std::result::Result<T, DbError>;

/// スキーマの版。`user_version` に記録し、開くたびに不足分だけ流す。
const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4];

const SCHEMA_V1: &str = r#"
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

/// グループ。状態を持たない容れ物で、進捗も完了も子から導出する。
const SCHEMA_V2: &str = r#"
CREATE TABLE groups (
  id          TEXT PRIMARY KEY,
  slug        TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  description TEXT,
  parent_id   TEXT REFERENCES groups(id),
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  CHECK (id <> parent_id)
);

CREATE TABLE group_deps (
  group_id      TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, depends_on_id),
  CHECK (group_id <> depends_on_id)
);

ALTER TABLE issues ADD COLUMN group_id TEXT REFERENCES groups(id);
"#;

/// 判断の記録。「なぜやるのか」「なぜやらないのか」「なぜ今やらないのか」を残す。
/// 類似の問題を考えるときや、決定を再考するときに参照する。
const SCHEMA_V3: &str = r#"
CREATE TABLE events (
  id        INTEGER PRIMARY KEY,
  issue_id  TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  field     TEXT NOT NULL,
  old_value TEXT,
  new_value TEXT,
  actor     TEXT NOT NULL,
  reason    TEXT,
  at        TEXT NOT NULL
);

CREATE INDEX idx_events_issue ON events (issue_id, at);
"#;

/// 記録の対象を判断 (採否・時期) に絞ったため、それ以外の記録を落とす。
const SCHEMA_V4: &str = r#"
DELETE FROM events WHERE field NOT IN ('commitment', 'condition');
"#;

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        conn.execute_batch(sql)?;
        conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
    }
    Ok(())
}

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
    git_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or(DbError::NotInRepo)
}

fn db_path() -> Result<PathBuf> {
    Ok(repo_root()?.join(".axon").join("axon.db"))
}

pub struct Store {
    conn: Connection,
}

impl Store {
    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        migrate(&conn)?;
        Ok(Store { conn })
    }

    pub fn init(prefix: &str) -> Result<PathBuf> {
        let path = db_path()?;
        if path.exists() {
            return Err(DbError::AlreadyInitialized(path));
        }
        std::fs::create_dir_all(path.parent().expect("親ディレクトリがある"))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let conn = Connection::open(&path)?;
        migrate(&conn)?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('prefix', ?1)",
            params![prefix],
        )?;
        Ok(path)
    }

    pub fn open() -> Result<Self> {
        let path = db_path()?;
        if !path.exists() {
            return Err(DbError::NotInitialized);
        }
        Self::from_conn(Connection::open(&path)?)
    }

    /// テストが実在の `.axon/` を触らないようにする。
    #[cfg(test)]
    pub fn in_memory(prefix: &str) -> Result<Self> {
        let store = Self::from_conn(Connection::open_in_memory()?)?;
        store.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('prefix', ?1)",
            params![prefix],
        )?;
        Ok(store)
    }

    pub fn prefix(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'prefix'", [], |r| {
                r.get(0)
            })?)
    }

    pub fn insert(&self, issue: &Issue) -> Result<()> {
        let claim = issue.progress.claim();
        self.conn.execute(
            "INSERT INTO issues (id, title, description, progress, commitment,
                                 cond_kind, cond_date, cond_ref,
                                 claimed_actor, claimed_session, claimed_pid, claimed_at,
                                 created_at, updated_at, group_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
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
                issue.group.as_ref().map(|g| g.as_str().to_string()),
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

    pub fn insert_group(&self, g: &Group) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO groups (id, slug, name, description, parent_id, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?6)",
            params![
                g.id.as_str(),
                g.slug,
                g.name,
                g.description,
                g.parent.as_ref().map(|p| p.as_str().to_string()),
                now
            ],
        )?;
        Ok(())
    }

    pub fn groups(&self) -> Result<Vec<Group>> {
        read_groups(&self.conn)
    }

    pub fn group_deps(&self) -> Result<Vec<(GroupId, GroupId)>> {
        read_group_deps(&self.conn)
    }

    pub fn resolve_slug(&self, slug: &str) -> Result<GroupId> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM groups WHERE slug = ?1",
                params![slug],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|s| GroupId::from_stored(&s))
            .ok_or_else(|| DbError::NoSuchGroup(slug.to_string()))
    }

    pub fn add_group_dep(&self, group: &GroupId, depends_on: &GroupId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO group_deps (group_id, depends_on_id) VALUES (?1, ?2)",
            params![group.as_str(), depends_on.as_str()],
        )?;
        Ok(())
    }

    pub fn remove_group_dep(&self, group: &GroupId, depends_on: &GroupId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM group_deps WHERE group_id = ?1 AND depends_on_id = ?2",
            params![group.as_str(), depends_on.as_str()],
        )?;
        Ok(())
    }

    /// 状態更新の唯一の経路。更新と履歴の記録を同じトランザクションで行う。
    pub fn apply(&mut self, id: &IssueId, change: Change, ctx: &Ctx) -> Result<()> {
        let before = self.get(id)?;
        let tx = self.conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        let (field, old, new) = describe(&before, &change);
        let record = records_decision(&change);
        match change {
            Change::Claim(c) => {
                let n = tx.execute(
                    "UPDATE issues
                        SET progress = 'in_progress',
                            claimed_actor = ?2, claimed_session = ?3,
                            claimed_pid = ?4, claimed_at = ?5, updated_at = ?6
                      WHERE id = ?1 AND progress = 'not_started'",
                    params![
                        id.as_str(),
                        c.actor,
                        c.session,
                        c.pid,
                        c.at.to_rfc3339(),
                        now
                    ],
                )?;
                if n == 0 {
                    let reason = match before.progress.claim() {
                        Some(cl) => format!("{} が着手中", cl.actor),
                        None => format!("状態が{}", before.progress.label()),
                    };
                    return Err(DbError::CannotClaim {
                        id: id.to_string(),
                        reason,
                    });
                }
            }
            Change::Release => {
                tx.execute(
                    "UPDATE issues
                        SET progress = 'not_started',
                            claimed_actor = NULL, claimed_session = NULL,
                            claimed_pid = NULL, claimed_at = NULL, updated_at = ?2
                      WHERE id = ?1",
                    params![id.as_str(), now],
                )?;
            }
            Change::End => {
                tx.execute(
                    "UPDATE issues
                        SET progress = 'ended',
                            claimed_actor = NULL, claimed_session = NULL,
                            claimed_pid = NULL, claimed_at = NULL, updated_at = ?2
                      WHERE id = ?1",
                    params![id.as_str(), now],
                )?;
            }
            Change::Decide(c) => {
                tx.execute(
                    "UPDATE issues SET commitment = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id.as_str(), c.as_db(), now],
                )?;
            }
            Change::SetTitle(ref t) => {
                tx.execute(
                    "UPDATE issues SET title = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id.as_str(), t, now],
                )?;
            }
            Change::SetDescription(ref d) => {
                tx.execute(
                    "UPDATE issues SET description = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id.as_str(), d, now],
                )?;
            }
            Change::SetGroup(g) => {
                tx.execute(
                    "UPDATE issues SET group_id = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id.as_str(), g.as_ref().map(|g| g.as_str().to_string()), now],
                )?;
            }
            Change::SetCondition(cond) => {
                tx.execute(
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
        if record {
            log_event(&tx, id, field, old, new, ctx)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn events(&self, id: &IssueId) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT field, old_value, new_value, actor, reason, at
             FROM events WHERE issue_id = ?1 ORDER BY at, id",
        )?;
        let rows = stmt.query_map(params![id.as_str()], |r| {
            Ok(Event {
                field: r.get(0)?,
                old_value: r.get(1)?,
                new_value: r.get(2)?,
                actor: r.get(3)?,
                reason: r.get(4)?,
                at: parse_ts(&r.get::<_, String>(5)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

/// 誰がなぜその操作をしたか。履歴に残す。
pub struct Ctx {
    pub actor: String,
    pub reason: Option<String>,
}

fn log_event(
    conn: &Connection,
    id: &IssueId,
    field: &str,
    old: Option<String>,
    new: Option<String>,
    ctx: &Ctx,
) -> Result<()> {
    conn.execute(
        "INSERT INTO events (issue_id, field, old_value, new_value, actor, reason, at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            id.as_str(),
            field,
            old,
            new,
            ctx.actor,
            ctx.reason,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

pub struct Event {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

/// 状態を変える操作。`Store::apply` 以外から状態を書き換えない。
pub enum Change {
    Claim(Claim),
    /// 着手を取り消して未着手に戻す。放置された claim を人手で解放するために使う。
    Release,
    End,
    Decide(Commitment),
    SetTitle(String),
    SetDescription(Option<String>),
    SetCondition(Option<Condition>),
    SetGroup(Option<GroupId>),
}

const SELECT_ISSUE: &str = "SELECT id, title, description, progress, commitment,
                                   cond_kind, cond_date, cond_ref,
                                   claimed_actor, claimed_session, claimed_pid, claimed_at,
                                   created_at, updated_at, group_id
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

fn read_groups(conn: &Connection) -> Result<Vec<Group>> {
    let mut stmt =
        conn.prepare("SELECT id, slug, name, description, parent_id FROM groups ORDER BY slug")?;
    let rows = stmt.query_map([], |r| {
        Ok(Group {
            id: GroupId::from_stored(&r.get::<_, String>(0)?),
            slug: r.get(1)?,
            name: r.get(2)?,
            description: r.get(3)?,
            parent: r
                .get::<_, Option<String>>(4)?
                .as_deref()
                .map(GroupId::from_stored),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn read_group_deps(conn: &Connection) -> Result<Vec<(GroupId, GroupId)>> {
    let mut stmt = conn.prepare("SELECT group_id, depends_on_id FROM group_deps")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            GroupId::from_stored(&r.get::<_, String>(0)?),
            GroupId::from_stored(&r.get::<_, String>(1)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
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

/// 判断として残すのは採否 (B) と時期 (C) だけ。
/// 進行 (A) や文面の編集は、後から「なぜそう決めたか」を辿る材料にならない。
/// いつ着手して終えたかは claim と updated_at で足りる。
fn records_decision(change: &Change) -> bool {
    matches!(change, Change::Decide(_) | Change::SetCondition(_))
}

fn describe(before: &Issue, change: &Change) -> (&'static str, Option<String>, Option<String>) {
    match change {
        Change::Claim(_) => (
            "progress",
            Some(before.progress.as_db().to_string()),
            Some("in_progress".to_string()),
        ),
        Change::Release => (
            "progress",
            Some(before.progress.as_db().to_string()),
            Some("not_started".to_string()),
        ),
        Change::End => (
            "progress",
            Some(before.progress.as_db().to_string()),
            Some("ended".to_string()),
        ),
        Change::Decide(c) => (
            "commitment",
            Some(before.commitment.as_db().to_string()),
            Some(c.as_db().to_string()),
        ),
        Change::SetTitle(t) => ("title", Some(before.title.clone()), Some(t.clone())),
        Change::SetDescription(d) => (
            "description",
            before.description.as_deref().map(summarize),
            d.as_deref().map(summarize),
        ),
        Change::SetCondition(c) => (
            "condition",
            before.condition.as_ref().map(describe_cond),
            c.as_ref().map(describe_cond),
        ),
        Change::SetGroup(g) => (
            "group",
            before.group.as_ref().map(|g| g.as_str().to_string()),
            g.as_ref().map(|g| g.as_str().to_string()),
        ),
    }
}

/// 本文をそのまま履歴に載せると読みにくいので、先頭だけ残す。
fn summarize(text: &str) -> String {
    let one_line = text.replace('\n', " ");
    let trimmed = one_line.trim();
    let mut out: String = trimmed.chars().take(30).collect();
    if trimmed.chars().count() > 30 {
        out.push('…');
    }
    out
}

fn describe_cond(c: &Condition) -> String {
    match c {
        Condition::At(d) => d.to_string(),
        Condition::AfterIssue(id) => format!("after {id}"),
    }
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
    group_id: Option<String>,
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
            group_id: r.get_unwrap(14),
        }
    }

    fn into_issue(self) -> std::result::Result<Issue, ParseError> {
        let claim = match (
            self.claimed_actor,
            self.claimed_session,
            self.claimed_pid,
            self.claimed_at,
        ) {
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
            group: self.group_id.as_deref().map(GroupId::from_stored),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::in_memory("t").expect("in-memory DB を開ける")
    }

    fn ctx() -> Ctx {
        Ctx {
            actor: "tester".to_string(),
            reason: Some("理由".to_string()),
        }
    }

    fn claim(pid: i32) -> Claim {
        Claim {
            actor: "tester".to_string(),
            session: "s".to_string(),
            pid,
            at: Utc::now(),
        }
    }

    /// created_at を明示して作る。read_all が created_at 順に返すため、順序が要る検査で効く。
    fn issue(id: &str, seq: i64) -> Issue {
        let at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::seconds(seq);
        Issue {
            id: IssueId::from_stored(id),
            title: format!("issue {id}"),
            description: None,
            progress: Progress::NotStarted,
            commitment: Commitment::Accepted,
            condition: None,
            group: None,
            created_at: at,
            updated_at: at,
        }
    }

    fn group(id: &str) -> Group {
        Group {
            id: GroupId::from_stored(id),
            slug: id.to_string(),
            name: id.to_string(),
            description: None,
            parent: None,
        }
    }

    fn iid(id: &str) -> IssueId {
        IssueId::from_stored(id)
    }

    #[test]
    fn migrate_applies_every_version_and_is_idempotent() {
        let s = store();
        let version = |s: &Store| -> i64 {
            s.conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(version(&s) as usize, MIGRATIONS.len());
        migrate(&s.conn).unwrap();
        assert_eq!(
            version(&s) as usize,
            MIGRATIONS.len(),
            "流し直しても増えない"
        );
        assert_eq!(s.prefix().unwrap(), "t");
    }

    /// 型 → DB → 型 で往復しても値が変わらない。
    #[test]
    fn issue_round_trips_through_the_database() {
        let s = store();
        s.insert_group(&group("g")).unwrap();

        let mut i = issue("t-1", 0);
        i.description = Some("説明".to_string());
        i.condition = Some(Condition::At("2026-03-01".parse().unwrap()));
        i.group = Some(GroupId::from_stored("g"));
        s.insert(&i).unwrap();

        let back = s.get(&i.id).unwrap();
        assert_eq!(back.title, i.title);
        assert_eq!(back.description, i.description);
        assert_eq!(back.condition, i.condition);
        assert_eq!(back.group, i.group);
        assert_eq!(back.commitment, i.commitment);
        assert_eq!(back.progress, i.progress);
        assert_eq!(back.created_at, i.created_at);
    }

    #[test]
    fn claim_round_trips_with_its_issue() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        let c = claim(4242);
        s.apply(&iid("t-1"), Change::Claim(c.clone()), &ctx())
            .unwrap();

        let back = s.get(&iid("t-1")).unwrap();
        let stored = back.progress.claim().expect("着手中なので claim がある");
        assert_eq!(stored.actor, c.actor);
        assert_eq!(stored.session, c.session);
        assert_eq!(stored.pid, c.pid);
    }

    #[test]
    fn get_reports_missing_issue() {
        let s = store();
        assert!(matches!(s.get(&iid("nope")), Err(DbError::NoSuchIssue(_))));
    }

    // ---- 進行の遷移 ----

    #[test]
    fn claim_only_from_not_started() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        assert!(matches!(
            s.get(&iid("t-1")).unwrap().progress,
            Progress::InProgress(_)
        ));

        let err = s
            .apply(&iid("t-1"), Change::Claim(claim(2)), &ctx())
            .unwrap_err();
        assert!(matches!(err, DbError::CannotClaim { .. }));
    }

    #[test]
    fn claim_is_rejected_after_end() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        let err = s
            .apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap_err();
        assert!(matches!(err, DbError::CannotClaim { .. }));
    }

    /// claim を消し忘れると、読み戻しで UnexpectedClaim になる。
    #[test]
    fn leaving_in_progress_clears_the_claim() {
        for change in [Change::Release, Change::End] {
            let mut s = store();
            s.insert(&issue("t-1", 0)).unwrap();
            s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
                .unwrap();
            s.apply(&iid("t-1"), change, &ctx()).unwrap();
            assert!(s.get(&iid("t-1")).unwrap().progress.claim().is_none());
        }
    }

    /// 「着手中 かつ 不採用」は、軸が独立していることから到達する。
    #[test]
    fn axes_do_not_move_each_other() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::Decide(Commitment::Rejected), &ctx())
            .unwrap();

        let after = s.get(&iid("t-1")).unwrap();
        assert!(
            matches!(after.progress, Progress::InProgress(_)),
            "採否が進行を動かさない"
        );
        assert_eq!(after.commitment, Commitment::Rejected);

        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        assert_eq!(
            s.get(&iid("t-1")).unwrap().commitment,
            Commitment::Rejected,
            "進行が採否を動かさない"
        );
    }

    // ---- 履歴 ----

    /// 履歴に残すのは判断 (採否・時期) だけ。
    #[test]
    fn only_decisions_are_logged() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        s.apply(
            &iid("t-1"),
            Change::SetTitle("新しい題".to_string()),
            &ctx(),
        )
        .unwrap();
        assert!(s.events(&iid("t-1")).unwrap().is_empty());

        s.apply(&iid("t-1"), Change::Decide(Commitment::Rejected), &ctx())
            .unwrap();
        s.apply(
            &iid("t-1"),
            Change::SetCondition(Some(Condition::At("2026-03-01".parse().unwrap()))),
            &ctx(),
        )
        .unwrap();

        let events = s.events(&iid("t-1")).unwrap();
        let fields: Vec<&str> = events.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["commitment", "condition"]);
        assert_eq!(events[0].old_value.as_deref(), Some("accepted"));
        assert_eq!(events[0].new_value.as_deref(), Some("rejected"));
        assert_eq!(events[0].actor, "tester");
        assert_eq!(events[0].reason.as_deref(), Some("理由"));
        assert_eq!(events[1].old_value, None, "条件は付いていなかった");
        assert_eq!(events[1].new_value.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn summarize_flattens_and_truncates() {
        assert_eq!(summarize("  短い\n本文  "), "短い 本文");
        assert_eq!(summarize(&"あ".repeat(30)), "あ".repeat(30));
        let long = summarize(&"あ".repeat(31));
        assert_eq!(long.chars().count(), 31, "30 文字 + 省略記号");
        assert!(long.ends_with('…'));
    }

    // ---- 参照の解決 ----

    #[test]
    fn resolve_id_needs_a_unique_match() {
        let s = store();
        s.insert(&issue("a-x1", 0)).unwrap();
        s.insert(&issue("b-x1", 1)).unwrap();
        s.insert(&issue("a-y2", 2)).unwrap();

        assert_eq!(s.resolve_id("y2").unwrap(), iid("a-y2"));
        assert_eq!(s.resolve_id("a-x1").unwrap(), iid("a-x1"));
        assert!(matches!(
            s.resolve_id("x1"),
            Err(DbError::AmbiguousId { .. })
        ));
        assert!(matches!(s.resolve_id("zz"), Err(DbError::NoSuchIssue(_))));
    }

    #[test]
    fn resolve_slug_reports_missing_group() {
        let s = store();
        s.insert_group(&group("g")).unwrap();
        assert_eq!(s.resolve_slug("g").unwrap(), GroupId::from_stored("g"));
        assert!(matches!(
            s.resolve_slug("nope"),
            Err(DbError::NoSuchGroup(_))
        ));
    }

    // ---- 依存 ----

    #[test]
    fn deps_are_added_once_and_removable() {
        let s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.insert(&issue("t-2", 1)).unwrap();

        s.add_dep(&iid("t-1"), &iid("t-2")).unwrap();
        s.add_dep(&iid("t-1"), &iid("t-2")).unwrap();
        assert_eq!(s.deps().unwrap(), [(iid("t-1"), iid("t-2"))], "重複しない");

        s.remove_dep(&iid("t-1"), &iid("t-2")).unwrap();
        assert!(s.deps().unwrap().is_empty());
    }

    #[test]
    fn group_deps_are_added_once_and_removable() {
        let s = store();
        s.insert_group(&group("g1")).unwrap();
        s.insert_group(&group("g0")).unwrap();

        let (a, b) = (GroupId::from_stored("g1"), GroupId::from_stored("g0"));
        s.add_group_dep(&a, &b).unwrap();
        s.add_group_dep(&a, &b).unwrap();
        assert_eq!(s.group_deps().unwrap(), [(a.clone(), b.clone())]);

        s.remove_group_dep(&a, &b).unwrap();
        assert!(s.group_deps().unwrap().is_empty());
    }

    #[test]
    fn groups_round_trip() {
        let s = store();
        let mut child = group("child");
        child.parent = Some(GroupId::from_stored("g"));
        child.description = Some("説明".to_string());
        s.insert_group(&group("g")).unwrap();
        s.insert_group(&child).unwrap();

        let stored = s.groups().unwrap();
        let found = stored
            .iter()
            .find(|g| g.slug == "child")
            .expect("child がある");
        assert_eq!(found.parent, child.parent);
        assert_eq!(found.description, child.description);
    }

    // ---- 文面とグループ ----

    #[test]
    fn description_and_group_are_updated_through_apply() {
        let mut s = store();
        s.insert_group(&group("g")).unwrap();
        s.insert(&issue("t-1", 0)).unwrap();

        s.apply(
            &iid("t-1"),
            Change::SetDescription(Some("申し送り".to_string())),
            &ctx(),
        )
        .unwrap();
        s.apply(
            &iid("t-1"),
            Change::SetGroup(Some(GroupId::from_stored("g"))),
            &ctx(),
        )
        .unwrap();
        let after = s.get(&iid("t-1")).unwrap();
        assert_eq!(after.description.as_deref(), Some("申し送り"));
        assert_eq!(after.group, Some(GroupId::from_stored("g")));

        s.apply(&iid("t-1"), Change::SetDescription(None), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::SetGroup(None), &ctx())
            .unwrap();
        let cleared = s.get(&iid("t-1")).unwrap();
        assert_eq!(cleared.description, None);
        assert_eq!(cleared.group, None);

        assert!(s.events(&iid("t-1")).unwrap().is_empty());
    }

    #[test]
    fn condition_after_issue_round_trips() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.insert(&issue("t-2", 1)).unwrap();

        let cond = Condition::AfterIssue(iid("t-2"));
        s.apply(
            &iid("t-1"),
            Change::SetCondition(Some(cond.clone())),
            &ctx(),
        )
        .unwrap();
        assert_eq!(s.get(&iid("t-1")).unwrap().condition, Some(cond));

        let events = s.events(&iid("t-1")).unwrap();
        assert_eq!(events[0].new_value.as_deref(), Some("after t-2"));

        s.apply(&iid("t-1"), Change::SetCondition(None), &ctx())
            .unwrap();
        assert_eq!(s.get(&iid("t-1")).unwrap().condition, None);
    }

    #[test]
    fn all_returns_issues_in_creation_order() {
        let s = store();
        s.insert(&issue("t-2", 1)).unwrap();
        s.insert(&issue("t-1", 0)).unwrap();
        let ids: Vec<String> = s.all().unwrap().iter().map(|i| i.id.to_string()).collect();
        assert_eq!(ids, ["t-1", "t-2"]);
    }
}
