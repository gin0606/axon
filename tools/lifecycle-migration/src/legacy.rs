use crate::{Result, require, util};
use axon::lifecycle::{Context, Kind, Note, Recorder};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub actor: String,
    pub worktree: String,
    pub at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Progress {
    NotStarted,
    InProgress(Claim),
    Ended,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Disposition {
    Undecided,
    Accepted,
    Rejected,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    Always,
    Manual,
    AtDate(String),
    AfterEntity(String),
    Command(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub description: Option<String>,
    pub progress: Progress,
    pub disposition: Disposition,
    pub current_revision: Option<String>,
    pub resurface_condition: Condition,
    pub parent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub progress: Progress,
    pub disposition: Disposition,
    pub resurface: Condition,
    pub current_revision: Option<String>,
    pub last_revision: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Origin {
    Operation,
    Created,
    Migration { schema: u32 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub owner: String,
    pub stream: Stream,
    pub parents: BTreeSet<String>,
    pub origin: Origin,
    pub result: Option<Proof>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stream {
    Note,
    Revision,
    State,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    pub last_revision: Option<String>,
    pub head: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub entity: Entity,
    pub dependencies: BTreeSet<String>,
    pub last_revision: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeInput {
    pub identity: String,
    pub heads: BTreeSet<String>,
    pub candidate: Bundle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Merge {
    pub inputs: Vec<MergeInput>,
    pub selected: String,
    pub result: Bundle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub bundle: Bundle,
    pub source_schema: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub parent: Option<String>,
    pub dependencies: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub baseline: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OldNote {
    pub id: String,
    pub body: String,
    pub actor: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub revision: Option<String>,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProgressKind {
    Start,
    Done,
    Release,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressEvent {
    pub id: String,
    pub kind: ProgressKind,
    pub actor: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Metadata {
    Text(String),
    Integer(i64),
    Real(u64),
    Bytes(Vec<u8>),
}
fn record_id(id: &str, prefix: &str) -> Result<()> {
    let suffix = id
        .strip_prefix(&format!("{prefix}-"))
        .ok_or("wrong legacy record kind")?;
    require(
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid legacy record ID",
    )
}
fn proof(e: &Entity, last: Option<String>) -> Proof {
    Proof {
        progress: e.progress.clone(),
        disposition: e.disposition,
        resurface: e.resurface_condition.clone(),
        current_revision: e.current_revision.clone(),
        last_revision: last,
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Row {
    Header {
        format: u32,
        schema: u32,
        metadata: BTreeMap<String, Metadata>,
    },
    Entity {
        entity: Entity,
        dependencies: Vec<String>,
        lineage: Lineage,
    },
    Revision {
        value: Revision,
        link: Link,
    },
    Note {
        value: OldNote,
        link: Link,
    },
    Decision {
        value: Decision,
        link: Link,
    },
    Progress {
        value: ProgressEvent,
        link: Link,
    },
    Merge {
        id: String,
        value: Merge,
        link: Link,
    },
    Baseline {
        id: String,
        value: Baseline,
        link: Link,
    },
}
impl Row {
    pub fn record(&self) -> Option<(&str, &Link, Stream)> {
        Some(match self {
            Self::Revision { value, link } => (&value.id, link, Stream::Revision),
            Self::Note { value, link } => (&value.id, link, Stream::Note),
            Self::Decision { value, link } => (&value.id, link, Stream::State),
            Self::Progress { value, link } => (&value.id, link, Stream::State),
            Self::Merge { id, link, .. } | Self::Baseline { id, link, .. } => {
                (id, link, Stream::State)
            }
            _ => return None,
        })
    }
}
pub struct Legacy {
    pub schema: u32,
    pub prefix: String,
    pub store: String,
    pub entities: BTreeMap<String, (Entity, BTreeSet<String>)>,
    pub notes: BTreeMap<String, Note>,
    pub owned: BTreeMap<String, Vec<String>>,
    pub global: Vec<String>,
    pub rows: Vec<Row>,
}
fn fields(raw: &Value, decoded: &Value) -> Result<()> {
    match (raw, decoded) {
        (Value::Object(a), Value::Object(b)) => {
            require(a.keys().eq(b.keys()), "unknown or missing legacy fields")?;
            for (key, v) in a {
                fields(v, &b[key])?;
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            require(a.len() == b.len(), "duplicate legacy set member")?;
            for (a, b) in a.iter().zip(b) {
                fields(a, b)?;
            }
        }
        _ => (),
    }
    Ok(())
}
pub fn date(schema: u32, text: &str) -> Result<DateTime<Utc>> {
    if schema == 13 {
        require(text.len() == 10, "schema 13 requires YYYY-MM-DD")?;
        Ok(NaiveDate::parse_from_str(text, "%Y-%m-%d")?
            .and_hms_opt(0, 0, 0)
            .ok_or("invalid midnight")?
            .and_utc())
    } else {
        Ok(DateTime::parse_from_rfc3339(text)?.with_timezone(&Utc))
    }
}
fn condition(
    schema: u32,
    c: &Condition,
    entities: &BTreeMap<String, (Entity, BTreeSet<String>)>,
) -> Result<()> {
    match c {
        Condition::AtDate(v) => {
            date(schema, v)?;
        }
        Condition::AfterEntity(id) => require(
            entities.contains_key(id),
            format!("unknown condition target {id}"),
        )?,
        Condition::Command(v) => require(!v.trim().is_empty(), "empty Command")?,
        _ => (),
    }
    Ok(())
}
impl Legacy {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut rows = Vec::new();
        let mut lines = Vec::new();
        for text in std::str::from_utf8(bytes)?
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let raw = util::json(text)?;
            let row: Row = serde_json::from_value(raw.clone())?;
            fields(&raw, &serde_json::to_value(&row)?)?;
            rows.push(row);
            lines.push(text.to_owned());
        }
        let Some(Row::Header {
            format: 1,
            schema: schema @ (13 | 14),
            metadata,
        }) = rows.first()
        else {
            return Err("expected legacy file format 1, schema 13 or 14".into());
        };
        let get = |key: &str| -> Result<String> {
            match metadata.get(key) {
                Some(Metadata::Text(v)) if !v.is_empty() => Ok(v.clone()),
                _ => Err(format!("missing {key}").into()),
            }
        };
        let mut result = Self {
            schema: *schema,
            prefix: get("prefix")?,
            store: get("store_id")?,
            entities: BTreeMap::new(),
            notes: BTreeMap::new(),
            owned: BTreeMap::new(),
            global: vec![lines[0].clone()],
            rows: rows.clone(),
        };
        record_id(&result.store, "store")?;
        let mut links = BTreeMap::new();
        let mut lineages = BTreeMap::new();
        for (row, text) in rows.iter().zip(&lines).skip(1) {
            match row {
                Row::Header { .. } => return Err("duplicate header".into()),
                Row::Entity {
                    entity,
                    dependencies,
                    lineage,
                } => {
                    require(
                        !entity.id.is_empty() && !entity.title.trim().is_empty(),
                        "empty Entity ID/title",
                    )?;
                    let deps: BTreeSet<_> = dependencies.iter().cloned().collect();
                    require(deps.len() == dependencies.len(), "duplicate dependency")?;
                    require(
                        result
                            .entities
                            .insert(entity.id.clone(), (entity.clone(), deps))
                            .is_none(),
                        "duplicate Entity ID",
                    )?;
                    lineages.insert(entity.id.clone(), lineage);
                    result
                        .owned
                        .entry(entity.id.clone())
                        .or_default()
                        .push(text.clone());
                }
                _ => {
                    let (id, link, stream) = row.record().ok_or("missing record")?;
                    require(link.stream == stream, "wrong causal stream")?;
                    require(
                        !id.is_empty() && links.insert(id.to_string(), link).is_none(),
                        "duplicate record ID",
                    )?;
                    result
                        .owned
                        .entry(link.owner.clone())
                        .or_default()
                        .push(text.clone());
                    if let Row::Note { value, link } = row {
                        result.notes.insert(
                            value.id.clone(),
                            Note {
                                id: value.id.clone().try_into()?,
                                entity: link.owner.clone().try_into()?,
                                parents: link
                                    .parents
                                    .iter()
                                    .cloned()
                                    .map(TryInto::try_into)
                                    .collect::<axon::lifecycle::Result<_>>()?,
                                context: Context {
                                    at: value.created_at,
                                    recorder: Some(Recorder {
                                        actor: value.actor.clone(),
                                        data: BTreeMap::from([(
                                            "legacy_link".into(),
                                            serde_json::to_value(link)?,
                                        )]),
                                    }),
                                },
                                body: value.body.clone(),
                            },
                        );
                    }
                }
            }
        }
        for (id, (e, deps)) in &result.entities {
            require(
                (e.disposition == Disposition::Undecided) == e.current_revision.is_none(),
                format!("{id}: invalid current revision"),
            )?;
            condition(result.schema, &e.resurface_condition, &result.entities)?;
            let mut seen = BTreeSet::from([id.clone()]);
            let mut parent = e.parent.as_ref();
            while let Some(p) = parent {
                require(seen.insert(p.clone()), "containment cycle")?;
                let next = &result.entities.get(p).ok_or("unknown parent")?.0;
                require(next.kind == Kind::Group, "parent is not a Group")?;
                parent = next.parent.as_ref();
            }
            for dep in deps {
                require(
                    dep != id && result.entities.contains_key(dep),
                    "invalid dependency",
                )?;
            }
            let lineage = lineages[id];
            for revision in [e.current_revision.as_ref(), lineage.last_revision.as_ref()]
                .into_iter()
                .flatten()
            {
                let link = links.get(revision).ok_or("missing revision")?;
                require(
                    link.owner == *id && link.stream == Stream::Revision,
                    "invalid revision owner/stream",
                )?;
            }
            if let Some(head) = &lineage.head {
                let link = links.get(head).ok_or("missing lineage head")?;
                require(
                    link.owner == *id && link.stream == Stream::State,
                    "invalid head owner/stream",
                )?;
                let expected = Proof {
                    progress: e.progress.clone(),
                    disposition: e.disposition,
                    resurface: e.resurface_condition.clone(),
                    current_revision: e.current_revision.clone(),
                    last_revision: lineage.last_revision.clone(),
                };
                require(
                    link.result.as_ref() == Some(&expected),
                    "current state differs from its proof",
                )?;
            }
        }
        for (id, link) in &links {
            require(
                result.entities.contains_key(&link.owner),
                format!("{id}: unknown record owner"),
            )?;
            if let Some(proof) = &link.result {
                condition(result.schema, &proof.resurface, &result.entities)?;
                for revision in [
                    proof.current_revision.as_ref(),
                    proof.last_revision.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    let r = links.get(revision).ok_or("missing proof revision")?;
                    require(
                        r.owner == link.owner && r.stream == Stream::Revision,
                        "invalid proof revision",
                    )?;
                }
            }
            for parent in &link.parents {
                let p = links.get(parent).ok_or("missing causal parent")?;
                require(
                    p.owner == link.owner && p.stream == link.stream,
                    "cross-owner/stream causal parent",
                )?;
            }
            require(
                !matches!(link.origin,Origin::Migration {schema} if !matches!(schema,11|12)),
                "unknown historical migration origin",
            )?;
            require(
                link.stream == Stream::State || link.result.is_none(),
                "non-state record has a state proof",
            )?;
            require(
                link.stream != Stream::State
                    || matches!(link.origin, Origin::Migration { .. })
                    || link.result.is_some(),
                "state record has no proof",
            )?;
        }
        let revisions: BTreeMap<_, _> = rows
            .iter()
            .filter_map(|r| {
                if let Row::Revision { value, link } = r {
                    Some((value.id.as_str(), (value, link)))
                } else {
                    None
                }
            })
            .collect();
        let validate_bundle = |bundle: &Bundle| -> Result<()> {
            let e = &bundle.entity;
            let current = &result.entities.get(&e.id).ok_or("unknown bundle owner")?.0;
            require(
                current.kind == e.kind && current.created_at == e.created_at,
                "historical bundle changes immutable fields",
            )?;
            require(
                bundle
                    .dependencies
                    .iter()
                    .all(|id| result.entities.contains_key(id))
                    && e.parent
                        .as_ref()
                        .is_none_or(|p| result.entities.contains_key(p)),
                "missing historical bundle reference",
            )?;
            condition(result.schema, &e.resurface_condition, &result.entities)?;
            require(
                (e.disposition == Disposition::Undecided) == e.current_revision.is_none()
                    && (e.current_revision.is_none() || e.current_revision == bundle.last_revision),
                "invalid historical revision state",
            )?;
            for id in [e.current_revision.as_ref(), bundle.last_revision.as_ref()]
                .into_iter()
                .flatten()
            {
                require(
                    revisions
                        .get(id.as_str())
                        .is_some_and(|(_, link)| link.owner == e.id),
                    "invalid historical revision owner",
                )?;
            }
            if let Some(id) = &e.current_revision {
                let r = revisions[id.as_str()].0;
                require(
                    r.title == e.title
                        && r.description == e.description
                        && r.parent == e.parent
                        && r.dependencies.iter().cloned().collect::<BTreeSet<_>>()
                            == bundle.dependencies,
                    "declaration differs from decided revision",
                )?;
            }
            Ok(())
        };
        for (id, (e, deps)) in &result.entities {
            validate_bundle(&Bundle {
                entity: e.clone(),
                dependencies: deps.clone(),
                last_revision: lineages[id].last_revision.clone(),
            })?;
        }
        for row in &rows {
            let Some((id, link, _)) = row.record() else {
                continue;
            };
            require(
                !matches!(link.origin, Origin::Created) || matches!(row, Row::Baseline { .. }),
                "Created origin requires a Baseline",
            )?;
            let kind = match row {
                Row::Revision { value, .. } => {
                    require(
                        value
                            .parent
                            .as_ref()
                            .is_none_or(|p| result.entities.contains_key(p))
                            && value
                                .dependencies
                                .iter()
                                .all(|d| result.entities.contains_key(d))
                            && value.dependencies.iter().collect::<BTreeSet<_>>().len()
                                == value.dependencies.len(),
                        "invalid historical Revision reference",
                    )?;
                    "rev"
                }
                Row::Note { value, .. } => {
                    require(!value.body.trim().is_empty(), "empty historical Note")?;
                    "note"
                }
                Row::Decision { value, .. } => {
                    let valid = match value.field.as_str() {
                        "disposition" => match value.new_value.as_deref() {
                            Some("undecided") => value.revision.is_none(),
                            Some("accepted" | "rejected") => {
                                value.revision.as_ref().is_some_and(|r| {
                                    revisions
                                        .get(r.as_str())
                                        .is_some_and(|(_, l)| l.owner == link.owner)
                                })
                            }
                            _ => false,
                        },
                        "resurface_condition" => value.revision.is_none(),
                        _ => false,
                    };
                    require(valid, "invalid historical Decision")?;
                    "decision"
                }
                Row::Progress { value, .. } => {
                    if let Some(p) = &link.result {
                        require(
                            matches!(
                                (&value.kind, &p.progress),
                                (ProgressKind::Start, Progress::InProgress(_))
                                    | (ProgressKind::Done, Progress::Ended)
                                    | (ProgressKind::Release, Progress::NotStarted)
                            ),
                            "Progress event differs from proof",
                        )?;
                    }
                    "progress"
                }
                Row::Baseline { value, .. } => {
                    validate_bundle(&value.bundle)?;
                    require(
                        value.bundle.entity.id == link.owner
                            && link.result
                                == Some(proof(
                                    &value.bundle.entity,
                                    value.bundle.last_revision.clone(),
                                )),
                        "Baseline owner or proof mismatch",
                    )?;
                    require(
                        matches!(
                            (&link.origin, value.source_schema),
                            (Origin::Created, None)
                                | (Origin::Migration { schema: 11 }, Some(11))
                                | (Origin::Migration { schema: 12 }, Some(12))
                        ),
                        "Baseline origin mismatch",
                    )?;
                    "baseline"
                }
                Row::Merge { value, .. } => {
                    validate_bundle(&value.result)?;
                    require(
                        value.result.entity.id == link.owner
                            && link.result
                                == Some(proof(
                                    &value.result.entity,
                                    value.result.last_revision.clone(),
                                )),
                        "Merge result mismatch",
                    )?;
                    let mut identities = BTreeSet::new();
                    let mut parents = BTreeSet::new();
                    for input in &value.inputs {
                        validate_bundle(&input.candidate)?;
                        require(
                            input.candidate.entity.id == link.owner
                                && identities.insert(&input.identity)
                                && input.identity.len() == 64
                                && input
                                    .identity
                                    .bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                                && !input.heads.is_empty(),
                            "invalid Merge input",
                        )?;
                        for head in &input.heads {
                            let l = links.get(head).ok_or("missing Merge input head")?;
                            require(
                                l.owner == link.owner
                                    && l.stream == Stream::State
                                    && l.result
                                        == Some(proof(
                                            &input.candidate.entity,
                                            input.candidate.last_revision.clone(),
                                        )),
                                "Merge input proof mismatch",
                            )?;
                            parents.insert(head.clone());
                        }
                    }
                    require(
                        value.inputs.len() >= 2
                            && parents == link.parents
                            && value.inputs.iter().any(|i| {
                                i.identity == value.selected && i.candidate == value.result
                            }),
                        "invalid Merge selection or parents",
                    )?;
                    "merge"
                }
                _ => unreachable!(),
            };
            record_id(id, kind)?;
        }
        let mut pending: BTreeSet<_> = links.keys().cloned().collect();
        let mut emitted = BTreeSet::new();
        while !pending.is_empty() {
            let ready: Vec<_> = pending
                .iter()
                .filter(|id| links[*id].parents.is_subset(&emitted))
                .cloned()
                .collect();
            require(!ready.is_empty(), "causal cycle")?;
            for id in ready {
                pending.remove(&id);
                emitted.insert(id);
            }
        }
        Ok(result)
    }
}
