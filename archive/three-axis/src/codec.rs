use crate::core::{
    Error, Event, History, MetadataValue, ProgressEvent, Result, StateSnapshot, StoreSnapshot,
};
use crate::derived::Evaluation;
use crate::domain::*;
use crate::history::{Baseline, CausalState, Lineage, Link, MergeRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::rc::Rc;

pub const FORMAT: u32 = 1;
pub const SCHEMA: u32 = 14;
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Row {
    Header {
        format: u32,
        schema: u32,
        metadata: BTreeMap<String, MetadataValue>,
    },
    Entity {
        entity: Entity,
        dependencies: Vec<EntityId>,
        lineage: Lineage,
    },
    Revision {
        value: DeclarationRevision,
        link: Link,
    },
    Note {
        value: Note,
        link: Link,
    },
    Decision {
        value: Event,
        link: Link,
    },
    Progress {
        value: ProgressEvent,
        link: Link,
    },
    Merge {
        id: RecordId,
        value: MergeRecord,
        link: Link,
    },
    Baseline {
        id: RecordId,
        value: Baseline,
        link: Link,
    },
}
fn invalid(message: impl ToString) -> Error {
    Error::InvalidState(message.to_string())
}
fn validate_metadata(state: &StateSnapshot) -> Result<()> {
    let Some(MetadataValue::Text(store)) = state.metadata.get("store_id") else {
        return Err(invalid("missing store ID"));
    };
    if store.parse::<RecordId>().map_err(invalid)?.kind() != RecordKind::Store {
        return Err(invalid("invalid store ID"));
    }
    if !matches!(state.metadata.get("prefix"), Some(MetadataValue::Text(p)) if !p.is_empty()) {
        return Err(invalid("missing Entity prefix"));
    }
    Ok(())
}
pub fn encode(state: &StateSnapshot) -> Result<Vec<u8>> {
    state.validate_history()?;
    validate_metadata(state)?;
    let mut bytes = Vec::new();
    let mut write = |row: Row| -> Result<()> {
        serde_json::to_writer(&mut bytes, &row).map_err(invalid)?;
        bytes.push(b'\n');
        Ok(())
    };
    write(Row::Header {
        format: FORMAT,
        schema: SCHEMA,
        metadata: state.metadata.clone(),
    })?;
    let mut entities = state.declaration.entities.iter().collect::<Vec<_>>();
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    for entity in entities {
        let mut dependencies: Vec<_> = state
            .declaration
            .dependencies
            .iter()
            .filter(|(id, _)| *id == entity.id)
            .map(|(_, target)| target.clone())
            .collect();
        dependencies.sort();
        write(Row::Entity {
            entity: entity.clone(),
            dependencies,
            lineage: state.causal.owners[&entity.id].clone(),
        })?;
    }
    let mut records = BTreeMap::new();
    for history in state.histories.values() {
        for value in &history.revisions {
            let mut value = value.clone();
            value.dependencies.sort();
            records.insert(
                value.id,
                Row::Revision {
                    link: state.causal.links[&value.id].clone(),
                    value,
                },
            );
        }
        for value in &history.notes {
            records.insert(
                value.id,
                Row::Note {
                    value: value.clone(),
                    link: state.causal.links[&value.id].clone(),
                },
            );
        }
        for value in &history.decisions {
            records.insert(
                value.id,
                Row::Decision {
                    value: value.clone(),
                    link: state.causal.links[&value.id].clone(),
                },
            );
        }
        for value in &history.progress {
            records.insert(
                value.id,
                Row::Progress {
                    value: value.clone(),
                    link: state.causal.links[&value.id].clone(),
                },
            );
        }
    }
    for (id, value) in &state.causal.baselines {
        records.insert(
            *id,
            Row::Baseline {
                id: *id,
                value: value.clone(),
                link: state.causal.links[id].clone(),
            },
        );
    }
    for (id, value) in &state.causal.merges {
        let mut value = value.clone();
        value.inputs.sort_by(|a, b| a.identity.cmp(&b.identity));
        records.insert(
            *id,
            Row::Merge {
                id: *id,
                value,
                link: state.causal.links[id].clone(),
            },
        );
    }
    for id in state.causal.order()? {
        write(
            records
                .remove(&id)
                .ok_or_else(|| invalid("missing encoded record"))?,
        )?;
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8], evaluation: Rc<Evaluation>) -> Result<StateSnapshot> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    let mut state = StateSnapshot {
        declaration: StoreSnapshot {
            entities: vec![],
            dependencies: vec![],
            evaluation,
        },
        metadata: BTreeMap::new(),
        histories: BTreeMap::new(),
        causal: CausalState::default(),
    };
    let mut header = false;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let raw: UniqueValue = serde_json::from_str(line).map_err(invalid)?;
        let row: Row = serde_json::from_value(raw.0.clone()).map_err(invalid)?;
        required_fields(&raw.0, &serde_json::to_value(&row).map_err(invalid)?)?;
        match row {
            Row::Header {
                format,
                schema,
                metadata,
            } => {
                if header || format != FORMAT || schema != SCHEMA {
                    return Err(invalid("duplicate or unsupported snapshot header"));
                }
                header = true;
                state.metadata = metadata;
            }
            Row::Entity {
                entity,
                dependencies,
                lineage,
            } => {
                if state
                    .causal
                    .owners
                    .insert(entity.id.clone(), lineage)
                    .is_some()
                {
                    return Err(invalid("duplicate Entity"));
                }
                state
                    .declaration
                    .dependencies
                    .extend(dependencies.into_iter().map(|d| (entity.id.clone(), d)));
                state.declaration.entities.push(entity);
            }
            Row::Revision { value, link } => {
                insert_link(&mut state, value.id, &link)?;
                state
                    .histories
                    .entry(link.owner)
                    .or_default()
                    .revisions
                    .push(value);
            }
            Row::Note { value, link } => {
                insert_link(&mut state, value.id, &link)?;
                state
                    .histories
                    .entry(link.owner)
                    .or_default()
                    .notes
                    .push(value);
            }
            Row::Decision { value, link } => {
                insert_link(&mut state, value.id, &link)?;
                state
                    .histories
                    .entry(link.owner)
                    .or_default()
                    .decisions
                    .push(value);
            }
            Row::Progress { value, link } => {
                insert_link(&mut state, value.id, &link)?;
                state
                    .histories
                    .entry(link.owner)
                    .or_default()
                    .progress
                    .push(value);
            }
            Row::Baseline { id, value, link } => {
                insert_link(&mut state, id, &link)?;
                state.causal.baselines.insert(id, value);
            }
            Row::Merge { id, value, link } => {
                insert_link(&mut state, id, &link)?;
                state.causal.merges.insert(id, value);
            }
        }
    }
    if !header {
        return Err(invalid("missing snapshot header"));
    }
    validate_metadata(&state)?;
    state.validate_history()?;
    crate::core::validate_snapshot_structure(&state.declaration)?;
    let ranks: BTreeMap<_, _> = state
        .causal
        .order()?
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    for history in state.histories.values_mut() {
        order_history(history, &ranks);
    }
    state.declaration.entities.sort_by(|a, b| a.id.cmp(&b.id));
    state.declaration.dependencies.sort();
    Ok(state)
}
fn insert_link(state: &mut StateSnapshot, id: RecordId, link: &Link) -> Result<()> {
    if state.causal.links.insert(id, link.clone()).is_some() {
        return Err(invalid("duplicate record ID"));
    }
    Ok(())
}
pub fn order_history(history: &mut History, ranks: &BTreeMap<RecordId, usize>) {
    history.notes.sort_by_key(|r| ranks[&r.id]);
    history.revisions.sort_by_key(|r| ranks[&r.id]);
    history.decisions.sort_by_key(|r| ranks[&r.id]);
    history.progress.sort_by_key(|r| ranks[&r.id]);
}

