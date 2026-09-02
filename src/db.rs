//! SQLite への永続化。DB は git 管理外に置き、worktree 間で共有する。

use crate::derived::View;
use crate::domain::*;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("not inside a Git repository")]
    NotInRepo,
    #[error("axon is not initialized; run `axon init`")]
    NotInitialized,
    #[error("axon is already initialized at {0}")]
    AlreadyInitialized(PathBuf),
    #[error("could not read a stored value: {0}")]
    Parse(#[from] ParseError),
    #[error("invalid stored progress event kind: {0}")]
    InvalidProgressEvent(String),
    #[error("issue {0} does not exist")]
    NoSuchIssue(String),
    #[error("group {0} does not exist")]
    NoSuchGroup(String),
    #[error("{id} matches multiple issues: {candidates}")]
    AmbiguousId { id: String, candidates: String },
    #[error("{id} cannot be claimed ({fact})")]
    CannotClaim { id: String, fact: String },
    #[error("{id} cannot be {action} (Progress is {progress})")]
    CannotProgress {
        id: String,
        action: &'static str,
        progress: &'static str,
    },
    #[error("{id}: {field} is already {current}")]
    Unchanged {
        id: String,
        field: &'static str,
        current: String,
    },
    #[error("cycle would be created: {path}")]
    Cycle { path: String },
}

pub type Result<T> = std::result::Result<T, DbError>;

/// スキーマの版。`user_version` に記録し、開くたびに不足分だけ流す。
const MIGRATIONS: &[&str] = &[
    SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6,
];

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
    (cond_kind IS NULL          AND cond_date IS NULL     AND cond_ref IS NULL) OR
    (cond_kind =  'date'        AND cond_date IS NOT NULL AND cond_ref IS NULL) OR
    (cond_kind =  'after_issue' AND cond_ref IS NOT NULL  AND cond_date IS NULL)
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

/// A (進行) の履歴。B/C の判断ログとは用途も表示先も違うため、別の表にする。
const SCHEMA_V5: &str = r#"
CREATE TABLE progress_events (
  id        INTEGER PRIMARY KEY,
  issue_id  TEXT NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  kind      TEXT NOT NULL CHECK (kind IN ('start','done','release')),
  actor     TEXT NOT NULL,
  reason    TEXT,
  at        TEXT NOT NULL
);

CREATE INDEX idx_progress_events_issue ON progress_events (issue_id, at);
"#;

/// 確定語彙に合わせる。履歴の意味と利用者の入力は保ったまま、名称と値の表現を変える。
const SCHEMA_V6: &str = r#"
ALTER TABLE issues RENAME COLUMN commitment TO disposition;
ALTER TABLE issues RENAME COLUMN cond_kind TO resurface_kind;
ALTER TABLE issues RENAME COLUMN cond_date TO resurface_date;
ALTER TABLE issues RENAME COLUMN cond_ref TO resurface_ref;
UPDATE events SET field = 'disposition' WHERE field = 'commitment';
UPDATE events
SET old_value = CASE
      WHEN old_value LIKE 'after %' THEN 'AfterIssue(' || substr(old_value, 7) || ')'
      ELSE 'AtDate(' || old_value || ')'
    END
WHERE field = 'condition' AND old_value IS NOT NULL;
UPDATE events
SET new_value = CASE
      WHEN new_value LIKE 'after %' THEN 'AfterIssue(' || substr(new_value, 7) || ')'
      ELSE 'AtDate(' || new_value || ')'
    END
WHERE field = 'condition' AND new_value IS NOT NULL;
UPDATE events SET field = 'resurface_condition' WHERE field = 'condition';
"#;

fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_with(conn, MIGRATIONS)
}

fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let Some(sql) = migrations.get(current as usize) else {
            tx.commit()?;
            return Ok(());
        };
        let version = current + 1;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
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

pub struct ShowSnapshot {
    pub id: IssueId,
    pub issues: Vec<Issue>,
    pub deps: Vec<(IssueId, IssueId)>,
    pub groups: Vec<Group>,
    pub group_deps: Vec<(GroupId, GroupId)>,
    pub progress_events: Vec<ProgressEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RelationKind {
    IssueDependency,
    AfterIssue,
    GroupDependency,
    GroupParent,
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IssueDependency => "issue dependency",
            Self::AfterIssue => "AfterIssue reference",
            Self::GroupDependency => "group dependency",
            Self::GroupParent => "group parent",
        })
    }
}

#[derive(Clone)]
struct GraphEdge<I> {
    from: I,
    to: I,
    kind: RelationKind,
}

fn reject_cycle<I, F>(
    edges: &[GraphEdge<I>],
    from: &I,
    to: &I,
    kind: RelationKind,
    label: F,
) -> Result<()>
where
    I: Clone + Eq + Hash + fmt::Display,
    F: Fn(&I) -> String,
{
    let Some(path) = path_between(edges, to, from) else {
        return Ok(());
    };
    let mut description = format!("{} -[{kind}]-> {}", label(from), label(to));
    for edge in path {
        description.push_str(&format!(" -[{}]-> {}", edge.kind, label(&edge.to)));
    }
    Err(DbError::Cycle { path: description })
}

