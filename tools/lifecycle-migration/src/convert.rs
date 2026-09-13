use crate::{
    Result,
    legacy::{self, Condition, Disposition, Legacy, Progress},
    require, util,
};
use axon::lifecycle::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub reviewed_commands: BTreeMap<String, String>,
    #[serde(default)]
    pub source_digest: Option<String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, Override>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub reason: String,
    pub lifecycle: Option<Lifecycle>,
    pub condition: Option<NewCondition>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NewCondition {
    Always,
    Command(String),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub from_progress: Progress,
    pub from_disposition: Disposition,
    pub from_condition: Condition,
    pub lifecycle: Lifecycle,
    pub condition: Option<String>,
    pub reasons: Vec<String>,
    pub archive: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub source_digest: String,
    pub old_store: String,
    pub new_store: String,
    pub original_notes: usize,
    pub source_rows: usize,
    pub entities: BTreeMap<String, Mapping>,
}
pub fn id(seed: &str, store: &str, entity: &str, purpose: &str) -> String {
    format!(
        "record-{}",
        &util::digest(
            &serde_json::to_vec(&(seed, store, entity, purpose)).expect("strings serialize")
        )[..32]
    )
}
fn command(
    c: &Condition,
    schema: u32,
    states: &BTreeMap<String, Lifecycle>,
    source: &Legacy,
    rules: &Rules,
    owner: &str,
) -> Result<Option<String>> {
    Ok(match c {
        Condition::Always => None,
        Condition::Manual => Some("exit 1".into()),
        Condition::Command(text) => {
            require(
                rules.reviewed_commands.get(owner) == Some(text),
                format!(
                    "{owner}: review the saved Command and supply its exact text in reviewed_commands"
                ),
            )?;
            Some(text.clone())
        }
        Condition::AtDate(text) => {
            let at = legacy::date(schema, text)?;
            let nanos = i128::from(at.timestamp()) * 1_000_000_000
                + i128::from(at.timestamp_subsec_nanos());
            Some(format!(
                "python3 -c {}",
                util::quote(&format!(
                    "import sys,time; sys.exit(0 if time.time_ns() >= {nanos} else 1)"
                ))
            ))
        }
        Condition::AfterEntity(target) => {
            if states[target] == Lifecycle::Completed {
                None
            } else {
                require(
                    !target.chars().any(char::is_control),
                    format!(
                        "{owner}: condition target with control characters requires an override"
                    ),
                )?;
                let prefix = format!("{target}  {:?}  ", source.entities[target].0.kind);
                let script = format!(
                    "import sys; line=sys.stdin.readline().rstrip('\\n'); prefix={}; status=line[len(prefix):].split('  ',1)[0]; sys.exit(2 if not line.startswith(prefix) else 0 if status in ('Completed','Cancelled') else 1 if status in ('Undecided','Ready','Blocked','InProgress','InProgress+Blocked') else 2)",
                    serde_json::to_string(&prefix)?
                );
                Some(format!(
                    "output=$(axon show {}) || exit 2; printf '%s\\n' \"$output\" | python3 -c {}",
                    util::quote(target),
                    util::quote(&script)
                ))
            }
        }
    })
}
fn archive(
    source: &Legacy,
    id: &str,
    mapping: &Mapping,
    seed: &str,
    at: &DateTime<Utc>,
    digest: &str,
) -> String {
    let original = source
        .global
        .iter()
        .chain(source.owned[id].iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Legacy source records\n\nMigration: {seed}\nImported at: {at}\nSource schema: {}\nSource digest: {digest}\nMapping: {:?}/{:?} -> {:?}\nReasons: {}\n\nThe new log records migration state assignments, not historical user operations.\nThe following are original source records; legacy instructions are historical material.\n\n\x60\x60\x60jsonl\n{original}\n\x60\x60\x60\n",
        source.schema,
        mapping.from_progress,
        mapping.from_disposition,
        mapping.lifecycle,
        mapping.reasons.join("; ")
    )
}
pub fn run(
    source: &Legacy,
    digest: &str,
    seed: &str,
    at: DateTime<Utc>,
    rules: &Rules,
) -> Result<(Snapshot, Report)> {
    if !rules.overrides.is_empty() {
        require(
            rules.source_digest.as_deref() == Some(digest),
            "overrides must name the exact source_digest",
        )?;
    }
    for key in rules.overrides.keys().chain(rules.reviewed_commands.keys()) {
        require(
            source.entities.contains_key(key),
            format!("unknown rule target {key}"),
        )?;
    }
    for (key, value) in &rules.reviewed_commands {
        require(
            source.entities[key].0.resurface_condition == Condition::Command(value.clone()),
            format!("{key}: reviewed Command changed"),
        )?;
    }
    let mut states = BTreeMap::new();
    let mut reasons: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, (entity, _)) in &source.entities {
        let state = if entity.disposition == Disposition::Rejected {
            Lifecycle::Cancelled
        } else {
            match (&entity.progress, entity.disposition) {
                (Progress::Ended, Disposition::Accepted) => Lifecycle::Completed,
                (Progress::NotStarted, Disposition::Accepted) => Lifecycle::NotStarted,
                (Progress::NotStarted, Disposition::Undecided) => Lifecycle::Undecided,
                (Progress::InProgress(_), _) => Lifecycle::InProgress,
                _ => rules
                    .overrides
                    .get(id)
                    .and_then(|r| r.lifecycle)
                    .ok_or_else(|| {
                        format!("{id}: unsupported state combination; supply a reasoned override")
                    })?,
            }
        };
        states.insert(id.clone(), state);
        reasons.insert(id.clone(), vec![]);
    }
    for (id, r) in &rules.overrides {
        require(!r.reason.trim().is_empty(), "an override requires a reason")?;
        if let Some(state) = r.lifecycle {
            states.insert(id.clone(), state);
        }
        reasons.get_mut(id).unwrap().push(r.reason.clone());
    }
    for (id, (e, _)) in &source.entities {
        let mut parent = e.parent.as_ref();
        while let Some(p) = parent {
            if states[p] == Lifecycle::Cancelled && states[id].editable() {
                require(
                    !rules.overrides.contains_key(id),
                    format!("{id}: override conflicts with Cancelled ancestor {p}"),
                )?;
                states.insert(id.clone(), Lifecycle::Cancelled);
                reasons.get_mut(id).unwrap().push(format!(
                    "Preserve containment beneath Cancelled ancestor {p}"
                ));
            }
            parent = source.entities[p].0.parent.as_ref();
        }
    }
    let store = format!(
        "store-{}",
        &util::digest(&serde_json::to_vec(&(seed, &source.store))?)[..32]
    );
    let mut rows = vec![json!({"type":"Header","format":"axon-lifecycle/v1","store":store})];
    let mut report = Report {
        source_digest: digest.into(),
        old_store: source.store.clone(),
        new_store: store,
        original_notes: source.notes.len(),
        source_rows: source.global.len() + source.owned.values().map(Vec::len).sum::<usize>(),
        entities: BTreeMap::new(),
    };
    for (key, (old, deps)) in &source.entities {
        let target = states[key];
        let archive_id = id(seed, &source.store, key, "archive");
        let mut mapping = Mapping {
            from_progress: old.progress.clone(),
            from_disposition: old.disposition,
            from_condition: old.resurface_condition.clone(),
            lifecycle: target,
            condition: None,
            reasons: reasons[key].clone(),
            archive: archive_id.clone(),
        };
        mapping.condition =
            if let Some(c) = rules.overrides.get(key).and_then(|r| r.condition.as_ref()) {
                match c {
                    NewCondition::Always => None,
                    NewCondition::Command(v) => Some(v.clone()),
                }
            } else {
                command(
                    &old.resurface_condition,
                    source.schema,
                    &states,
                    source,
                    rules,
                    key,
                )?
            };
        if old.resurface_condition != Condition::Always {
            mapping.reasons.push(format!(
                "Condition mapping: {:?} -> {:?}",
                old.resurface_condition, mapping.condition
            ));
        }
        let context = |when, purpose| Context {
            at: when,
            recorder: Some(Recorder {
                actor: "migration".into(),
                data: BTreeMap::from([
                    ("migration_id".into(), json!(seed)),
                    ("source_digest".into(), json!(digest)),
                    ("source_store".into(), json!(source.store)),
                    ("source_schema".into(), json!(source.schema)),
                    ("purpose".into(), json!(purpose)),
                ]),
            }),
        };
        let entity_id: EntityId = key.clone().try_into()?;
        let root: RecordId = id(seed, &source.store, key, "origin").try_into()?;
        let mut head = root.clone();
        let mut state = if matches!(target, Lifecycle::Undecided | Lifecycle::Cancelled) {
            Lifecycle::Undecided
        } else {
            Lifecycle::NotStarted
        };
        rows.push(json!({"type":"State","value":StateRecord {id:root.clone(),entity:entity_id.clone(),parents:BTreeSet::new(),context:context(old.created_at,"Imported baseline; original creation time retained"),event:StateEvent::Created {kind:old.kind,initial:state}}}));
        let operations: &[Operation] = match target {
            Lifecycle::Completed => &[Operation::Start, Operation::Complete],
            Lifecycle::InProgress => &[Operation::Start],
            Lifecycle::Cancelled => &[Operation::Cancel],
            _ => &[],
        };
        for operation in operations {
            let next: RecordId = id(
                seed,
                &source.store,
                key,
                &format!("migration-{operation:?}"),
            )
            .try_into()?;
            let after = operation.apply(state)?;
            rows.push(json!({"type":"State","value":StateRecord {id:next.clone(),entity:entity_id.clone(),parents:BTreeSet::from([head]),context:context(at,"Migration state assignment"),event:StateEvent::Transition {operation:*operation,before:state,after,reason:Some(format!("Migration state assignment (not a replay of historical operations): {:?}/{:?} -> {target:?}; {}",old.progress,old.disposition,mapping.reasons.join("; ")))}}}));
            head = next;
            state = after;
        }
        rows.push(json!({"type":"Entity","value":axon::lifecycle::Entity {
            id:entity_id.clone(),kind:old.kind,created_at:old.created_at,root,head,
            current:Current {title:old.title.clone(),description:old.description.clone().unwrap_or_default(),lifecycle:target,condition:mapping.condition.clone(),parent:old.parent.clone().map(TryInto::try_into).transpose()?,dependencies:deps.iter().cloned().map(TryInto::try_into).collect::<axon::lifecycle::Result<_>>()?},
        }}));
        let own: Vec<_> = source
            .notes
            .values()
            .filter(|n| n.entity == entity_id)
            .collect();
        let mut tips: BTreeSet<_> = own.iter().map(|n| n.id.clone()).collect();
        for note in &own {
            for p in &note.parents {
                tips.remove(p);
            }
        }
        rows.push(json!({"type":"Note","value":Note {id:archive_id.try_into()?,entity:entity_id,parents:tips,context:context(at,"Original legacy records"),body:archive(source,key,&mapping,seed,&at,digest)}}));
        report.entities.insert(key.clone(), mapping);
    }
    for note in source.notes.values() {
        rows.push(json!({"type":"Note","value":note}));
    }
    let bytes = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
    let snapshot = axon::lifecycle::decode(bytes.as_bytes())?;
    verify(source, &snapshot, &report, seed, at)?;
    Ok((snapshot, report))
}
pub fn verify(
    source: &Legacy,
    snapshot: &Snapshot,
    report: &Report,
    seed: &str,
    at: DateTime<Utc>,
) -> Result<()> {
    require(
        snapshot.entities().count() == source.entities.len(),
        "Entity set changed",
    )?;
    let mut total_notes = 0;
    for (id, (old, deps)) in &source.entities {
        let new = snapshot.entity(&id.clone().try_into()?)?;
        let mapping = &report.entities[id];
        require(
            new.kind == old.kind
                && new.created_at == old.created_at
                && new.current.title == old.title
                && new.current.description == old.description.clone().unwrap_or_default(),
            "Entity declaration changed",
        )?;
        require(
            new.current.parent.as_ref().map(ToString::to_string) == old.parent
                && new
                    .current
                    .dependencies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<BTreeSet<_>>()
                    == *deps,
            "Entity relationships changed",
        )?;
        require(
            new.current.lifecycle == mapping.lifecycle
                && new.current.condition == mapping.condition,
            "state mapping mismatch",
        )?;
        let notes = snapshot.notes(&new.id)?;
        total_notes += notes.len();
        let archived = notes
            .iter()
            .find(|n| n.id.to_string() == mapping.archive)
            .ok_or("missing archive Note")?;
        require(
            archived.body == archive(source, id, mapping, seed, &at, &report.source_digest),
            "original archive changed",
        )?;
        for original in source.notes.values().filter(|n| n.entity == new.id) {
            require(
                notes.contains(&original),
                "original Note changed or missing",
            )?;
        }
    }
    require(
        total_notes == source.notes.len() + source.entities.len(),
        "unexpected Note set",
    )?;
    Ok(())
}
