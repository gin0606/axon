use super::{DbError, FRESH_SCHEMA, SCHEMA_VERSION};
use crate::record_id::{RecordId, RecordKind};
use rusqlite::types::Value;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::collections::BTreeMap;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    SchemaUpdate,
    BackendConversion,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "schema migration for {database} (stage: {stage}, version {from:?} -> {to}, applied: {applied:?}, backup: {backup:?}): {source}"
)]
pub struct Failure {
    pub kind: Kind,
    pub database: PathBuf,
    pub stage: &'static str,
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
    if version == SCHEMA_VERSION {
        Ok(FRESH_SCHEMA)
    } else {
        Err(DbError::UnsupportedSchema {
            found: version,
            expected: SCHEMA_VERSION,
        })
    }
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
    validate_ddl(conn, found, schema(found)?)
}

pub(super) fn validate_ddl(conn: &Connection, found: i64, ddl: &str) -> super::Result<()> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(ddl)?;
    if catalog(conn)? != catalog(&reference)? {
        return Err(invalid(format!(
            "unknown structure for schema version {found}"
        )));
    }
    Ok(())
}

pub(super) fn integrity(conn: &Connection) -> super::Result<()> {
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

pub fn open(database: &Path) -> Result<(Connection, Outcome), Failure> {
    super::upgrade::open(database)
}

pub(super) fn validate_record_ids(conn: &Connection) -> super::Result<()> {
    let store: String = conn.query_row("SELECT value FROM meta WHERE key='store_id'", [], |r| {
        r.get(0)
    })?;
    let store: RecordId = store.parse()?;
    if store.kind() != RecordKind::Store {
        return Err(invalid("invalid store ID kind"));
    }
    for (table, column, kind) in [
        ("entity_notes", "record_id", RecordKind::Note),
        ("declaration_revisions", "revision", RecordKind::Revision),
        ("entity_events", "record_id", RecordKind::Decision),
        ("entity_progress_events", "record_id", RecordKind::Progress),
    ] {
        let mut stmt = conn.prepare(&format!("SELECT {column} FROM {table}"))?;
        for id in stmt.query_map([], |r| r.get::<_, RecordId>(0))? {
            if id?.kind() != kind {
                return Err(invalid(format!("invalid ID kind in {table}")));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}
const TABLES: &[(&str, &str)] = &[
    ("meta", "key"),
    ("entities", "id"),
    ("entity_deps", "entity_id,depends_on_id"),
    ("declaration_revisions", "entity_id,revision"),
    ("revision_dependencies", "entity_id,revision,depends_on_id"),
    ("entity_notes", "entity_id,note"),
    ("entity_events", "id"),
    ("entity_progress_events", "id"),
];
fn tables(conn: &Connection) -> super::Result<BTreeMap<String, Table>> {
    let mut result = BTreeMap::new();
    for (name, order) in TABLES {
        let columns = conn
            .prepare(&format!("PRAGMA table_info({name})"))?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let rows = conn
            .prepare(&format!("SELECT * FROM {name} ORDER BY {order}"))?
            .query_map([], |r| {
                (0..columns.len())
                    .map(|i| r.get(i))
                    .collect::<rusqlite::Result<Vec<Value>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        result.insert(name.to_string(), Table { columns, rows });
    }
    Ok(result)
}
fn encode(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}
fn encode_value(bytes: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => bytes.push(0),
        Value::Integer(v) => {
            bytes.push(1);
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        Value::Real(v) => {
            bytes.push(2);
            bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        Value::Text(v) => {
            bytes.push(3);
            encode(bytes, v.as_bytes());
        }
        Value::Blob(v) => {
            bytes.push(4);
            encode(bytes, v);
        }
    }
}
fn logical_digest(tables: &BTreeMap<String, Table>) -> String {
    let mut bytes = b"axon v11 logical snapshot v1".to_vec();
    for (name, table) in tables {
        encode(&mut bytes, name.as_bytes());
        bytes.extend_from_slice(&(table.columns.len() as u64).to_be_bytes());
        for column in &table.columns {
            encode(&mut bytes, column.as_bytes());
        }
        bytes.extend_from_slice(&(table.rows.len() as u64).to_be_bytes());
        for row in &table.rows {
            for value in row {
                encode_value(&mut bytes, value);
            }
        }
    }
    blake3::hash(&bytes).to_hex().to_string()
}
pub(super) fn copy_database(source: &Connection, destination: &Path) -> super::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut target = Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let result = Backup::new(source, &mut target)?.step(-1)?;
    if result != StepResult::Done {
        return Err(invalid(format!("backup incomplete: {result:?}")));
    }
    integrity(&target)?;
    target.close().map_err(|(_, e)| DbError::from(e))?;
    file.sync_all()?;
    Ok(())
}

/// The source is read-only; only a newly created output directory is written.
/// A failed output is retained for diagnosis and must not be used as a live store.
pub fn migrate(
    source_path: &Path,
    output: &Path,
    backend: crate::storage::Backend,
) -> Result<Outcome, Failure> {
    migrate_with_checkpoint(source_path, output, backend, |_| Ok(()))
}

fn migrate_with_checkpoint(
    source_path: &Path,
    output: &Path,
    backend: crate::storage::Backend,
    mut checkpoint: impl FnMut(&str) -> super::Result<()>,
) -> Result<Outcome, Failure> {
    let mut failure = Failure {
        kind: Kind::BackendConversion,
        database: source_path.to_owned(),
        stage: "opening source",
        from: None,
        to: SCHEMA_VERSION,
        backup: None,
        applied: ApplicationState::NotApplied,
        source: Box::new(invalid("conversion not run")),
    };
    let result = (|| -> super::Result<_> {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        source.busy_timeout(std::time::Duration::from_secs(5))?;
        source.execute_batch("BEGIN")?;
        let found = version(&source)?;
        failure.from = Some(found);
        if found != SCHEMA_VERSION {
            return Err(invalid(format!(
                "backend conversion requires current schema v{SCHEMA_VERSION}, found v{found}"
            )));
        }
        validate_schema(&source, found)?;
        integrity(&source)?;
        fs::create_dir(output)?;
        failure.stage = "saving source snapshot";
        let backup = output.join(format!("source-v{found}.db"));
        failure.backup = Some(backup.clone());
        copy_database(&source, &backup)?;
        source.execute_batch("COMMIT")?;
        checkpoint("after-backup")?;
        checkpoint("after-conversion")?;
        let fixed = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        validate_schema(&fixed, SCHEMA_VERSION)?;
        integrity(&fixed)?;
        validate_record_ids(&fixed)?;
        let evaluation = std::rc::Rc::new(crate::derived::Evaluation::new(PathBuf::from(".")));
        let state = super::read_state(&fixed, evaluation.clone())?;
        let canonical = crate::codec::encode(&state)?;
        let decoded = crate::codec::decode(&canonical, evaluation.clone())?;
        if crate::codec::encode(&decoded)? != canonical {
            return Err(invalid("canonical round-trip mismatch"));
        }
        let store_id: String =
            fixed.query_row("SELECT value FROM meta WHERE key='store_id'", [], |r| {
                r.get(0)
            })?;
        failure.stage = "writing backend";
        let destination = output.join(match backend {
            crate::storage::Backend::Sqlite => "axon.db",
            crate::storage::Backend::File => "state.jsonl",
        });
        match backend {
            crate::storage::Backend::Sqlite => {
                copy_database(&fixed, &destination)?;
                let target =
                    Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                validate_schema(&target, SCHEMA_VERSION)?;
                if crate::codec::encode(&super::read_state(&target, evaluation.clone())?)?
                    != canonical
                {
                    return Err(invalid("final SQLite snapshot mismatch"));
                }
            }
            crate::storage::Backend::File => {
                write_new(&destination, &canonical)?;
                let reread = crate::codec::decode(&fs::read(&destination)?, evaluation)?;
                if crate::codec::encode(&reread)? != canonical {
                    return Err(invalid("final file snapshot mismatch"));
                }
            }
        }
        write_new(&output.join("snapshot.jsonl"), &canonical)?;
        checkpoint("after-backend")?;
        let input = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let input_tables = all_tables(&input)?;
        let final_tables = all_tables(&fixed)?;
        let manifest = serde_json::json!({
            "format":2, "source_schema":found, "target_schema":SCHEMA_VERSION, "backend":backend,
            "source_backup":backup.file_name().unwrap().to_string_lossy(),
            "target":destination.file_name().unwrap().to_string_lossy(),
            "store_id":store_id, "source_logical_digest":logical_digest(&input_tables),
            "source_backup_blake3":blake3::hash(&fs::read(&backup)?).to_hex().to_string(),
            "target_blake3":blake3::hash(&fs::read(&destination)?).to_hex().to_string(),
            "snapshot_blake3":blake3::hash(&canonical).to_hex().to_string(),
            "row_counts":input_tables.iter().map(|(n,t)|(n.clone(),t.rows.len())).collect::<BTreeMap<_,_>>(),
            "target_row_counts":final_tables.iter().map(|(n,t)|(n.clone(),t.rows.len())).collect::<BTreeMap<_,_>>(),
            "mappings":[], "identity":"all store and record IDs preserved",
            "verification":"all source fields retained through backend conversion; schema, integrity, foreign keys, causal validation, canonical round-trip and final backend readback passed"
        });
        failure.stage = "publishing manifest";
        checkpoint("before-manifest")?;
        failure.applied = ApplicationState::Unknown;
        write_new(
            &output.join("manifest.yaml"),
            serde_saphyr::to_string(&manifest)
                .map_err(|e| invalid(e.to_string()))?
                .as_bytes(),
        )?;
        checkpoint("after-manifest")?;
        File::open(output)?.sync_all()?;
        File::open(
            output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()?;
        checkpoint("after-sync")?;
        Ok(Outcome::Migrated {
            database: destination,
            from: found,
            to: SCHEMA_VERSION,
            backup,
        })
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
}

fn all_tables(conn: &Connection) -> super::Result<BTreeMap<String, Table>> {
    let mut result = tables(conn)?;
    if version(conn)? == 13 {
        for (name, order) in [
            ("history_lineage", "entity_id"),
            ("causal_links", "record_id"),
            ("history_baselines", "record_id"),
            ("history_merges", "record_id"),
        ] {
            let columns = conn
                .prepare(&format!("PRAGMA table_info({name})"))?
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let rows = conn
                .prepare(&format!("SELECT * FROM {name} ORDER BY {order}"))?
                .query_map([], |r| {
                    (0..columns.len())
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            result.insert(name.into(), Table { columns, rows });
        }
    }
    Ok(result)
}
fn write_new(path: &Path, bytes: &[u8]) -> super::Result<()> {
    use std::io::Write;
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_packages_preserve_source_staging_and_retriable_identity() {
        let root = std::env::temp_dir().join(format!(
            "axon-migration-fault-{}",
            RecordId::new(RecordKind::Store)
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.db");
        let mut conn = Connection::open(&source).unwrap();
        super::super::initialize_schema(&mut conn).unwrap();
        conn.execute("INSERT INTO meta VALUES ('prefix','fault')", [])
            .unwrap();
        drop(conn);
        let before = fs::read(&source).unwrap();
        for backend in [
            crate::storage::Backend::Sqlite,
            crate::storage::Backend::File,
        ] {
            for point in [
                "after-backup",
                "after-conversion",
                "after-backend",
                "before-manifest",
                "after-manifest",
                "after-sync",
            ] {
                let output = root.join(format!("{backend:?}-{point}"));
                let error = migrate_with_checkpoint(&source, &output, backend, |stage| {
                    if stage == point {
                        Err(invalid("injected interruption"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
                let published = ["after-manifest", "after-sync"].contains(&point);
                assert_eq!(
                    error.applied,
                    if published {
                        ApplicationState::Unknown
                    } else {
                        ApplicationState::NotApplied
                    }
                );
                assert_eq!(output.join("manifest.yaml").exists(), published);
                assert_eq!(fs::read(&source).unwrap(), before);
                let backup = output.join("source-v13.db");
                assert!(backup.exists());
                let saved = fs::read(&backup).unwrap();
                assert!(migrate(&source, &output, backend).is_err());
                assert_eq!(fs::read(&backup).unwrap(), saved);
                let retry = root.join(format!("{backend:?}-{point}-retry"));
                migrate(&backup, &retry, backend).unwrap();
                if output.join("snapshot.jsonl").exists() {
                    assert_eq!(
                        fs::read(output.join("snapshot.jsonl")).unwrap(),
                        fs::read(retry.join("snapshot.jsonl")).unwrap()
                    );
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
