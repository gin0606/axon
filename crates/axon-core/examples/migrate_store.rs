//! One-off conversion of an `axon-file/v1` store (`.axon/state.jsonl`) into the record set.
//!
//! Usage: `migrate_store OLD_STATE_JSONL NEW_AXON_DIR`. NEW_AXON_DIR must not exist. The tool
//! writes `records/`, `header.json`, `.gitignore` and `.gitattributes`, then reads the written
//! files back and compares every Entity's current value with the old Entity rows. It prints
//! the counts per mapping row as `key<TAB>count`.
use axon_core::lifecycle::record::{
    Current, Entry, Header, Nonce, Note, Record, RecordId, RecordKind, Store, decode_as, encode,
    encode_header,
};
use axon_core::lifecycle::{EntityId, Kind, Lifecycle, Operation, Recorder, StoreId};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Fail = Box<dyn std::error::Error>;

const MIGRATION_KEYS: [&str; 8] = [
    "imported_at",
    "operation",
    "purpose",
    "source_entity",
    "source_schema",
    "source_sha256",
    "source_store",
    "legacy_link",
];

struct OldEntity {
    kind: Kind,
    current: Value,
}
struct OldState {
    entity: String,
    parents: Vec<String>,
    context: Value,
    event: Value,
}

#[derive(Default)]
struct Counts(BTreeMap<String, usize>);
impl Counts {
    fn add(&mut self, key: impl Into<String>) {
        *self.0.entry(key.into()).or_default() += 1;
    }
}

