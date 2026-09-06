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
pub const SCHEMA: u32 = 13;
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
