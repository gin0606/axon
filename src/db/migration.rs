use super::{DbError, FRESH_SCHEMA, SCHEMA_VERSION, configure};
use crate::record_id::{RecordId, RecordKind};
use rusqlite::types::Value;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

const OLDEST_VERSION: i64 = 9;
const SCHEMAS: &[&str] = &[
    include_str!("schema_v9.sql"),
    include_str!("schema_v10.sql"),
    include_str!("schema_v11.sql"),
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
    "schema migration for {database} (stage: {stage}, version {from:?} -> {to}, applied: {applied:?}, backup: {backup:?}): {source}"
)]
pub struct Failure {
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

pub fn open(database: &Path) -> Result<(Connection, Outcome), Failure> {
    let result = (|| -> super::Result<_> {
        let conn = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&conn)?;
        conn.execute_batch("BEGIN")?;
        let found = version(&conn)?;
        if found != SCHEMA_VERSION {
            return Err(DbError::UnsupportedSchema {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        validate_schema(&conn, found)?;
        validate_record_ids(&conn)?;
        conn.execute_batch("COMMIT")?;
        Ok((
            conn,
            Outcome::Current {
                database: database.to_owned(),
                version: found,
            },
        ))
    })();
    result.map_err(|source| Failure {
        database: database.to_owned(),
        stage: "checking schema",
        from: None,
        to: SCHEMA_VERSION,
        backup: None,
        applied: ApplicationState::NotApplied,
        source: Box::new(source),
    })
}

fn validate_record_ids(conn: &Connection) -> super::Result<()> {
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
fn field<'a>(table: &Table, row: &'a [Value], column: &str) -> &'a Value {
    &row[table
        .columns
        .iter()
        .position(|c| c == column)
        .expect("known schema column")]
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
fn mapped_id(digest: &str, table: &str, keys: &[&Value], kind: RecordKind) -> RecordId {
    let mut bytes = Vec::new();
    encode(&mut bytes, digest.as_bytes());
    encode(&mut bytes, table.as_bytes());
    for key in keys {
        encode_value(&mut bytes, key);
    }
    RecordId::deterministic(kind, &bytes)
}

#[derive(Serialize)]
struct Mapping {
    table: String,
    entity: String,
    old_number: i64,
    id: String,
}
#[derive(Serialize)]
struct Manifest {
    format: u32,
    source_schema: i64,
    target_schema: i64,
    source_logical_digest: String,
    source_backup_blake3: String,
    target_blake3: String,
    store_id: String,
    row_counts: BTreeMap<String, usize>,
    mappings: Vec<Mapping>,
    verification: &'static str,
}

fn copy_database(source: &Connection, destination: &Path) -> super::Result<()> {
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
pub fn migrate(source_path: &Path, output: &Path) -> Result<Outcome, Failure> {
    let mut failure = Failure {
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
        if found != 11 {
            return Err(invalid(format!(
                "manual stable-ID migration requires schema v11, found v{found}"
            )));
        }
        validate_schema(&source, found)?;
        integrity(&source)?;
        fs::create_dir(output)?;
        failure.stage = "saving source snapshot";
        let backup = output.join("source-v11.db");
        failure.backup = Some(backup.clone());
        copy_database(&source, &backup)?;
        source.execute_batch("COMMIT")?;
        let fixed = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let input = tables(&fixed)?;
        let digest = logical_digest(&input);
        let store_id = RecordId::deterministic(RecordKind::Store, digest.as_bytes());
        if input["meta"]
            .rows
            .iter()
            .any(|r| field(&input["meta"], r, "key") == &Value::Text("store_id".into()))
        {
            return Err(invalid(
                "v11 metadata already contains reserved store_id; no metadata was overwritten",
            ));
        }
        failure.stage = "converting records";
        let destination = output.join("axon.db");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let mut target =
            Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&target)?;
        let tx = target.transaction()?;
        tx.execute_batch(FRESH_SCHEMA)?;
        tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
        let mut mappings = Vec::new();
        let mut expected = BTreeMap::new();
        for (name, table) in &input {
            let mut converted = table.clone();
            match name.as_str() {
                "declaration_revisions" => converted.columns.push("sequence".into()),
                "entity_notes" | "entity_events" | "entity_progress_events" => {
                    converted.columns.push("record_id".into())
                }
                _ => {}
            }
            for row in &mut converted.rows {
                let entity = if name == "entities" {
                    field(table, row, "id").clone()
                } else if table.columns.iter().any(|c| c == "entity_id") {
                    field(table, row, "entity_id").clone()
                } else {
                    Value::Null
                };
                let reference_column = match name.as_str() {
                    "entities" => Some("current_revision"),
                    "declaration_revisions" | "revision_dependencies" | "entity_events" => {
                        Some("revision")
                    }
                    _ => None,
                };
                if let Some(column) = reference_column {
                    let index = table.columns.iter().position(|c| c == column).unwrap();
                    if row[index] != Value::Null {
                        let old = row[index].clone();
                        let id = mapped_id(
                            &digest,
                            "declaration_revisions",
                            &[&entity, &old],
                            RecordKind::Revision,
                        );
                        row[index] = Value::Text(id.to_string());
                        if name == "declaration_revisions" {
                            let Value::Integer(number) = old else {
                                return Err(invalid("non-integer old revision"));
                            };
                            row.push(Value::Integer(number));
                            let Value::Text(owner) = &entity else {
                                return Err(invalid("invalid revision owner"));
                            };
                            mappings.push(Mapping {
                                table: name.clone(),
                                entity: owner.clone(),
                                old_number: number,
                                id: id.to_string(),
                            });
                        }
                    }
                }
                let record = match name.as_str() {
                    "entity_notes" => Some(("note", RecordKind::Note)),
                    "entity_events" => Some(("id", RecordKind::Decision)),
                    "entity_progress_events" => Some(("id", RecordKind::Progress)),
                    _ => None,
                };
                if let Some((column, kind)) = record {
                    let old = field(table, row, column);
                    let id = mapped_id(&digest, name, &[&entity, old], kind);
                    let Value::Integer(number) = old else {
                        return Err(invalid("non-integer old record key"));
                    };
                    let Value::Text(owner) = &entity else {
                        return Err(invalid("invalid record owner"));
                    };
                    mappings.push(Mapping {
                        table: name.clone(),
                        entity: owner.clone(),
                        old_number: *number,
                        id: id.to_string(),
                    });
                    row.push(Value::Text(id.to_string()));
                }
            }
            if name == "meta" {
                converted.rows.push(vec![
                    Value::Text("store_id".into()),
                    Value::Text(store_id.to_string()),
                ]);
            }
            let columns = converted.columns.join(",");
            let placeholders = vec!["?"; converted.columns.len()].join(",");
            for row in &converted.rows {
                tx.execute(
                    &format!("INSERT INTO {name} ({columns}) VALUES ({placeholders})"),
                    rusqlite::params_from_iter(row),
                )?;
            }
            expected.insert(name.clone(), converted);
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        validate_schema(&tx, SCHEMA_VERSION)?;
        integrity(&tx)?;
        validate_record_ids(&tx)?;
        for entity in super::read_all(&tx)? {
            super::read_notes(&tx, &entity.id)?;
            super::read_revisions(&tx, &entity.id)?;
            super::read_events(&tx, &entity.id)?;
            super::read_progress_events(&tx, &entity.id)?;
        }
        // Compare every value by column name, independent of physical column/row order.
        let actual = tables(&tx)?;
        for (name, expected_table) in &expected {
            let actual_table = &actual[name];
            let mut expected_rows: Vec<_> = expected_table
                .rows
                .iter()
                .map(|r| {
                    let mut bytes = Vec::new();
                    for c in &actual_table.columns {
                        encode_value(&mut bytes, field(expected_table, r, c));
                    }
                    bytes
                })
                .collect();
            let mut actual_rows: Vec<_> = actual_table
                .rows
                .iter()
                .map(|r| {
                    let mut b = Vec::new();
                    for v in r {
                        encode_value(&mut b, v);
                    }
                    b
                })
                .collect();
            expected_rows.sort();
            actual_rows.sort();
            if expected_rows != actual_rows {
                return Err(invalid(format!("full data comparison failed for {name}")));
            }
        }
        tx.commit()?;
        target.close().map_err(|(_, e)| DbError::from(e))?;
        file.sync_all()?;
        let manifest = Manifest {
            format: 1,
            source_schema: 11,
            target_schema: SCHEMA_VERSION,
            source_logical_digest: digest,
            source_backup_blake3: blake3::hash(&fs::read(&backup)?).to_hex().to_string(),
            target_blake3: blake3::hash(&fs::read(&destination)?).to_hex().to_string(),
            store_id: store_id.to_string(),
            row_counts: input
                .iter()
                .map(|(n, t)| (n.clone(), t.rows.len()))
                .collect(),
            mappings,
            verification: "all fields/rows mapped and compared; schema, foreign keys, integrity and typed reads passed",
        };
        failure.stage = "publishing manifest";
        let bytes = serde_saphyr::to_string(&manifest).map_err(|e| invalid(e.to_string()))?;
        use std::io::Write;
        let mut manifest_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("manifest.yaml"))?;
        failure.applied = ApplicationState::Unknown;
        manifest_file.write_all(bytes.as_bytes())?;
        manifest_file.sync_all()?;
        File::open(output)?.sync_all()?;
        File::open(
            output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()?;
        failure.applied = ApplicationState::Applied;
        Ok(Outcome::Migrated {
            database: destination,
            from: 11,
            to: SCHEMA_VERSION,
            backup,
        })
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
}
