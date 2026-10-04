//! Records across a kind conversion are judged by the kind at their time (the witnesses
//! `wConvertToGroupAfterStartAndRelease` and `wConvertToIssueAfterReopen` of
//! `spec/lifecycle_information.qnt`).
use super::*;
use Operation::*;

fn kinds_around(store: &Store, convert: &RecordId) -> (Kind, Kind) {
    let record = store.record(convert).unwrap();
    assert_eq!(record.kind, RecordKind::Convert);
    let parent = store.record(record.parents.first().unwrap()).unwrap();
    (parent.after.kind, record.after.kind)
}

#[test]
fn issue_started_and_released_then_converted_to_group_is_a_valid_history() {
    let mut r = Replica::new("r0");
    r.op("i3", Start);
    r.op("i3", Release);
    let convert = r
        .store
        .convert(&id("i3"), Kind::Group, None, r.tick())
        .unwrap()
        .unwrap();
    let convert = insert(&mut r.store, convert);
    let view = r.view();
    assert!(view.is_valid());
    let current = view.current(&id("i3")).unwrap();
    assert_eq!(
        (current.kind, current.lifecycle),
        (Kind::Group, Lifecycle::NotStarted)
    );
    assert_eq!(kinds_around(&r.store, &convert), (Kind::Issue, Kind::Group));
    let kinds: Vec<_> = r
        .store
        .history(&id("i3"))
        .unwrap()
        .iter()
        .map(|(_, record)| (record.kind.clone(), record.after.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (RecordKind::Created, Kind::Issue),
            (RecordKind::Transition(Start), Kind::Issue),
            (RecordKind::Transition(Release), Kind::Issue),
            (RecordKind::Convert, Kind::Group),
        ]
    );
    // The Group now takes children and completes from NotStarted.
    r.move_to("i1", Some("i3"));
    r.op("i1", Cancel);
    r.op("i3", Complete);
    assert!(r.view().is_valid());
}

#[test]
fn group_completed_and_reopened_then_converted_to_issue_is_a_valid_history() {
    let mut r = Replica::new("r0");
    r.op("g2", Complete);
    r.op("g2", Reopen);
    let convert = r
        .store
        .convert(&id("g2"), Kind::Issue, None, r.tick())
        .unwrap()
        .unwrap();
    let convert = insert(&mut r.store, convert);
    let view = r.view();
    assert!(view.is_valid());
    assert_eq!(view.current(&id("g2")).unwrap().kind, Kind::Issue);
    assert_eq!(kinds_around(&r.store, &convert), (Kind::Group, Kind::Issue));
    // The Issue now starts, which the Group could not.
    r.op("g2", Start);
    assert_eq!(lifecycle(&r.view(), "g2"), Lifecycle::InProgress);
}

#[test]
fn conversion_is_rejected_while_in_progress_terminal_or_with_children() {
    let mut r = Replica::new("r0");
    r.op("i3", Start);
    assert!(error(r.store.convert(&id("i3"), Kind::Group, None, r.tick())).contains("release"));
    r.op("i3", Complete);
    assert!(
        error(r.store.convert(&id("i3"), Kind::Group, None, r.tick()))
            .contains("reopen or reconsider")
    );
    let rejected = error(r.store.convert(&id("g0"), Kind::Issue, None, r.tick()));
    assert!(
        rejected.contains("children") && rejected.contains("i1"),
        "{rejected}"
    );
    r.op("g2", Cancel);
    assert!(
        error(r.store.convert(&id("g2"), Kind::Issue, None, r.tick()))
            .contains("reopen or reconsider")
    );
}

#[test]
fn encode_rejects_a_group_storing_in_progress() {
    let r = Replica::new("r0");
    let head = r.head("g2");
    let mut start = r.store.record(&head).unwrap().clone();
    start.kind = RecordKind::Transition(Start);
    start.parents = BTreeSet::from([head.clone()]);
    start.at = r.tick().at;
    start.after.lifecycle = Lifecycle::InProgress;
    assert_eq!(
        error(encode(&Entry::Record(start))),
        "a Group never stores InProgress"
    );
}

/// Judging every record by the Entity's current kind would reject both histories above; the
/// set instead rejects records that do not fit the kind at their own time.
#[test]
fn transitions_that_break_the_rules_of_their_own_kind_are_corruption() {
    let mut r = Replica::new("r0");
    // A Complete of an Issue whose parent record is NotStarted skips the Start.
    let head = r.head("i3");
    let mut complete = r.store.record(&head).unwrap().clone();
    complete.kind = RecordKind::Transition(Complete);
    complete.parents = BTreeSet::from([head]);
    complete.at = r.tick().at;
    complete.after.lifecycle = Lifecycle::Completed;
    insert(&mut r.store, complete);
    assert!(error(r.store.view()).contains("Complete"));
    // A conversion that keeps the kind is corruption too.
    let mut r = Replica::new("r0");
    let head = r.head("i3");
    let mut convert = r.store.record(&head).unwrap().clone();
    convert.kind = RecordKind::Convert;
    convert.parents = BTreeSet::from([head]);
    convert.at = r.tick().at;
    insert(&mut r.store, convert);
    assert!(error(r.store.view()).contains("keeps the kind"));
}