fn main() -> Result<(), Fail> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: migrate_store OLD_STATE_JSONL NEW_AXON_DIR".into());
    }
    let (old, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    if out.exists() {
        return Err(format!("{} already exists", out.display()).into());
    }
    let text = fs::read_to_string(&old)?;
    let mut lines = text.lines();
    let file_header: Value = serde_json::from_str(lines.next().ok_or("empty file")?)?;
    if file_header["format"] != "axon-file/v1" {
        return Err(format!("unknown file format {}", file_header["format"]).into());
    }
    let prefix = file_header["prefix"]
        .as_str()
        .ok_or("no prefix")?
        .to_string();
    let store_row: Value = serde_json::from_str(lines.next().ok_or("no store header")?)?;
    if store_row["type"] != "Header" || store_row["format"] != "axon-lifecycle/v1" {
        return Err("unexpected second line".into());
    }
    let store = StoreId::try_from(store_row["store"].as_str().ok_or("no store")?.to_string())?;

    let mut counts = Counts::default();
    counts.add("old.header_lines");
    counts.add("old.header_lines");
    let mut entities: BTreeMap<String, OldEntity> = BTreeMap::new();
    let mut states: BTreeMap<String, OldState> = BTreeMap::new();
    let mut notes: Vec<Value> = Vec::new();
    for line in lines {
        let row: Value = serde_json::from_str(line)?;
        let value = row["value"].clone();
        match row["type"].as_str() {
            Some("Entity") => {
                counts.add("old.entity");
                let kind = serde_json::from_value(value["kind"].clone())?;
                let id = value["id"].as_str().ok_or("entity id")?.to_string();
                let current = value["current"].clone();
                entities.insert(id, OldEntity { kind, current });
            }
            Some("State") => {
                let id = value["id"].as_str().ok_or("state id")?.to_string();
                let parents = value["parents"]
                    .as_array()
                    .ok_or("state parents")?
                    .iter()
                    .map(|p| p.as_str().unwrap().to_string())
                    .collect();
                states.insert(
                    id,
                    OldState {
                        entity: value["entity"].as_str().ok_or("state entity")?.to_string(),
                        parents,
                        context: value["context"].clone(),
                        event: value["event"].clone(),
                    },
                );
            }
            Some("Note") => {
                counts.add("old.note");
                notes.push(value);
            }
            other => return Err(format!("unknown row type {other:?}").into()),
        }
    }

    let mut written: Vec<Entry> = Vec::new();
    // old State ID -> the new record ID that stands for it (a collapsed Group Start or
    // Release stands for its parent).
    let mut mapped: BTreeMap<String, RecordId> = BTreeMap::new();
    let mut new_records: BTreeMap<RecordId, Record> = BTreeMap::new();

    for (entity_id, entity) in &entities {
        let order = causal_order(entity_id, &states)?;
        let text = text_of(&entity.current)?;
        for old_id in order {
            let state = &states[&old_id];
            let (at, recorder, stripped) = context(&state.context)?;
            let event_name = state
                .event
                .as_object()
                .unwrap()
                .keys()
                .next()
                .unwrap()
                .clone();
            let event = &state.event[&event_name];
            let parent_ids: Vec<RecordId> = state
                .parents
                .iter()
                .map(|p| {
                    mapped
                        .get(p)
                        .cloned()
                        .ok_or(format!("{old_id}: parent {p} unmapped"))
                })
                .collect::<Result<_, _>>()?;
            let entity_ref = EntityId::try_from(entity_id.clone())?;
            let record = match event_name.as_str() {
                "Created" => {
                    counts.add("old.state.created");
                    let kind: Kind = serde_json::from_value(event["kind"].clone())?;
                    if kind != entity.kind {
                        return Err(format!("{entity_id}: kind changed").into());
                    }
                    let lifecycle: Lifecycle = serde_json::from_value(event["initial"].clone())?;
                    Some(Record {
                        entity: entity_ref,
                        kind: RecordKind::Created,
                        parents: BTreeSet::new(),
                        at,
                        recorder: recorder.clone(),
                        reason: None,
                        after: current(kind, lifecycle, None, &text),
                    })
                }
                "Transition" => {
                    let operation = operation(event["operation"].as_str().ok_or("operation")?)?;
                    let kind_name = if entity.kind == Kind::Group {
                        "group"
                    } else {
                        "issue"
                    };
                    counts.add(format!("old.state.transition.{kind_name}.{operation:?}"));
                    let [parent] = parent_ids.as_slice() else {
                        return Err(format!("{old_id}: transition without one parent").into());
                    };
                    let before = &new_records[parent].after;
                    let old_before: Lifecycle = serde_json::from_value(event["before"].clone())?;
                    if stored(entity.kind, old_before) != before.lifecycle {
                        return Err(format!("{old_id}: before does not match its parent").into());
                    }
                    let reason = event["reason"].as_str().map(str::to_string);
                    if entity.kind == Kind::Group
                        && matches!(operation, Operation::Start | Operation::Release)
                    {
                        counts.add(format!("new.note.group_{operation:?}"));
                        let command = if operation == Operation::Start {
                            "start"
                        } else {
                            "release"
                        };
                        let actor =
                            recorder.as_ref().map_or("記録者なし", |r| r.actor.as_str());
                        let reason_text = reason
                            .as_ref()
                            .map_or(String::new(), |r| format!("、理由: {r}"));
                        let body = format!(
                            "旧形式の保存先から移行した記録: この Group は {} に {actor} が `axon {command}` した{reason_text}。新形式では Group の `{operation:?}` を記録しないため、この Note に写した。",
                            at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
                        );
                        written.push(Entry::Note(Note {
                            entity: entity_ref,
                            nonce: nonce(&format!("group-{command}:{old_id}")),
                            at,
                            recorder: recorder.clone(),
                            reason: None,
                            body,
                        }));
                        if stripped {
                            counts.add("new.recorder_data_stripped");
                        }
                        mapped.insert(old_id.clone(), parent.clone());
                        None
                    } else {
                        let lifecycle = operation.apply_as(entity.kind, before.lifecycle)?;
                        let old_after: Lifecycle = serde_json::from_value(event["after"].clone())?;
                        if stored(entity.kind, old_after) != lifecycle {
                            return Err(format!("{old_id}: after differs from the rule").into());
                        }
                        let owner = match (operation, lifecycle) {
                            (Operation::Start, _) => recorder.as_ref().map(|r| r.actor.clone()),
                            (_, Lifecycle::InProgress) => before.owner.clone(),
                            _ => None,
                        };
                        Some(Record {
                            entity: entity_ref,
                            kind: RecordKind::Transition(operation),
                            parents: BTreeSet::from([parent.clone()]),
                            at,
                            recorder: recorder.clone(),
                            reason,
                            after: current(entity.kind, lifecycle, owner, &text),
                        })
                    }
                }
                "Integration" => {
                    let inputs = event["inputs"].as_array().ok_or("inputs")?;
                    let selected = event["selected"].as_u64().ok_or("selected")? as usize;
                    let chosen_old = inputs[selected]["head"].as_str().ok_or("head")?;
                    let chosen = mapped.get(chosen_old).cloned().ok_or("chosen unmapped")?;
                    let old_lifecycle: Lifecycle =
                        serde_json::from_value(inputs[selected]["current"]["lifecycle"].clone())?;
                    if stored(entity.kind, old_lifecycle) != new_records[&chosen].after.lifecycle {
                        return Err(format!("{old_id}: chosen lifecycle differs").into());
                    }
                    let distinct: BTreeSet<RecordId> = parent_ids.iter().cloned().collect();
                    let reason = event["reason"].as_str().map(str::to_string);
                    let value = new_records[&chosen].after.clone();
                    let concurrent = distinct.len() == 2 && {
                        let v: Vec<&RecordId> = distinct.iter().collect();
                        !precedes(&new_records, v[0], v[1]) && !precedes(&new_records, v[1], v[0])
                    };
                    if concurrent {
                        counts.add("old.state.integration.concurrent");
                        Some(Record {
                            entity: entity_ref,
                            kind: RecordKind::Resolve { chosen },
                            parents: distinct,
                            at,
                            recorder: recorder.clone(),
                            reason,
                            after: value,
                        })
                    } else {
                        let key = match (state.parents.len(), inputs.len(), distinct.len()) {
                            (2, 2, 2) => "ancestral",
                            (2, 2, 1) => "ancestral_after_collapse",
                            (1, 2, 1) => "single_parent",
                            _ => return Err(format!("{old_id}: unexpected integration").into()),
                        };
                        counts.add(format!("old.state.integration.{key}"));
                        let descendant = distinct
                            .iter()
                            .find(|d| {
                                distinct
                                    .iter()
                                    .all(|o| o == *d || precedes(&new_records, o, d))
                            })
                            .cloned()
                            .ok_or("no descendant")?;
                        if new_records[&descendant].after != value {
                            return Err(format!(
                                "{old_id}: the chosen value differs from the descendant; decide by hand"
                            )
                            .into());
                        }
                        let note = "旧形式の統合の記録 (親が祖先関係にある) を通常の記録に写した";
                        let reason = Some(match reason {
                            Some(r) => format!("{note}: {r}"),
                            None => note.to_string(),
                        });
                        Some(Record {
                            entity: entity_ref,
                            kind: RecordKind::Edit,
                            parents: BTreeSet::from([descendant]),
                            at,
                            recorder: recorder.clone(),
                            reason,
                            after: value,
                        })
                    }
                }
                other => return Err(format!("{old_id}: unknown event {other}").into()),
            };
            if let Some(record) = record {
                if stripped {
                    counts.add("new.recorder_data_stripped");
                }
                let entry = Entry::Record(record.clone());
                let id = entry.id()?;
                mapped.insert(old_id.clone(), id.clone());
                new_records.insert(id, record);
                written.push(entry);
            }
        }
    }
    for value in &notes {
        let (at, recorder, stripped) = context(&value["context"])?;
        if stripped {
            counts.add("new.recorder_data_stripped");
        }
        if value["parents"].as_array().is_some_and(|p| !p.is_empty()) {
            counts.add("old.note.parents_dropped");
        }
        let old_id = value["id"].as_str().ok_or("note id")?;
        written.push(Entry::Note(Note {
            entity: EntityId::try_from(value["entity"].as_str().ok_or("note entity")?.to_string())?,
            nonce: nonce(&format!("note:{old_id}")),
            at,
            recorder,
            reason: None,
            body: value["body"].as_str().ok_or("note body")?.to_string(),
        }));
    }

    write_store(&out, &store, &prefix, &written)?;
    verify(&out, &entities, &written, &mut counts)?;
    for (key, count) in &counts.0 {
        println!("{key}\t{count}");
    }
    Ok(())
}

