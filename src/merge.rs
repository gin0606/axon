//! In-memory three-way integration; callers own input files and atomic publication.
#![allow(dead_code)] // The CLI adapter is a separate integration layer.
use crate::{
    codec,
    core::{self, Context, Error, Operation, Result, StateSnapshot},
    derived::Evaluation,
    domain::*,
    history::*,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

fn invalid(message: impl ToString) -> Error {
    Error::InvalidState(message.to_string())
}
fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(invalid)
}

pub struct Input {
    bytes: Vec<u8>,
    identity: String,
    state: StateSnapshot,
}
impl Input {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub conflict: String,
    pub input: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct EntityChoice {
    pub id: String,
    pub owner: EntityId,
    pub base: Option<Bundle>,
    pub ours: Option<Bundle>,
    pub theirs: Option<Bundle>,
    pub automatic: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub id: String,
    pub owner: Option<EntityId>,
    pub message: String,
}

pub struct Prepared {
    base: Input,
    ours: Input,
    theirs: Input,
    choices: Vec<EntityChoice>,
}
/// Context and ID allocation are frozen by the caller's resolution artifact.
pub struct Repair<'a> {
    pub operation: Operation,
    pub context: Context<'a>,
}
pub struct Complete {
    state: StateSnapshot,
    bytes: Vec<u8>,
}
impl Complete {
    pub fn state(&self) -> &StateSnapshot {
        &self.state
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
pub enum Outcome {
    Complete(Complete),
    Conflicts(Vec<Conflict>),
}

fn bundles(state: &StateSnapshot) -> BTreeMap<EntityId, Bundle> {
    state
        .declaration
        .entities
        .iter()
        .map(|e| (e.id.clone(), state.bundle(e)))
        .collect()
}
fn same(a: &Bundle, b: &Bundle) -> bool {
    let mut a = a.clone();
    a.entity.updated_at = b.entity.updated_at;
    a == *b
}
fn automatic(base: Option<&Bundle>, ours: &Bundle, theirs: &Bundle) -> Option<bool> {
    if same(ours, theirs) {
        Some(ours.entity.updated_at >= theirs.entity.updated_at)
    } else if base.is_some_and(|b| same(b, ours)) {
        Some(false)
    } else if base.is_some_and(|b| same(b, theirs)) {
        Some(true)
    } else {
        None
    }
}
fn absorbed(into: &StateSnapshot, from: &StateSnapshot, owner: &EntityId) -> bool {
    let Some(entity) = from.declaration.entities.iter().find(|e| e.id == *owner) else {
        return false;
    };
    let candidate = from.bundle(entity);
    let Some(lineage) = into.causal.owners.get(owner) else {
        return false;
    };
    let mut todo: Vec<_> = lineage.head.into_iter().collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(m) = into.causal.merges.get(&id)
            && m.inputs.iter().any(|i| {
                i.heads == from.causal.owners[owner].head.into_iter().collect()
                    && same(&i.candidate, &candidate)
            })
        {
            return true;
        }
        if let Some(link) = into.causal.links.get(&id) {
            todo.extend(&link.parents);
        }
    }
    false
}
fn records(state: &StateSnapshot) -> Result<BTreeMap<RecordId, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for h in state.histories.values() {
        for r in &h.notes {
            result.insert(r.id, json(r)?);
        }
        for r in &h.revisions {
            result.insert(r.id, json(r)?);
        }
        for r in &h.decisions {
            result.insert(r.id, json(r)?);
        }
        for r in &h.progress {
            result.insert(r.id, json(r)?);
        }
    }
    for (id, r) in &state.causal.baselines {
        result.insert(*id, json(r)?);
    }
    for (id, r) in &state.causal.merges {
        result.insert(*id, json(r)?);
    }
    Ok(result)
}
fn retained(base: &StateSnapshot, side: &StateSnapshot) -> Result<()> {
    let side_records = records(side)?;
    for (id, payload) in records(base)? {
        if side_records.get(&id) != Some(&payload)
            || side.causal.links.get(&id) != base.causal.links.get(&id)
        {
            return Err(invalid(format!(
                "deleted or modified immutable record {id}"
            )));
        }
    }
    let sides = bundles(side);
    for e in &base.declaration.entities {
        if sides
            .get(&e.id)
            .is_none_or(|b| b.entity.kind != e.kind || b.entity.created_at != e.created_at)
        {
            return Err(invalid(format!(
                "deleted or modified Entity identity {}",
                e.id
            )));
        }
    }
    Ok(())
}
fn union<T: Clone + PartialEq>(
    a: &mut BTreeMap<RecordId, T>,
    b: &BTreeMap<RecordId, T>,
) -> Result<()> {
    for (id, value) in b {
        if a.get(id).is_some_and(|old| old != value) {
            return Err(invalid(format!("immutable record ID collision {id}")));
        }
        a.insert(*id, value.clone());
    }
    Ok(())
}
fn union_vec<T: Clone + PartialEq>(
    a: &mut Vec<T>,
    b: &[T],
    id: impl Fn(&T) -> RecordId,
) -> Result<()> {
    let mut values: BTreeMap<_, _> = a.iter().map(|r| (id(r), r.clone())).collect();
    union(&mut values, &b.iter().map(|r| (id(r), r.clone())).collect())?;
    *a = values.into_values().collect();
    Ok(())
}
fn history_union(a: &mut StateSnapshot, b: &StateSnapshot) -> Result<()> {
    for (owner, h) in &b.histories {
        let into = a.histories.entry(owner.clone()).or_default();
        union_vec(&mut into.notes, &h.notes, |r| r.id)?;
        union_vec(&mut into.revisions, &h.revisions, |r| r.id)?;
        union_vec(&mut into.decisions, &h.decisions, |r| r.id)?;
        union_vec(&mut into.progress, &h.progress, |r| r.id)?;
    }
    union(&mut a.causal.links, &b.causal.links)?;
    union(&mut a.causal.baselines, &b.causal.baselines)?;
    union(&mut a.causal.merges, &b.causal.merges)
}
fn structure(
    state: &StateSnapshot,
    group: &EntityId,
) -> BTreeMap<EntityId, (Option<EntityId>, BTreeSet<EntityId>)> {
    let view = state.declaration.view();
    std::iter::once(group.clone())
        .chain(view.descendants(group).into_iter().map(|e| e.id.clone()))
        .map(|id| {
            let e = view.get(&id).unwrap();
            (id, (e.parent.clone(), state.bundle(e).dependencies))
        })
        .collect()
}
pub fn validate(state: &StateSnapshot) -> Result<()> {
    state.validate_history()?;
    core::validate_snapshot_structure(&state.declaration)?;
    let view = state.declaration.view();
    for group in view
        .iter()
        .filter(|e| e.kind == EntityKind::Group && e.progress == Progress::Ended)
    {
        if let Some(child) = view
            .descendants(&group.id)
            .into_iter()
            .find(|e| !e.is_terminal())
        {
            return Err(invalid(format!(
                "Ended Group {} has unfinished descendant {}",
                group.id, child.id
            )));
        }
    }
    Ok(())
}
impl Prepared {
    pub fn new(
        base: Vec<u8>,
        ours: Vec<u8>,
        theirs: Vec<u8>,
        evaluation: Rc<Evaluation>,
    ) -> Result<Self> {
        let input = |bytes: Vec<u8>| -> Result<Input> {
            let state = codec::decode(&bytes, evaluation.clone())?;
            validate(&state)?;
            Ok(Input {
                identity: digest(&bytes),
                bytes,
                state,
            })
        };
        let (base, ours, theirs) = (input(base)?, input(ours)?, input(theirs)?);
        if base.state.metadata != ours.state.metadata
            || base.state.metadata != theirs.state.metadata
        {
            return Err(invalid(
                "store identity or metadata differs between merge inputs",
            ));
        }
        retained(&base.state, &ours.state)?;
        retained(&base.state, &theirs.state)?;
        let mut combined = ours.state.clone();
        history_union(&mut combined, &theirs.state)?;
        let (b, o, t) = (
            bundles(&base.state),
            bundles(&ours.state),
            bundles(&theirs.state),
        );
        let mut choices = Vec::new();
        let mut identities = [ours.identity.clone(), theirs.identity.clone()];
        identities.sort();
        for owner in o.keys().chain(t.keys()).collect::<BTreeSet<_>>() {
            if let (Some(l), Some(r)) = (o.get(owner), t.get(owner)) {
                if l.entity.kind != r.entity.kind || l.entity.created_at != r.entity.created_at {
                    return Err(invalid(format!("Entity identity collision {owner}")));
                }
                // Shared creation evidence distinguishes an imported Entity from an independent ID collision.
                if !b.contains_key(owner)
                    && !ours.state.causal.baselines.iter().any(|(id, baseline)| {
                        baseline.bundle.entity.id == *owner
                            && theirs.state.causal.baselines.get(id) == Some(baseline)
                    })
                {
                    return Err(invalid(format!("independent Entity ID collision {owner}")));
                }
            }
            let mut chosen = match (o.get(owner), t.get(owner)) {
                (Some(l), Some(r)) => automatic(b.get(owner), l, r).map(|left| {
                    if left {
                        ours.identity.clone()
                    } else {
                        theirs.identity.clone()
                    }
                }),
                (Some(_), None) => Some(ours.identity.clone()),
                (None, Some(_)) => Some(theirs.identity.clone()),
                _ => unreachable!(),
            };
            match (
                absorbed(&ours.state, &theirs.state, owner),
                absorbed(&theirs.state, &ours.state, owner),
            ) {
                (true, false) => chosen = Some(ours.identity.clone()),
                (false, true) => chosen = Some(theirs.identity.clone()),
                _ => {}
            }
            if let (Some(l), Some(r)) = (o.get(owner), t.get(owner)) {
                if same(l, r) {
                    let covers = |a: &StateSnapshot, b: &StateSnapshot| {
                        b.causal
                            .links
                            .iter()
                            .filter(|(_, link)| {
                                link.owner == *owner && link.stream == Stream::State
                            })
                            .all(|(id, link)| a.causal.links.get(id) == Some(link))
                    };
                    match (
                        covers(&ours.state, &theirs.state),
                        covers(&theirs.state, &ours.state),
                    ) {
                        (true, false) => chosen = Some(ours.identity.clone()),
                        (false, true) => chosen = Some(theirs.identity.clone()),
                        _ => {}
                    }
                }
                if same(l, r)
                    && l.entity.updated_at == r.entity.updated_at
                    && (ours.state.causal.owners[owner].head
                        == theirs.state.causal.owners[owner].head
                        || (!ours
                            .state
                            .causal
                            .links
                            .contains_key(&theirs.state.causal.owners[owner].head.unwrap())
                            && !theirs
                                .state
                                .causal
                                .links
                                .contains_key(&ours.state.causal.owners[owner].head.unwrap())))
                {
                    chosen = Some(identities[0].clone());
                }
                if l.entity.kind == EntityKind::Group
                    && l.entity.progress == Progress::Ended
                    && r.entity.progress == Progress::Ended
                    && structure(&ours.state, owner) != structure(&theirs.state, owner)
                {
                    chosen = None;
                }
            }
            let id = digest(&json(&(
                "entity-choice-v1",
                &base.identity,
                &identities,
                owner,
            ))?);
            choices.push(EntityChoice {
                id,
                owner: owner.clone(),
                base: b.get(owner).cloned(),
                ours: o.get(owner).cloned(),
                theirs: t.get(owner).cloned(),
                automatic: chosen,
            });
        }
        Ok(Self {
            base,
            ours,
            theirs,
            choices,
        })
    }
    pub fn inputs(&self) -> [&Input; 3] {
        [&self.base, &self.ours, &self.theirs]
    }
    pub fn choices(&self) -> &[EntityChoice] {
        &self.choices
    }
    fn conflict(&self, owner: Option<EntityId>, message: String) -> Conflict {
        let mut sides = [&self.ours.identity, &self.theirs.identity];
        sides.sort();
        Conflict {
            id: digest(
                &json(&(
                    "structural-conflict-v1",
                    &self.base.identity,
                    sides,
                    &owner,
                    &message,
                ))
                .expect("serializable diagnostic"),
            ),
            owner,
            message,
        }
    }
    pub fn resolve(&self, choices: &[Choice], repairs: &mut [Repair<'_>]) -> Result<Outcome> {
        let mut selected = BTreeMap::new();
        for choice in choices {
            if !self.choices.iter().any(|c| c.id == choice.conflict)
                || selected.insert(&choice.conflict, &choice.input).is_some()
            {
                return Err(invalid("unknown, stale, or duplicate Entity choice"));
            }
        }
        let mut result = self.ours.state.clone();
        history_union(&mut result, &self.theirs.state)?;
        result.declaration.entities.clear();
        result.declaration.dependencies.clear();
        result.causal.owners.clear();
        let mut conflicts = Vec::new();
        let mut sources = BTreeMap::new();
        for choice in &self.choices {
            let explicit = selected.get(&choice.id);
            let Some(identity) = explicit.copied().or(choice.automatic.as_ref()) else {
                conflicts.push(Conflict {
                    id: choice.id.clone(),
                    owner: Some(choice.owner.clone()),
                    message: "both inputs changed the Entity bundle; select an input identity"
                        .into(),
                });
                continue;
            };
            let source = if identity == &self.ours.identity && choice.ours.is_some() {
                &self.ours
            } else if identity == &self.theirs.identity && choice.theirs.is_some() {
                &self.theirs
            } else {
                return Err(invalid(
                    "choice does not name an input containing the Entity",
                ));
            };
            let bundle = source.state.bundle(
                source
                    .state
                    .declaration
                    .entities
                    .iter()
                    .find(|e| e.id == choice.owner)
                    .unwrap(),
            );
            sources.insert(choice.owner.clone(), source);
            result.declaration.entities.push(bundle.entity.clone());
            result.declaration.dependencies.extend(
                bundle
                    .dependencies
                    .iter()
                    .map(|d| (choice.owner.clone(), d.clone())),
            );
            let owner = &choice.owner;
            let mut lineage = source.state.causal.owners[owner].clone();
            let tips = result.causal.frontier(owner, Stream::State);
            // A dominated input needs no new record when its chosen current value is unchanged.
            let covered = tips == lineage.head.into_iter().collect();
            let other = if identity == &self.ours.identity {
                &self.theirs
            } else {
                &self.ours
            };
            let selection_changes_value = match (&choice.ours, &choice.theirs) {
                (Some(a), Some(b)) => {
                    !same(a, b)
                        || (a.entity.kind == EntityKind::Group
                            && a.entity.progress == Progress::Ended
                            && b.entity.progress == Progress::Ended
                            && structure(&self.ours.state, owner)
                                != structure(&self.theirs.state, owner))
                }
                _ => false,
            };
            if !covered
                || (explicit.is_some()
                    && selection_changes_value
                    && !absorbed(&source.state, &other.state, owner))
            {
                let mut inputs: Vec<_> = [&self.ours, &self.theirs]
                    .into_iter()
                    .filter_map(|input| {
                        input
                            .state
                            .declaration
                            .entities
                            .iter()
                            .find(|e| e.id == *owner)
                            .map(|e| MergeInput {
                                identity: input.identity.clone(),
                                heads: input.state.causal.owners[owner].head.into_iter().collect(),
                                candidate: input.state.bundle(e),
                            })
                    })
                    .collect();
                inputs.sort_by(|a, b| a.identity.cmp(&b.identity));
                inputs.dedup_by(|a, b| a.identity == b.identity);
                if inputs.len() < 2 {
                    return Err(invalid(
                        "cannot integrate state without two distinct input candidates",
                    ));
                }
                let merge = MergeRecord {
                    inputs,
                    selected: identity.clone(),
                    result: bundle.clone(),
                };
                let id = RecordId::deterministic(
                    RecordKind::Merge,
                    &json(&("merge-v1", owner, &merge))?,
                );
                let parents = merge
                    .inputs
                    .iter()
                    .flat_map(|i| i.heads.iter().copied())
                    .collect();
                let link = Link {
                    owner: owner.clone(),
                    stream: Stream::State,
                    parents,
                    origin: Origin::Operation,
                    result: Some(ControlProof::of(&bundle.entity, bundle.last_revision)),
                };
                union(&mut result.causal.merges, &BTreeMap::from([(id, merge)]))?;
                union(&mut result.causal.links, &BTreeMap::from([(id, link)]))?;
                lineage.head = Some(id);
            }
            result.causal.owners.insert(owner.clone(), lineage);
        }
        if !conflicts.is_empty() {
            return Ok(Outcome::Conflicts(conflicts));
        }
        let ranks = result
            .causal
            .order()?
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, rank))
            .collect();
        for h in result.histories.values_mut() {
            codec::order_history(h, &ranks);
        }
        // State proofs must be valid before ordinary operations can append further history.
        result.validate_history()?;
        for repair in repairs {
            result = result
                .execute(repair.operation.clone(), &mut repair.context)?
                .state()
                .clone();
        }
        if let Err(error) = validate(&result) {
            conflicts.push(self.conflict(None, error.to_string()));
        }
        for e in &result.declaration.entities {
            if e.kind == EntityKind::Group
                && e.progress == Progress::Ended
                && let Some(source) = sources.get(&e.id)
            {
                // If a repair newly ends the Group, the core's done guard owns this check.
                let source_entity = source
                    .state
                    .declaration
                    .entities
                    .iter()
                    .find(|s| s.id == e.id)
                    .unwrap();
                if source_entity.progress == Progress::Ended
                    && structure(&result, &e.id) != structure(&source.state, &e.id)
                {
                    conflicts.push(self.conflict(Some(e.id.clone()), format!("Ended Group {} must retain its selected input's parent, dependencies and subtree structure", e.id)));
                }
            }
        }
        if !conflicts.is_empty() {
            return Ok(Outcome::Conflicts(conflicts));
        }
        let bytes = codec::encode(&result)?;
        Ok(Outcome::Complete(Complete {
            state: result,
            bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Change, MetadataValue, tests as f};
    fn run(s: &StateSnapshot, op: Operation, tick: u64) -> StateSnapshot {
        f::execute(s, op, tick).unwrap().state().clone()
    }
    fn change(s: &StateSnapshot, owner: &str, c: Change, tick: u64) -> StateSnapshot {
        run(s, Operation::Change(f::id(owner), c), tick)
    }
    fn fixture() -> StateSnapshot {
        let mut s = f::empty();
        s.metadata.insert(
            "store_id".into(),
            MetadataValue::Text(
                RecordId::deterministic(RecordKind::Store, b"merge fixture").to_string(),
            ),
        );
        s.metadata
            .insert("prefix".into(), MetadataValue::Text("t".into()));
        for (n, id) in ["a", "b"].iter().enumerate() {
            s = run(
                &s,
                Operation::Insert(f::entity(id, EntityKind::Issue, None)),
                n as u64 + 1,
            );
        }
        s
    }
    fn prepare(b: &StateSnapshot, o: &StateSnapshot, t: &StateSnapshot) -> Prepared {
        Prepared::new(
            codec::encode(b).unwrap(),
            codec::encode(o).unwrap(),
            codec::encode(t).unwrap(),
            b.declaration.evaluation.clone(),
        )
        .unwrap()
    }
    fn complete(p: &Prepared, choices: &[Choice]) -> Complete {
        match p.resolve(choices, &mut []).unwrap() {
            Outcome::Complete(c) => c,
            Outcome::Conflicts(c) => panic!("{c:?}"),
        }
    }
    fn conflicts(p: &Prepared) -> Vec<Conflict> {
        match p.resolve(&[], &mut []).unwrap() {
            Outcome::Conflicts(c) => c,
            Outcome::Complete(_) => panic!("expected conflict"),
        }
    }
    fn pick(p: &Prepared, owner: &str, ours: bool) -> Choice {
        Choice {
            conflict: p
                .choices
                .iter()
                .find(|c| c.owner == f::id(owner))
                .unwrap()
                .id
                .clone(),
            input: if ours {
                p.ours.identity.clone()
            } else {
                p.theirs.identity.clone()
            },
        }
    }
    fn note(s: &StateSnapshot, owner: &str, tick: u64) -> StateSnapshot {
        run(
            s,
            Operation::AddNote {
                owner: f::id(owner),
                body: "same text".into(),
            },
            tick,
        )
    }
    fn claim(actor: &str) -> Claim {
        Claim {
            actor: actor.into(),
            worktree: format!("/tmp/{actor}"),
            at: f::at(),
        }
    }
    fn assert_kept(s: &StateSnapshot, sides: &[&StateSnapshot]) {
        for side in sides {
            retained(side, s).unwrap();
        }
        validate(s).unwrap();
        let ranks: BTreeMap<_, _> = s
            .causal
            .order()
            .unwrap()
            .into_iter()
            .enumerate()
            .map(|(n, id)| (id, n))
            .collect();
        for (id, link) in &s.causal.links {
            for parent in &link.parents {
                assert!(ranks[parent] < ranks[id]);
            }
        }
    }
    #[test]
    fn selection_truth_table_is_symmetric_and_does_not_use_time_to_choose_different_values() {
        let s = fixture();
        let base = s.bundle(&s.declaration.entities[0]);
        for b in 0..3 {
            for o in 0..3 {
                for t in 0..3 {
                    let with = |n| {
                        let mut value = base.clone();
                        value.entity.title = format!("{n}");
                        value
                    };
                    let (b, o, t) = (with(b), with(o), with(t));
                    let expected = if o == t || b == t {
                        Some(o.clone())
                    } else if b == o {
                        Some(t.clone())
                    } else {
                        None
                    };
                    let select = |o: &Bundle, t: &Bundle| {
                        automatic(Some(&b), o, t).map(|l| if l { o.clone() } else { t.clone() })
                    };
                    assert_eq!(select(&o, &t), expected);
                    assert_eq!(select(&t, &o), expected);
                }
            }
        }
    }
    #[test]
    fn independent_changes_notes_and_repeat_integration() {
        let b = fixture();
        let o = change(&b, "a", Change::SetTitle("left".into()), 3);
        let t = change(&b, "b", Change::SetTitle("right".into()), 4);
        let c = complete(&prepare(&b, &o, &t), &[]);
        assert_eq!(c.state.declaration.entities[0].title, "left");
        assert_eq!(c.state.declaration.entities[1].title, "right");
        assert_kept(c.state(), &[&o, &t]);
        assert_eq!(c.bytes(), complete(&prepare(&b, &t, &o), &[]).bytes());
        assert_eq!(
            c.bytes(),
            complete(&prepare(&b, c.state(), &t), &[]).bytes()
        );
        let o = note(&b, "a", 5);
        let t = change(&b, "a", Change::Decide(Disposition::Accepted), 6);
        let c = complete(&prepare(&b, &o, &t), &[]);
        assert_kept(c.state(), &[&o, &t]);
        assert_eq!(c.state.histories[&f::id("a")].notes.len(), 1);
        let t = note(&b, "a", 7);
        let c = complete(&prepare(&b, &o, &t), &[]);
        assert_eq!(c.state.histories[&f::id("a")].notes.len(), 2);
        assert_eq!(c.bytes(), complete(&prepare(&b, &t, &o), &[]).bytes());
        assert_eq!(
            c.bytes(),
            complete(&prepare(&b, c.state(), &t), &[]).bytes()
        );
    }
    #[test]
    fn revision_branches_and_draft_acceptance_require_explicit_bundle_selection() {
        let b = fixture();
        let o = change(&b, "a", Change::Decide(Disposition::Accepted), 3);
        let t = change(&b, "a", Change::SetTitle("draft".into()), 4);
        let p = prepare(&b, &o, &t);
        assert_eq!(conflicts(&p).len(), 1);
        let c = complete(&p, &[pick(&p, "a", false)]);
        assert_kept(c.state(), &[&o, &t]);
        assert_eq!(
            c.state.declaration.entities[0].disposition,
            Disposition::Undecided
        );
        assert_eq!(c.state.histories[&f::id("a")].revisions.len(), 1);
        let t = change(&t, "a", Change::Decide(Disposition::Accepted), 5);
        let p = prepare(&b, &o, &t);
        let choice = pick(&p, "a", true);
        let c = complete(&p, std::slice::from_ref(&choice));
        assert_eq!(c.state.histories[&f::id("a")].revisions.len(), 2);
        assert_kept(c.state(), &[&o, &t]);
        assert_eq!(c.bytes(), complete(&p, &[choice]).bytes());
        let swapped = prepare(&b, &t, &o);
        assert_eq!(
            c.bytes(),
            complete(&swapped, &[pick(&swapped, "a", false)]).bytes()
        );
        assert_eq!(
            c.bytes(),
            complete(&prepare(&b, c.state(), &t), &[]).bytes()
        );
    }
    #[test]
    fn double_start_and_done_release_keep_both_histories() {
        let b = change(&fixture(), "a", Change::Decide(Disposition::Accepted), 3);
        let o = change(&b, "a", Change::Start(claim("left")), 4);
        let t = change(&b, "a", Change::Start(claim("right")), 5);
        let p = prepare(&b, &o, &t);
        assert_eq!(conflicts(&p).len(), 1);
        let c = complete(&p, &[pick(&p, "a", true)]);
        assert_kept(c.state(), &[&o, &t]);
        assert_eq!(c.state.histories[&f::id("a")].progress.len(), 2);
        let b = o;
        let o = change(&b, "a", Change::Done, 6);
        let t = change(&b, "a", Change::Release, 7);
        let p = prepare(&b, &o, &t);
        let c = complete(&p, &[pick(&p, "a", false)]);
        assert_eq!(
            c.state.declaration.entities[0].progress,
            Progress::NotStarted
        );
        assert_eq!(c.state.histories[&f::id("a")].progress.len(), 3);
        assert_kept(c.state(), &[&o, &t]);
        let m = c.state.causal.merges.values().next().unwrap();
        assert!(
            m.inputs
                .iter()
                .any(|i| i.candidate.entity.progress == Progress::Ended)
        );
    }
    #[test]
    fn cross_entity_cycles_are_conflicts_and_choices_can_override_automatic_selection() {
        let b = fixture();
        let dep = |source, target| Operation::Dependency {
            source: f::id(source),
            target: f::id(target),
            present: true,
        };
        let o = run(&b, dep("a", "b"), 3);
        let t = run(&b, dep("b", "a"), 4);
        let p = prepare(&b, &o, &t);
        assert!(conflicts(&p)[0].message.contains("cycle"));
        let c = complete(&p, &[pick(&p, "b", true)]);
        assert_eq!(c.state.declaration.dependencies.len(), 1);
        assert_kept(c.state(), &[&o, &t]);
        let o = change(
            &b,
            "a",
            Change::SetResurfaceCondition(ResurfaceCondition::AfterEntity(f::id("b"))),
            5,
        );
        let t = change(
            &b,
            "b",
            Change::SetResurfaceCondition(ResurfaceCondition::AfterEntity(f::id("a"))),
            6,
        );
        assert!(conflicts(&prepare(&b, &o, &t))[0].message.contains("cycle"));
    }
    #[test]
    fn ordinary_repairs_can_remove_conflicting_edges_without_bypassing_fixed_declarations() {
        let b = fixture();
        let dep = |source, target| Operation::Dependency {
            source: f::id(source),
            target: f::id(target),
            present: true,
        };
        let o = run(&b, dep("a", "b"), 3);
        let t = run(&b, dep("b", "a"), 4);
        let p = prepare(&b, &o, &t);
        let mut n = 0;
        let mut ids = |kind| {
            n += 1;
            RecordId::deterministic(kind, format!("repair-{n}").as_bytes())
        };
        let context = Context {
            at: f::at(),
            actor: "repair",
            reason: None,
            evaluation: b.declaration.evaluation.clone(),
            ids: &mut ids,
        };
        let operation = Operation::Dependency {
            source: f::id("b"),
            target: f::id("a"),
            present: false,
        };
        match p
            .resolve(&[], &mut [Repair { operation, context }])
            .unwrap()
        {
            Outcome::Complete(c) => {
                assert_eq!(c.state.declaration.dependencies.len(), 1);
                assert_kept(c.state(), &[&o, &t]);
            }
            Outcome::Conflicts(c) => panic!("{c:?}"),
        }
        let o = change(&b, "a", Change::Decide(Disposition::Accepted), 8);
        let p = prepare(&b, &o, &b);
        let context = Context {
            at: f::at(),
            actor: "repair",
            reason: None,
            evaluation: b.declaration.evaluation.clone(),
            ids: &mut ids,
        };
        assert!(
            p.resolve(
                &[],
                &mut [Repair {
                    operation: Operation::Change(f::id("a"), Change::SetTitle("illegal".into())),
                    context
                }]
            )
            .is_err()
        );
    }
    fn group_fixture() -> StateSnapshot {
        let s = run(
            &fixture(),
            Operation::Insert(f::entity("g", EntityKind::Group, None)),
            3,
        );
        let s = change(&s, "g", Change::Decide(Disposition::Accepted), 4);
        change(&s, "g", Change::Start(claim("group")), 5)
    }
    #[test]
    fn ended_group_subtree_structure_cannot_silently_change_even_when_every_child_is_terminal() {
        let b = group_fixture();
        let o = change(&b, "g", Change::Done, 6);
        let t = change(&b, "a", Change::SetParent(Some(f::id("g"))), 7);
        let t = change(&t, "a", Change::Decide(Disposition::Rejected), 8);
        let p = prepare(&b, &o, &t);
        assert!(conflicts(&p).iter().any(|c| c.message.contains("subtree")));
        let c = complete(&p, &[pick(&p, "a", true)]);
        assert_eq!(
            c.state
                .declaration
                .entities
                .iter()
                .find(|e| e.id == f::id("g"))
                .unwrap()
                .progress,
            Progress::Ended
        );
        let t = change(&t, "g", Change::Done, 9);
        let p = prepare(&b, &o, &t);
        assert!(conflicts(&p).iter().any(|c| c.owner == Some(f::id("g"))));
        let c = complete(&p, &[pick(&p, "g", false)]);
        assert_kept(c.state(), &[&o, &t]);
        assert!(
            c.state
                .declaration
                .entities
                .iter()
                .find(|e| e.id == f::id("a"))
                .unwrap()
                .parent
                .is_some()
        );
    }
    #[test]
    fn rejects_deletions_record_tampering_id_collisions_metadata_and_stale_choices() {
        let b = note(&fixture(), "a", 3);
        let mut t = b.clone();
        t.histories.get_mut(&f::id("a")).unwrap().notes[0].body = "tampered".into();
        assert!(
            Prepared::new(
                codec::encode(&b).unwrap(),
                codec::encode(&b).unwrap(),
                codec::encode(&t).unwrap(),
                b.declaration.evaluation.clone()
            )
            .is_err()
        );
        let t = fixture();
        assert!(
            Prepared::new(
                codec::encode(&b).unwrap(),
                codec::encode(&b).unwrap(),
                codec::encode(&t).unwrap(),
                b.declaration.evaluation.clone()
            )
            .is_err()
        );
        let mut t = b.clone();
        t.metadata
            .insert("prefix".into(), MetadataValue::Text("other".into()));
        assert!(
            Prepared::new(
                codec::encode(&b).unwrap(),
                codec::encode(&b).unwrap(),
                codec::encode(&t).unwrap(),
                b.declaration.evaluation.clone()
            )
            .is_err()
        );
        let p = prepare(&b, &b, &b);
        assert!(
            p.resolve(
                &[Choice {
                    conflict: "stale".into(),
                    input: p.ours.identity.clone()
                }],
                &mut []
            )
            .is_err()
        );
        let b = fixture();
        let o = run(
            &b,
            Operation::Insert(f::entity("new", EntityKind::Issue, None)),
            10,
        );
        let t = run(
            &b,
            Operation::Insert(f::entity("new", EntityKind::Issue, None)),
            11,
        );
        assert!(
            Prepared::new(
                codec::encode(&b).unwrap(),
                codec::encode(&o).unwrap(),
                codec::encode(&t).unwrap(),
                b.declaration.evaluation.clone()
            )
            .is_err()
        );
    }
    #[test]
    fn explicit_selection_of_sole_or_already_covered_candidates_is_a_noop() {
        let b = fixture();
        let o = run(
            &b,
            Operation::Insert(f::entity("new", EntityKind::Issue, None)),
            10,
        );
        let p = prepare(&b, &o, &b);
        let choices: Vec<_> = p
            .choices()
            .iter()
            .map(|c| Choice {
                conflict: c.id.clone(),
                input: p.ours.identity.clone(),
            })
            .collect();
        let c = complete(&p, &choices);
        assert_eq!(c.bytes(), complete(&p, &[]).bytes());
        assert!(c.state.causal.merges.is_empty());
        let o = change(&b, "a", Change::Decide(Disposition::Accepted), 11);
        let t = change(&b, "a", Change::Decide(Disposition::Rejected), 12);
        let p = prepare(&b, &o, &t);
        let c = complete(&p, &[pick(&p, "a", true)]);
        let again = prepare(&b, c.state(), &t);
        assert_eq!(
            c.bytes(),
            complete(&again, &[pick(&again, "a", true)]).bytes()
        );
        let switched = complete(&again, &[pick(&again, "a", false)]);
        assert_eq!(
            switched.state.declaration.entities[0].disposition,
            Disposition::Rejected
        );
        assert_kept(switched.state(), &[c.state(), &t]);
    }
    #[test]
    fn command_conditions_are_not_evaluated_by_merge() {
        let b = fixture();
        let o = change(
            &b,
            "a",
            Change::SetResurfaceCondition(ResurfaceCondition::Command("exit 42".into())),
            3,
        );
        let t = note(&b, "a", 4);
        let c = complete(&prepare(&b, &o, &t), &[]);
        assert_kept(c.state(), &[&o, &t]);
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    use crate::core::{Change, MetadataValue, tests as f};
    #[test]
    fn no_new_merge_record_for_a_contained_history_with_identical_current_value() {
        let mut base = f::empty();
        base.metadata.insert(
            "store_id".into(),
            MetadataValue::Text(RecordId::deterministic(RecordKind::Store, b"extra").to_string()),
        );
        base.metadata
            .insert("prefix".into(), MetadataValue::Text("t".into()));
        let base = f::execute(
            &base,
            Operation::Insert(f::entity("a", EntityKind::Issue, None)),
            1,
        )
        .unwrap()
        .state()
        .clone();
        let ours = f::execute(
            &base,
            Operation::Change(
                f::id("a"),
                Change::SetResurfaceCondition(ResurfaceCondition::Manual),
            ),
            2,
        )
        .unwrap()
        .state()
        .clone();
        let ours = f::execute(
            &ours,
            Operation::Change(
                f::id("a"),
                Change::SetResurfaceCondition(ResurfaceCondition::Always),
            ),
            3,
        )
        .unwrap()
        .state()
        .clone();
        let p = Prepared::new(
            codec::encode(&base).unwrap(),
            codec::encode(&ours).unwrap(),
            codec::encode(&base).unwrap(),
            base.declaration.evaluation.clone(),
        )
        .unwrap();
        match p.resolve(&[], &mut []).unwrap() {
            Outcome::Complete(c) => assert!(c.state.causal.merges.is_empty()),
            _ => panic!("unexpected conflict"),
        }
    }
    #[test]
    #[ignore = "release-mode synthetic performance measurement"]
    fn measure_merge() {
        for count in [100, 1000] {
            let mut base = f::empty();
            base.metadata.insert(
                "store_id".into(),
                MetadataValue::Text(
                    RecordId::deterministic(RecordKind::Store, b"perf").to_string(),
                ),
            );
            base.metadata
                .insert("prefix".into(), MetadataValue::Text("t".into()));
            let before = base.clone();
            for n in 0..count {
                base.declaration.entities.push(f::entity(
                    &format!("e{n:04}"),
                    EntityKind::Issue,
                    None,
                ));
            }
            let mut seq = 0;
            let mut ids = |kind| {
                seq += 1;
                RecordId::deterministic(kind, format!("perf-{seq}").as_bytes())
            };
            base.append_causality(
                &before,
                &mut Context {
                    at: f::at(),
                    actor: "perf",
                    reason: None,
                    evaluation: base.declaration.evaluation.clone(),
                    ids: &mut ids,
                },
            )
            .unwrap();
            let ours = f::execute(
                &base,
                Operation::Change(
                    f::id("e0000"),
                    Change::SetResurfaceCondition(ResurfaceCondition::Manual),
                ),
                10001,
            )
            .unwrap()
            .state()
            .clone();
            let theirs = f::execute(
                &base,
                Operation::Change(
                    f::id("e0001"),
                    Change::SetResurfaceCondition(ResurfaceCondition::Manual),
                ),
                10002,
            )
            .unwrap()
            .state()
            .clone();
            let (b, o, t) = (
                codec::encode(&base).unwrap(),
                codec::encode(&ours).unwrap(),
                codec::encode(&theirs).unwrap(),
            );
            let mut times = Vec::new();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                let p = Prepared::new(
                    b.clone(),
                    o.clone(),
                    t.clone(),
                    base.declaration.evaluation.clone(),
                )
                .unwrap();
                assert!(matches!(
                    p.resolve(&[], &mut []).unwrap(),
                    Outcome::Complete(_)
                ));
                times.push(start.elapsed());
            }
            times.sort();
            eprintln!(
                "merge entities={count} base_bytes={} median_ms={:.2}",
                b.len(),
                times[2].as_secs_f64() * 1000.0
            );
        }
    }
}
