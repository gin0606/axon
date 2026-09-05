use super::{DbError, FRESH_SCHEMA, SCHEMA_VERSION, configure};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

const OLDEST_VERSION: i64 = 9;
const SCHEMAS: &[&str] = &[
    include_str!("schema_v9.sql"),
    include_str!("schema_v10.sql"),
    FRESH_SCHEMA,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Current {
        database: PathBuf,
        version: i64,
    },
    Migrated {
        database: PathBuf,
        from: i64,
        to: i64,
        backup: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationState {
    NotApplied,
    Applied,
    Unknown,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "schema migration for {database} (version {from:?} -> {to}, applied: {applied:?}, backup: {backup:?}): {source}"
)]
pub struct Failure {
    pub database: PathBuf,
    pub from: Option<i64>,
    pub to: i64,
    pub backup: Option<PathBuf>,
    pub applied: ApplicationState,
    #[source]
    pub source: Box<DbError>,
}

fn invalid(message: impl Into<String>) -> DbError {
    DbError::InvalidSchema(message.into())
}

fn version(conn: &Connection) -> super::Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn schema(version: i64) -> super::Result<&'static str> {
    SCHEMAS
        .get((version - OLDEST_VERSION) as usize)
        .copied()
        .ok_or(DbError::UnsupportedSchema {
            found: version,
            expected: SCHEMA_VERSION,
        })
}

fn normalize(sql: &str) -> String {
    let sql = sql.replace("CREATE TABLE \"entities\"", "CREATE TABLE entities");
    let mut tokens = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_whitespace() {
            continue;
        }
        let mut token = String::from(c);
        if c == '\'' || c == '"' || c == '`' {
            while let Some(next) = chars.next() {
                token.push(next);
                if next == c {
                    if chars.peek() == Some(&c) {
                        token.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
        } else if c.is_alphanumeric() || c == '_' {
            while chars
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || *next == '_')
            {
                token.push(chars.next().unwrap());
            }
            token.make_ascii_lowercase();
        }
        tokens.push(token);
    }
    tokens.join(" ")
}

type SchemaObject = (String, String, String, Option<String>);

fn catalog(conn: &Connection) -> super::Result<Vec<SchemaObject>> {
    Ok(conn.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_,Option<String>>(3)?.map(|s| normalize(&s)))))?
        .collect::<rusqlite::Result<_>>()?)
}

fn validate_schema(conn: &Connection, found: i64) -> super::Result<()> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(schema(found)?)?;
    if catalog(conn)? != catalog(&reference)? {
        return Err(invalid(format!(
            "unknown structure for schema version {found}"
        )));
    }
    Ok(())
}

fn integrity(conn: &Connection) -> super::Result<()> {
    let checks = conn
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if checks != ["ok"] {
        return Err(invalid(format!("integrity_check: {}", checks.join("; "))));
    }
    if conn
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(invalid("foreign_key_check failed"));
    }
    Ok(())
}