/// Every record kind may change only its own fields relative to its parent; a record that
/// changes anything else is corruption, so no record kind bypasses a rule of another.
#[test]
fn records_that_change_fields_outside_their_kind_are_corruption() {
    let r = Replica::new("r0");
    let head = r.head("i3");
    let base = r.store.record(&head).unwrap().clone();
    let with = |kind: RecordKind, change: &dyn Fn(&mut Current)| -> String {
        let mut record = base.clone();
        record.kind = kind;
        record.parents = BTreeSet::from([head.clone()]);
        record.at = r.tick().at;
        change(&mut record.after);
        let mut store = r.store.clone();
        match store.insert(Entry::Record(record)) {
            Ok(_) => error(store.view()),
            Err(rejected) => rejected.to_string(),
        }
    };
    // An edit completing an Issue, or taking ownership of it.
    assert!(
        with(RecordKind::Edit, &|c| c.lifecycle = Lifecycle::Completed)
            .contains("changes the lifecycle")
    );
    assert!(with(RecordKind::Edit, &|c| c.parent = Some(id("g0"))).contains("changes the parent"));
    // A transition renaming, moving or adding a dependency on the way.
    assert!(
        with(RecordKind::Transition(Start), &|c| {
            c.lifecycle = Lifecycle::InProgress;
            c.owner = Some("setup".into());
            c.title = "renamed".into();
        })
        .contains("changes the text")
    );
    assert!(
        with(RecordKind::Transition(Cancel), &|c| {
            c.lifecycle = Lifecycle::Cancelled;
            c.needs.insert(id("g2"));
        })
        .contains("changes the needs")
    );
    // A transition changing the kind, which is what conversion records are for.
    assert!(
        with(RecordKind::Transition(Cancel), &|c| {
            c.lifecycle = Lifecycle::Cancelled;
            c.kind = Kind::Group;
        })
        .contains("changes the kind")
    );
    // A conversion that also completes, or from a started Entity.
    assert!(
        with(RecordKind::Convert, &|c| {
            c.kind = Kind::Group;
            c.lifecycle = Lifecycle::Completed;
        })
        .contains("started or terminal")
    );
    assert!(
        with(RecordKind::Parent, &|c| c.condition = Some("exit 0".into()))
            .contains("changes the condition")
    );
    assert!(
        with(RecordKind::Dependency, &|c| c.parent = Some(id("g0"))).contains("changes the parent")
    );
    assert!(with(RecordKind::Condition, &|c| c.title = "x".into()).contains("changes the text"));
    assert!(
        with(RecordKind::Import, &|c| c.lifecycle = Lifecycle::Cancelled)
            .contains("changes the lifecycle")
    );
    // The label changes only through a label or import record.
    for kind in [
        RecordKind::Edit,
        RecordKind::Parent,
        RecordKind::Dependency,
        RecordKind::Condition,
    ] {
        assert!(
            with(kind.clone(), &|c| c.label = Label::Bug).contains("changes the label"),
            "{kind:?}"
        );
    }
    assert!(
        with(RecordKind::Transition(Cancel), &|c| {
            c.lifecycle = Lifecycle::Cancelled;
            c.label = Label::Bug;
        })
        .contains("changes the label")
    );
    assert!(
        with(RecordKind::Convert, &|c| {
            c.kind = Kind::Group;
            c.label = Label::Bug;
        })
        .contains("changes the label")
    );
    assert!(with(RecordKind::Label, &|c| c.title = "x".into()).contains("changes the text"));
    assert!(with(RecordKind::Label, &|c| c.parent = Some(id("g0"))).contains("changes the parent"));
    // The changes a kind may make derive when built by hand too.
    let r = Replica::new("r0");
    let head = r.head("i3");
    let mut edit = r.store.record(&head).unwrap().clone();
    edit.kind = RecordKind::Edit;
    edit.parents = BTreeSet::from([head.clone()]);
    edit.at = r.tick().at;
    edit.after.title = "renamed".into();
    edit.after.description = "changed".into();
    let mut store = r.store.clone();
    insert(&mut store, edit);
    assert_eq!(
        store.view().unwrap().current(&id("i3")).unwrap().title,
        "renamed"
    );
    let mut label = r.store.record(&head).unwrap().clone();
    label.kind = RecordKind::Label;
    label.parents = BTreeSet::from([head.clone()]);
    label.at = r.tick().at;
    label.after.label = Label::Spike;
    let mut store = r.store.clone();
    insert(&mut store, label);
    assert_eq!(
        store.view().unwrap().current(&id("i3")).unwrap().label,
        Label::Spike
    );
    let mut import = r.store.record(&head).unwrap().clone();
    import.kind = RecordKind::Import;
    import.parents = BTreeSet::from([head]);
    import.at = r.tick().at;
    import.after.parent = Some(id("g0"));
    import.after.needs.insert(id("g2"));
    import.after.label = Label::Refactor;
    let mut store = r.store.clone();
    insert(&mut store, import);
    assert!(store.view().unwrap().is_valid());
    // A conversion from a started Entity is corruption even with its parent present.
    let mut r = Replica::new("r0");
    r.op("i3", Start);
    let head = r.head("i3");
    let mut convert = r.store.record(&head).unwrap().clone();
    convert.kind = RecordKind::Convert;
    convert.parents = BTreeSet::from([head]);
    convert.at = r.tick().at;
    convert.after.kind = Kind::Group;
    convert.after.lifecycle = Lifecycle::NotStarted;
    convert.after.owner = None;
    insert(&mut r.store, convert);
    assert!(error(r.store.view()).contains("started or terminal"));
}

/// A resolve record takes the chosen head's value as it is: no field-wise merging.
#[test]
fn a_resolve_record_whose_value_differs_from_its_chosen_head_is_corruption() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let mine = r0.op_as("i1", Start, "a");
    r1.op_as("i1", Start, "b");
    r0.sync(&r1);
    let mut resolve = r0.store.resolve(&id("i1"), &mine, None, r0.tick()).unwrap();
    resolve.after.owner = Some("b".into());
    insert(&mut r0.store, resolve);
    assert!(error(r0.store.view()).contains("chosen head"));
}
