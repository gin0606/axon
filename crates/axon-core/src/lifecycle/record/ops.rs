//! Ordinary operations: check the prerequisites against the derived view and produce exactly
//! one new record, which the caller publishes. Nothing here changes the store.
use super::model::{Current, Entry, Note, Record, RecordId, RecordKind};
use super::store::Store;
use super::view::View;
use super::{Context, EntityId, Kind, Lifecycle, Nonce, Operation, Result};
use crate::lifecycle::{invalid, validate_reason};

impl Store {
    fn settled_view(&self) -> Result<View> {
        let view = self.view()?;
        if !view.conflicted().is_empty() {
            let ids: Vec<_> = view.conflicted().iter().map(ToString::to_string).collect();
            return Err(invalid(format!(
                "conflicted Entities block every operation except resolve and note add; list their heads with axon resolve: {}",
                ids.join(", ")
            )));
        }
        Ok(view)
    }

    /// Registers an Entity as Undecided or NotStarted with its initial value. The parent must
    /// be a settled, unfinished Group; dependencies must be settled and outside the
    /// containment line. The result may not add a violation nor put a new completion-path
    /// edge on a cycle.
    pub fn create(&self, id: EntityId, current: Current, context: Context) -> Result<Record> {
        let view = self.settled_view()?;
        create_in(self, &view, id, current, context)
    }
    /// One lifecycle transition. Performing `Complete` on a Group is the caller's explicit
    /// final confirmation of that plan. The result may not add a violation: a waiver relaxes
    /// a prerequisite of the Entity in violation, never the whole check.
    pub fn perform(
        &self,
        id: &EntityId,
        operation: Operation,
        reason: Option<String>,
        context: Context,
    ) -> Result<Record> {
        let view = self.settled_view()?;
        perform_in(self, &view, id, operation, reason, context)
    }
    /// Edits title and description of an unfinished Entity. None when nothing changes; a
    /// terminal Entity is rejected before that, even for its own values.
    pub fn write(
        &self,
        id: &EntityId,
        title: Option<String>,
        description: Option<String>,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        write_in(&view, id, title, description, context)
    }
    /// Sets or clears the condition command without evaluating it. None when unchanged.
    pub fn set_condition(
        &self,
        id: &EntityId,
        command: Option<String>,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        let current = view.require_settled(id)?;
        if current.condition == command {
            return Ok(None);
        }
        let after = Current {
            condition: command,
            ..current.clone()
        };
        after.validate()?;
        Ok(Some(follow(
            &view,
            id,
            RecordKind::Condition,
            after,
            None,
            context,
        )))
    }
    /// Moves the Entity under another Group or out of any. None when unchanged. The result may
    /// not add a violation nor put a new completion-path edge on a cycle.
    pub fn set_parent(
        &self,
        id: &EntityId,
        parent: Option<EntityId>,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        set_parent_in(self, &view, id, parent, context)
    }
    /// Adds a dependency. None when present. The result may not add a violation nor put the
    /// new edge on a completion cycle, even between Entities already on one.
    pub fn add_dependency(
        &self,
        id: &EntityId,
        target: &EntityId,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        add_dependency_in(self, &view, id, target, context)
    }
    pub fn remove_dependency(
        &self,
        id: &EntityId,
        target: &EntityId,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        remove_dependency_in(&view, id, target, context)
    }
    /// Converts between Issue and Group. None when the Entity already has that kind. The
    /// result may not add a violation.
    pub fn convert(&self, id: &EntityId, kind: Kind, context: Context) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        let current = view.require_settled(id)?;
        if current.kind == kind {
            return Ok(None);
        }
        match current.lifecycle {
            Lifecycle::Undecided | Lifecycle::NotStarted => {}
            Lifecycle::InProgress => {
                return Err(invalid("release the InProgress Issue before converting it"));
            }
            Lifecycle::Completed | Lifecycle::Cancelled => {
                return Err(invalid(
                    "a Completed or Cancelled Entity is not converted; reopen or reconsider it first",
                ));
            }
        }
        if kind == Kind::Issue && !view.children(id).is_empty() {
            let children: Vec<_> = view.children(id).iter().map(ToString::to_string).collect();
            return Err(invalid(format!(
                "a Group with children is not converted to an Issue: {}",
                children.join(", ")
            )));
        }
        let after = Current {
            kind,
            ..current.clone()
        };
        let record = follow(&view, id, RecordKind::Convert, after, None, context);
        without_new_violations(self, &view, record).map(Some)
    }
    /// The final value `axon import apply` gives an existing Entity, validated as the sequence
    /// text edit, dependency removals, move, dependency additions, and recorded once. None
    /// when nothing changes. Conflicts block it like every ordinary operation; the rule that
    /// a whole declaration is also rejected while the store has violations, except on a retry
    /// whose remaining records remove them, needs the whole declaration and is the caller's.
    pub fn import(
        &self,
        id: &EntityId,
        title: String,
        description: String,
        parent: Option<EntityId>,
        needs: std::collections::BTreeSet<EntityId>,
        context: Context,
    ) -> Result<Option<Record>> {
        let view = self.settled_view()?;
        let head = view.settled_entity(id).map(|e| e.head.clone());
        let before = view.require_settled(id)?.clone();
        let mut scratch = self.clone();
        let apply = |scratch: &mut Store, record: Option<Record>| -> Result<()> {
            if let Some(record) = record {
                scratch.insert(Entry::Record(record))?;
            }
            Ok(())
        };
        let title = (before.title != title).then_some(title);
        let description = (before.description != description).then_some(description);
        if title.is_some() || description.is_some() {
            let record = write_in(&view, id, title, description, context.clone())?;
            apply(&mut scratch, record)?;
        }
        for target in before.needs.difference(&needs) {
            let view = scratch.view()?;
            let record = remove_dependency_in(&view, id, target, context.clone())?;
            apply(&mut scratch, record)?;
        }
        let record = set_parent_in(&scratch, &scratch.view()?, id, parent, context.clone())?;
        apply(&mut scratch, record)?;
        for target in needs.difference(&before.needs) {
            let view = scratch.view()?;
            let record = add_dependency_in(&scratch, &view, id, target, context.clone())?;
            apply(&mut scratch, record)?;
        }
        let after = scratch.view()?.require_settled(id)?.clone();
        if after == before {
            return Ok(None);
        }
        Ok(Some(Record {
            entity: id.clone(),
            kind: RecordKind::Import,
            parents: head.into_iter().collect(),
            at: context.at,
            recorder: context.recorder,
            reason: None,
            after,
        }))
    }
    /// Resolves a conflicted Entity by taking the value of one of its heads. The record joins
    /// every head. Violations are not checked here: the head count returns to one first and
    /// ordinary operations repair the rest.
    pub fn resolve(
        &self,
        id: &EntityId,
        chosen: &RecordId,
        reason: Option<String>,
        context: Context,
    ) -> Result<Record> {
        validate_reason(&reason)?;
        let view = self.view()?;
        let heads = view
            .heads(id)
            .ok_or_else(|| invalid(format!("missing Entity {id}")))?;
        if !view.is_conflicted(id) {
            return Err(invalid(format!("Entity {id} is not conflicted")));
        }
        if !heads.contains(chosen) {
            return Err(invalid(format!("{chosen} is not a head of {id}")));
        }
        let after = self.record(chosen).expect("head is a record").after.clone();
        Ok(Record {
            entity: id.clone(),
            kind: RecordKind::Resolve {
                chosen: chosen.clone(),
            },
            parents: heads.clone(),
            at: context.at,
            recorder: context.recorder,
            reason,
            after,
        })
    }
    /// A Note for a known Entity, allowed while conflicts exist.
    pub fn add_note(
        &self,
        id: &EntityId,
        body: String,
        reason: Option<String>,
        context: Context,
    ) -> Result<Note> {
        if !self.records().any(|(_, record)| &record.entity == id) {
            return Err(invalid(format!("missing Entity {id}")));
        }
        let note = Note {
            entity: id.clone(),
            nonce: Nonce::generate(),
            at: context.at,
            recorder: context.recorder,
            reason,
            body,
        };
        note.validate()?;
        Ok(note)
    }
}

