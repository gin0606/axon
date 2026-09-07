use crate::core::{Context, Error, Result, StateSnapshot};
use crate::domain::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stream {
    Note,
    Revision,
    State,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Origin {
    Operation,
    Created,
    Migration { schema: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlProof {
    pub progress: Progress,
    pub disposition: Disposition,
    pub resurface: ResurfaceCondition,
    pub current_revision: Option<RecordId>,
    pub last_revision: Option<RecordId>,
}
impl ControlProof {
    pub fn of(entity: &Entity, last_revision: Option<RecordId>) -> Self {
        Self {
            progress: entity.progress.clone(),
            disposition: entity.disposition,
            resurface: entity.resurface_condition.clone(),
            current_revision: entity.current_revision,
            last_revision,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub owner: EntityId,
    pub stream: Stream,
    pub parents: BTreeSet<RecordId>,
    pub origin: Origin,
    pub result: Option<ControlProof>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    pub last_revision: Option<RecordId>,
    pub head: Option<RecordId>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub entity: Entity,
    pub dependencies: BTreeSet<EntityId>,
    pub last_revision: Option<RecordId>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeInput {
    pub identity: String,
    pub heads: BTreeSet<RecordId>,
    pub candidate: Bundle,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRecord {
    pub inputs: Vec<MergeInput>,
    pub selected: String,
    pub result: Bundle,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub bundle: Bundle,
    pub source_schema: Option<u32>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CausalState {
    pub owners: BTreeMap<EntityId, Lineage>,
    pub links: BTreeMap<RecordId, Link>,
    pub merges: BTreeMap<RecordId, MergeRecord>,
    pub baselines: BTreeMap<RecordId, Baseline>,
}
fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidState(message.into())
}
impl CausalState {
    pub fn display(&self, owner: &EntityId) -> String {
        let interesting = self
            .links
            .values()
            .any(|l| l.owner == *owner && l.parents.len() > 1)
            || self.merges.values().any(|m| m.result.entity.id == *owner);
        let interesting = interesting
            || self.frontier(owner, Stream::Note).len() > 1
            || self.frontier(owner, Stream::Revision).len() > 1;
        if !interesting {
            return String::new();
        }
        let mut output =
            String::from("\nCausal history (concurrent records are ordered by ID, not time)\n");
        if let Some(lineage) = self.owners.get(owner) {
            output.push_str(&format!(
                "  Current head: {}  Last Revision: {}\n",
                lineage.head.map(|id| id.to_string()).unwrap_or_default(),
                lineage
                    .last_revision
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "none".into())
            ));
        }
        if let Ok(order) = self.order() {
            for id in order
                .into_iter()
                .filter(|id| self.links[id].owner == *owner)
            {
                let link = &self.links[&id];
                output.push_str(&format!(
                    "  {id} <- {}\n",
                    link.parents
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                if let Some(merge) = self.merges.get(&id) {
                    output.push_str(&format!("    Merge selected input {}\n", merge.selected));
                }
            }
        }
        output
    }
    pub fn frontier(&self, owner: &EntityId, stream: Stream) -> BTreeSet<RecordId> {
        let mut tips: BTreeSet<_> = self
            .links
            .iter()
            .filter(|(_, r)| r.owner == *owner && r.stream == stream)
            .map(|(id, _)| *id)
            .collect();
        for link in self
            .links
            .values()
            .filter(|r| r.owner == *owner && r.stream == stream)
        {
            for parent in &link.parents {
                tips.remove(parent);
            }
        }
        tips
    }
    pub fn order(&self) -> Result<Vec<RecordId>> {
        let mut todo: BTreeSet<_> = self.links.keys().copied().collect();
        let mut emitted = BTreeSet::new();
        let mut result = Vec::new();
        while !todo.is_empty() {
            let next = todo
                .iter()
                .find(|id| self.links[id].parents.is_subset(&emitted))
                .copied()
                .ok_or_else(|| invalid("causal cycle or missing predecessor"))?;
            todo.remove(&next);
            emitted.insert(next);
            result.push(next);
        }
        Ok(result)
    }
}
impl StateSnapshot {
    pub(crate) fn bundle(&self, entity: &Entity) -> Bundle {
        Bundle {
            entity: entity.clone(),
            dependencies: self
                .declaration
                .dependencies
                .iter()
                .filter(|(owner, _)| *owner == entity.id)
                .map(|(_, target)| target.clone())
                .collect(),
            last_revision: self
                .causal
                .owners
                .get(&entity.id)
                .and_then(|l| l.last_revision),
        }
    }
    pub fn append_causality(&mut self, before: &Self, ctx: &mut Context<'_>) -> Result<()> {
        for entity in self.declaration.entities.clone() {
            let owner = &entity.id;
            let old = before.histories.get(owner).cloned().unwrap_or_default();
            let history = self.histories.get(owner).cloned().unwrap_or_default();
            let previous_last = self.causal.owners.get(owner).and_then(|l| l.last_revision);
            let new_revisions = &history.revisions[old.revisions.len()..];
            let last = new_revisions.last().map(|r| r.id).or(previous_last);
            self.causal
                .owners
                .entry(owner.clone())
                .or_default()
                .last_revision = last;
            let mut predecessor = previous_last;
            for r in new_revisions {
                let parents = predecessor.into_iter().collect();
                predecessor = Some(r.id);
                self.causal.links.insert(
                    r.id,
                    Link {
                        owner: owner.clone(),
                        stream: Stream::Revision,
                        parents,
                        origin: Origin::Operation,
                        result: None,
                    },
                );
            }
            for n in &history.notes[old.notes.len()..] {
                let parents = self.causal.frontier(owner, Stream::Note);
                self.causal.links.insert(
                    n.id,
                    Link {
                        owner: owner.clone(),
                        stream: Stream::Note,
                        parents,
                        origin: Origin::Operation,
                        result: None,
                    },
                );
            }
            if !before.causal.owners.contains_key(owner) {
                let id = self.fresh_history_id(RecordKind::Baseline, ctx)?;
                let bundle = self.bundle(&entity);
                self.causal.baselines.insert(
                    id,
                    Baseline {
                        bundle,
                        source_schema: None,
                    },
                );
                self.causal.links.insert(
                    id,
                    Link {
                        owner: owner.clone(),
                        stream: Stream::State,
                        parents: BTreeSet::new(),
                        origin: Origin::Created,
                        result: Some(ControlProof::of(&entity, last)),
                    },
                );
                self.causal.owners.get_mut(owner).unwrap().head = Some(id);
            }
            for id in history.decisions[old.decisions.len()..]
                .iter()
                .map(|r| r.id)
                .chain(history.progress[old.progress.len()..].iter().map(|r| r.id))
            {
                let parents = self.causal.frontier(owner, Stream::State);
                self.causal.links.insert(
                    id,
                    Link {
                        owner: owner.clone(),
                        stream: Stream::State,
                        parents,
                        origin: Origin::Operation,
                        result: Some(ControlProof::of(&entity, last)),
                    },
                );
                self.causal.owners.get_mut(owner).unwrap().head = Some(id);
            }
        }
        self.validate_history()
    }
    fn fresh_history_id(&self, kind: RecordKind, ctx: &mut Context<'_>) -> Result<RecordId> {
        let id = (ctx.ids)(kind);
        if id.kind() != kind || self.causal.links.contains_key(&id) {
            return Err(invalid("generated history ID collision or kind mismatch"));
        }
        Ok(id)
    }
    /// v12 only knows within-table order; each chain remains independent until this baseline.
    #[cfg(test)]
    pub fn migrate_causality(&mut self, source_schema: u32) -> Result<()> {
        if !self.causal.links.is_empty() || !self.causal.owners.is_empty() {
            return Err(invalid("already has causal history"));
        }
        for entity in self.declaration.entities.clone() {
            let owner = &entity.id;
            let history = self.histories.get(owner).cloned().unwrap_or_default();
            let last = history.revisions.last().map(|r| r.id);
            for (stream, ids) in [
                (
                    Stream::Revision,
                    history.revisions.iter().map(|r| r.id).collect::<Vec<_>>(),
                ),
                (Stream::Note, history.notes.iter().map(|r| r.id).collect()),
                (
                    Stream::State,
                    history.decisions.iter().map(|r| r.id).collect(),
                ),
                (
                    Stream::State,
                    history.progress.iter().map(|r| r.id).collect(),
                ),
            ] {
                let mut parents = BTreeSet::new();
                for id in ids {
                    if self
                        .causal
                        .links
                        .insert(
                            id,
                            Link {
                                owner: owner.clone(),
                                stream,
                                parents,
                                origin: Origin::Migration {
                                    schema: source_schema,
                                },
                                result: None,
                            },
                        )
                        .is_some()
                    {
                        return Err(invalid("duplicate source record ID"));
                    }
                    parents = BTreeSet::from([id]);
                }
            }
            self.causal.owners.insert(
                owner.clone(),
                Lineage {
                    last_revision: last,
                    head: None,
                },
            );
            let bundle = self.bundle(&entity);
            let payload = serde_json::to_vec(&(
                source_schema,
                &self.metadata,
                &bundle,
                self.causal.frontier(owner, Stream::State),
            ))
            .map_err(|e| invalid(e.to_string()))?;
            let id = RecordId::deterministic(RecordKind::Baseline, &payload);
            let parents = self.causal.frontier(owner, Stream::State);
            self.causal.baselines.insert(
                id,
                Baseline {
                    bundle,
                    source_schema: Some(source_schema),
                },
            );
            self.causal.links.insert(
                id,
                Link {
                    owner: owner.clone(),
                    stream: Stream::State,
                    parents,
                    origin: Origin::Migration {
                        schema: source_schema,
                    },
                    result: Some(ControlProof::of(&entity, last)),
                },
            );
            self.causal.owners.get_mut(owner).unwrap().head = Some(id);
        }
        self.validate_history()
    }
    fn validate_resurface(&self, condition: &ResurfaceCondition) -> Result<()> {
        if let ResurfaceCondition::AfterEntity(target) = condition
            && !self
                .declaration
                .entities
                .iter()
                .any(|entity| entity.id == *target)
        {
            return Err(invalid("missing historical Resurface reference"));
        }
        Ok(())
    }
    fn validate_bundle(&self, bundle: &Bundle) -> Result<()> {
        let owner = &bundle.entity.id;
        let entity = self
            .declaration
            .entities
            .iter()
            .find(|e| e.id == *owner)
            .ok_or_else(|| invalid("missing bundle owner"))?;
        if entity.kind != bundle.entity.kind || entity.created_at != bundle.entity.created_at {
            return Err(invalid("bundle changes immutable identity"));
        }
        let ids: BTreeSet<_> = self.declaration.entities.iter().map(|e| &e.id).collect();
        if bundle.dependencies.iter().any(|d| !ids.contains(d))
            || bundle
                .entity
                .parent
                .as_ref()
                .is_some_and(|p| !ids.contains(p))
        {
            return Err(invalid("missing bundle reference"));
        }
        self.validate_resurface(&bundle.entity.resurface_condition)?;
        let proof = ControlProof::of(&bundle.entity, bundle.last_revision);
        if (proof.disposition == Disposition::Undecided) != proof.current_revision.is_none()
            || (proof.current_revision.is_some() && proof.current_revision != proof.last_revision)
        {
            return Err(invalid("invalid bundle Revision state"));
        }
        for id in [proof.last_revision, proof.current_revision]
            .into_iter()
            .flatten()
        {
            if !self
                .histories
                .get(owner)
                .is_some_and(|h| h.revisions.iter().any(|r| r.id == id))
            {
                return Err(invalid("invalid bundle Revision owner"));
            }
        }
        if let Some(id) = proof.current_revision {
            let revision = self.histories[owner]
                .revisions
                .iter()
                .find(|r| r.id == id)
                .unwrap();
            if revision.title != bundle.entity.title
                || revision.description != bundle.entity.description
                || revision.parent != bundle.entity.parent
                || revision
                    .dependencies
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    != bundle.dependencies
            {
                return Err(invalid("bundle differs from decided Revision"));
            }
        }
        Ok(())
    }
    pub fn validate_history(&self) -> Result<()> {
        let entities: BTreeMap<_, _> = self
            .declaration
            .entities
            .iter()
            .map(|e| (&e.id, e))
            .collect();
        if entities.len() != self.declaration.entities.len()
            || entities.len() != self.causal.owners.len()
            || self
                .causal
                .owners
                .keys()
                .any(|id| !entities.contains_key(id))
        {
            return Err(invalid("Entity/lineage ownership mismatch"));
        }
        let mut expected = BTreeMap::new();
        for (owner, history) in &self.histories {
            if !entities.contains_key(owner) {
                return Err(invalid("history owner is missing"));
            }
            for (kind, ids) in [
                (
                    RecordKind::Revision,
                    history.revisions.iter().map(|r| r.id).collect::<Vec<_>>(),
                ),
                (
                    RecordKind::Note,
                    history.notes.iter().map(|r| r.id).collect(),
                ),
                (
                    RecordKind::Decision,
                    history.decisions.iter().map(|r| r.id).collect(),
                ),
                (
                    RecordKind::Progress,
                    history.progress.iter().map(|r| r.id).collect(),
                ),
            ] {
                for id in ids {
                    if id.kind() != kind || expected.insert(id, owner.clone()).is_some() {
                        return Err(invalid("duplicate record ID or wrong kind"));
                    }
                }
            }
            for revision in &history.revisions {
                if revision
                    .parent
                    .as_ref()
                    .is_some_and(|p| !entities.contains_key(p))
                    || revision
                        .dependencies
                        .iter()
                        .any(|d| !entities.contains_key(d))
                    || revision.dependencies.iter().collect::<BTreeSet<_>>().len()
                        != revision.dependencies.len()
                {
                    return Err(invalid("invalid Revision reference"));
                }
            }
            for note in &history.notes {
                if note.body.trim().is_empty() {
                    return Err(invalid("empty Note"));
                }
            }
            for event in &history.decisions {
                let correct = match event.field.as_str() {
                    "disposition" => match event.new_value.as_deref() {
                        Some("undecided") => event.revision.is_none(),
                        Some("accepted" | "rejected") => event
                            .revision
                            .is_some_and(|id| history.revisions.iter().any(|r| r.id == id)),
                        _ => false,
                    },
                    "resurface_condition" => event.revision.is_none(),
                    _ => false,
                };
                if !correct {
                    return Err(invalid("invalid decision payload or Revision owner"));
                }
            }
        }
        for (id, b) in &self.causal.baselines {
            let link = self
                .causal
                .links
                .get(id)
                .ok_or_else(|| invalid("baseline link missing"))?;
            if link.result.as_ref()
                != Some(&ControlProof::of(&b.bundle.entity, b.bundle.last_revision))
                || !matches!(
                    (&link.origin, b.source_schema),
                    (Origin::Created, None)
                        | (Origin::Migration { schema: 11 }, Some(11))
                        | (Origin::Migration { schema: 12 }, Some(12))
                )
            {
                return Err(invalid("baseline origin or result mismatch"));
            }
            self.validate_bundle(&b.bundle)?;
            if id.kind() != RecordKind::Baseline
                || expected.insert(*id, b.bundle.entity.id.clone()).is_some()
            {
                return Err(invalid("invalid baseline ID"));
            }
        }
        for (id, m) in &self.causal.merges {
            if id.kind() != RecordKind::Merge
                || expected.insert(*id, m.result.entity.id.clone()).is_some()
            {
                return Err(invalid("invalid merge ID"));
            }
            self.validate_bundle(&m.result)?;
            for input in &m.inputs {
                self.validate_bundle(&input.candidate)?;
                if input.candidate.entity.id != m.result.entity.id
                    || input.identity.len() != 64
                    || !input
                        .identity
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || input.heads.is_empty()
                {
                    return Err(invalid("invalid merge input identity or owner"));
                }
                for head in &input.heads {
                    let link = self
                        .causal
                        .links
                        .get(head)
                        .ok_or_else(|| invalid("missing merge input head"))?;
                    if link.owner != m.result.entity.id || link.stream != Stream::State {
                        return Err(invalid("invalid merge input head"));
                    }
                    if link.result.as_ref()
                        != Some(&ControlProof::of(
                            &input.candidate.entity,
                            input.candidate.last_revision,
                        ))
                    {
                        return Err(invalid("merge input candidate and head proof disagree"));
                    }
                }
            }
            if self.causal.links.get(id).and_then(|l| l.result.as_ref())
                != Some(&ControlProof::of(&m.result.entity, m.result.last_revision))
            {
                return Err(invalid("merge result proof mismatch"));
            }
            let chosen = m
                .inputs
                .iter()
                .find(|i| i.identity == m.selected)
                .ok_or_else(|| invalid("merge selection missing"))?;
            if chosen.candidate != m.result
                || m.inputs.len() < 2
                || m.inputs
                    .iter()
                    .map(|i| &i.identity)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != m.inputs.len()
            {
                return Err(invalid("invalid merge result or identities"));
            }
            let parents: BTreeSet<_> = m
                .inputs
                .iter()
                .flat_map(|i| i.heads.iter().copied())
                .collect();
            if self
                .causal
                .links
                .get(id)
                .is_none_or(|l| l.parents != parents)
            {
                return Err(invalid("merge predecessor mismatch"));
            }
        }
        if expected.len() != self.causal.links.len() {
            return Err(invalid("record/link mismatch"));
        }
        for (id, link) in &self.causal.links {
            if expected.get(id) != Some(&link.owner) || !entities.contains_key(&link.owner) {
                return Err(invalid("record owner mismatch"));
            }
            let stream = match id.kind() {
                RecordKind::Note => Stream::Note,
                RecordKind::Revision => Stream::Revision,
                RecordKind::Decision
                | RecordKind::Progress
                | RecordKind::Baseline
                | RecordKind::Merge => Stream::State,
                RecordKind::Store => return Err(invalid("store ID used as record")),
            };
            if link.stream != stream {
                return Err(invalid("record stream mismatch"));
            }
            for p in &link.parents {
                if self
                    .causal
                    .links
                    .get(p)
                    .is_none_or(|l| l.owner != link.owner || l.stream != stream)
                {
                    return Err(invalid("invalid predecessor owner/stream"));
                }
            }
            if matches!(link.origin, Origin::Created) && id.kind() != RecordKind::Baseline {
                return Err(invalid("Created origin requires baseline"));
            }
            if matches!(link.origin, Origin::Migration{schema} if schema!=11 && schema!=12) {
                return Err(invalid("unknown migration origin"));
            }
            if stream != Stream::State && link.result.is_some() {
                return Err(invalid("non-state record has Control proof"));
            }
            if stream == Stream::State
                && !matches!(link.origin, Origin::Migration { .. })
                && link.result.is_none()
            {
                return Err(invalid("missing Control proof"));
            }
            if let Some(proof) = &link.result {
                self.validate_resurface(&proof.resurface)?;
                let history = self.histories.get(&link.owner).cloned().unwrap_or_default();
                if let Some(event) = history.decisions.iter().find(|e| e.id == *id) {
                    let matches = match event.field.as_str() {
                        "disposition" => {
                            event.new_value.as_deref() == Some(proof.disposition.as_db())
                                && event.revision == proof.current_revision
                        }
                        "resurface_condition" => {
                            event.new_value
                                == (!matches!(proof.resurface, ResurfaceCondition::Always))
                                    .then(|| proof.resurface.label())
                        }
                        _ => false,
                    };
                    if !matches {
                        return Err(invalid("decision and result proof disagree"));
                    }
                }
                if let Some(event) = history.progress.iter().find(|e| e.id == *id) {
                    let matches = match event.kind {
                        crate::core::ProgressEventKind::Start => {
                            matches!(proof.progress, Progress::InProgress(_))
                        }
                        crate::core::ProgressEventKind::Done => {
                            matches!(proof.progress, Progress::Ended)
                        }
                        crate::core::ProgressEventKind::Release => {
                            matches!(proof.progress, Progress::NotStarted)
                        }
                    };
                    if !matches {
                        return Err(invalid("progress record and result proof disagree"));
                    }
                }
                let history = self.histories.get(&link.owner).cloned().unwrap_or_default();
                for revision in [proof.current_revision, proof.last_revision]
                    .into_iter()
                    .flatten()
                {
                    if !history.revisions.iter().any(|r| r.id == revision) {
                        return Err(invalid("proof references another owner's Revision"));
                    }
                }
                if (proof.disposition == Disposition::Undecided) != proof.current_revision.is_none()
                    || (proof.current_revision.is_some()
                        && proof.current_revision != proof.last_revision)
                {
                    return Err(invalid("invalid Control proof Revision"));
                }
            }
        }
        self.causal.order()?;
        for entity in &self.declaration.entities {
            let lineage = &self.causal.owners[&entity.id];
            let head = lineage
                .head
                .ok_or_else(|| invalid("missing current history head"))?;
            if self.causal.frontier(&entity.id, Stream::State) != BTreeSet::from([head]) {
                return Err(invalid("current history head does not cover state records"));
            }
            if self.causal.links[&head].result.as_ref()
                != Some(&ControlProof::of(entity, lineage.last_revision))
            {
                return Err(invalid("current value has no matching history proof"));
            }
            if let Some(current) = entity.current_revision {
                let history = self
                    .histories
                    .get(&entity.id)
                    .ok_or_else(|| invalid("missing current Revision"))?;
                let revision = history
                    .revisions
                    .iter()
                    .find(|r| r.id == current)
                    .ok_or_else(|| invalid("missing current Revision"))?;
                let deps: BTreeSet<_> = self
                    .declaration
                    .dependencies
                    .iter()
                    .filter(|(id, _)| *id == entity.id)
                    .map(|(_, id)| id.clone())
                    .collect();
                if revision.title != entity.title
                    || revision.description != entity.description
                    || revision.parent != entity.parent
                    || revision
                        .dependencies
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        != deps
                {
                    return Err(invalid("decided declaration differs from current Revision"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::{Change, MetadataValue, Operation, tests as fixture};
    pub fn branching() -> StateSnapshot {
        let mut common = fixture::execute(
            &fixture::empty(),
            Operation::Insert(fixture::entity("i", EntityKind::Issue, None), Vec::new()),
            1,
        )
        .unwrap()
        .state()
        .clone();
        common.metadata.insert(
            "store_id".into(),
            MetadataValue::Text(RecordId::deterministic(RecordKind::Store, b"fixture").to_string()),
        );
        common
            .metadata
            .insert("prefix".into(), MetadataValue::Text("t".into()));
        let change = |state: &StateSnapshot, change, tick| {
            fixture::execute(state, Operation::Change(fixture::id("i"), change), tick)
                .unwrap()
                .state()
                .clone()
        };
        let mut left = change(&common, Change::Decide(Disposition::Accepted), 2);
        left = fixture::execute(
            &left,
            Operation::AddNote {
                owner: fixture::id("i"),
                body: "same body".into(),
            },
            3,
        )
        .unwrap()
        .state()
        .clone();
        let right = change(&common, Change::SetTitle("other".into()), 4);
        let mut right = change(&right, Change::Decide(Disposition::Accepted), 5);
        right = fixture::execute(
            &right,
            Operation::AddNote {
                owner: fixture::id("i"),
                body: "same body".into(),
            },
            6,
        )
        .unwrap()
        .state()
        .clone();
        let owner = fixture::id("i");
        let inputs = [&left, &right]
            .iter()
            .enumerate()
            .map(|(i, state)| MergeInput {
                identity: format!("{i:064x}"),
                heads: state.causal.frontier(&owner, Stream::State),
                candidate: state.bundle(&state.declaration.entities[0]),
            })
            .collect::<Vec<_>>();
        let mut merged = left.clone();
        let history = merged.histories.get_mut(&owner).unwrap();
        history
            .revisions
            .extend(right.histories[&owner].revisions.clone());
        history.notes.extend(right.histories[&owner].notes.clone());
        history
            .decisions
            .extend(right.histories[&owner].decisions.clone());
        merged.causal.links.extend(right.causal.links.clone());
        let id = RecordId::deterministic(RecordKind::Merge, b"fixture merge");
        let result = inputs[0].candidate.clone();
        let parents = inputs
            .iter()
            .flat_map(|i| i.heads.iter().copied())
            .collect();
        merged.causal.links.insert(
            id,
            Link {
                owner: owner.clone(),
                stream: Stream::State,
                parents,
                origin: Origin::Operation,
                result: Some(ControlProof::of(&result.entity, result.last_revision)),
            },
        );
        merged.causal.merges.insert(
            id,
            MergeRecord {
                selected: inputs[0].identity.clone(),
                inputs,
                result,
            },
        );
        merged.causal.owners.get_mut(&owner).unwrap().head = Some(id);
        merged.validate_history().unwrap();
        merged
    }
    #[test]
    fn edited_revision_uses_only_selected_lineage() {
        let mut state = branching();
        let owner = fixture::id("i");
        let selected = state.causal.owners[&owner].last_revision.unwrap();
        for (tick, change) in [
            Change::Decide(Disposition::Undecided),
            Change::SetTitle("third declaration".into()),
            Change::Decide(Disposition::Accepted),
        ]
        .into_iter()
        .enumerate()
        {
            state = fixture::execute(
                &state,
                Operation::Change(owner.clone(), change),
                tick as u64 + 10,
            )
            .unwrap()
            .state()
            .clone();
        }
        let new = state.causal.owners[&owner].last_revision.unwrap();
        assert_ne!(new, selected);
        assert_eq!(state.causal.links[&new].parents, BTreeSet::from([selected]));
        assert_eq!(state.causal.frontier(&owner, Stream::Revision).len(), 2);
        crate::codec::decode(
            &crate::codec::encode(&state).unwrap(),
            state.declaration.evaluation.clone(),
        )
        .unwrap();
    }
    #[test]
    fn rejects_fabricated_merge_control_state() {
        for selected in [false, true] {
            let mut state = branching();
            let (id, merge) = state.causal.merges.iter_mut().next().unwrap();
            let index = if selected { 0 } else { 1 };
            merge.inputs[index].candidate.entity.progress = Progress::Ended;
            if selected {
                merge.result.entity.progress = Progress::Ended;
                state.declaration.entities[0].progress = Progress::Ended;
                state
                    .causal
                    .links
                    .get_mut(id)
                    .unwrap()
                    .result
                    .as_mut()
                    .unwrap()
                    .progress = Progress::Ended;
            }
            assert!(state.validate_history().is_err());
            assert!(crate::codec::encode(&state).is_err());
        }
    }
    #[test]
    fn rejects_dangling_historical_resurface() {
        for location in 0..3 {
            let mut state = branching();
            let missing = ResurfaceCondition::AfterEntity(fixture::id("missing"));
            match location {
                0 => {
                    state.causal.merges.values_mut().next().unwrap().inputs[1]
                        .candidate
                        .entity
                        .resurface_condition = missing
                }
                1 => {
                    let (id, baseline) = state.causal.baselines.iter_mut().next().unwrap();
                    baseline.bundle.entity.resurface_condition = missing.clone();
                    state
                        .causal
                        .links
                        .get_mut(id)
                        .unwrap()
                        .result
                        .as_mut()
                        .unwrap()
                        .resurface = missing;
                }
                _ => {
                    let id = *state.causal.merges.values().next().unwrap().inputs[1]
                        .heads
                        .first()
                        .unwrap();
                    state
                        .causal
                        .links
                        .get_mut(&id)
                        .unwrap()
                        .result
                        .as_mut()
                        .unwrap()
                        .resurface = missing;
                }
            }
            assert!(state.validate_history().is_err());
            assert!(crate::codec::encode(&state).is_err());
        }
    }
    #[test]
    fn selected_revision_survives_other_branch_and_undecide() {
        let state = branching();
        let owner = fixture::id("i");
        let selected = state.causal.owners[&owner].last_revision;
        let undecided = fixture::execute(
            &state,
            Operation::Change(owner.clone(), Change::Decide(Disposition::Undecided)),
            10,
        )
        .unwrap();
        assert_eq!(
            undecided.state().causal.owners[&owner].last_revision,
            selected
        );
        let decided = fixture::execute(
            undecided.state(),
            Operation::Change(owner.clone(), Change::Decide(Disposition::Accepted)),
            11,
        )
        .unwrap();
        assert_eq!(
            decided.state().declaration.entities[0].current_revision,
            selected
        );
        assert_eq!(decided.state().histories[&owner].revisions.len(), 2);
        assert_eq!(decided.state().histories[&owner].notes.len(), 2);
        let note = fixture::execute(
            decided.state(),
            Operation::AddNote {
                owner: owner.clone(),
                body: "after merge".into(),
            },
            12,
        )
        .unwrap();
        let id = note.state().histories[&owner].notes.last().unwrap().id;
        assert_eq!(
            note.state().causal.links[&id].parents,
            state.causal.frontier(&owner, Stream::Note)
        );
    }
    #[test]
    fn codec_is_canonical_and_preserves_branch_payloads() {
        let mut state = branching();
        state
            .metadata
            .insert("real".into(), MetadataValue::Real(f64::INFINITY));
        state
            .metadata
            .insert("blob".into(), MetadataValue::Bytes(vec![0, 255]));
        let bytes = crate::codec::encode(&state).unwrap();
        let mut lines = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        lines.reverse();
        let shuffled = lines
            .into_iter()
            .map(|l| format!("  {l}  \n"))
            .collect::<String>();
        let decoded =
            crate::codec::decode(shuffled.as_bytes(), state.declaration.evaluation.clone())
                .unwrap();
        assert_eq!(crate::codec::encode(&decoded).unwrap(), bytes);
        assert_eq!(state.causal, decoded.causal);
        assert_eq!(state.metadata, decoded.metadata);
        assert_eq!(
            state.histories[&fixture::id("i")].notes.len(),
            decoded.histories[&fixture::id("i")].notes.len()
        );
    }
    #[test]
    fn codec_rejects_corruption_without_running_commands() {
        let state = branching();
        let bytes = crate::codec::encode(&state).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let eval = state.declaration.evaluation.clone();
        let first = text.lines().next().unwrap();
        for invalid in [
            format!("{text}{first}\n"),
            text.replacen("\"format\":1", "\"format\":99", 1),
            text.replacen("\"format\":1", "\"format\":1,\"format\":1", 1),
            text.replacen("\"description\":null,", "", 1),
            text.replacen("\"format\":1", "\"surprise\":true,\"format\":1", 1),
        ] {
            assert!(crate::codec::decode(invalid.as_bytes(), eval.clone()).is_err());
        }
        let mut corrupt = state.clone();
        let head = corrupt.causal.owners[&fixture::id("i")].head.unwrap();
        corrupt
            .causal
            .links
            .get_mut(&head)
            .unwrap()
            .parents
            .insert(head);
        assert!(corrupt.validate_history().is_err());
        let mut corrupt = state.clone();
        corrupt
            .causal
            .owners
            .get_mut(&fixture::id("i"))
            .unwrap()
            .last_revision = None;
        assert!(corrupt.validate_history().is_err());
        let mut corrupt = state.clone();
        corrupt.histories.get_mut(&fixture::id("i")).unwrap().notes[0].id =
            corrupt.histories[&fixture::id("i")].notes[1].id;
        assert!(corrupt.validate_history().is_err());
    }
}