fn backup(database: &Path, from: i64, destination: &mut Option<PathBuf>) -> super::Result<()> {
    let directory = database
        .parent()
        .ok_or_else(|| invalid("database has no parent directory"))?
        .join("migration-backups");
    fs::create_dir_all(&directory)?;
    let (path, file) = loop {
        let path = directory.join(format!(
            "v{from}-to-v{SCHEMA_VERSION}-{:032x}.db",
            rand::random::<u128>()
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    *destination = Some(path.clone());
    // A new read connection after BEGIN IMMEDIATE sees the locked committed state,
    // including WAL pages. The writing connection cannot be a backup source.
    let source = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut target = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let result = Backup::new(&source, &mut target)?.step(-1)?;
    if !matches!(result, StepResult::Done) {
        return Err(invalid(format!("backup did not complete: {result:?}")));
    }
    integrity(&target)?;
    if version(&target)? != from {
        return Err(invalid("backup version differs from locked source"));
    }
    target.close().map_err(|(_, error)| DbError::from(error))?;
    file.sync_all()?;
    File::open(&directory)?.sync_all()?;
    File::open(directory.parent().expect("backup directory has a parent"))?.sync_all()?;
    Ok(())
}

fn step(conn: &Connection, from: i64) -> super::Result<()> {
    let next = match from {
        9 => 10,
        10 => 11,
        _ => return Err(invalid(format!("missing migration from version {from}"))),
    };
    let target = Connection::open_in_memory()?;
    target.execute_batch(schema(next)?)?;
    let ddl: String = target.query_row(
        "SELECT sql FROM sqlite_schema WHERE name='entities'",
        [],
        |row| row.get(0),
    )?;
    conn.execute_batch(&ddl.replacen(
        "CREATE TABLE entities",
        "CREATE TABLE entities_migrating",
        1,
    ))?;
    let columns = conn
        .prepare("PRAGMA table_info(entities)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let names = columns
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");
    conn.execute_batch(&format!("INSERT INTO entities_migrating ({names}) SELECT {names} FROM entities; DROP TABLE entities; ALTER TABLE entities_migrating RENAME TO entities;"))?;
    validate_schema(conn, next)?;
    conn.pragma_update(None, "user_version", next)?;
    Ok(())
}

/// Opens an existing database; the caller continues its operation only on success.
/// Backup paths on failure can be incomplete and are never removed automatically.
pub fn open(database: &Path) -> Result<(Connection, Outcome), Failure> {
    open_with_hook(database, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Observed,
    Locked,
    BackedUp,
    Step,
    BeforeCommit,
}

fn open_with_hook(
    database: &Path,
    mut hook: impl FnMut(Phase) -> super::Result<()>,
) -> Result<(Connection, Outcome), Failure> {
    let mut failure = Failure {
        database: database.to_owned(),
        from: None,
        to: SCHEMA_VERSION,
        backup: None,
        applied: ApplicationState::NotApplied,
        source: Box::new(invalid("migration did not run")),
    };
    let result = (|| -> super::Result<(Connection, Outcome)> {
        let resolved = fs::canonicalize(database)?;
        let database = resolved.as_path();
        failure.database = resolved.clone();
        let conn = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&conn)?;
        // End the initial read snapshot before waiting for the writer lock.
        conn.execute_batch("BEGIN")?;
        let found = version(&conn)?;
        failure.from = Some(found);
        validate_schema(&conn, found)?;
        conn.execute_batch("COMMIT")?;
        if found == SCHEMA_VERSION {
            return Ok((
                conn,
                Outcome::Current {
                    database: database.to_owned(),
                    version: found,
                },
            ));
        }
        hook(Phase::Observed)?;
        conn.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE")?;
        let migration = (|| -> super::Result<Outcome> {
            let found = version(&conn)?;
            failure.from = Some(found);
            validate_schema(&conn, found)?;
            hook(Phase::Locked)?;
            if found == SCHEMA_VERSION {
                return Ok(Outcome::Current {
                    database: database.to_owned(),
                    version: found,
                });
            }
            integrity(&conn)?;
            backup(database, found, &mut failure.backup)?;
            hook(Phase::BackedUp)?;
            for current in found..SCHEMA_VERSION {
                step(&conn, current)?;
                hook(Phase::Step)?;
            }
            integrity(&conn)?;
            super::read_all(&conn)?;
            hook(Phase::BeforeCommit)?;
            Ok(Outcome::Migrated {
                database: database.to_owned(),
                from: found,
                to: SCHEMA_VERSION,
                backup: failure.backup.clone().expect("backup completed"),
            })
        })();
        let outcome = match migration {
            Ok(outcome) => outcome,
            Err(error) => {
                if conn.execute_batch("ROLLBACK").is_err() {
                    failure.applied = ApplicationState::Unknown;
                }
                return Err(error);
            }
        };
        if let Err(error) = conn.execute_batch("COMMIT") {
            // A failed COMMIT can leave a transaction active, or SQLite can have
            // rolled it back. An I/O error is not evidence of either outcome.
            failure.applied = ApplicationState::Unknown;
            if !conn.is_autocommit() && conn.execute_batch("ROLLBACK").is_ok() {
                failure.applied = ApplicationState::NotApplied;
            }
            return Err(error.into());
        }
        if matches!(outcome, Outcome::Migrated { .. }) {
            failure.applied = ApplicationState::Applied;
        }
        configure(&conn)?;
        Ok((conn, outcome))
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
}

#[cfg(test)]
mod tests;