/// A record that continues the single head of a settled Entity.
fn follow(
    view: &View,
    id: &EntityId,
    kind: RecordKind,
    after: Current,
    reason: Option<String>,
    context: Context,
) -> Record {
    Record {
        entity: id.clone(),
        kind,
        parents: [view.head(id).expect("settled Entity").clone()].into(),
        at: context.at,
        recorder: context.recorder,
        reason,
        after,
    }
}

/// Rejects a record whose result adds a violation the store did not have.
fn without_new_violations(store: &Store, view: &View, record: Record) -> Result<Record> {
    let after = view_after(store, &record)?;
    reject_new_violations(view, &after)?;
    Ok(record)
}

/// Rejects a move, dependency or registration whose result adds a violation or puts an
/// edge its new relations induce on a completion cycle. In a valid store any cycle through
/// a new edge adds members, and the violation check has rejected it already.
fn without_new_violations_or_cycle(store: &Store, view: &View, record: Record) -> Result<Record> {
    let after = view_after(store, &record)?;
    reject_new_violations(view, &after)?;
    if !view.violations().is_empty()
        && let Some((dependent, predecessor)) = after.relation_on_cycle(view, &record.entity)
    {
        return Err(invalid(format!(
            "the change would put {dependent} waiting on {predecessor} on a completion cycle"
        )));
    }
    Ok(record)
}