/// The Entity's State IDs with every parent before its children.
fn causal_order(entity: &str, states: &BTreeMap<String, OldState>) -> Result<Vec<String>, Fail> {
    let own: BTreeMap<&String, &OldState> =
        states.iter().filter(|(_, s)| s.entity == entity).collect();
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut order = Vec::new();
    while order.len() < own.len() {
        let ready: Vec<&String> = own
            .iter()
            .filter(|(id, s)| !done.contains(**id) && s.parents.iter().all(|p| done.contains(p)))
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            return Err(format!("{entity}: missing parent or cycle").into());
        }
        for id in ready {
            done.insert(id.clone());
            order.push(id.clone());
        }
    }
    Ok(order)
}

fn precedes(records: &BTreeMap<RecordId, Record>, before: &RecordId, after: &RecordId) -> bool {
    let mut stack = vec![after.clone()];
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if &id == before {
            return true;
        }
        if seen.insert(id.clone()) {
            stack.extend(records[&id].parents.iter().cloned());
        }
    }
    false
}

struct Text {
    title: String,
    description: String,
    condition: Option<String>,
    parent: Option<EntityId>,
    needs: BTreeSet<EntityId>,
}
fn text_of(current: &Value) -> Result<Text, Fail> {
    Ok(Text {
        title: current["title"].as_str().ok_or("title")?.to_string(),
        description: current["description"]
            .as_str()
            .ok_or("description")?
            .to_string(),
        condition: current["condition"].as_str().map(str::to_string),
        parent: current["parent"]
            .as_str()
            .map(|p| EntityId::try_from(p.to_string()))
            .transpose()?,
        needs: current["dependencies"]
            .as_array()
            .ok_or("dependencies")?
            .iter()
            .map(|d| EntityId::try_from(d.as_str().unwrap().to_string()))
            .collect::<Result<_, _>>()?,
    })
}
fn current(kind: Kind, lifecycle: Lifecycle, owner: Option<String>, text: &Text) -> Current {
    Current {
        kind,
        lifecycle,
        owner,
        title: text.title.clone(),
        description: text.description.clone(),
        condition: text.condition.clone(),
        parent: text.parent.clone(),
        needs: text.needs.clone(),
    }
}
/// A Group never stores InProgress: its InProgress is derived from its children.
fn stored(kind: Kind, lifecycle: Lifecycle) -> Lifecycle {
    if kind == Kind::Group && lifecycle == Lifecycle::InProgress {
        Lifecycle::NotStarted
    } else {
        lifecycle
    }
}
fn operation(name: &str) -> Result<Operation, Fail> {
    Ok(serde_json::from_value(Value::String(name.to_string()))?)
}
fn nonce(seed: &str) -> Nonce {
    let hex = blake3::hash(seed.as_bytes()).to_hex().to_string();
    Nonce::try_from(&hex[..32]).unwrap()
}
/// The time and the recorder with the migration-only data keys removed; true if any was.
fn context(context: &Value) -> Result<(DateTime<Utc>, Option<Recorder>, bool), Fail> {
    let at: DateTime<Utc> = serde_json::from_value(context["at"].clone())?;
    let Some(recorder) = context["recorder"].as_object() else {
        return Ok((at, None, false));
    };
    let mut recorder: Map<String, Value> = recorder.clone();
    let mut stripped = false;
    if let Some(data) = recorder.get_mut("data").and_then(Value::as_object_mut) {
        for key in MIGRATION_KEYS {
            stripped |= data.remove(key).is_some();
        }
    }
    let recorder: Recorder = serde_json::from_str(&serde_json::to_string(&recorder)?)?;
    Ok((at, Some(recorder), stripped))
}

