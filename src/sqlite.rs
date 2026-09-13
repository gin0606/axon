//! SQLite persists the complete common snapshot, including causal branches.
use crate::lifecycle::{self, Snapshot, StoreId};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{path::Path, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("Result unknown: SQLite commit failed: {0}; inspect saved state before retrying")]
    Commit(rusqlite::Error),
    #[error(transparent)]
    Core(#[from] lifecycle::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
const APPLICATION: i64 = 0x41584c43;
const VERSION: i64 = 1;
const SCHEMA: &str = "CREATE TABLE lifecycle_store (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), prefix TEXT NOT NULL, snapshot BLOB NOT NULL)";

pub struct Store {
    connection: Connection,
}
impl Store {
    /// Creates a new file exclusively. Existing stores must use `open`.
    pub fn create(path: &Path, prefix: &str, snapshot: &Snapshot) -> Result<Self> {
        validate_prefix(prefix)?;
        let bytes = lifecycle::encode(snapshot)?;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(None, "application_id", APPLICATION)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.execute(
            "INSERT INTO lifecycle_store VALUES (1, ?1, ?2)",
            rusqlite::params![prefix, bytes],
        )?;
        tx.commit().map_err(Error::Commit)?;
        connection.busy_timeout(Duration::from_secs(30))?;
        Ok(Self { connection })
    }
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(30))?;
        let store = Self { connection };
        store.read()?;
        Ok(store)
    }
    pub fn read(&self) -> Result<(String, Snapshot)> {
        read(&self.connection)
    }
    /// The lock precedes the read, so concurrent writers never apply stale state.
    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&str, &mut Snapshot) -> Result<T>,
    ) -> Result<T> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (prefix, mut snapshot) = read(&tx)?;
        let original = snapshot.clone();
        let identity = snapshot.store().clone();
        let result = change(&prefix, &mut snapshot)?;
        if snapshot.store() != &identity {
            return Err(invalid("store identity cannot change"));
        }
        if snapshot == original {
            return Ok(result);
        }
        let bytes = lifecycle::encode(&snapshot)?;
        tx.execute(
            "UPDATE lifecycle_store SET snapshot = ?1 WHERE singleton = 1",
            [bytes],
        )?;
        tx.commit().map_err(Error::Commit)?;
        Ok(result)
    }
}
fn read(connection: &Connection) -> Result<(String, Snapshot)> {
    let app: i64 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if app != APPLICATION || version != VERSION {
        return Err(invalid(
            "unsupported or legacy SQLite schema; no migration was performed",
        ));
    }
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
    )?;
    let schema = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if schema != vec![("lifecycle_store".into(), SCHEMA.into())] {
        return Err(invalid("unexpected SQLite schema"));
    }
    let check: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(invalid(format!("corrupt SQLite storage: {check}")));
    }
    let count: i64 =
        connection.query_row("SELECT count(*) FROM lifecycle_store", [], |r| r.get(0))?;
    if count != 1 {
        return Err(invalid("invalid SQLite store row count"));
    }
    let (prefix, bytes): (String, Vec<u8>) = connection.query_row(
        "SELECT prefix, snapshot FROM lifecycle_store WHERE singleton = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    validate_prefix(&prefix)?;
    Ok((prefix, lifecycle::decode(&bytes)?))
}
pub fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Err(invalid("Entity prefix must not be empty"));
    }
    Ok(())
}
pub fn empty() -> Snapshot {
    Snapshot::new(StoreId::generate())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::*;
    #[test]
    fn failed_commit_keeps_the_previous_snapshot() {
        let path =
            std::env::temp_dir().join(format!("axon-sqlite-{:032x}.db", rand::random::<u128>()));
        let before = empty();
        let mut store = Store::create(&path, "t", &before).unwrap();
        store.connection.commit_hook(Some(|| true)).unwrap();
        let result = store.update(|_, snapshot| {
            snapshot.create(
                EntityId::generate("test"),
                Kind::Issue,
                Current {
                    title: "never committed".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::NotStarted,
                    condition: None,
                    parent: None,
                    dependencies: Default::default(),
                },
                Context {
                    at: chrono::Utc::now(),
                    recorder: None,
                },
            )?;
            Ok(())
        });
        assert!(matches!(result, Err(super::Error::Commit(_))));
        assert_eq!(store.read().unwrap().1, before);
        drop(store);
        assert_eq!(Store::open(&path).unwrap().read().unwrap().1, before);
        std::fs::remove_file(path).unwrap();
    }
}
