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
    include_str!("schema_v12.sql"),
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
        super::read_state(
            &conn,
            std::rc::Rc::new(crate::derived::Evaluation::new(
                database.parent().unwrap_or(Path::new(".")).to_owned(),
            )),
        )?;
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
        if ![11, 12, 13].contains(&found) {
            return Err(invalid(format!(
                "manual migration requires schema v11/v12/v13, found v{found}"
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
        let staging = output.join("staging");
        fs::create_dir(&staging)?;
        failure.stage = "converting staged snapshot";
        let mut current = backup.clone();
        let mut mappings = serde_json::json!([]);
        if found == 11 {
            let linear = staging.join("linear");
            migrate_v11(&current, &linear).map_err(|e| *e.source)?;
            let manifest: serde_json::Value =
                serde_saphyr::from_str(&fs::read_to_string(linear.join("manifest.yaml"))?)
                    .map_err(|e| invalid(e.to_string()))?;
            mappings = manifest["mappings"].clone();
            current = linear.join("axon.db");
        }
        if found <= 12 {
            let causal = staging.join("causal");
            migrate_v12(&current, &causal).map_err(|e| *e.source)?;
            current = causal.join("axon.db");
        }
        checkpoint("after-conversion")?;
        let fixed = Connection::open_with_flags(&current, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
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
                let (target, _) = open(&destination).map_err(|e| *e.source)?;
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
            "mappings":mappings, "identity":"v11 deterministic IDs; all v12/v13 store and record IDs preserved",
            "verification":"all source fields retained through staged conversion; schema, integrity, foreign keys, causal validation, canonical round-trip and final backend readback passed"
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
fn migrate_v11(source_path: &Path, output: &Path) -> Result<Outcome, Failure> {
    let mut failure = Failure {
        database: source_path.to_owned(),
        stage: "opening source",
        from: None,
        to: 12,
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
        tx.execute_batch(include_str!("schema_v12.sql"))?;
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
        tx.pragma_update(None, "user_version", 12)?;
        validate_schema(&tx, 12)?;
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
            target_schema: 12,
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
            to: 12,
            backup,
        })
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
}

fn add_causality(conn: &Connection, source_schema: u32) -> super::Result<Vec<u8>> {
    let evaluation = std::rc::Rc::new(crate::derived::Evaluation::new(PathBuf::from(".")));
    let mut state = super::read_linear_state(conn, evaluation.clone())?;
    state.migrate_causality(source_schema)?;
    super::publish_causal(conn, &Default::default(), &state.causal)?;
    let canonical = crate::codec::encode(&state)?;
    let decoded = crate::codec::decode(&canonical, evaluation)?;
    if crate::codec::encode(&decoded)? != canonical {
        return Err(invalid("canonical round-trip mismatch"));
    }
    Ok(canonical)
}
fn write_new(path: &Path, bytes: &[u8]) -> super::Result<()> {
    use std::io::Write;
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn migrate_v12(source_path: &Path, output: &Path) -> Result<Outcome, Failure> {
    let mut failure = Failure {
        database: source_path.to_owned(),
        stage: "opening v12 source",
        from: Some(12),
        to: SCHEMA_VERSION,
        backup: None,
        applied: ApplicationState::NotApplied,
        source: Box::new(invalid("conversion not run")),
    };
    let result = (|| -> super::Result<Outcome> {
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        source.busy_timeout(std::time::Duration::from_secs(5))?;
        source.execute_batch("BEGIN")?;
        if version(&source)? != 12 {
            return Err(invalid("source version changed"));
        }
        validate_schema(&source, 12)?;
        integrity(&source)?;
        validate_record_ids(&source)?;
        fs::create_dir(output)?;
        let backup = output.join("source-v12.db");
        failure.backup = Some(backup.clone());
        failure.stage = "saving v12 snapshot";
        copy_database(&source, &backup)?;
        source.execute_batch("COMMIT")?;
        let fixed = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let original = tables(&fixed)?;
        let destination = output.join("axon.db");
        copy_database(&fixed, &destination)?;
        let mut target =
            Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        configure(&target)?;
        let tx = target.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        failure.stage = "adding causal history";
        tx.execute_batch(include_str!("schema_causal.sql"))?;
        let canonical = add_causality(&tx, 12)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        validate_schema(&tx, SCHEMA_VERSION)?;
        integrity(&tx)?;
        let after = tables(&tx)?;
        for (name, table) in &original {
            if logical_digest(&BTreeMap::from([(name.clone(), table.clone())]))
                != logical_digest(&BTreeMap::from([(name.clone(), after[name].clone())]))
            {
                return Err(invalid(format!("v12 table changed: {name}")));
            }
        }
        tx.commit()?;
        target.close().map_err(|(_, e)| DbError::from(e))?;
        File::open(&destination)?.sync_all()?;
        write_new(&output.join("snapshot.jsonl"), &canonical)?;
        let manifest = serde_json::json!({"format":1,"source_schema":12,"target_schema":SCHEMA_VERSION,
            "source_logical_digest":logical_digest(&original),"source_backup_blake3":blake3::hash(&fs::read(&backup)?).to_hex().to_string(),
            "target_blake3":blake3::hash(&fs::read(&destination)?).to_hex().to_string(),
            "snapshot_blake3":blake3::hash(&canonical).to_hex().to_string(),
            "identity":"all existing store and record IDs preserved", "verification":"all original tables unchanged; causal integrity and canonical round-trip passed"});
        failure.stage = "publishing manifest";
        failure.applied = ApplicationState::Unknown;
        write_new(
            &output.join("manifest.yaml"),
            serde_saphyr::to_string(&manifest)
                .map_err(|e| invalid(e.to_string()))?
                .as_bytes(),
        )?;
        File::open(output)?.sync_all()?;
        File::open(output.parent().unwrap_or(Path::new(".")))?.sync_all()?;
        Ok(Outcome::Migrated {
            database: destination,
            from: 12,
            to: SCHEMA_VERSION,
            backup,
        })
    })();
    result.map_err(|source| {
        *failure.source = source;
        failure
    })
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
