use super::*;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

const FORMAT: &str = "axon-lifecycle/v1";
#[derive(Serialize)]
#[serde(tag = "type")]
enum Row {
    Header { format: String, store: StoreId },
    Entity { value: Entity },
    State { value: StateRecord },
    Note { value: Note },
}

fn parse_row(line: &str) -> Result<Row> {
    #[derive(Deserialize)]
    struct Tag {
        #[serde(rename = "type")]
        kind: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Header {
        #[serde(rename = "type")]
        _kind: String,
        format: String,
        store: StoreId,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Payload<T> {
        #[serde(rename = "type")]
        _kind: String,
        value: T,
    }
    fn parse<T: DeserializeOwned>(line: &str) -> Result<T> {
        serde_json::from_str(line).map_err(|error| invalid(error.to_string()))
    }
    // Tagged enum buffering would erase the raw JSON needed to distinguish
    // literal metadata objects from serde_json's private number transport.
    match parse::<Tag>(line)?.kind.as_str() {
        "Header" => {
            let header = parse::<Header>(line)?;
            Ok(Row::Header {
                format: header.format,
                store: header.store,
            })
        }
        "Entity" => Ok(Row::Entity {
            value: parse::<Payload<Entity>>(line)?.value,
        }),
        "State" => Ok(Row::State {
            value: parse::<Payload<StateRecord>>(line)?.value,
        }),
        "Note" => Ok(Row::Note {
            value: parse::<Payload<Note>>(line)?.value,
        }),
        _ => Err(invalid("unknown snapshot row type")),
    }
}

pub(super) fn deserialize_recorder_data<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries = BTreeMap::<String, Box<RawValue>>::deserialize(deserializer)?;
    entries
        .into_iter()
        .map(|(key, raw)| {
            literal_json(&raw)
                .map(|value| (key, value))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

fn literal_json(raw: &RawValue) -> serde_json::Result<Value> {
    let text = raw.get();
    match text.as_bytes()[0] {
        b'{' => {
            let entries: BTreeMap<String, &RawValue> = serde_json::from_str(text)?;
            entries
                .into_iter()
                .map(|(key, value)| literal_json(value).map(|v| (key, v)))
                .collect::<serde_json::Result<serde_json::Map<_, _>>>()
                .map(Value::Object)
        }
        b'[' => {
            let entries: Vec<&RawValue> = serde_json::from_str(text)?;
            entries
                .into_iter()
                .map(literal_json)
                .collect::<serde_json::Result<Vec<_>>>()
                .map(Value::Array)
        }
        b'"' => serde_json::from_str(text).map(Value::String),
        b't' | b'f' => serde_json::from_str(text).map(Value::Bool),
        b'n' => Ok(Value::Null),
        _ => serde_json::from_str(text).map(Value::Number),
    }
}
/// Encodes a validated logical snapshot, without opening or publishing a file.
/// Entity keys are sorted; records are topologically ordered with ID tie-breaks.
pub fn encode(snapshot: &Snapshot) -> Result<Vec<u8>> {
    snapshot.validate()?;
    let mut bytes = Vec::new();
    let mut write = |row: Row| -> Result<()> {
        serde_json::to_writer(&mut bytes, &row).map_err(|error| invalid(error.to_string()))?;
        bytes.push(b'\n');
        Ok(())
    };
    write(Row::Header {
        format: FORMAT.into(),
        store: snapshot.store.clone(),
    })?;
    for entity in snapshot.entities.values() {
        write(Row::Entity {
            value: entity.clone(),
        })?;
    }
    for record in snapshot.state_order()? {
        write(Row::State {
            value: record.clone(),
        })?;
    }
    for note in snapshot.note_order()? {
        write(Row::Note {
            value: note.clone(),
        })?;
    }
    Ok(bytes)
}
/// Rejects unknown formats/fields, duplicate rows, invalid records and references.
/// Input row order may differ from canonical order; record identity never does.
pub fn decode(bytes: &[u8]) -> Result<Snapshot> {
    let text = std::str::from_utf8(bytes).map_err(|error| invalid(error.to_string()))?;
    let mut rows = text.lines();
    let first = parse_row(rows.next().ok_or_else(|| invalid("missing header"))?)?;
    let Row::Header { format, store } = first else {
        return Err(invalid("header must be first"));
    };
    if format != FORMAT {
        return Err(invalid("unsupported snapshot format"));
    }
    let mut snapshot = Snapshot::new(store);
    for line in rows {
        match parse_row(line)? {
            Row::Header { .. } => return Err(invalid("duplicate header")),
            Row::Entity { value } => {
                if snapshot.entities.insert(value.id.clone(), value).is_some() {
                    return Err(invalid("duplicate Entity ID"));
                }
            }
            Row::State { value } => {
                if snapshot.notes.contains_key(&value.id)
                    || snapshot.states.insert(value.id.clone(), value).is_some()
                {
                    return Err(invalid("duplicate record ID"));
                }
            }
            Row::Note { value } => {
                if snapshot.states.contains_key(&value.id)
                    || snapshot.notes.insert(value.id.clone(), value).is_some()
                {
                    return Err(invalid("duplicate record ID"));
                }
            }
        }
    }
    snapshot.validate()?;
    Ok(snapshot)
}