fn view_after(store: &Store, record: &Record) -> Result<View> {
    store.view_with(&Entry::Record(record.clone()))
}

fn reject_new_violations(view: &View, after: &View) -> Result<()> {
    let added: Vec<_> = after
        .violations()
        .difference(view.violations())
        .map(|v| format!("{} ({})", v.entity, v.kind.label()))
        .collect();
    if !added.is_empty() {
        return Err(invalid(format!(
            "the change would add a structural violation: {}",
            added.join(", ")
        )));
    }
    Ok(())
}

fn require_open_group(view: &View, parent: Option<&EntityId>) -> Result<()> {
    if let Some(parent) = parent {
        let open = view
            .current(parent)
            .is_some_and(|p| p.kind == Kind::Group && !p.is_terminal());
        if !open {
            return Err(invalid("parent must be an unfinished Group"));
        }
    }
    Ok(())
}

fn require_dependency_target(view: &View, id: &EntityId, target: &EntityId) -> Result<()> {
    if id == target {
        return Err(invalid("an Entity does not depend on itself"));
    }
    view.require_settled(target)?;
    if view.ancestors(id).contains(target) {
        return Err(invalid(format!("{target} is an ancestor of {id}")));
    }
    if view.ancestors(target).contains(id) {
        return Err(invalid(format!("{target} is a descendant of {id}")));
    }
    Ok(())
}

fn create_in(
    store: &Store,
    view: &View,
    id: EntityId,
    current: Current,
    context: Context,
) -> Result<Record> {
    current.validate()?;
    if !matches!(
        current.lifecycle,
        Lifecycle::Undecided | Lifecycle::NotStarted
    ) {
        return Err(invalid("creation requires Undecided or NotStarted"));
    }
    if view.is_known(&id) {
        return Err(invalid(format!("duplicate Entity {id}")));
    }
    require_open_group(view, current.parent.as_ref())?;
    let line: Vec<EntityId> = current
        .parent
        .iter()
        .cloned()
        .chain(current.parent.iter().flat_map(|p| view.ancestors(p)))
        .collect();
    for target in &current.needs {
        if target == &id {
            return Err(invalid("an Entity does not depend on itself"));
        }
        view.require_settled(target)?;
        if line.contains(target) {
            return Err(invalid(format!("{target} is an ancestor of {id}")));
        }
    }
    let record = Record {
        entity: id,
        kind: RecordKind::Created,
        parents: Default::default(),
        at: context.at,
        recorder: context.recorder,
        reason: None,
        after: current,
    };
    without_new_violations_or_cycle(store, view, record)
}

