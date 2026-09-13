use crate::{Result, legacy::Legacy, require, util};
use rusqlite::{Connection, OpenFlags, backup::Backup, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs::File, path::Path, time::Duration};

pub const SCHEMA: &str = concat!(
    include_str!("../../../archive/three-axis/src/db/schema_v12.sql"),
    include_str!("../../../archive/three-axis/src/db/schema_causal.sql")
);
pub fn open(path: &Path) -> Result<Connection> {
    util::read(path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_secs(5))?;
    Ok(db)
}
pub fn backup(source: &Path, destination: &Path) -> Result<()> {
    let source = open(source)?;
    util::save_new(destination, &[])?;
    let mut output = Connection::open(destination)?;
    let backup = Backup::new(&source, &mut output)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match backup.step(128)? {
            rusqlite::backup::StepResult::Done => break,
            _ => {
                require(
                    std::time::Instant::now() < deadline,
                    "SQLite backup timed out; retain incomplete artifact",
                )?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    drop(backup);
    output.close().map_err(|(_, e)| e)?;
    File::open(destination)?.sync_all()?;
    util::sync_dir(destination.parent().ok_or("missing parent")?)
}
pub fn quiesce(path: &Path) -> Result<()> {
    util::read(path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(Duration::from_secs(5))?;
    let (busy, _, _): (i64, i64, i64) =
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    require(
        busy == 0,
        "SQLite checkpoint is busy; close all connections",
    )?;
    let mode: String = db.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    require(mode == "delete", "could not leave SQLite WAL mode")?;
    db.execute_batch("BEGIN EXCLUSIVE; COMMIT")?;
    db.close().map_err(|(_, e)| e)?;
    File::open(path)?.sync_all()?;
    util::sync_dir(path.parent().ok_or("missing parent")?)
}
fn schema(db: &Connection) -> Result<Vec<(String, String, String)>> {
    Ok(db.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name")?
        .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<std::result::Result<_,_>>()?)
}
#[derive(Clone, Serialize, Deserialize)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}
impl SqlValue {
    fn json(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(v) => json!(v),
            Self::Text(v) => json!(v),
            Self::Real(v) => json!({"Real":v}),
            Self::Blob(v) => json!({"Bytes":v}),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}
#[derive(Serialize, Deserialize)]
pub struct Dump {
    pub schema: u32,
    pub tables: BTreeMap<String, Table>,
}
pub fn dump(path: &Path) -> Result<Dump> {
    let db = open(path)?;
    db.execute_batch("BEGIN")?;
    let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    require(
        matches!(version, 13 | 14),
        format!("unsupported legacy SQLite schema {version}"),
    )?;
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(SCHEMA)?;
    require(
        schema(&db)? == schema(&expected)?,
        "unknown SQLite schema structure",
    )?;
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    require(integrity == "ok", format!("SQLite integrity: {integrity}"))?;
    require(
        !db.prepare("PRAGMA foreign_key_check")?.exists([])?,
        "SQLite foreign key violation",
    )?;
    let mut tables = BTreeMap::new();
    for (kind, name, _) in schema(&expected)? {
        if kind != "table" {
            continue;
        }
        let mut stmt = db.prepare(&format!("SELECT * FROM {name}"))?;
        let columns = stmt
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut rows = stmt
            .query_map([], |r| {
                (0..columns.len())
                    .map(|i| {
                        Ok(match r.get_ref(i)? {
                            ValueRef::Null => SqlValue::Null,
                            ValueRef::Integer(v) => SqlValue::Integer(v),
                            ValueRef::Real(v) => SqlValue::Real(v.to_bits()),
                            ValueRef::Text(v) => SqlValue::Text(
                                std::str::from_utf8(v)
                                    .map_err(|e| {
                                        rusqlite::Error::FromSqlConversionFailure(
                                            i,
                                            rusqlite::types::Type::Text,
                                            Box::new(e),
                                        )
                                    })?
                                    .into(),
                            ),
                            ValueRef::Blob(v) => SqlValue::Blob(v.into()),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.sort_by_cached_key(|r| serde_json::to_string(r).expect("SQL values serialize"));
        tables.insert(name, Table { columns, rows });
    }
    Ok(Dump {
        schema: version,
        tables,
    })
}
impl Dump {
    pub fn digest(&self) -> Result<String> {
        Ok(util::digest(&serde_json::to_vec(self)?))
    }
    fn rows(&self, table: &str) -> Result<Vec<Value>> {
        let t = self
            .tables
            .get(table)
            .ok_or_else(|| format!("missing table {table}"))?;
        Ok(t.rows
            .iter()
            .map(|r| {
                Value::Object(
                    t.columns
                        .iter()
                        .cloned()
                        .zip(r.iter().map(SqlValue::json))
                        .collect(),
                )
            })
            .collect())
    }
    pub fn legacy(&self) -> Result<Legacy> {
        let mut metadata = BTreeMap::new();
        for r in self.rows("meta")? {
            let key = text(&r, "key")?;
            let value = &r["value"];
            let meta = match value {
                Value::String(s) => json!({"Text":s}),
                Value::Number(n) => json!({"Integer":n}),
                Value::Object(_) => value.clone(),
                _ => return Err("unsupported metadata type".into()),
            };
            require(metadata.insert(key, meta).is_none(), "duplicate metadata")?;
        }
        let mut rows =
            vec![json!({"type":"Header","format":1,"schema":self.schema,"metadata":metadata})];
        let mut links = BTreeMap::new();
        for r in self.rows("causal_links")? {
            let id = text(&r, "record_id")?;
            let value = util::json(&text(&r, "payload")?)?;
            require(links.insert(id, value).is_none(), "duplicate causal link")?;
        }
        let lineage: BTreeMap<_, _> = self
            .rows("history_lineage")?
            .into_iter()
            .map(|r| Ok((text(&r, "entity_id")?, util::json(&text(&r, "payload")?)?)))
            .collect::<Result<_>>()?;
        let dependencies = self.rows("entity_deps")?;
        for e in self.rows("entities")? {
            let id = text(&e, "id")?;
            let progress = match e["progress"].as_str() {
                Some("not_started") => json!("NotStarted"),
                Some("ended") => json!("Ended"),
                Some("in_progress") => {
                    json!({"InProgress":{"actor":e["claimed_actor"],"worktree":e["claimed_worktree"],"at":e["claimed_at"]}})
                }
                _ => return Err("unknown progress".into()),
            };
            let disposition = match e["disposition"].as_str() {
                Some("undecided") => "Undecided",
                Some("accepted") => "Accepted",
                Some("rejected") => "Rejected",
                _ => return Err("unknown disposition".into()),
            };
            let condition = match e["resurface_kind"].as_str() {
                None if e["resurface_kind"].is_null() => json!("Always"),
                Some("manual") => json!("Manual"),
                Some("date") => json!({"AtDate":e["resurface_date"]}),
                Some("after_entity") => json!({"AfterEntity":e["resurface_ref"]}),
                Some("command") => json!({"Command":e["resurface_command"]}),
                _ => return Err("unknown condition".into()),
            };
            rows.push(json!({"type":"Entity","entity":{
                "id":id,"kind":kind(&e["kind"])?,"title":e["title"],"description":e["description"],
                "progress":progress,"disposition":disposition,"current_revision":e["current_revision"],
                "resurface_condition":condition,"parent":e["parent_id"],"created_at":e["created_at"],"updated_at":e["updated_at"]
            },"dependencies":dependencies.iter().filter(|r|r["entity_id"]==id).map(|r|r["depends_on_id"].clone()).collect::<Vec<_>>(),
              "lineage":lineage.get(&id).ok_or("missing lineage")?}));
        }
        let revisions = self.rows("revision_dependencies")?;
        for (table, tag) in [
            ("declaration_revisions", "Revision"),
            ("entity_notes", "Note"),
            ("entity_events", "Decision"),
            ("entity_progress_events", "Progress"),
            ("history_baselines", "Baseline"),
            ("history_merges", "Merge"),
        ] {
            for r in self.rows(table)? {
                let id = text(
                    &r,
                    if tag == "Revision" {
                        "revision"
                    } else {
                        "record_id"
                    },
                )?;
                let link = links.get(&id).ok_or("record has no causal link")?;
                if let Some(owner) = r.get("entity_id") {
                    require(owner == &link["owner"], "SQL/link owner mismatch")?;
                }
                let value = match tag {
                    "Revision" => {
                        json!({"id":id,"title":r["title"],"description":r["description"],"parent":r["parent_id"],"dependencies":revisions.iter().filter(|d|d["revision"]==id).map(|d|d["depends_on_id"].clone()).collect::<Vec<_>>(),"created_at":r["created_at"],"baseline":r["baseline"]==1})
                    }
                    "Note" => {
                        json!({"id":id,"body":r["body"],"actor":r["actor"],"created_at":r["at"]})
                    }
                    "Decision" => {
                        json!({"id":id,"field":r["field"],"old_value":r["old_value"],"new_value":r["new_value"],"revision":r["revision"],"actor":r["actor"],"reason":r["reason"],"at":r["at"]})
                    }
                    "Progress" => {
                        json!({"id":id,"kind":match r["kind"].as_str() {Some("start")=>"Start",Some("done")=>"Done",Some("release")=>"Release",_=>return Err("invalid progress event".into())},"actor":r["actor"],"reason":r["reason"],"at":r["at"]})
                    }
                    _ => util::json(&text(&r, "payload")?)?,
                };
                let mut row = json!({"type":tag,"value":value,"link":link});
                if matches!(tag, "Baseline" | "Merge") {
                    row["id"] = json!(id);
                }
                rows.push(row);
            }
        }
        let bytes = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
        let mut result = Legacy::parse(bytes.as_bytes())?;
        let record_count = result.rows.iter().filter(|r| r.record().is_some()).count();
        require(
            record_count == links.len(),
            "orphan or missing SQL causal record",
        )?;
        require(lineage.len() == result.entities.len(), "orphan SQL lineage")?;
        result.owned.clear();
        result.global.clear();
        for (table, t) in &self.tables {
            for r in &t.rows {
                let map: Value = Value::Object(
                    t.columns
                        .iter()
                        .cloned()
                        .zip(r.iter().map(SqlValue::json))
                        .collect(),
                );
                let raw =
                    serde_json::to_string(&json!({"table":table,"columns":t.columns,"row":r}))?;
                if table == "meta" {
                    result.global.push(raw);
                    continue;
                }
                let owner = if table == "entities" {
                    text(&map, "id")?
                } else if let Some(owner) = map.get("entity_id") {
                    owner.as_str().ok_or("invalid SQL owner")?.into()
                } else {
                    let id = text(&map, "record_id")?;
                    text(links.get(&id).ok_or("orphan SQL record")?, "owner")?
                };
                require(result.entities.contains_key(&owner), "orphan SQL row")?;
                result.owned.entry(owner).or_default().push(raw);
            }
        }
        Ok(result)
    }
}
fn text(value: &Value, key: &str) -> Result<String> {
    Ok(value[key]
        .as_str()
        .ok_or_else(|| format!("expected text column {key}"))?
        .into())
}
fn kind(value: &Value) -> Result<&'static str> {
    match value.as_str() {
        Some("issue") => Ok("Issue"),
        Some("group") => Ok("Group"),
        _ => Err("invalid kind".into()),
    }
}
