use crate::db::{DbError, StoreSnapshot};
use crate::derived::{EvaluationError, View};
use crate::domain::*;
use crate::storage::Store;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, Serializer};
use serde_saphyr::options::{DuplicateKeyPolicy, MergeKeyPolicy};
use serde_saphyr::{DoubleQuoted, FlowMap, LitStr};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const SCHEMA: &str = "axon-plan/v2";
const FINGERPRINT_VERSION: &str = "axon-entity-fingerprint/v2";

#[derive(Debug, thiserror::Error)]
pub enum DeclarationError {
    #[error("{0}")]
    Evaluation(#[from] EvaluationError),
    #[error("{0}")]
    Invalid(String),
    #[error("{reason}")]
    Reference { reason: String, guidance: String },
    #[error("{0}")]
    Database(#[from] DbError),
    #[error(
        "declaration file refresh failed: {source}\nApplied: storage declaration values\nNot applied: declaration file refresh at {path}\nHelp: Inspect saved information with `axon list --skip-command-evaluation` and preserve the file; retry the same file only after verifying the declared final values match storage."
    )]
    Refresh {
        path: String,
        #[source]
        source: Box<DeclarationError>,
    },
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, DeclarationError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFile {
    schema: String,
    issues: Vec<EntityRecord>,
    groups: Vec<EntityRecord>,
    relations: Relations,
    references: References,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityRecord {
    #[serde(deserialize_with = "required_option")]
    id: Option<String>,
    #[serde(deserialize_with = "required_option")]
    key: Option<String>,
    #[serde(deserialize_with = "required_option", serialize_with = "quoted_option")]
    base: Option<String>,
    title: String,
    #[serde(
        deserialize_with = "required_option",
        serialize_with = "description_option"
    )]
    description: Option<String>,
    observed: Observed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observed {
    progress: ProgressRecord,
    #[serde(deserialize_with = "required_option")]
    claim: Option<ClaimRecord>,
    disposition: DispositionRecord,
    resurface: ResurfaceRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProgressRecord {
    NotStarted,
    InProgress,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DispositionRecord {
    Undecided,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimRecord {
    actor: String,
    worktree: String,
    #[serde(serialize_with = "quoted_string")]
    at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ResurfaceRecord {
    Always,
    Manual,
    Command {
        command: String,
    },
    AtDate {
        #[serde(serialize_with = "quoted_string")]
        date: String,
    },
    AfterEntity {
        #[serde(serialize_with = "flow_reference")]
        entity: EntityReference,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Relations {
    editable: RelationSet,
    readonly: RelationSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationSet {
    parents: Vec<ParentRelation>,
    dependencies: Vec<DependencyRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParentRelation {
    #[serde(serialize_with = "flow_reference")]
    child: EntityReference,
    #[serde(serialize_with = "flow_reference")]
    parent: EntityReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyRelation {
    #[serde(serialize_with = "flow_reference")]
    dependent: EntityReference,
    #[serde(serialize_with = "flow_reference")]
    prerequisite: EntityReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct References {
    entities: Vec<ReferenceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceRecord {
    id: String,
    kind: KindRecord,
    #[serde(serialize_with = "quoted_string")]
    base: String,
    title: String,
    observed: Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KindRecord {
    Issue,
    Group,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValidationMode {
    Prepare,
    Check,
    Apply,
}

struct ValidatedPlan {
    desired: StoreSnapshot,
    report: String,
}

pub fn export(
    store: &mut Store,
    ids: &[String],
    groups: &[String],
    recursive: bool,
) -> Result<String> {
    if ids.is_empty() && groups.is_empty() {
        return Err(DeclarationError::Invalid(
            "export requires at least one Entity ID or --group selector".to_string(),
        ));
    }
    if recursive && groups.is_empty() {
        return Err(DeclarationError::Invalid(
            "--recursive requires at least one --group selector".to_string(),
        ));
    }
    let snapshot = store.snapshot()?;
    let view = snapshot.view();
    let mut selected = BTreeSet::new();
    for raw in ids {
        selected.insert(resolve_snapshot_id(&snapshot, raw)?);
    }
    for raw in groups {
        let id = resolve_snapshot_id(&snapshot, raw)?;
        let entity = view
            .get(&id)
            .ok_or_else(|| DeclarationError::Invalid(format!("entity {id} does not exist")))?;
        if entity.kind != EntityKind::Group {
            return Err(DeclarationError::Invalid(format!("{id} is not a group")));
        }
        selected.insert(id.clone());
        let children = if recursive {
            view.descendants(&id)
        } else {
            view.direct_children(&id)
        };
        selected.extend(children.into_iter().map(|child| child.id.clone()));
    }
    let mut document = document_from_snapshot(&snapshot, &selected, &HashMap::new())?;
    validate_local(&document, false)?;
    canonicalize(&mut document)?;
    serialize(&document)
}

pub fn prepare(store: &mut Store, path: &Path) -> Result<()> {
    let input = fs::read_to_string(path)?;
    let mut document = parse(&input)?;
    normalize_values(&mut document)?;
    validate_local(&document, true)?;
    let snapshot = store.snapshot()?;
    let prefix = store.prefix()?;
    assign_ids(&mut document, &snapshot, &prefix)?;
    canonicalize(&mut document)?;
    validate_local(&document, false)?;
    validate_against(&document, &snapshot, &prefix, ValidationMode::Prepare)?;
    atomic_write(path, &serialize(&document)?).map_err(|e| {
        DeclarationError::Invalid(format!(
            "{e}\nNot applied: declaration file replacement at {}",
            path.display()
        ))
    })
}

pub fn check(store: &mut Store, path: &Path) -> Result<String> {
    let input = fs::read_to_string(path)?;
    let document = parse_canonical(&input)?;
    let snapshot = store.snapshot()?;
    let prefix = store.prefix()?;
    Ok(validate_against(&document, &snapshot, &prefix, ValidationMode::Check)?.report)
}

pub fn apply(store: &mut Store, path: &Path) -> Result<String> {
    let input = fs::read_to_string(path)?;
    let document = parse_canonical(&input)?;
    let selected_keys = selected_keys(&document)?;
    let document_for_transaction = document.clone();
    let prefix = store.prefix()?;
    let (validated, after) = store.transactional_import(|before| {
        let validated = validate_against(
            &document_for_transaction,
            &before,
            &prefix,
            ValidationMode::Apply,
        )?;
        let desired = validated.desired.clone();
        Ok::<_, DeclarationError>((validated, desired))
    })?;
    let refresh = (|| -> Result<()> {
        let selected = selected_ids(&document)?;
        let mut refreshed = document_from_snapshot(&after, &selected, &selected_keys)?;
        validate_local(&refreshed, false)?;
        canonicalize(&mut refreshed)?;
        atomic_write(path, &serialize(&refreshed)?)
    })();
    refresh.map_err(|source| DeclarationError::Refresh {
        path: path.display().to_string(),
        source: Box::new(source),
    })?;
    Ok(validated.report)
}

fn parse_canonical(input: &str) -> Result<PlanFile> {
    let document = parse(input)?;
    let mut normalized = document.clone();
    normalize_values(&mut normalized)?;
    validate_local(&normalized, false)?;
    canonicalize(&mut normalized)?;
    if serialize(&normalized)? != input {
        return Err(DeclarationError::Invalid(
            "declaration is not canonical; run `axon import prepare <file>`".to_string(),
        ));
    }
    Ok(normalized)
}

fn parse(input: &str) -> Result<PlanFile> {
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_aliases: 0,
            max_anchors: 0,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true,
        no_schema: true,
        reject_unsupported_tags: true,
        with_snippet: true,
    };
    serde_saphyr::from_str_with_options(input, options)
        .map_err(|error| DeclarationError::Invalid(format!("invalid declaration YAML: {error}")))
}

fn serialize(document: &PlanFile) -> Result<String> {
    let options = serde_saphyr::ser_options! {
        indent_step: 2,
        compact_list_indent: false,
        prefer_block_scalars: true,
    };
    let serialized = serde_saphyr::to_string_with_options(document, options).map_err(|error| {
        DeclarationError::Invalid(format!("could not serialize declaration: {error}"))
    })?;
    Ok(space_flow_references(&serialized))
}

fn space_flow_references(serialized: &str) -> String {
    let mut block_scalar_indent = None;
    let mut lines = Vec::new();
    for line in serialized.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if let Some(base_indent) = block_scalar_indent {
            if trimmed.is_empty() || indent > base_indent {
                lines.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }
        let scalar_value = trimmed.split_once(':').map(|(_, value)| value.trim());
        if scalar_value.is_some_and(|value| value.starts_with('|') || value.starts_with('>')) {
            block_scalar_indent = Some(indent);
            lines.push(line.to_string());
            continue;
        }
        if trimmed.is_empty() {
            lines.push(String::new());
        } else {
            let is_reference = [
                "- child: {",
                "parent: {",
                "- dependent: {",
                "prerequisite: {",
                "entity: {",
            ]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix));
            if !is_reference {
                lines.push(line.to_string());
            } else {
                let mut line = line.replacen("{id:", "{ id:", 1);
                line = line.replacen("{key:", "{ key:", 1);
                if line.ends_with('}') {
                    line.insert(line.len() - 1, ' ');
                }
                lines.push(line);
            }
        }
    }
    lines.join("\n") + "\n"
}

fn normalize_values(document: &mut PlanFile) -> Result<()> {
    for entity in document.issues.iter_mut().chain(document.groups.iter_mut()) {
        entity.title = entity.title.trim().to_string();
        if entity
            .description
            .as_ref()
            .is_some_and(|description| description.trim().is_empty())
        {
            entity.description = None;
        }
        normalize_observed(&mut entity.observed)?;
    }
    for reference in &mut document.references.entities {
        normalize_observed(&mut reference.observed)?;
    }
    Ok(())
}

fn normalize_observed(observed: &mut Observed) -> Result<()> {
    if let Some(claim) = &mut observed.claim {
        let at = DateTime::parse_from_rfc3339(&claim.at).map_err(|_| {
            DeclarationError::Invalid(format!("invalid RFC 3339 claim timestamp: {}", claim.at))
        })?;
        claim.at = at
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true);
    }
    if let ResurfaceRecord::AtDate { date } = &mut observed.resurface {
        *date = date
            .parse::<chrono::NaiveDate>()
            .map_err(|_| DeclarationError::Invalid(format!("invalid resurface date: {date}")))?
            .to_string();
    }
    Ok(())
}

fn validate_local(document: &PlanFile, allow_null_ids: bool) -> Result<()> {
    if document.schema != SCHEMA {
        return Err(DeclarationError::Invalid(format!(
            "schema must be {SCHEMA}"
        )));
    }
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for (record, _) in records(document) {
        if record.id.is_none() && !allow_null_ids {
            return Err(DeclarationError::Invalid(
                "every Entity must have an ID; run `axon import prepare <file>`".to_string(),
            ));
        }
        if record.id.is_none() && record.key.is_none() {
            return Err(DeclarationError::Invalid(
                "an Entity with id: null must have a key".to_string(),
            ));
        }
        if let Some(id) = &record.id
            && (id.is_empty() || !ids.insert(id.clone()))
        {
            return Err(DeclarationError::Invalid(format!(
                "duplicate or empty Entity ID: {id:?}"
            )));
        }
        if let Some(key) = &record.key
            && (!valid_key(key) || !keys.insert(key.clone()))
        {
            return Err(DeclarationError::Invalid(format!(
                "invalid or duplicate Entity key: {key}"
            )));
        }
        validate_title(&record.title)?;
        if let Some(base) = &record.base {
            validate_fingerprint(base)?;
        }
        validate_observed(&record.observed)?;
    }
    let mut reference_ids = HashSet::new();
    for reference in &document.references.entities {
        if !reference_ids.insert(reference.id.clone()) || ids.contains(&reference.id) {
            return Err(DeclarationError::Invalid(format!(
                "duplicate Entity/reference ID: {}",
                reference.id
            )));
        }
        validate_fingerprint(&reference.base)?;
        validate_title(&reference.title)?;
        validate_observed(&reference.observed)?;
    }
    Ok(())
}

fn validate_title(title: &str) -> Result<()> {
    if title.is_empty() || title.trim() != title || title.contains(['\n', '\r']) {
        return Err(DeclarationError::Invalid(
            "Entity titles must be trimmed, non-empty single-line strings".to_string(),
        ));
    }
    Ok(())
}

fn validate_observed(observed: &Observed) -> Result<()> {
    match (observed.progress, observed.claim.as_ref()) {
        (ProgressRecord::InProgress, Some(claim)) => {
            if claim.actor.is_empty() || claim.worktree.is_empty() {
                return Err(DeclarationError::Invalid(
                    "claim actor and worktree must be non-empty".to_string(),
                ));
            }
            DateTime::parse_from_rfc3339(&claim.at).map_err(|_| {
                DeclarationError::Invalid(format!("invalid RFC 3339 claim timestamp: {}", claim.at))
            })?;
        }
        (ProgressRecord::InProgress, None) => {
            return Err(DeclarationError::Invalid(
                "in_progress requires a claim".to_string(),
            ));
        }
        (_, Some(_)) => {
            return Err(DeclarationError::Invalid(
                "only in_progress may have a claim".to_string(),
            ));
        }
        (_, None) => {}
    }
    if let ResurfaceRecord::AtDate { date } = &observed.resurface {
        date.parse::<chrono::NaiveDate>()
            .map_err(|_| DeclarationError::Invalid(format!("invalid resurface date: {date}")))?;
    }
    validate_reference_in_resurface(&observed.resurface)
}

fn validate_reference_in_resurface(resurface: &ResurfaceRecord) -> Result<()> {
    if let ResurfaceRecord::AfterEntity { entity } = resurface {
        validate_reference_shape(entity)?;
    }
    Ok(())
}

fn validate_reference_shape(reference: &EntityReference) -> Result<()> {
    match (&reference.id, &reference.key) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        _ => Err(DeclarationError::Invalid(
            "an Entity reference must contain exactly one of id or key".to_string(),
        )),
    }
}

fn validate_fingerprint(value: &str) -> Result<()> {
    let hex = value
        .strip_prefix("blake3:")
        .ok_or_else(|| DeclarationError::Invalid(format!("invalid Entity fingerprint: {value}")))?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DeclarationError::Invalid(format!(
            "invalid Entity fingerprint: {value}"
        )));
    }
    Ok(())
}

fn valid_key(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key.as_bytes()[0].is_ascii_lowercase()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_generated_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix('-'))
        .is_some_and(|suffix| {
            suffix.len() == 6
                && suffix
                    .bytes()
                    .all(|byte| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&byte))
        })
}

fn assign_ids(document: &mut PlanFile, snapshot: &StoreSnapshot, prefix: &str) -> Result<()> {
    let mut used = snapshot
        .entities
        .iter()
        .map(|entity| entity.id.to_string())
        .chain(records(document).filter_map(|(record, _)| record.id.clone()))
        .collect::<HashSet<_>>();
    for record in document.issues.iter_mut().chain(document.groups.iter_mut()) {
        if record.id.is_some() {
            continue;
        }
        let id = loop {
            let candidate = EntityId::generate(prefix).to_string();
            if used.insert(candidate.clone()) {
                break candidate;
            }
        };
        record.id = Some(id);
    }
    Ok(())
}

fn validate_against(
    document: &PlanFile,
    snapshot: &StoreSnapshot,
    prefix: &str,
    mode: ValidationMode,
) -> Result<ValidatedPlan> {
    validate_local(document, false)?;
    let resolver = Resolver::new(document)?;
    validate_and_resolve_all_references(document, &resolver)?;
    let selected = selected_ids(document)?;
    validate_reference_scope(document, &resolver, &selected)?;
    let current = snapshot.view();
    let mut desired_entities = snapshot.entities.clone();
    let mut stale = Vec::new();
    let mut stale_observed = Vec::new();

    for (record, kind) in records(document) {
        let id = EntityId::from_stored(record.id.as_deref().expect("validated ID"));
        if let Some(entity) = current.get(&id) {
            if entity.kind != kind {
                return Err(DeclarationError::Invalid(format!(
                    "{} is a {}, not a {}",
                    id,
                    entity.kind.label(),
                    kind.label()
                )));
            }
            let expected_base = fingerprint(&current, entity);
            let base_matches = record.base.as_deref() == Some(expected_base.as_str());
            if !base_matches {
                stale.push(id.clone());
            }
            if !observed_matches(&record.observed, entity, &resolver)? {
                if base_matches {
                    return Err(DeclarationError::Invalid(format!(
                        "read-only observed state was edited for {id}"
                    )));
                }
                stale_observed.push(id.clone());
            }
        } else {
            if record.base.is_some() {
                return Err(DeclarationError::Invalid(format!(
                    "Entity {id} no longer exists"
                )));
            }
            if record.observed != initial_observed() {
                return Err(DeclarationError::Invalid(format!(
                    "new Entity {id} must use the initial observed state"
                )));
            }
            if record.key.is_none() || !valid_generated_id(id.as_str(), prefix) {
                return Err(DeclarationError::Invalid(format!(
                    "new Entity {id} must retain a key and use an ID assigned by `axon import prepare`"
                )));
            }
            let now = Utc::now();
            desired_entities.push(Entity {
                id: id.clone(),
                kind,
                title: record.title.clone(),
                description: record.description.clone(),
                progress: Progress::NotStarted,
                disposition: Disposition::Accepted,
                current_revision: Some(RecordId::new(RecordKind::Revision)),
                resurface_condition: ResurfaceCondition::Always,
                parent: None,
                created_at: now,
                updated_at: now,
            });
        }
    }

    validate_references(document, &current, &resolver)?;
    let editable_parents = resolve_parents(&document.relations.editable.parents, &resolver)?;
    let readonly_parents = resolve_parents(&document.relations.readonly.parents, &resolver)?;
    let editable_dependencies =
        resolve_dependencies(&document.relations.editable.dependencies, &resolver)?;
    let readonly_dependencies =
        resolve_dependencies(&document.relations.readonly.dependencies, &resolver)?;
    validate_relation_ownership(
        &selected,
        &editable_parents,
        &readonly_parents,
        &editable_dependencies,
        &readonly_dependencies,
    )?;
    validate_readonly_relations(
        snapshot,
        &selected,
        &readonly_parents,
        &readonly_dependencies,
    )?;

    let parent_by_child = editable_parents
        .iter()
        .map(|(child, parent)| (child.clone(), parent.clone()))
        .collect::<HashMap<_, _>>();
    for entity in &mut desired_entities {
        if !selected.contains(&entity.id) {
            continue;
        }
        let (record, _) = record_by_id(document, &entity.id)?;
        entity.title = record.title.clone();
        entity.description = record.description.clone();
        entity.parent = parent_by_child.get(&entity.id).cloned();
    }
    let mut desired_dependencies = snapshot
        .dependencies
        .iter()
        .filter(|(source, _)| !selected.contains(source))
        .cloned()
        .chain(editable_dependencies.iter().cloned())
        .collect::<Vec<_>>();
    desired_dependencies.sort();
    let desired = StoreSnapshot {
        evaluation: snapshot.evaluation.clone(),
        entities: desired_entities,
        dependencies: desired_dependencies,
    };

    if !stale.is_empty() {
        let recovery = mode == ValidationMode::Apply
            && stale_observed.is_empty()
            && selected
                .iter()
                .all(|id| owned_values_match(snapshot, &desired, id));
        if !recovery {
            return Err(DeclarationError::Invalid(format!(
                "stale base fingerprint for {}; export again before applying",
                stale
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }

    crate::db::validate_import_snapshot(snapshot, &desired)?;
    let report = if mode == ValidationMode::Prepare {
        String::new()
    } else {
        render_report(snapshot, &desired, &selected)?
    };
    Ok(ValidatedPlan { desired, report })
}

fn validate_references(document: &PlanFile, current: &View, resolver: &Resolver) -> Result<()> {
    for record in &document.references.entities {
        let id = EntityId::from_stored(&record.id);
        let entity = current.get(&id).ok_or_else(|| {
            DeclarationError::Reference {
                reason: format!("referenced Entity {id} does not exist in the current DB"),
                guidance: "Check the active root and ID with `axon list --skip-command-evaluation`; export the intended existing Entity. See `axon docs declaration` for external snapshots.".to_string(),
            }
        })?;
        if record.base != fingerprint(current, entity) {
            return Err(DeclarationError::Reference {
                reason: format!("stale or incorrect reference base fingerprint for {id}"),
                guidance: format!(
                    "Preserve the file and compare a fresh `axon export {id}` snapshot; reconcile the reference before preparing again. See `axon docs declaration`."
                ),
            });
        }
        if record.kind != kind_record(entity.kind)
            || record.title != entity.title
            || !observed_matches(&record.observed, entity, resolver)?
        {
            return Err(DeclarationError::Reference {
                reason: format!(
                    "read-only reference snapshot differs from the current DB for {id} despite a matching base"
                ),
                guidance: format!(
                    "Restore kind, title and observed from `axon export {id}`; these fields do not request changes to the referenced Entity. See `axon docs declaration`."
                ),
            });
        }
    }
    Ok(())
}

fn observed_matches(record: &Observed, entity: &Entity, resolver: &Resolver) -> Result<bool> {
    let expected = observed(entity);
    if record.progress != expected.progress
        || record.claim != expected.claim
        || record.disposition != expected.disposition
    {
        return Ok(false);
    }
    match (&record.resurface, &entity.resurface_condition) {
        (ResurfaceRecord::Always, ResurfaceCondition::Always) => Ok(true),
        (ResurfaceRecord::Manual, ResurfaceCondition::Manual) => Ok(true),
        (ResurfaceRecord::Command { command }, ResurfaceCondition::Command(expected)) => {
            Ok(command == expected)
        }
        (ResurfaceRecord::AtDate { date: record }, ResurfaceCondition::AtDate(expected)) => {
            Ok(record == &expected.to_string())
        }
        (
            ResurfaceRecord::AfterEntity { entity: record },
            ResurfaceCondition::AfterEntity(expected),
        ) => Ok(resolver.resolve(record)? == *expected),
        _ => Ok(false),
    }
}

fn validate_relation_ownership(
    selected: &BTreeSet<EntityId>,
    editable_parents: &[(EntityId, EntityId)],
    readonly_parents: &[(EntityId, EntityId)],
    editable_dependencies: &[(EntityId, EntityId)],
    readonly_dependencies: &[(EntityId, EntityId)],
) -> Result<()> {
    if let Some((child, _)) = editable_parents
        .iter()
        .find(|(child, _)| !selected.contains(child))
    {
        return Err(DeclarationError::Invalid(format!(
            "editable parent owner {child} is outside the edit set"
        )));
    }
    if let Some((child, parent)) = readonly_parents
        .iter()
        .find(|(child, parent)| selected.contains(child) || !selected.contains(parent))
    {
        return Err(DeclarationError::Invalid(format!(
            "readonly parent relation has invalid ownership: {child} -> {parent}"
        )));
    }
    if let Some((source, _)) = editable_dependencies
        .iter()
        .find(|(source, _)| !selected.contains(source))
    {
        return Err(DeclarationError::Invalid(format!(
            "editable dependency owner {source} is outside the edit set"
        )));
    }
    if let Some((source, target)) = readonly_dependencies
        .iter()
        .find(|(source, target)| selected.contains(source) || !selected.contains(target))
    {
        return Err(DeclarationError::Invalid(format!(
            "readonly dependency has invalid ownership: {source} -> {target}"
        )));
    }
    Ok(())
}

fn validate_readonly_relations(
    snapshot: &StoreSnapshot,
    selected: &BTreeSet<EntityId>,
    readonly_parents: &[(EntityId, EntityId)],
    readonly_dependencies: &[(EntityId, EntityId)],
) -> Result<()> {
    let mut expected_parents = snapshot
        .entities
        .iter()
        .filter(|entity| !selected.contains(&entity.id))
        .filter_map(|entity| {
            entity
                .parent
                .as_ref()
                .filter(|parent| selected.contains(*parent))
                .map(|parent| (entity.id.clone(), parent.clone()))
        })
        .collect::<Vec<_>>();
    expected_parents.sort();
    let mut actual_parents = readonly_parents.to_vec();
    actual_parents.sort();
    if actual_parents != expected_parents {
        return Err(DeclarationError::Invalid(
            "read-only parent relations were edited or changed".to_string(),
        ));
    }

    let mut expected_dependencies = snapshot
        .dependencies
        .iter()
        .filter(|(source, target)| !selected.contains(source) && selected.contains(target))
        .cloned()
        .collect::<Vec<_>>();
    expected_dependencies.sort();
    let mut actual_dependencies = readonly_dependencies.to_vec();
    actual_dependencies.sort();
    if actual_dependencies != expected_dependencies {
        return Err(DeclarationError::Invalid(
            "read-only dependency relations were edited or changed".to_string(),
        ));
    }
    Ok(())
}

fn owned_values_match(before: &StoreSnapshot, after: &StoreSnapshot, id: &EntityId) -> bool {
    let before_view = before.view();
    let after_view = after.view();
    let (Some(before_entity), Some(after_entity)) = (before_view.get(id), after_view.get(id))
    else {
        return false;
    };
    before_entity.kind == after_entity.kind
        && before_entity.title == after_entity.title
        && before_entity.description == after_entity.description
        && before_entity.parent == after_entity.parent
        && direct_dependency_set(before, id) == direct_dependency_set(after, id)
}

fn validate_and_resolve_all_references(document: &PlanFile, resolver: &Resolver) -> Result<()> {
    for observed in document
        .issues
        .iter()
        .chain(document.groups.iter())
        .map(|record| &record.observed)
        .chain(
            document
                .references
                .entities
                .iter()
                .map(|record| &record.observed),
        )
    {
        if let ResurfaceRecord::AfterEntity { entity } = &observed.resurface {
            resolver.resolve(entity)?;
        }
    }
    resolve_parents(&document.relations.editable.parents, resolver)?;
    resolve_parents(&document.relations.readonly.parents, resolver)?;
    resolve_dependencies(&document.relations.editable.dependencies, resolver)?;
    resolve_dependencies(&document.relations.readonly.dependencies, resolver)?;
    Ok(())
}

fn validate_reference_scope(
    document: &PlanFile,
    resolver: &Resolver,
    selected: &BTreeSet<EntityId>,
) -> Result<()> {
    let mut required = BTreeSet::new();
    {
        let mut add = |reference: &EntityReference| -> Result<()> {
            let id = resolver.resolve(reference)?;
            if !selected.contains(&id) {
                required.insert(id);
            }
            Ok(())
        };
        for relation in document
            .relations
            .editable
            .parents
            .iter()
            .chain(document.relations.readonly.parents.iter())
        {
            add(&relation.child)?;
            add(&relation.parent)?;
        }
        for relation in document
            .relations
            .editable
            .dependencies
            .iter()
            .chain(document.relations.readonly.dependencies.iter())
        {
            add(&relation.dependent)?;
            add(&relation.prerequisite)?;
        }
        for record in document.issues.iter().chain(document.groups.iter()) {
            if let ResurfaceRecord::AfterEntity { entity } = &record.observed.resurface {
                add(entity)?;
            }
        }
    }

    let references = document
        .references
        .entities
        .iter()
        .map(|record| (EntityId::from_stored(&record.id), record))
        .collect::<HashMap<_, _>>();
    let mut pending = required.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let record = references.get(&id).ok_or_else(|| {
            DeclarationError::Invalid(format!("missing read-only reference for {id}"))
        })?;
        if let ResurfaceRecord::AfterEntity { entity } = &record.observed.resurface {
            let target = resolver.resolve(entity)?;
            if !selected.contains(&target) && required.insert(target.clone()) {
                pending.push(target);
            }
        }
    }
    let actual = references.keys().cloned().collect::<BTreeSet<_>>();
    if actual != required {
        return Err(DeclarationError::Reference {
            reason: format!("references.entities contains unrelated snapshots: {}", actual.difference(&required).map(ToString::to_string).collect::<Vec<_>>().join(", ")),
            guidance: "Keep only snapshots required by relations and observed AfterEntity references (including their recursive AfterEntity targets); do not expand issues/groups to silence this error. See `axon docs declaration`.".to_string(),
        });
    }
    Ok(())
}

fn resolve_parents(
    relations: &[ParentRelation],
    resolver: &Resolver,
) -> Result<Vec<(EntityId, EntityId)>> {
    let mut resolved = Vec::new();
    let mut children = HashSet::new();
    for relation in relations {
        let child = resolver.resolve(&relation.child)?;
        let parent = resolver.resolve(&relation.parent)?;
        if child == parent {
            return Err(DeclarationError::Invalid(format!(
                "Entity {child} cannot be its own parent"
            )));
        }
        if !children.insert(child.clone()) {
            return Err(DeclarationError::Invalid(format!(
                "multiple parents declared for {child}"
            )));
        }
        if resolver.kind(&parent)? != EntityKind::Group {
            return Err(DeclarationError::Invalid(format!(
                "{parent} is not a group"
            )));
        }
        resolved.push((child, parent));
    }
    Ok(resolved)
}

fn resolve_dependencies(
    relations: &[DependencyRelation],
    resolver: &Resolver,
) -> Result<Vec<(EntityId, EntityId)>> {
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();
    for relation in relations {
        let source = resolver.resolve(&relation.dependent)?;
        let target = resolver.resolve(&relation.prerequisite)?;
        if source == target {
            return Err(DeclarationError::Invalid(format!(
                "Entity {source} cannot depend on itself"
            )));
        }
        if !seen.insert((source.clone(), target.clone())) {
            return Err(DeclarationError::Invalid(format!(
                "duplicate dependency: {source} -> {target}"
            )));
        }
        resolved.push((source, target));
    }
    Ok(resolved)
}

struct Resolver {
    ids: HashMap<String, EntityKind>,
    keys: HashMap<String, String>,
}

impl Resolver {
    fn new(document: &PlanFile) -> Result<Self> {
        let mut ids = HashMap::new();
        let mut keys = HashMap::new();
        for (record, kind) in records(document) {
            let id = record.id.clone().ok_or_else(|| {
                DeclarationError::Invalid("an Entity reference has no assigned ID".to_string())
            })?;
            ids.insert(id.clone(), kind);
            if let Some(key) = &record.key {
                keys.insert(key.clone(), id);
            }
        }
        for reference in &document.references.entities {
            ids.insert(reference.id.clone(), entity_kind(reference.kind));
        }
        Ok(Self { ids, keys })
    }

    fn resolve(&self, reference: &EntityReference) -> Result<EntityId> {
        validate_reference_shape(reference)?;
        let id = match (&reference.id, &reference.key) {
            (Some(id), None) => id,
            (None, Some(key)) => self
                .keys
                .get(key)
                .ok_or_else(|| DeclarationError::Invalid(format!("unknown Entity key: {key}")))?,
            _ => unreachable!("shape validated"),
        };
        if !self.ids.contains_key(id) {
            return Err(DeclarationError::Reference {
                reason: format!(
                    "Entity ID {id} is missing from this declaration file (issues, groups, or references.entities); DB existence has not been checked"
                ),
                guidance: format!(
                    "Check the ID; for an external Entity, obtain `axon export {id}` and copy its snapshot into references.entities without adding it to the edit set. See `axon docs declaration`."
                ),
            });
        }
        Ok(EntityId::from_stored(id))
    }

    fn kind(&self, id: &EntityId) -> Result<EntityKind> {
        self.ids
            .get(id.as_str())
            .copied()
            .ok_or_else(|| DeclarationError::Invalid(format!("unknown Entity ID: {id}")))
    }
}

fn canonicalize(document: &mut PlanFile) -> Result<()> {
    let resolver = Resolver::new(document)?;
    canonicalize_references(document, &resolver)?;
    document.issues.sort_by_key(entity_sort_key);
    document.groups.sort_by_key(entity_sort_key);
    document
        .references
        .entities
        .sort_by(|left, right| left.id.as_bytes().cmp(right.id.as_bytes()));
    let resolver = Resolver::new(document)?;
    sort_relation_set(&mut document.relations.editable, &resolver)?;
    sort_relation_set(&mut document.relations.readonly, &resolver)?;
    Ok(())
}

fn canonicalize_references(document: &mut PlanFile, resolver: &Resolver) -> Result<()> {
    let keys = document
        .issues
        .iter()
        .chain(document.groups.iter())
        .filter_map(|record| Some((record.id.clone()?, record.key.clone()?)))
        .collect::<HashMap<_, _>>();
    let canonical = |reference: &mut EntityReference| -> Result<()> {
        let id = resolver.resolve(reference)?.to_string();
        if let Some(key) = keys.get(&id) {
            reference.id = None;
            reference.key = Some(key.clone());
        } else {
            reference.id = Some(id);
            reference.key = None;
        }
        Ok(())
    };
    for relation in document
        .relations
        .editable
        .parents
        .iter_mut()
        .chain(document.relations.readonly.parents.iter_mut())
    {
        canonical(&mut relation.child)?;
        canonical(&mut relation.parent)?;
    }
    for relation in document
        .relations
        .editable
        .dependencies
        .iter_mut()
        .chain(document.relations.readonly.dependencies.iter_mut())
    {
        canonical(&mut relation.dependent)?;
        canonical(&mut relation.prerequisite)?;
    }
    for observed in document
        .issues
        .iter_mut()
        .chain(document.groups.iter_mut())
        .map(|record| &mut record.observed)
        .chain(
            document
                .references
                .entities
                .iter_mut()
                .map(|record| &mut record.observed),
        )
    {
        if let ResurfaceRecord::AfterEntity { entity } = &mut observed.resurface {
            canonical(entity)?;
        }
    }
    Ok(())
}

fn sort_relation_set(relations: &mut RelationSet, resolver: &Resolver) -> Result<()> {
    relations.parents.sort_by(|left, right| {
        let left = (
            resolver.resolve(&left.child).unwrap().to_string(),
            resolver.resolve(&left.parent).unwrap().to_string(),
        );
        let right = (
            resolver.resolve(&right.child).unwrap().to_string(),
            resolver.resolve(&right.parent).unwrap().to_string(),
        );
        left.cmp(&right)
    });
    relations.dependencies.sort_by(|left, right| {
        let left = (
            resolver.resolve(&left.dependent).unwrap().to_string(),
            resolver.resolve(&left.prerequisite).unwrap().to_string(),
        );
        let right = (
            resolver.resolve(&right.dependent).unwrap().to_string(),
            resolver.resolve(&right.prerequisite).unwrap().to_string(),
        );
        left.cmp(&right)
    });
    Ok(())
}

fn entity_sort_key(record: &EntityRecord) -> (bool, Vec<u8>) {
    match (&record.id, &record.key) {
        (Some(id), _) => (false, id.as_bytes().to_vec()),
        (None, Some(key)) => (true, key.as_bytes().to_vec()),
        (None, None) => (true, Vec::new()),
    }
}

fn document_from_snapshot(
    snapshot: &StoreSnapshot,
    selected: &BTreeSet<EntityId>,
    selected_keys: &HashMap<String, String>,
) -> Result<PlanFile> {
    let view = snapshot.view();
    let mut issues = Vec::new();
    let mut groups = Vec::new();
    for id in selected {
        let entity = view.get(id).ok_or_else(|| {
            DeclarationError::Invalid(format!("selected Entity {id} does not exist"))
        })?;
        let record = entity_record(&view, entity, selected_keys.get(id.as_str()).cloned());
        match entity.kind {
            EntityKind::Issue => issues.push(record),
            EntityKind::Group => groups.push(record),
        }
    }

    let mut editable_parents = Vec::new();
    let mut readonly_parents = Vec::new();
    for entity in view.iter() {
        let Some(parent) = &entity.parent else {
            continue;
        };
        let relation = ParentRelation {
            child: id_reference(&entity.id),
            parent: id_reference(parent),
        };
        if selected.contains(&entity.id) {
            editable_parents.push(relation);
        } else if selected.contains(parent) {
            readonly_parents.push(relation);
        }
    }
    let mut editable_dependencies = Vec::new();
    let mut readonly_dependencies = Vec::new();
    for (source, target) in &snapshot.dependencies {
        let relation = DependencyRelation {
            dependent: id_reference(source),
            prerequisite: id_reference(target),
        };
        if selected.contains(source) {
            editable_dependencies.push(relation);
        } else if selected.contains(target) {
            readonly_dependencies.push(relation);
        }
    }

    let mut reference_ids = BTreeSet::new();
    for relation in editable_parents.iter().chain(readonly_parents.iter()) {
        add_external_reference(&relation.child, selected, &mut reference_ids);
        add_external_reference(&relation.parent, selected, &mut reference_ids);
    }
    for relation in editable_dependencies
        .iter()
        .chain(readonly_dependencies.iter())
    {
        add_external_reference(&relation.dependent, selected, &mut reference_ids);
        add_external_reference(&relation.prerequisite, selected, &mut reference_ids);
    }
    for id in selected {
        if let Some(entity) = view.get(id)
            && let ResurfaceCondition::AfterEntity(target) = &entity.resurface_condition
            && !selected.contains(target)
        {
            reference_ids.insert(target.clone());
        }
    }
    let mut pending = reference_ids.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let entity = view.get(&id).ok_or_else(|| {
            DeclarationError::Reference {
                reason: format!("referenced Entity {id} does not exist in the current DB"),
                guidance: "Check the active root and ID with `axon list --skip-command-evaluation`; export the intended existing Entity. See `axon docs declaration` for external snapshots.".to_string(),
            }
        })?;
        if let ResurfaceCondition::AfterEntity(target) = &entity.resurface_condition
            && !selected.contains(target)
            && reference_ids.insert(target.clone())
        {
            pending.push(target.clone());
        }
    }
    let entities = reference_ids
        .iter()
        .map(|id| {
            let entity = view.get(id).expect("reference closure validated");
            ReferenceRecord {
                id: id.to_string(),
                kind: kind_record(entity.kind),
                base: fingerprint(&view, entity),
                title: entity.title.clone(),
                observed: observed(entity),
            }
        })
        .collect();

    Ok(PlanFile {
        schema: SCHEMA.to_string(),
        issues,
        groups,
        relations: Relations {
            editable: RelationSet {
                parents: editable_parents,
                dependencies: editable_dependencies,
            },
            readonly: RelationSet {
                parents: readonly_parents,
                dependencies: readonly_dependencies,
            },
        },
        references: References { entities },
    })
}

fn add_external_reference(
    reference: &EntityReference,
    selected: &BTreeSet<EntityId>,
    output: &mut BTreeSet<EntityId>,
) {
    if let Some(id) = &reference.id {
        let id = EntityId::from_stored(id);
        if !selected.contains(&id) {
            output.insert(id);
        }
    }
}

fn entity_record(view: &View, entity: &Entity, key: Option<String>) -> EntityRecord {
    EntityRecord {
        id: Some(entity.id.to_string()),
        key,
        base: Some(fingerprint(view, entity)),
        title: entity.title.clone(),
        description: entity.description.clone(),
        observed: observed(entity),
    }
}

fn observed(entity: &Entity) -> Observed {
    let (progress, claim) = match &entity.progress {
        Progress::NotStarted => (ProgressRecord::NotStarted, None),
        Progress::InProgress(claim) => (
            ProgressRecord::InProgress,
            Some(ClaimRecord {
                actor: claim.actor.clone(),
                worktree: claim.worktree.clone(),
                at: claim.at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            }),
        ),
        Progress::Ended => (ProgressRecord::Ended, None),
    };
    let disposition = match entity.disposition {
        Disposition::Undecided => DispositionRecord::Undecided,
        Disposition::Accepted => DispositionRecord::Accepted,
        Disposition::Rejected => DispositionRecord::Rejected,
    };
    let resurface = match &entity.resurface_condition {
        ResurfaceCondition::Always => ResurfaceRecord::Always,
        ResurfaceCondition::Manual => ResurfaceRecord::Manual,
        ResurfaceCondition::Command(command) => ResurfaceRecord::Command {
            command: command.clone(),
        },
        ResurfaceCondition::AtDate(date) => ResurfaceRecord::AtDate {
            date: date.to_string(),
        },
        ResurfaceCondition::AfterEntity(id) => ResurfaceRecord::AfterEntity {
            entity: id_reference(id),
        },
    };
    Observed {
        progress,
        claim,
        disposition,
        resurface,
    }
}

fn initial_observed() -> Observed {
    Observed {
        progress: ProgressRecord::NotStarted,
        claim: None,
        disposition: DispositionRecord::Accepted,
        resurface: ResurfaceRecord::Always,
    }
}

fn id_reference(id: &EntityId) -> EntityReference {
    EntityReference {
        id: Some(id.to_string()),
        key: None,
    }
}

fn fingerprint(view: &View, entity: &Entity) -> String {
    let mut hasher = blake3::Hasher::new();
    hash_token(&mut hasher, SCHEMA);
    hash_token(&mut hasher, FINGERPRINT_VERSION);
    hash_token(&mut hasher, entity.kind.as_db());
    hash_token(&mut hasher, entity.id.as_str());
    hash_token(&mut hasher, &entity.title);
    match &entity.description {
        None => hash_token(&mut hasher, "none"),
        Some(description) => {
            hash_token(&mut hasher, "some");
            hash_token(&mut hasher, description);
        }
    }
    match &entity.parent {
        None => hash_token(&mut hasher, "none"),
        Some(parent) => {
            hash_token(&mut hasher, "some");
            hash_token(&mut hasher, parent.as_str());
        }
    }
    hash_token(&mut hasher, entity.progress.as_db());
    match entity.progress.claim() {
        None => hash_token(&mut hasher, "none"),
        Some(claim) => {
            hash_token(&mut hasher, "some");
            hash_token(&mut hasher, &claim.actor);
            hash_token(&mut hasher, &claim.worktree);
            hash_token(
                &mut hasher,
                &claim.at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            );
        }
    }
    hash_token(&mut hasher, entity.disposition.as_db());
    match &entity.resurface_condition {
        ResurfaceCondition::Always => hash_token(&mut hasher, "always"),
        ResurfaceCondition::Manual => hash_token(&mut hasher, "manual"),
        ResurfaceCondition::Command(command) => {
            hash_token(&mut hasher, "command");
            hash_token(&mut hasher, command);
        }
        ResurfaceCondition::AtDate(date) => {
            hash_token(&mut hasher, "at_date");
            hash_token(&mut hasher, &date.to_string());
        }
        ResurfaceCondition::AfterEntity(target) => {
            hash_token(&mut hasher, "after_entity");
            hash_token(&mut hasher, target.as_str());
        }
    }
    let mut dependencies = view
        .direct_dependencies(&entity.id)
        .into_iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    hasher.update(&(dependencies.len() as u64).to_be_bytes());
    for dependency in dependencies {
        hash_token(&mut hasher, dependency);
    }
    format!("blake3:{}", hasher.finalize().to_hex())
}

fn hash_token(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn render_report(
    before: &StoreSnapshot,
    after: &StoreSnapshot,
    selected: &BTreeSet<EntityId>,
) -> Result<String> {
    let before_view = before.view();
    let after_view = after.view();
    let mut changes = Vec::new();
    for id in selected {
        let after_entity = after_view
            .get(id)
            .expect("selected Entity exists after import");
        let before_entity = before_view.get(id);
        if before_entity.is_none() {
            changes.push(format!(
                "  create {} {}: {}",
                after_entity.kind.label(),
                id,
                after_entity.title
            ));
        } else if let Some(before_entity) = before_entity {
            if before_entity.title != after_entity.title {
                changes.push(format!("  {id}: title updated"));
            }
            if before_entity.description != after_entity.description {
                changes.push(format!("  {id}: description updated"));
            }
        }
        let before_parent = before_entity.and_then(|entity| entity.parent.as_ref());
        if before_parent != after_entity.parent.as_ref() {
            changes.push(format!(
                "  {id}: parent {} -> {}",
                optional_id(before_parent),
                optional_id(after_entity.parent.as_ref())
            ));
        }
        let before_deps = direct_dependency_set(before, id);
        let after_deps = direct_dependency_set(after, id);
        for removed in before_deps.difference(&after_deps) {
            changes.push(format!("  {id}: dependency removed {removed}"));
        }
        for added in after_deps.difference(&before_deps) {
            changes.push(format!("  {id}: dependency added {added}"));
        }
    }

    let all_ids = before
        .entities
        .iter()
        .map(|entity| entity.id.clone())
        .chain(after.entities.iter().map(|entity| entity.id.clone()))
        .collect::<BTreeSet<_>>();
    let mut derived = Vec::new();
    for id in all_ids {
        let before_flags = before_view
            .get(&id)
            .map(|entity| flags(&before_view, entity))
            .transpose()?;
        let after_flags = after_view
            .get(&id)
            .map(|entity| flags(&after_view, entity))
            .transpose()?;
        if before_flags != after_flags {
            derived.push(format!(
                "  {id}: {} -> {}",
                before_flags
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "absent".to_string()),
                after_flags
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "absent".to_string())
            ));
        }
    }

    let mut output = String::from("Plan is valid.\nChanges:\n");
    if changes.is_empty() {
        output.push_str("  none\n");
    } else {
        output.push_str(&changes.join("\n"));
        output.push('\n');
    }
    output
        .push_str("Derived changes (ready, blocked, orphaned, active_scope, group_completable):\n");
    if derived.is_empty() {
        output.push_str("  none\n");
    } else {
        output.push_str(&derived.join("\n"));
        output.push('\n');
    }
    for entity in after_view
        .iter()
        .filter(|entity| after_view.is_orphaned(&entity.id))
    {
        if before_view
            .get(&entity.id)
            .is_none_or(|_| !before_view.is_orphaned(&entity.id))
        {
            output.push_str(&format!("Warning: {} becomes orphaned.\n", entity.id));
        }
    }
    Ok(output)
}

#[derive(PartialEq, Eq)]
struct DerivedFlags {
    ready: bool,
    blocked: bool,
    orphaned: bool,
    active_scope: bool,
    group_completable: Option<bool>,
}

impl std::fmt::Display for DerivedFlags {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ready={}, blocked={}, orphaned={}, active_scope={}, group_completable={}",
            self.ready,
            self.blocked,
            self.orphaned,
            self.active_scope,
            self.group_completable
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string())
        )
    }
}

fn flags(view: &View, entity: &Entity) -> Result<DerivedFlags> {
    Ok(DerivedFlags {
        ready: view.is_ready(entity)?,
        blocked: view.is_blocked(&entity.id),
        orphaned: view.is_orphaned(&entity.id),
        active_scope: view.within_active_scope(&entity.id)?,
        group_completable: (entity.kind == EntityKind::Group)
            .then(|| view.can_complete_group(&entity.id)),
    })
}

fn direct_dependency_set(snapshot: &StoreSnapshot, source: &EntityId) -> BTreeSet<EntityId> {
    snapshot
        .dependencies
        .iter()
        .filter(|(candidate, _)| candidate == source)
        .map(|(_, target)| target.clone())
        .collect()
}

fn optional_id(id: Option<&EntityId>) -> &str {
    id.map(EntityId::as_str).unwrap_or("null")
}

fn resolve_snapshot_id(snapshot: &StoreSnapshot, input: &str) -> Result<EntityId> {
    let matches = snapshot
        .entities
        .iter()
        .filter(|entity| entity.id.as_str() == input || entity.id.as_str().ends_with(input))
        .map(|entity| entity.id.clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(DeclarationError::Invalid(format!(
            "entity {input} does not exist"
        ))),
        candidates => Err(DeclarationError::Invalid(format!(
            "{input} matches multiple entities: {}",
            candidates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn selected_ids(document: &PlanFile) -> Result<BTreeSet<EntityId>> {
    records(document)
        .map(|(record, _)| {
            record
                .id
                .as_deref()
                .map(EntityId::from_stored)
                .ok_or_else(|| DeclarationError::Invalid("Entity ID is null".to_string()))
        })
        .collect()
}

fn selected_keys(document: &PlanFile) -> Result<HashMap<String, String>> {
    records(document)
        .filter_map(|(record, _)| {
            record
                .key
                .as_ref()
                .map(|key| (record.id.as_ref(), key.clone()))
        })
        .map(|(id, key)| {
            id.cloned()
                .map(|id| (id, key))
                .ok_or_else(|| DeclarationError::Invalid("Entity ID is null".to_string()))
        })
        .collect()
}

fn records(document: &PlanFile) -> impl Iterator<Item = (&EntityRecord, EntityKind)> {
    document
        .issues
        .iter()
        .map(|record| (record, EntityKind::Issue))
        .chain(
            document
                .groups
                .iter()
                .map(|record| (record, EntityKind::Group)),
        )
}

fn record_by_id<'a>(
    document: &'a PlanFile,
    id: &EntityId,
) -> Result<(&'a EntityRecord, EntityKind)> {
    records(document)
        .find(|(record, _)| record.id.as_deref() == Some(id.as_str()))
        .ok_or_else(|| DeclarationError::Invalid(format!("Entity {id} is not editable")))
}

fn kind_record(kind: EntityKind) -> KindRecord {
    match kind {
        EntityKind::Issue => KindRecord::Issue,
        EntityKind::Group => KindRecord::Group,
    }
}

fn entity_kind(kind: KindRecord) -> EntityKind {
    match kind {
        KindRecord::Issue => EntityKind::Issue,
        KindRecord::Group => EntityKind::Group,
    }
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        DeclarationError::Invalid(format!("invalid declaration path: {}", path.display()))
    })?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    for _ in 0..100 {
        let temporary = parent.join(format!(
            ".{}.axon-{:016x}.tmp",
            file_name.to_string_lossy(),
            rand::random::<u64>()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(contents.as_bytes())?;
            if let Some(permissions) = permissions.clone() {
                file.set_permissions(permissions)?;
            }
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result.map_err(Into::into);
    }
    Err(DeclarationError::Invalid(
        "could not allocate a temporary declaration file".to_string(),
    ))
}

fn required_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn quoted_string<S>(value: &String, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    DoubleQuoted(value).serialize(serializer)
}

fn quoted_option<S>(value: &Option<String>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&DoubleQuoted(value)),
        None => serializer.serialize_none(),
    }
}

fn description_option<S>(
    value: &Option<String>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) if value.contains('\n') => serializer.serialize_some(&LitStr(value)),
        Some(value) => serializer.serialize_some(value),
        None => serializer.serialize_none(),
    }
}

fn flow_reference<S>(value: &EntityReference, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    FlowMap(value).serialize(serializer)
}