fn perform_in(
    store: &Store,
    view: &View,
    id: &EntityId,
    operation: Operation,
    reason: Option<String>,
    context: Context,
) -> Result<Record> {
    validate_reason(&reason)?;
    view.check_operation(id, operation)?;
    let current = view.require_settled(id)?;
    let lifecycle = operation.apply_as(current.kind, current.lifecycle)?;
    let owner = if operation == Operation::Start {
        context.recorder.as_ref().map(|r| r.actor.clone())
    } else {
        None
    };
    let after = Current {
        lifecycle,
        owner,
        ..current.clone()
    };
    after.validate()?;
    let record = follow(
        view,
        id,
        RecordKind::Transition(operation),
        after,
        reason,
        context,
    );
    without_new_violations(store, view, record)
}

fn write_in(
    view: &View,
    id: &EntityId,
    title: Option<String>,
    description: Option<String>,
    context: Context,
) -> Result<Option<Record>> {
    let current = view.require_settled(id)?;
    if current.is_terminal() {
        return Err(invalid("terminal text is fixed"));
    }
    let mut after = current.clone();
    if let Some(title) = title {
        after.title = title;
    }
    if let Some(description) = description {
        after.description = description;
    }
    if after == *current {
        return Ok(None);
    }
    after.validate()?;
    Ok(Some(follow(
        view,
        id,
        RecordKind::Edit,
        after,
        None,
        context,
    )))
}

fn set_parent_in(
    store: &Store,
    view: &View,
    id: &EntityId,
    parent: Option<EntityId>,
    context: Context,
) -> Result<Option<Record>> {
    let current = view.require_settled(id)?;
    if current.parent == parent {
        return Ok(None);
    }
    if !view.parent_open(id) && !view.in_violation(id) {
        return Err(invalid("parent must be an unfinished Group"));
    }
    if let Some(destination) = &parent {
        if destination == id {
            return Err(invalid("a Group does not contain itself"));
        }
        require_open_group(view, Some(destination))?;
        if view.ancestors(destination).contains(id) {
            return Err(invalid("containment cycle"));
        }
        if matches!(
            view.effective_lifecycle(id),
            Some(Lifecycle::InProgress | Lifecycle::Completed)
        ) && (view.current(destination).expect("open Group").lifecycle != Lifecycle::NotStarted
            || !view.ancestors_adopted(destination))
        {
            return Err(invalid(
                "InProgress or Completed work moves only under a Group that is adopted (NotStarted) along with all of its ancestors",
            ));
        }
    }
    let after = Current {
        parent,
        ..current.clone()
    };
    let record = follow(view, id, RecordKind::Parent, after, None, context);
    without_new_violations_or_cycle(store, view, record).map(Some)
}

fn add_dependency_in(
    store: &Store,
    view: &View,
    id: &EntityId,
    target: &EntityId,
    context: Context,
) -> Result<Option<Record>> {
    let current = view.require_settled(id)?;
    if current.needs.contains(target) {
        return Ok(None);
    }
    if current.lifecycle == Lifecycle::Completed {
        return Err(invalid("Completed dependencies are fixed"));
    }
    require_dependency_target(view, id, target)?;
    let mut after = current.clone();
    after.needs.insert(target.clone());
    let record = follow(view, id, RecordKind::Dependency, after, None, context);
    without_new_violations_or_cycle(store, view, record).map(Some)
}

fn remove_dependency_in(
    view: &View,
    id: &EntityId,
    target: &EntityId,
    context: Context,
) -> Result<Option<Record>> {
    let current = view.require_settled(id)?;
    if !current.needs.contains(target) {
        return Ok(None);
    }
    if current.lifecycle == Lifecycle::Completed && !view.in_violation(id) {
        return Err(invalid("Completed dependencies are fixed"));
    }
    let mut after = current.clone();
    after.needs.remove(target);
    Ok(Some(follow(
        view,
        id,
        RecordKind::Dependency,
        after,
        None,
        context,
    )))
}