fn required_fields(input: &serde_json::Value, normalized: &serde_json::Value) -> Result<()> {
    match (input, normalized) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            if a.keys().collect::<Vec<_>>() != b.keys().collect::<Vec<_>>() {
                return Err(invalid("missing field; explicit null is required"));
            }
            for (key, value) in a {
                required_fields(value, &b[key])?;
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            if a.len() != b.len() {
                return Err(invalid("duplicate set member"));
            }
            for (a, b) in a.iter().zip(b) {
                required_fields(a, b)?;
            }
        }
        _ => {}
    }
    Ok(())
}
struct UniqueValue(serde_json::Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut value = serde_json::Map::new();
                while let Some((key, item)) = map.next_entry::<String, UniqueValue>()? {
                    if value.insert(key, item.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(UniqueValue(serde_json::Value::Object(value)))
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(serde_json::Value::Array(values)))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(serde_json::Value::Null))
            }
        }
        d.deserialize_any(Visitor)
    }
}

pub(crate) fn upgrade_v13(bytes: &[u8]) -> crate::db::Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| crate::db::DbError::Storage(error.to_string()))?;
    let mut output = Vec::new();
    let mut header = false;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut raw: UniqueValue = serde_json::from_str(line)
            .map_err(|error| crate::db::DbError::Storage(error.to_string()))?;
        if raw.0["type"] == "Header" {
            if header || raw.0["format"] != FORMAT || raw.0["schema"] != 13 {
                return Err(crate::db::DbError::Storage(
                    "duplicate or unsupported snapshot header".into(),
                ));
            }
            raw.0["schema"] = SCHEMA.into();
            header = true;
        }
        upgrade_row_v13(&mut raw.0)?;
        serde_json::to_writer(&mut output, &raw.0)
            .map_err(|error| crate::db::DbError::Storage(error.to_string()))?;
        output.push(b'\n');
    }
    if !header {
        return Err(crate::db::DbError::Storage(
            "missing snapshot Header".into(),
        ));
    }
    Ok(output)
}