fn path_between<I>(edges: &[GraphEdge<I>], start: &I, goal: &I) -> Option<Vec<GraphEdge<I>>>
where
    I: Clone + Eq + Hash,
{
    if start == goal {
        return Some(Vec::new());
    }

    let mut queue = VecDeque::from([start.clone()]);
    let mut seen = HashSet::from([start.clone()]);
    let mut previous: HashMap<I, GraphEdge<I>> = HashMap::new();

    while let Some(current) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| edge.from == current) {
            if !seen.insert(edge.to.clone()) {
                continue;
            }
            previous.insert(edge.to.clone(), edge.clone());
            if &edge.to == goal {
                let mut path = Vec::new();
                let mut node = goal.clone();
                while &node != start {
                    let edge = previous.get(&node)?.clone();
                    node = edge.from.clone();
                    path.push(edge);
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(edge.to.clone());
        }
    }
    None
}

impl Store {
    fn from_conn(mut conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        migrate(&mut conn)?;
        Ok(Store { conn })
    }

    pub fn init(prefix: &str) -> Result<PathBuf> {
        let path = db_path()?;
        if path.exists() {
            return Err(DbError::AlreadyInitialized(path));
        }
        std::fs::create_dir_all(path.parent().expect("database path has a parent directory"))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let mut conn = Connection::open(&path)?;
        migrate(&mut conn)?;
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

    pub fn insert(&mut self, issue: &Issue) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let ResurfaceCondition::AfterIssue(target) = &issue.resurface_condition {
            reject_cycle(
                &read_issue_wait_edges(&tx)?,
                &issue.id,
                target,
                RelationKind::AfterIssue,
                ToString::to_string,
            )?;
        }
        let claim = issue.progress.claim();
        tx.execute(
            "INSERT INTO issues (id, title, description, progress, disposition,
                                 resurface_kind, resurface_date, resurface_ref,
                                 claimed_actor, claimed_session, claimed_pid, claimed_at,
                                 created_at, updated_at, group_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                issue.id.as_str(),
                issue.title,
                issue.description,
                issue.progress.as_db(),
                issue.disposition.as_db(),
                issue.resurface_condition.kind_db(),
                resurface_date(&issue.resurface_condition),
                resurface_ref(&issue.resurface_condition),
                claim.map(|c| c.actor.clone()),
                claim.map(|c| c.session.clone()),
                claim.map(|c| c.pid),
                claim.map(|c| c.at.to_rfc3339()),
                issue.created_at.to_rfc3339(),
                issue.updated_at.to_rfc3339(),
                issue.group.as_ref().map(|g| g.as_str().to_string()),
            ],
        )?;
        tx.commit()?;
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
        resolve_issue_id(&self.conn, input)
    }

    pub fn get(&self, id: &IssueId) -> Result<Issue> {
        read_issue(&self.conn, id)
    }

    pub fn add_dep(&mut self, issue: &IssueId, depends_on: &IssueId) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM issue_deps WHERE issue_id = ?1 AND depends_on_id = ?2
             )",
            params![issue.as_str(), depends_on.as_str()],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            tx.commit()?;
            return Ok(());
        }
        reject_cycle(
            &read_issue_wait_edges(&tx)?,
            issue,
            depends_on,
            RelationKind::IssueDependency,
            ToString::to_string,
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO issue_deps (issue_id, depends_on_id) VALUES (?1, ?2)",
            params![issue.as_str(), depends_on.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_dep(&self, issue: &IssueId, depends_on: &IssueId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM issue_deps WHERE issue_id = ?1 AND depends_on_id = ?2",
            params![issue.as_str(), depends_on.as_str()],
        )?;
        Ok(())
    }

    pub fn insert_group(&mut self, g: &Group) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(parent) = &g.parent {
            let mut labels = read_groups(&tx)?
                .into_iter()
                .map(|group| (group.id, group.slug))
                .collect::<HashMap<_, _>>();
            labels.insert(g.id.clone(), g.slug.clone());
            reject_cycle(
                &read_group_parent_edges(&tx)?,
                &g.id,
                parent,
                RelationKind::GroupParent,
                |id| labels.get(id).cloned().unwrap_or_else(|| id.to_string()),
            )?;
        }
        let now = Utc::now().to_rfc3339();
        tx.execute(
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
        tx.commit()?;
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

    pub fn add_group_dep(&mut self, group: &GroupId, depends_on: &GroupId) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM group_deps WHERE group_id = ?1 AND depends_on_id = ?2
             )",
            params![group.as_str(), depends_on.as_str()],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            tx.commit()?;
            return Ok(());
        }
        let labels = read_groups(&tx)?
            .into_iter()
            .map(|group| (group.id, group.slug))
            .collect::<HashMap<_, _>>();
        reject_cycle(
            &read_group_dependency_edges(&tx)?,
            group,
            depends_on,
            RelationKind::GroupDependency,
            |id| labels.get(id).cloned().unwrap_or_else(|| id.to_string()),
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO group_deps (group_id, depends_on_id) VALUES (?1, ?2)",
            params![group.as_str(), depends_on.as_str()],
        )?;
        tx.commit()?;
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
    pub fn apply(&mut self, id: &IssueId, change: Change, ctx: &Ctx) -> Result<ApplyOutcome> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = read_issue(&tx, id)?;

        if let Some((field, current)) = unchanged_transition(&before, &change) {
            return Err(DbError::Unchanged {
                id: id.to_string(),
                field,
                current,
            });
        }
        if unchanged_setting(&before, &change) {
            tx.commit()?;
            return Ok(ApplyOutcome::Unchanged);
        }

        let now = Utc::now();
        let progress_event = progress_event(&change, ctx, &now);
        let now = now.to_rfc3339();
        let (field, old, new) = describe(&before, &change);
        let record = records_decision(&change);
        match change {
            Change::Claim(c) => {
                let view = View::new(
                    read_all(&tx)?,
                    read_deps(&tx)?,
                    read_groups(&tx)?,
                    read_group_deps(&tx)?,
                );
                if !view.is_ready(&before) {
                    return Err(DbError::CannotClaim {
                        id: id.to_string(),
                        fact: not_ready_fact(&view, &before),
                    });
                }
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
                    return Err(DbError::CannotClaim {
                        id: id.to_string(),
                        fact: format!("Progress is {}", before.progress.label()),
                    });
                }
            }
            Change::Release => {
                let n = tx.execute(
                    "UPDATE issues
                        SET progress = 'not_started',
                            claimed_actor = NULL, claimed_session = NULL,
                            claimed_pid = NULL, claimed_at = NULL, updated_at = ?2
                      WHERE id = ?1 AND progress = 'in_progress'",
                    params![id.as_str(), now],
                )?;
                if n == 0 {
                    return Err(DbError::CannotProgress {
                        id: id.to_string(),
                        action: "released",
                        progress: before.progress.label(),
                    });
                }
            }
            Change::End => {
                let n = tx.execute(
                    "UPDATE issues
                        SET progress = 'ended',
                            claimed_actor = NULL, claimed_session = NULL,
                            claimed_pid = NULL, claimed_at = NULL, updated_at = ?2
                      WHERE id = ?1 AND progress = 'in_progress'",
                    params![id.as_str(), now],
                )?;
                if n == 0 {
                    return Err(DbError::CannotProgress {
                        id: id.to_string(),
                        action: "ended",
                        progress: before.progress.label(),
                    });
                }
            }
            Change::Decide(c) | Change::ConvergeDisposition(c) => {
                tx.execute(
                    "UPDATE issues SET disposition = ?2, updated_at = ?3 WHERE id = ?1",
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
            Change::SetResurfaceCondition(resurface_condition) => {
                if let ResurfaceCondition::AfterIssue(target) = &resurface_condition {
                    let mut edges = read_issue_wait_edges(&tx)?;
                    edges.retain(|edge| {
                        edge.kind != RelationKind::AfterIssue || edge.from != before.id
                    });
                    reject_cycle(
                        &edges,
                        id,
                        target,
                        RelationKind::AfterIssue,
                        ToString::to_string,
                    )?;
                }
                tx.execute(
                    "UPDATE issues SET resurface_kind = ?2, resurface_date = ?3, resurface_ref = ?4, updated_at = ?5
                      WHERE id = ?1",
                    params![
                        id.as_str(),
                        resurface_condition.kind_db(),
                        resurface_date(&resurface_condition),
                        resurface_ref(&resurface_condition),
                        now
                    ],
                )?;
            }
        }
        if record {
            log_event(&tx, id, field, old, new, ctx)?;
        }
        if let Some(event) = progress_event {
            log_progress_event(&tx, id, &event)?;
        }
        tx.commit()?;
        Ok(ApplyOutcome::Changed)
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

    #[cfg(test)]
    fn progress_events(&self, id: &IssueId) -> Result<Vec<ProgressEvent>> {
        read_progress_events(&self.conn, id)
    }

    pub fn show_snapshot(&mut self, input: &str) -> Result<ShowSnapshot> {
        let tx = self.conn.transaction()?;
        let snapshot = read_show_snapshot(&tx, input)?;
        tx.commit()?;
        Ok(snapshot)
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

fn log_progress_event(conn: &Connection, id: &IssueId, event: &ProgressEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO progress_events (issue_id, kind, actor, reason, at)
         VALUES (?1,?2,?3,?4,?5)",
        params![
            id.as_str(),
            event.kind.as_db(),
            event.actor,
            event.reason,
            event.at.to_rfc3339()
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressEventKind {
    Start,
    Done,
    Release,
}

impl ProgressEventKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Done => "done",
            Self::Release => "release",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "start" => Ok(Self::Start),
            "done" => Ok(Self::Done),
            "release" => Ok(Self::Release),
            other => Err(DbError::InvalidProgressEvent(other.to_string())),
        }
    }
}

pub struct ProgressEvent {
    pub kind: ProgressEventKind,
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
    Decide(Disposition),
    /// バッチ設定で採否を目標値へ収束させる。同値は成功 no-op。
    ConvergeDisposition(Disposition),
    SetTitle(String),
    SetDescription(Option<String>),
    SetResurfaceCondition(ResurfaceCondition),
    SetGroup(Option<GroupId>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    Changed,
    Unchanged,
}

fn progress_event(change: &Change, ctx: &Ctx, now: &DateTime<Utc>) -> Option<ProgressEvent> {
    match change {
        Change::Claim(claim) => Some(ProgressEvent {
            kind: ProgressEventKind::Start,
            actor: claim.actor.clone(),
            reason: None,
            at: claim.at,
        }),
        Change::End => Some(ProgressEvent {
            kind: ProgressEventKind::Done,
            actor: ctx.actor.clone(),
            reason: ctx.reason.clone(),
            at: *now,
        }),
        Change::Release => Some(ProgressEvent {
            kind: ProgressEventKind::Release,
            actor: ctx.actor.clone(),
            reason: ctx.reason.clone(),
            at: *now,
        }),
        _ => None,
    }
}

const SELECT_ISSUE: &str = "SELECT id, title, description, progress, disposition,
                                   resurface_kind, resurface_date, resurface_ref,
                                   claimed_actor, claimed_session, claimed_pid, claimed_at,
                                   created_at, updated_at, group_id
                            FROM issues";

fn read_issue(conn: &Connection, id: &IssueId) -> Result<Issue> {
    let sql = format!("{SELECT_ISSUE} WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let raw = stmt
        .query_row(params![id.as_str()], |r| Ok(RawIssue::from_row(r)))
        .optional()?
        .ok_or_else(|| DbError::NoSuchIssue(id.to_string()))?;
    Ok(raw.into_issue()?)
}

fn unchanged_transition(issue: &Issue, change: &Change) -> Option<(&'static str, String)> {
    match change {
        Change::Decide(disposition) if issue.disposition == *disposition => {
            Some(("Disposition", issue.disposition.label().to_string()))
        }
        Change::SetResurfaceCondition(resurface_condition)
            if &issue.resurface_condition == resurface_condition =>
        {
            Some((
                "Resurface condition",
                describe_resurface_condition(&issue.resurface_condition),
            ))
        }
        _ => None,
    }
}

fn unchanged_setting(issue: &Issue, change: &Change) -> bool {
    match change {
        Change::ConvergeDisposition(disposition) => issue.disposition == *disposition,
        Change::SetTitle(title) => issue.title == title.as_str(),
        Change::SetDescription(description) => &issue.description == description,
        Change::SetGroup(group) => issue.group.as_ref() == group.as_ref(),
        _ => false,
    }
}

fn not_ready_fact(view: &View, issue: &Issue) -> String {
    if !matches!(issue.progress, Progress::NotStarted) {
        format!("Progress is {}", issue.progress.label())
    } else if issue.disposition != Disposition::Accepted {
        format!("Disposition is {}", issue.disposition.label())
    } else if !view.is_surfaced(issue) {
        "resurface condition is not satisfied".to_string()
    } else if view.is_orphaned(&issue.id) {
        "a dependency is Rejected".to_string()
    } else if view.is_blocked(&issue.id) {
        "an unresolved dependency exists".to_string()
    } else if view.is_group_blocked(issue) {
        "an unresolved group dependency exists".to_string()
    } else {
        "issue is not ready".to_string()
    }
}

fn resolve_issue_id(conn: &Connection, input: &str) -> Result<IssueId> {
    let mut stmt = conn.prepare("SELECT id FROM issues WHERE id = ?1 OR id LIKE '%-' || ?1")?;
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

fn read_issue_wait_edges(conn: &Connection) -> Result<Vec<GraphEdge<IssueId>>> {
    let mut edges = read_deps(conn)?
        .into_iter()
        .map(|(from, to)| GraphEdge {
            from,
            to,
            kind: RelationKind::IssueDependency,
        })
        .collect::<Vec<_>>();
    edges.extend(read_all(conn)?.into_iter().filter_map(|issue| {
        let ResurfaceCondition::AfterIssue(to) = issue.resurface_condition else {
            return None;
        };
        Some(GraphEdge {
            from: issue.id,
            to,
            kind: RelationKind::AfterIssue,
        })
    }));
    Ok(edges)
}

fn read_group_dependency_edges(conn: &Connection) -> Result<Vec<GraphEdge<GroupId>>> {
    Ok(read_group_deps(conn)?
        .into_iter()
        .map(|(from, to)| GraphEdge {
            from,
            to,
            kind: RelationKind::GroupDependency,
        })
        .collect())
}

fn read_group_parent_edges(conn: &Connection) -> Result<Vec<GraphEdge<GroupId>>> {
    Ok(read_groups(conn)?
        .into_iter()
        .filter_map(|group| {
            group.parent.map(|to| GraphEdge {
                from: group.id,
                to,
                kind: RelationKind::GroupParent,
            })
        })
        .collect())
}

fn read_progress_events(conn: &Connection, id: &IssueId) -> Result<Vec<ProgressEvent>> {
    let mut stmt = conn.prepare(
        "SELECT kind, actor, reason, at
         FROM progress_events WHERE issue_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![id.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (kind, actor, reason, at) = row?;
        events.push(ProgressEvent {
            kind: ProgressEventKind::from_db(&kind)?,
            actor,
            reason,
            at: parse_ts(&at),
        });
    }
    Ok(events)
}

fn read_show_snapshot(conn: &Connection, input: &str) -> Result<ShowSnapshot> {
    let id = resolve_issue_id(conn, input)?;
    Ok(ShowSnapshot {
        progress_events: read_progress_events(conn, &id)?,
        issues: read_all(conn)?,
        deps: read_deps(conn)?,
        groups: read_groups(conn)?,
        group_deps: read_group_deps(conn)?,
        id,
    })
}

fn records_decision(change: &Change) -> bool {
    matches!(
        change,
        Change::Decide(_) | Change::ConvergeDisposition(_) | Change::SetResurfaceCondition(_)
    )
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
        Change::Decide(c) | Change::ConvergeDisposition(c) => (
            "disposition",
            Some(before.disposition.as_db().to_string()),
            Some(c.as_db().to_string()),
        ),
        Change::SetTitle(t) => ("title", Some(before.title.clone()), Some(t.clone())),
        Change::SetDescription(d) => (
            "description",
            before.description.as_deref().map(summarize),
            d.as_deref().map(summarize),
        ),
        Change::SetResurfaceCondition(c) => (
            "resurface_condition",
            resurface_condition_event_value(&before.resurface_condition),
            resurface_condition_event_value(c),
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

fn describe_resurface_condition(c: &ResurfaceCondition) -> String {
    match c {
        ResurfaceCondition::Always => "Always".to_string(),
        ResurfaceCondition::AtDate(d) => format!("AtDate({d})"),
        ResurfaceCondition::AfterIssue(id) => format!("AfterIssue({id})"),
    }
}

fn resurface_condition_event_value(c: &ResurfaceCondition) -> Option<String> {
    match c {
        ResurfaceCondition::Always => None,
        _ => Some(describe_resurface_condition(c)),
    }
}

fn resurface_date(c: &ResurfaceCondition) -> Option<String> {
    match c {
        ResurfaceCondition::AtDate(d) => Some(d.to_string()),
        _ => None,
    }
}

fn resurface_ref(c: &ResurfaceCondition) -> Option<String> {
    match c {
        ResurfaceCondition::AfterIssue(id) => Some(id.as_str().to_string()),
        _ => None,
    }
}

/// SQLite の行をそのまま受けた形。ここが型への境界になる。
struct RawIssue {
    id: String,
    title: String,
    description: Option<String>,
    progress: String,
    disposition: String,
    resurface_kind: Option<String>,
    resurface_date: Option<String>,
    resurface_ref: Option<String>,
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
            disposition: r.get_unwrap(4),
            resurface_kind: r.get_unwrap(5),
            resurface_date: r.get_unwrap(6),
            resurface_ref: r.get_unwrap(7),
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
        let resurface_condition = ResurfaceCondition::from_db(
            self.resurface_kind.as_deref(),
            self.resurface_date.as_deref(),
            self.resurface_ref.as_deref(),
        )?;
        Ok(Issue {
            id: IssueId::from_stored(&self.id),
            title: self.title,
            description: self.description,
            progress: Progress::from_db(&self.progress, claim)?,
            disposition: Disposition::from_db(&self.disposition)?,
            resurface_condition,
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
            disposition: Disposition::Accepted,
            resurface_condition: ResurfaceCondition::Always,
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
        let mut s = store();
        let version = |s: &Store| -> i64 {
            s.conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(version(&s) as usize, MIGRATIONS.len());
        migrate(&mut s.conn).unwrap();
        assert_eq!(
            version(&s) as usize,
            MIGRATIONS.len(),
            "流し直しても増えない"
        );
        assert_eq!(s.prefix().unwrap(), "t");
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_version_together() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = "CREATE TABLE half_applied (id INTEGER); SELECT * FROM missing_table;";

        assert!(migrate_with(&mut conn, &[broken]).is_err());
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'half_applied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 0);
        assert_eq!(table_count, 0);

        migrate_with(&mut conn, &["CREATE TABLE half_applied (id INTEGER);"]).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1, "中断後と同じmigrationを再実行できる");
    }

    #[test]
    fn show_snapshot_keeps_state_and_history_at_one_point_in_time() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "axon-show-snapshot-{}-{nonce}.db",
            std::process::id()
        ));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL").unwrap();
        let mut reader = Store::from_conn(conn).unwrap();
        reader.insert(&issue("t-1", 0)).unwrap();
        let mut writer = Store::from_conn(Connection::open(&path).unwrap()).unwrap();

        let tx = reader.conn.transaction().unwrap();
        assert_eq!(
            read_all(&tx).unwrap().len(),
            1,
            "更新前のsnapshotを確定する"
        );
        writer
            .apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        let snapshot = read_show_snapshot(&tx, "t-1").unwrap();
        let shown = snapshot
            .issues
            .iter()
            .find(|issue| issue.id == snapshot.id)
            .unwrap();
        assert_eq!(shown.progress, Progress::NotStarted);
        assert!(snapshot.progress_events.is_empty());
        tx.commit().unwrap();

        let fresh = reader.show_snapshot("t-1").unwrap();
        let shown = fresh
            .issues
            .iter()
            .find(|issue| issue.id == fresh.id)
            .unwrap();
        assert!(matches!(shown.progress, Progress::InProgress(_)));
        assert_eq!(fresh.progress_events.len(), 1);

        drop(reader);
        drop(writer);
        for candidate in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    #[test]
    fn migration_adds_progress_history_without_recreating_deleted_events() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, sql) in MIGRATIONS.iter().take(3).enumerate() {
            conn.execute_batch(sql).unwrap();
            conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
                .unwrap();
        }
        conn.execute(
            "INSERT INTO issues
               (id, title, progress, commitment, created_at, updated_at)
             VALUES ('t-1', '既存 issue', 'not_started', 'accepted', ?1, ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events
               (issue_id, field, old_value, new_value, actor, reason, at)
             VALUES ('t-1', 'progress', 'not_started', 'in_progress', 'old', NULL, ?1)",
            params![Utc::now().to_rfc3339()],
        )
        .unwrap();

        let mut s = Store::from_conn(conn).unwrap();
        assert!(s.events(&iid("t-1")).unwrap().is_empty());
        assert!(s.progress_events(&iid("t-1")).unwrap().is_empty());

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        assert_eq!(s.progress_events(&iid("t-1")).unwrap().len(), 1);
    }

    #[test]
    fn disposition_migration_preserves_issues_and_decision_history() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, sql) in MIGRATIONS.iter().take(5).enumerate() {
            conn.execute_batch(sql).unwrap();
            conn.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
                .unwrap();
        }
        let at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO issues
               (id, title, description, progress, commitment, cond_kind, cond_date,
                created_at, updated_at)
             VALUES ('t-1', '既存 issue', '日本語の説明', 'not_started', 'accepted',
                     'date', '2099-12-31', ?1, ?1)",
            params![at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events
               (issue_id, field, old_value, new_value, actor, reason, at)
             VALUES ('t-1', 'condition', NULL, '2099-12-31', 'old', '後回しする理由', ?1)",
            params![at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events
               (issue_id, field, old_value, new_value, actor, reason, at)
             VALUES ('t-1', 'commitment', 'undecided', 'accepted', 'old', '採用理由', ?1)",
            params![at],
        )
        .unwrap();

        let s = Store::from_conn(conn).unwrap();
        let issue = s.get(&iid("t-1")).unwrap();
        assert_eq!(issue.title, "既存 issue");
        assert_eq!(issue.description.as_deref(), Some("日本語の説明"));
        assert_eq!(issue.disposition, Disposition::Accepted);
        assert_eq!(
            issue.resurface_condition,
            ResurfaceCondition::AtDate("2099-12-31".parse().unwrap())
        );

        let events = s.events(&iid("t-1")).unwrap();
        assert_eq!(events.len(), 2);
        let disposition = events
            .iter()
            .find(|event| event.field == "disposition")
            .unwrap();
        assert_eq!(disposition.old_value.as_deref(), Some("undecided"));
        assert_eq!(disposition.new_value.as_deref(), Some("accepted"));
        assert_eq!(disposition.reason.as_deref(), Some("採用理由"));
        let resurface = events
            .iter()
            .find(|event| event.field == "resurface_condition")
            .unwrap();
        assert_eq!(resurface.old_value, None);
        assert_eq!(resurface.new_value.as_deref(), Some("AtDate(2099-12-31)"));
        assert_eq!(resurface.reason.as_deref(), Some("後回しする理由"));
    }

    /// 型 → DB → 型 で往復しても値が変わらない。
    #[test]
    fn issue_round_trips_through_the_database() {
        let mut s = store();
        s.insert_group(&group("g")).unwrap();

        let mut i = issue("t-1", 0);
        i.description = Some("説明".to_string());
        i.resurface_condition = ResurfaceCondition::AtDate("2026-03-01".parse().unwrap());
        i.group = Some(GroupId::from_stored("g"));
        s.insert(&i).unwrap();

        let back = s.get(&i.id).unwrap();
        assert_eq!(back.title, i.title);
        assert_eq!(back.description, i.description);
        assert_eq!(back.resurface_condition, i.resurface_condition);
        assert_eq!(back.group, i.group);
        assert_eq!(back.disposition, i.disposition);
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
        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        let err = s
            .apply(&iid("t-1"), Change::Claim(claim(2)), &ctx())
            .unwrap_err();
        assert!(matches!(err, DbError::CannotClaim { .. }));
    }

    #[test]
    fn end_only_from_in_progress_and_records_only_the_transition() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        let err = s.apply(&iid("t-1"), Change::End, &ctx()).unwrap_err();
        assert!(matches!(
            err,
            DbError::CannotProgress {
                action: "ended",
                ..
            }
        ));
        assert!(s.progress_events(&iid("t-1")).unwrap().is_empty());

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        let err = s.apply(&iid("t-1"), Change::End, &ctx()).unwrap_err();
        assert!(matches!(
            err,
            DbError::CannotProgress {
                action: "ended",
                ..
            }
        ));
        let kinds: Vec<_> = s
            .progress_events(&iid("t-1"))
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(kinds, [ProgressEventKind::Start, ProgressEventKind::Done]);
    }

    #[test]
    fn release_only_from_in_progress_and_records_only_the_transition() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        let err = s.apply(&iid("t-1"), Change::Release, &ctx()).unwrap_err();
        assert!(matches!(
            err,
            DbError::CannotProgress {
                action: "released",
                ..
            }
        ));
        assert!(s.progress_events(&iid("t-1")).unwrap().is_empty());

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::Release, &ctx()).unwrap();
        let err = s.apply(&iid("t-1"), Change::Release, &ctx()).unwrap_err();
        assert!(matches!(
            err,
            DbError::CannotProgress {
                action: "released",
                ..
            }
        ));
        let kinds: Vec<_> = s
            .progress_events(&iid("t-1"))
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(
            kinds,
            [ProgressEventKind::Start, ProgressEventKind::Release]
        );
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
        s.apply(&iid("t-1"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();

        let after = s.get(&iid("t-1")).unwrap();
        assert!(
            matches!(after.progress, Progress::InProgress(_)),
            "採否が進行を動かさない"
        );
        assert_eq!(after.disposition, Disposition::Rejected);

        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();
        assert_eq!(
            s.get(&iid("t-1")).unwrap().disposition,
            Disposition::Rejected,
            "進行が採否を動かさない"
        );
    }

    #[test]
    fn convergent_disposition_skips_a_target_changed_after_batch_selection() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        assert_eq!(
            s.apply(
                &iid("t-1"),
                Change::ConvergeDisposition(Disposition::Rejected),
                &ctx(),
            )
            .unwrap(),
            ApplyOutcome::Changed
        );
        let after_change = s.get(&iid("t-1")).unwrap();
        let events_after_change = s.events(&iid("t-1")).unwrap().len();

        assert_eq!(
            s.apply(
                &iid("t-1"),
                Change::ConvergeDisposition(Disposition::Rejected),
                &ctx(),
            )
            .unwrap(),
            ApplyOutcome::Unchanged
        );
        let after_noop = s.get(&iid("t-1")).unwrap();
        assert_eq!(after_noop.updated_at, after_change.updated_at);
        assert_eq!(s.events(&iid("t-1")).unwrap().len(), events_after_change);
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

        s.apply(&iid("t-1"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();
        s.apply(
            &iid("t-1"),
            Change::SetResurfaceCondition(ResurfaceCondition::AtDate(
                "2026-03-01".parse().unwrap(),
            )),
            &ctx(),
        )
        .unwrap();

        let events = s.events(&iid("t-1")).unwrap();
        let fields: Vec<&str> = events.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["disposition", "resurface_condition"]);
        assert_eq!(events[0].old_value.as_deref(), Some("accepted"));
        assert_eq!(events[0].new_value.as_deref(), Some("rejected"));
        assert_eq!(events[0].actor, "tester");
        assert_eq!(events[0].reason.as_deref(), Some("理由"));
        assert_eq!(events[1].old_value, None, "条件は付いていなかった");
        assert_eq!(events[1].new_value.as_deref(), Some("AtDate(2026-03-01)"));
    }

    #[test]
    fn progress_events_record_transitions_separately_from_decisions() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::Release, &ctx()).unwrap();
        s.apply(&iid("t-1"), Change::Claim(claim(2)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::End, &ctx()).unwrap();

        let events = s.progress_events(&iid("t-1")).unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].kind, ProgressEventKind::Start);
        assert_eq!(events[0].actor, "tester");
        assert_eq!(events[0].reason, None, "start は理由を受け取らない");
        assert_eq!(events[1].kind, ProgressEventKind::Release);
        assert_eq!(events[1].reason.as_deref(), Some("理由"));
        assert_eq!(events[2].kind, ProgressEventKind::Start);
        assert_eq!(events[3].kind, ProgressEventKind::Done);
        assert_eq!(events[3].reason.as_deref(), Some("理由"));
        assert!(s.events(&iid("t-1")).unwrap().is_empty());

        s.apply(&iid("t-1"), Change::Decide(Disposition::Rejected), &ctx())
            .unwrap();
        assert_eq!(s.events(&iid("t-1")).unwrap().len(), 1);
        assert_eq!(
            s.progress_events(&iid("t-1")).unwrap().len(),
            4,
            "判断は進行履歴に混ぜない"
        );
    }

    #[test]
    fn progress_events_follow_transition_sequence_not_timestamps() {
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();

        s.apply(&iid("t-1"), Change::Claim(claim(1)), &ctx())
            .unwrap();
        s.apply(&iid("t-1"), Change::Release, &ctx()).unwrap();
        let mut later_start = claim(2);
        later_start.at -= chrono::Duration::days(1);
        s.apply(&iid("t-1"), Change::Claim(later_start), &ctx())
            .unwrap();

        let events = s.progress_events(&iid("t-1")).unwrap();
        assert!(events[2].at < events[1].at, "後の遷移が古い時刻を持つ再現");
        let kinds: Vec<_> = events.into_iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            [
                ProgressEventKind::Start,
                ProgressEventKind::Release,
                ProgressEventKind::Start
            ]
        );
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
        let mut s = store();
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
        let mut s = store();
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
        let mut s = store();
        s.insert(&issue("t-1", 0)).unwrap();
        s.insert(&issue("t-2", 1)).unwrap();

        s.add_dep(&iid("t-1"), &iid("t-2")).unwrap();
        s.add_dep(&iid("t-1"), &iid("t-2")).unwrap();
        assert_eq!(s.deps().unwrap(), [(iid("t-1"), iid("t-2"))], "重複しない");

        s.remove_dep(&iid("t-1"), &iid("t-2")).unwrap();
        assert!(s.deps().unwrap().is_empty());
    }

    #[test]
    fn issue_dependency_cycles_are_rejected_at_the_store_boundary() {
        let mut s = store();
        for (seq, id) in ["t-a", "t-b", "t-c"].into_iter().enumerate() {
            s.insert(&issue(id, seq as i64)).unwrap();
        }
        s.add_dep(&iid("t-a"), &iid("t-b")).unwrap();
        s.add_dep(&iid("t-b"), &iid("t-c")).unwrap();

        let error = s.add_dep(&iid("t-c"), &iid("t-a")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: t-c -[issue dependency]-> t-a \
             -[issue dependency]-> t-b -[issue dependency]-> t-c"
        );
        assert_eq!(s.deps().unwrap().len(), 2, "失敗した辺は保存しない");
    }

    #[test]
    fn after_issue_cycles_leave_the_issue_history_and_timestamp_unchanged() {
        let mut s = store();
        for (seq, id) in ["t-a", "t-b", "t-c"].into_iter().enumerate() {
            s.insert(&issue(id, seq as i64)).unwrap();
        }
        s.apply(
            &iid("t-a"),
            Change::SetResurfaceCondition(ResurfaceCondition::AfterIssue(iid("t-b"))),
            &ctx(),
        )
        .unwrap();
        s.apply(
            &iid("t-b"),
            Change::SetResurfaceCondition(ResurfaceCondition::AfterIssue(iid("t-c"))),
            &ctx(),
        )
        .unwrap();
        let before = s.get(&iid("t-c")).unwrap();

        let error = s
            .apply(
                &iid("t-c"),
                Change::SetResurfaceCondition(ResurfaceCondition::AfterIssue(iid("t-a"))),
                &ctx(),
            )
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: t-c -[AfterIssue reference]-> t-a \
             -[AfterIssue reference]-> t-b -[AfterIssue reference]-> t-c"
        );
        let after = s.get(&iid("t-c")).unwrap();
        assert_eq!(after.resurface_condition, ResurfaceCondition::Always);
        assert_eq!(after.updated_at, before.updated_at);
        assert!(s.events(&iid("t-c")).unwrap().is_empty());
    }

    #[test]
    fn inserting_an_issue_with_after_issue_cannot_close_a_cycle() {
        let mut s = store();
        let mut first = issue("t-a", 0);
        first.resurface_condition = ResurfaceCondition::AfterIssue(iid("t-b"));
        s.insert(&first).unwrap();
        let mut second = issue("t-b", 1);
        second.resurface_condition = ResurfaceCondition::AfterIssue(iid("t-a"));

        let error = s.insert(&second).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: t-b -[AfterIssue reference]-> t-a \
             -[AfterIssue reference]-> t-b"
        );
        assert!(matches!(s.get(&iid("t-b")), Err(DbError::NoSuchIssue(_))));
    }

    #[test]
    fn issue_wait_cycle_checks_cross_dependency_and_resurface_edges() {
        let mut s = store();
        for (seq, id) in ["t-a", "t-b", "t-c"].into_iter().enumerate() {
            s.insert(&issue(id, seq as i64)).unwrap();
        }
        s.add_dep(&iid("t-a"), &iid("t-b")).unwrap();
        s.apply(
            &iid("t-b"),
            Change::SetResurfaceCondition(ResurfaceCondition::AfterIssue(iid("t-c"))),
            &ctx(),
        )
        .unwrap();

        let error = s.add_dep(&iid("t-c"), &iid("t-a")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: t-c -[issue dependency]-> t-a \
             -[issue dependency]-> t-b -[AfterIssue reference]-> t-c"
        );
    }

    #[test]
    fn group_deps_are_added_once_and_removable() {
        let mut s = store();
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
    fn group_dependency_cycles_are_rejected_at_the_store_boundary() {
        let mut s = store();
        for id in ["foundation", "platform", "delivery"] {
            s.insert_group(&group(id)).unwrap();
        }
        let foundation = GroupId::from_stored("foundation");
        let platform = GroupId::from_stored("platform");
        let delivery = GroupId::from_stored("delivery");
        s.add_group_dep(&foundation, &platform).unwrap();
        s.add_group_dep(&platform, &delivery).unwrap();

        let error = s.add_group_dep(&delivery, &foundation).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: delivery -[group dependency]-> foundation \
             -[group dependency]-> platform -[group dependency]-> delivery"
        );
        assert_eq!(s.group_deps().unwrap().len(), 2);
    }

    #[test]
    fn group_parent_cycles_are_rejected_at_the_store_boundary() {
        let mut s = store();
        let mut cyclic = group("loop");
        cyclic.parent = Some(cyclic.id.clone());

        let error = s.insert_group(&cyclic).unwrap_err();

        assert_eq!(
            error.to_string(),
            "cycle would be created: loop -[group parent]-> loop"
        );
        assert!(s.groups().unwrap().is_empty());
    }

    #[test]
    fn existing_cycles_can_be_read_and_removed() {
        let mut s = store();
        s.insert(&issue("t-a", 0)).unwrap();
        s.insert(&issue("t-b", 1)).unwrap();
        s.conn
            .execute_batch(
                "INSERT INTO issue_deps VALUES ('t-a', 't-b');
                 INSERT INTO issue_deps VALUES ('t-b', 't-a');",
            )
            .unwrap();

        assert_eq!(s.deps().unwrap().len(), 2);
        assert_eq!(read_issue_wait_edges(&s.conn).unwrap().len(), 2);
        s.remove_dep(&iid("t-a"), &iid("t-b")).unwrap();
        s.remove_dep(&iid("t-b"), &iid("t-a")).unwrap();
        assert!(s.deps().unwrap().is_empty());
    }

    #[test]
    fn groups_round_trip() {
        let mut s = store();
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

        let cond = ResurfaceCondition::AfterIssue(iid("t-2"));
        s.apply(
            &iid("t-1"),
            Change::SetResurfaceCondition(cond.clone()),
            &ctx(),
        )
        .unwrap();
        assert_eq!(s.get(&iid("t-1")).unwrap().resurface_condition, cond);

        let events = s.events(&iid("t-1")).unwrap();
        assert_eq!(events[0].new_value.as_deref(), Some("AfterIssue(t-2)"));

        s.apply(
            &iid("t-1"),
            Change::SetResurfaceCondition(ResurfaceCondition::Always),
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            s.get(&iid("t-1")).unwrap().resurface_condition,
            ResurfaceCondition::Always
        );
    }

    #[test]
    fn all_returns_issues_in_creation_order() {
        let mut s = store();
        s.insert(&issue("t-2", 1)).unwrap();
        s.insert(&issue("t-1", 0)).unwrap();
        let ids: Vec<String> = s.all().unwrap().iter().map(|i| i.id.to_string()).collect();
        assert_eq!(ids, ["t-1", "t-2"]);
    }
}