fn write_store(out: &Path, store: &StoreId, prefix: &str, entries: &[Entry]) -> Result<(), Fail> {
    let records = out.join("records");
    fs::create_dir_all(&records)?;
    fs::write(out.join(".gitignore"), "*.lock\n*.tmp\n")?;
    fs::write(out.join(".gitattributes"), "* -text\n")?;
    let mut header = Header::new(prefix)?;
    header.store = store.clone();
    fs::write(out.join("header.json"), encode_header(&header)?)?;
    for entry in entries {
        let bytes = encode(entry)?;
        let id = RecordId::of(&bytes);
        let directory = records.join(id.subdirectory());
        fs::create_dir_all(&directory)?;
        let path = directory.join(id.as_ref());
        if path.exists() {
            return Err(format!("duplicate record {id}").into());
        }
        fs::write(path, bytes)?;
    }
    Ok(())
}

/// Reads the written files back and compares each Entity's current value with the old row.
fn verify(
    out: &Path,
    entities: &BTreeMap<String, OldEntity>,
    written: &[Entry],
    counts: &mut Counts,
) -> Result<(), Fail> {
    let mut store = Store::new();
    for directory in fs::read_dir(out.join("records"))? {
        for file in fs::read_dir(directory?.path())? {
            let path = file?.path();
            let id = RecordId::try_from(path.file_name().unwrap().to_str().unwrap())?;
            store.insert(decode_as(&id, &fs::read(&path)?)?)?;
        }
    }
    if let Some((id, problem)) = store.problems().first() {
        return Err(format!("record {id}: {problem}").into());
    }
    let view = store.view()?;
    if !view.gaps().is_empty() || !view.violations().is_empty() {
        return Err("gaps or violations after conversion".into());
    }
    let written_notes = written.iter().filter(|e| e.as_note().is_some()).count();
    if store.len() != written.len() || store.notes().count() != written_notes {
        return Err("the files read back differ from the records written".into());
    }
    let old_notes = counts.0.get("old.note").copied().unwrap_or(0);
    let group_notes: usize = counts
        .0
        .iter()
        .filter(|(key, _)| key.starts_with("new.note.group_"))
        .map(|(_, count)| count)
        .sum();
    if written_notes != old_notes + group_notes {
        return Err("Note count differs from the old Notes and the Group Start Notes".into());
    }
    counts.0.insert("new.records_total".into(), store.len());
    counts.0.insert("new.notes".into(), written_notes);
    for (id, old) in entities {
        let entity = EntityId::try_from(id.clone())?;
        let heads = view.heads(&entity).ok_or(format!("{id}: missing"))?;
        if heads.len() != 1 {
            return Err(format!("{id}: {} heads", heads.len()).into());
        }
        let now = view.current(&entity).ok_or(format!("{id}: no current"))?;
        let text = text_of(&old.current)?;
        let old_lifecycle: Lifecycle = serde_json::from_value(old.current["lifecycle"].clone())?;
        // A Group's effective lifecycle is derived now. An old InProgress Group must still be
        // effectively InProgress; any other Group whose derived value moved is reported.
        let effective = view.effective_lifecycle(&entity);
        if effective != Some(old_lifecycle) {
            if old_lifecycle == Lifecycle::InProgress {
                return Err(format!("{id}: no longer effectively InProgress").into());
            }
            counts.add(format!(
                "verified.group_effective_changed.{old_lifecycle:?}->{effective:?}"
            ));
            eprintln!("effective lifecycle changed: {id} {old_lifecycle:?} -> {effective:?}");
        }
        let same = now.kind == old.kind
            && now.lifecycle == stored(old.kind, old_lifecycle)
            && now.title == text.title
            && now.description == text.description
            && now.condition == text.condition
            && now.parent == text.parent
            && now.needs == text.needs;
        if !same {
            return Err(format!("{id}: current value differs").into());
        }
        counts.add("verified.entity");
    }
    if view.settled().count() != entities.len() {
        return Err("entity count differs".into());
    }
    Ok(())
}