fn upgrade_row_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    let Some(fields) = value.as_object_mut() else {
        return Ok(false);
    };
    let kind = fields
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut changed = false;
    match kind.as_str() {
        "Entity" => {
            if let Some(entity) = fields.get_mut("entity") {
                changed |= upgrade_entity_v13(entity)?;
            }
        }
        "Decision" => {
            if let Some(event) = fields.get_mut("value") {
                changed |= upgrade_event_v13(event)?;
            }
        }
        "Merge" => {
            if let Some(value) = fields.get_mut("value") {
                changed |= upgrade_merge_v13(value)?;
            }
        }
        "Baseline" => {
            if let Some(value) = fields.get_mut("value") {
                changed |= upgrade_baseline_v13(value)?;
            }
        }
        _ => {}
    }
    if matches!(
        kind.as_str(),
        "Revision" | "Note" | "Decision" | "Progress" | "Merge" | "Baseline"
    ) && let Some(link) = fields.get_mut("link")
    {
        changed |= upgrade_link_v13(link)?;
    }
    Ok(changed)
}

fn upgrade_condition_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    let Some(fields) = value.as_object_mut() else {
        return Ok(false);
    };
    let Some(at) = fields.get_mut("AtDate") else {
        return Ok(false);
    };
    let serde_json::Value::String(date) = at else {
        return Err(crate::db::DbError::InvalidSchema(
            "v13 AtDate payload must be a date string".into(),
        ));
    };
    let parsed = date.parse::<chrono::NaiveDate>().map_err(|_| {
        crate::db::DbError::InvalidSchema(format!("invalid v13 AtDate payload: {date}"))
    })?;
    *date = format!("{parsed}T00:00:00Z");
    Ok(true)
}

fn upgrade_entity_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    value
        .get_mut("resurface_condition")
        .map(upgrade_condition_v13)
        .transpose()
        .map(|changed| changed.unwrap_or(false))
}

fn upgrade_proof_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    value
        .get_mut("resurface")
        .map(upgrade_condition_v13)
        .transpose()
        .map(|changed| changed.unwrap_or(false))
}

pub(crate) fn upgrade_link_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    let Some(result) = value.get_mut("result") else {
        return Ok(false);
    };
    if result.is_null() {
        Ok(false)
    } else {
        upgrade_proof_v13(result)
    }
}

fn upgrade_bundle_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    value
        .get_mut("entity")
        .map(upgrade_entity_v13)
        .transpose()
        .map(|changed| changed.unwrap_or(false))
}

pub(crate) fn upgrade_merge_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    let mut changed = false;
    if let Some(inputs) = value
        .get_mut("inputs")
        .and_then(serde_json::Value::as_array_mut)
    {
        for input in inputs {
            if let Some(candidate) = input.get_mut("candidate") {
                changed |= upgrade_bundle_v13(candidate)?;
            }
        }
    }
    if let Some(result) = value.get_mut("result") {
        changed |= upgrade_bundle_v13(result)?;
    }
    Ok(changed)
}

pub(crate) fn upgrade_baseline_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    value
        .get_mut("bundle")
        .map(upgrade_bundle_v13)
        .transpose()
        .map(|changed| changed.unwrap_or(false))
}

fn upgrade_event_v13(value: &mut serde_json::Value) -> crate::db::Result<bool> {
    let Some(fields) = value.as_object_mut() else {
        return Ok(false);
    };
    if fields.get("field").and_then(serde_json::Value::as_str) != Some("resurface_condition") {
        return Ok(false);
    }
    let mut changed = false;
    for key in ["old_value", "new_value"] {
        if let Some(serde_json::Value::String(label)) = fields.get_mut(key)
            && let Some(date) = label
                .strip_prefix("AtDate(")
                .and_then(|value| value.strip_suffix(')'))
        {
            let parsed = date.parse::<chrono::NaiveDate>().map_err(|_| {
                crate::db::DbError::InvalidSchema(format!(
                    "invalid v13 AtDate history label: {label}"
                ))
            })?;
            *label = format!("AtDate({parsed}T00:00:00Z)");
            changed = true;
        }
    }
    Ok(changed)
}
