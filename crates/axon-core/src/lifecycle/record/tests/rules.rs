//! Rules of ordinary operations on a single store: lifecycle transitions, derived Group work,
//! containment, dependencies, text, conditions, Notes and recorders. Operations only return
//! a record, so a rejection leaves nothing to publish and the store as it was.
use super::*;
use Operation::*;

fn empty() -> Replica {
    Replica {
        store: Store::new(),
        clock: Cell::new(100),
        name: "r0".into(),
    }
}
fn register(r: &mut Replica, name: &str, kind: Kind, parent: Option<&str>) -> RecordId {
    r.create(name, kind, Lifecycle::NotStarted, parent)
}
fn effective(r: &Replica, name: &str) -> Lifecycle {
    r.view().effective_lifecycle(&id(name)).unwrap()
}
/// The Entity's records other than Notes in causal order, by kind.
fn history_kinds(r: &Replica, name: &str) -> Vec<RecordKind> {
    r.store
        .history(&id(name))
        .unwrap()
        .into_iter()
        .map(|(_, record)| record.kind.clone())
        .collect()
}
fn transitions(r: &Replica, name: &str) -> Vec<RecordId> {
    r.store
        .history(&id(name))
        .unwrap()
        .into_iter()
        .filter(|(_, record)| matches!(record.kind, RecordKind::Transition(_)))
        .map(|(record_id, _)| record_id.clone())
        .collect()
}

#[test]
fn operation_and_history_are_atomic_and_other_entities_unchanged() {
    let mut r = empty();
    let mut value = current(Kind::Issue, Lifecycle::Undecided, None);
    value.condition = Some("exit 1".into());
    let created = r.try_create("item", value).unwrap();
    insert(&mut r.store, created);
    register(&mut r, "other", Kind::Group, None);
    let other = r.view().settled_entity(&id("other")).unwrap().clone();
    for (operation, expected) in [
        (Accept, Lifecycle::NotStarted),
        (Start, Lifecycle::InProgress),
        (Release, Lifecycle::NotStarted),
        (Withdraw, Lifecycle::Undecided),
        (Cancel, Lifecycle::Cancelled),
        (Reconsider, Lifecycle::Undecided),
        (Accept, Lifecycle::NotStarted),
        (Start, Lifecycle::InProgress),
        (Complete, Lifecycle::Completed),
        (Reopen, Lifecycle::NotStarted),
        (Start, Lifecycle::InProgress),
        (Complete, Lifecycle::Completed),
    ] {
        let before = r.view().settled_entity(&id("item")).unwrap().clone();
        // Every record carries the same time, earlier than the creation.
        let record = r
            .store
            .perform(
                &id("item"),
                operation,
                Some("reason 理由".into()),
                ctx(-10, "r0"),
            )
            .unwrap();
        let recorded = insert(&mut r.store, record);
        let after = r.current("item");
        assert_eq!(after.lifecycle, expected);
        // Only the lifecycle and the owner of a start change.
        assert_eq!(
            after,
            Current {
                lifecycle: after.lifecycle,
                owner: after.owner.clone(),
                ..before.current.clone()
            }
        );
        let record = r.store.record(&recorded).unwrap();
        assert_eq!(record.parents, BTreeSet::from([before.head]));
        assert_eq!(record.kind, RecordKind::Transition(operation));
        assert_eq!(record.reason.as_deref(), Some("reason 理由"));
        assert_eq!(r.head("item"), recorded);
        assert_eq!(r.view().settled_entity(&id("other")), Some(&other));
    }
    for operation in [
        Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider,
    ] {
        assert!(r.try_op("item", operation).is_err(), "{operation:?}");
    }
}

#[test]
fn text_edit_contract_and_notes_hold_for_issues_and_groups() {
    let issue = [
        None,
        Some(Accept),
        Some(Start),
        Some(Cancel),
        Some(Reconsider),
        Some(Accept),
        Some(Start),
        Some(Complete),
        Some(Reopen),
        Some(Start),
        Some(Complete),
    ];
    let group = [
        None,
        Some(Accept),
        Some(Cancel),
        Some(Reconsider),
        Some(Accept),
        Some(Complete),
        Some(Reopen),
        Some(Complete),
    ];
    for (kind, operations) in [(Kind::Issue, &issue[..]), (Kind::Group, &group[..])] {
        let mut r = empty();
        r.create("item", kind, Lifecycle::Undecided, None);
        for (step, operation) in operations.iter().copied().enumerate() {
            if let Some(operation) = operation {
                r.op("item", operation);
            }
            let before = r.current("item");
            let notes = r.store.notes_of(&id("item")).len();
            let editable = before.lifecycle.editable();
            // A different text each step, so every step asks for a change.
            let result = r.store.write(
                &id("item"),
                Some(format!("changed {step}")),
                Some(format!("new description {step}")),
                r.tick(),
            );
            assert_eq!(result.is_ok(), editable, "{kind:?} {:?}", before.lifecycle);
            if let Ok(record) = result {
                let record = record.expect("a change");
                assert_eq!(record.kind, RecordKind::Edit);
                insert(&mut r.store, record);
                let after = r.current("item");
                assert_eq!(after.title, format!("changed {step}"));
                assert_eq!(after.lifecycle, before.lifecycle);
                assert_eq!(after.owner, before.owner);
            }
            assert_eq!(r.store.notes_of(&id("item")).len(), notes);
            // A Note is allowed in every lifecycle and leaves the Entity as it was.
            let entity = r.view().settled_entity(&id("item")).unwrap().clone();
            r.note("item", "supplement");
            assert_eq!(r.view().settled_entity(&id("item")), Some(&entity));
            assert_eq!(r.store.notes_of(&id("item")).len(), notes + 1);
        }
    }
}

#[test]
fn invalid_operations_and_empty_information_are_rejected() {
    let mut r = empty();
    r.create("item", Kind::Issue, Lifecycle::Undecided, None);
    assert!(
        r.store
            .write(
                &id("item"),
                Some(" \n".into()),
                Some("must not persist".into()),
                r.tick()
            )
            .is_err()
    );
    assert!(
        r.store
            .add_note(&id("item"), "\n\t".into(), None, r.tick())
            .is_err()
    );
    assert!(r.try_op("item", Complete).is_err());
    assert!(
        r.store
            .add_note(&id("absent"), "note".into(), None, r.tick())
            .is_err()
    );
    assert!(
        r.try_create("item", current(Kind::Group, Lifecycle::NotStarted, None))
            .is_err()
    );
    assert!(
        r.try_create("new", current(Kind::Issue, Lifecycle::Completed, None))
            .is_err()
    );
}

#[test]
fn creation_has_an_origin_not_a_fabricated_transition() {
    for lifecycle in [Lifecycle::Undecided, Lifecycle::NotStarted] {
        let mut r = empty();
        let created = r.create("item", Kind::Issue, lifecycle, None);
        let history = r.store.history(&id("item")).unwrap();
        assert_eq!(history.len(), 1);
        let (record_id, record) = history[0];
        assert_eq!(record_id, &created);
        assert_eq!(record.kind, RecordKind::Created);
        assert!(record.parents.is_empty());
        assert_eq!(record.after.kind, Kind::Issue);
        assert_eq!(record.after.lifecycle, lifecycle);
    }
}

#[test]
fn equal_or_reverse_times_do_not_determine_causality() {
    let mut r = empty();
    let root = register(&mut r, "item", Kind::Issue, None);
    // Both records carry the same time, earlier than the creation.
    let at_zero = |r: &mut Replica, operation| {
        let record = r
            .store
            .perform(&id("item"), operation, None, ctx(0, "r0"))
            .unwrap();
        insert(&mut r.store, record)
    };
    let start = at_zero(&mut r, Start);
    let done = at_zero(&mut r, Complete);
    assert!(r.store.precedes(&root, &done));
    assert!(r.store.precedes(&start, &done));
    assert!(!r.store.precedes(&done, &start));
    assert_eq!(
        r.store
            .history(&id("item"))
            .unwrap()
            .into_iter()
            .map(|(record_id, _)| record_id)
            .collect::<Vec<_>>(),
        vec![&root, &start, &done]
    );
}

#[test]
fn absent_recorder_and_empty_data_are_retained_without_permissions() {
    let mut r = empty();
    register(&mut r, "item", Kind::Issue, None);
    let mut context = ctx(0, "r0");
    context.recorder = None;
    let start = r
        .store
        .perform(&id("item"), Start, None, context.clone())
        .unwrap();
    let start = insert(&mut r.store, start);
    let note = r
        .store
        .add_note(&id("item"), "note".into(), None, context)
        .unwrap();
    let note = r.store.insert(Entry::Note(note)).unwrap();
    assert_eq!(r.store.record(&start).unwrap().recorder, None);
    assert_eq!(r.current("item").owner, None);
    assert_eq!(r.store.note(&note).unwrap().recorder, None);
    // Another actor with empty data releases the work: recorders grant no permission.
    let mut context = ctx(-1, "r0");
    let other = Recorder {
        actor: "another actor".into(),
        data: BTreeMap::new(),
    };
    context.recorder = Some(other.clone());
    let release = r
        .store
        .perform(&id("item"), Release, None, context)
        .unwrap();
    let release = insert(&mut r.store, release);
    assert_eq!(r.store.record(&release).unwrap().recorder, Some(other));
}

#[test]
fn nested_group_work_is_derived_from_issues_and_needs_final_confirmation() {
    let mut r = empty();
    register(&mut r, "root", Kind::Group, None);
    register(&mut r, "nested", Kind::Group, Some("root"));
    register(&mut r, "child", Kind::Issue, Some("nested"));
    for name in ["root", "nested"] {
        for operation in [Start, Release] {
            assert!(r.try_op(name, operation).is_err(), "{name} {operation:?}");
        }
    }
    r.op("child", Withdraw);
    for operation in [Complete, Cancel] {
        assert!(error(r.try_op("nested", operation)).contains("children must be terminal"));
    }
    r.op("child", Accept);
    assert_eq!(effective(&r, "root"), Lifecycle::NotStarted);
    r.op("child", Start);
    // Three levels: the grandchild's start makes the grandparent InProgress through a
    // NotStarted child Group, without changing either Group's stored lifecycle.
    for name in ["root", "nested"] {
        assert_eq!(effective(&r, name), Lifecycle::InProgress);
        assert_eq!(r.current(name).lifecycle, Lifecycle::NotStarted);
        assert_eq!(history_kinds(&r, name), vec![RecordKind::Created]);
        assert!(error(r.try_op(name, Withdraw)).contains("cannot be withdrawn"));
    }
    r.op("child", Cancel);
    assert_eq!(effective(&r, "nested"), Lifecycle::NotStarted);
    assert!(r.view().check_operation(&id("nested"), Complete).is_ok());
    assert!(error(r.try_op("root", Complete)).contains("children must be terminal"));
    r.op("nested", Complete);
    assert_eq!(effective(&r, "root"), Lifecycle::InProgress);
    r.op("root", Complete);
    // Everything below a Completed root is frozen.
    assert!(error(r.try_op("child", Reconsider)).contains("unfinished Group"));
    assert!(error(r.try_op("nested", Reopen)).contains("unfinished Group"));
    assert!(error(r.try_move("nested", None)).contains("unfinished Group"));
    // Reopening the root allows composition edits again and derives InProgress again.
    r.op("root", Reopen);
    assert_eq!(effective(&r, "root"), Lifecycle::InProgress);
    r.move_to("nested", None);
}

#[test]
fn empty_and_all_cancelled_groups_complete_from_not_started() {
    let mut r = empty();
    register(&mut r, "empty", Kind::Group, None);
    register(&mut r, "cancelled", Kind::Group, None);
    register(&mut r, "a", Kind::Issue, Some("cancelled"));
    register(&mut r, "b", Kind::Issue, Some("cancelled"));
    r.op("a", Cancel);
    assert!(error(r.try_op("cancelled", Complete)).contains("children must be terminal"));
    r.op("b", Cancel);
    for name in ["empty", "cancelled"] {
        assert_eq!(effective(&r, name), Lifecycle::NotStarted);
        let before = r.head(name);
        let complete = r.op(name, Complete);
        let record = r.store.record(&complete).unwrap();
        assert_eq!(record.kind, RecordKind::Transition(Complete));
        assert_eq!(record.parents, BTreeSet::from([before.clone()]));
        assert_eq!(
            r.store.record(&before).unwrap().after.lifecycle,
            Lifecycle::NotStarted
        );
        assert_eq!(record.after.lifecycle, Lifecycle::Completed);
        assert_eq!(record.reason, None);
    }
}

#[test]
fn reopen_returns_to_not_started_and_waits_for_completed_dependents() {
    let mut r = empty();
    register(&mut r, "base", Kind::Issue, None);
    register(&mut r, "user", Kind::Issue, None);
    r.add_dep("user", "base");
    r.op("base", Start);
    r.op("base", Complete);
    r.op("user", Start);
    r.op("user", Complete);
    let message = error(r.view().check_operation(&id("base"), Reopen));
    assert!(message.contains("user"), "{message}");
    assert!(r.try_op("base", Reopen).is_err());
    r.op("user", Reopen);
    r.op("base", Reopen);
    assert_eq!(r.current("base").lifecycle, Lifecycle::NotStarted);
    // Reopened work is editable, keeps its dependencies and starts again.
    let edit = r
        .store
        .write(&id("base"), Some("revised".into()), None, r.tick())
        .unwrap()
        .expect("a change");
    insert(&mut r.store, edit);
    assert!(r.current("user").needs.contains(&id("base")));
    r.remove_dep("user", "base");
    assert!(r.try_op("base", Complete).is_err());
    r.op("base", Start);
    r.op("base", Complete);
    // A Group with a Completed child reopens into effective InProgress.
    register(&mut r, "plan", Kind::Group, None);
    register(&mut r, "done", Kind::Issue, Some("plan"));
    r.op("done", Start);
    r.op("done", Complete);
    r.op("plan", Complete);
    r.op("plan", Reopen);
    assert_eq!(r.current("done").lifecycle, Lifecycle::Completed);
    assert_eq!(effective(&r, "plan"), Lifecycle::InProgress);
    // A reopened child keeps its parent from being withdrawn.
    register(&mut r, "outer", Kind::Group, None);
    r.move_to("plan", Some("outer"));
    r.op("plan", Complete);
    r.op("outer", Complete);
    assert!(error(r.try_op("plan", Reopen)).contains("unfinished Group"));
    r.op("outer", Reopen);
    r.op("plan", Reopen);
    assert!(error(r.try_op("outer", Withdraw)).contains("cannot be withdrawn"));
}

#[test]
fn ancestor_dependencies_gate_the_start_of_descendant_issues() {
    let mut r = empty();
    register(&mut r, "gate", Kind::Issue, None);
    register(&mut r, "outer", Kind::Group, None);
    register(&mut r, "inner", Kind::Group, Some("outer"));
    register(&mut r, "leaf", Kind::Issue, Some("inner"));
    r.add_dep("outer", "gate");
    let message = error(r.view().check_operation(&id("leaf"), Start));
    assert!(message.contains("outer"), "{message}");
    assert!(r.try_op("leaf", Start).is_err());
    r.op("gate", Start);
    r.op("gate", Complete);
    r.op("leaf", Start);
    // An Undecided ancestor also blocks the start, even with a NotStarted parent.
    r.op("leaf", Release);
    r.op("outer", Withdraw);
    assert!(error(r.try_op("leaf", Start)).contains("adopted"));
    r.op("outer", Accept);
    r.op("leaf", Start);
}

#[test]
fn working_groups_move_only_under_adopted_groups_and_accept_needs_adopted_ancestors() {
    let mut r = empty();
    register(&mut r, "plan", Kind::Group, None);
    register(&mut r, "work", Kind::Issue, Some("plan"));
    register(&mut r, "adopted", Kind::Group, None);
    register(&mut r, "pending", Kind::Group, None);
    r.op("pending", Withdraw);
    register(&mut r, "under-pending", Kind::Group, Some("pending"));
    r.op("work", Start);
    for (name, destination) in [
        ("plan", "pending"),
        ("plan", "under-pending"),
        ("work", "pending"),
    ] {
        assert!(
            error(r.try_move(name, Some(destination))).contains("adopted"),
            "{name} under {destination}"
        );
    }
    r.move_to("plan", Some("adopted"));
    assert_eq!(effective(&r, "adopted"), Lifecycle::InProgress);
    r.move_to("plan", None);
    // A Group with work below it is accepted only under adopted ancestors.
    r.op("work", Complete);
    r.op("plan", Cancel);
    r.op("plan", Reconsider);
    r.move_to("plan", Some("pending"));
    assert!(error(r.try_op("plan", Accept)).contains("adopted ancestors"));
    r.op("pending", Accept);
    r.op("plan", Accept);
    assert_eq!(effective(&r, "pending"), Lifecycle::InProgress);
}

#[test]
fn dependency_added_during_work_blocks_completion_without_releasing() {
    let mut r = empty();
    register(&mut r, "work", Kind::Issue, None);
    register(&mut r, "needs", Kind::Group, None);
    let start = r.op("work", Start);
    let started = r.current("work");
    let dependency = r.add_dep("work", "needs");
    // The dependency is its own record after the start; the work stays started as it was.
    let record = r.store.record(&dependency).unwrap();
    assert_eq!(record.kind, RecordKind::Dependency);
    assert_eq!(record.parents, BTreeSet::from([start.clone()]));
    assert_eq!(transitions(&r, "work"), vec![start]);
    let after = r.current("work");
    assert_eq!(after.lifecycle, Lifecycle::InProgress);
    assert_eq!(after.owner, started.owner);
    assert!(error(r.try_op("work", Complete)).contains("dependencies must be Completed"));
    r.op("needs", Cancel);
    assert!(error(r.try_op("work", Complete)).contains("dependencies must be Completed"));
    r.op("work", Release);
    assert!(r.try_op("work", Start).is_err());
    r.op("needs", Reconsider);
    r.op("needs", Accept);
    assert!(r.try_op("work", Start).is_err());
    r.op("needs", Complete);
    r.op("work", Start);
    r.op("work", Complete);
    assert!(error(r.try_remove_dep("work", "needs")).contains("Completed dependencies are fixed"));
}

#[test]
fn registration_and_relation_cycles_are_atomic_for_dynamic_nested_plans() {
    let mut r = empty();
    for i in 0..20 {
        let parent = format!("g{}", i - 1);
        register(
            &mut r,
            &format!("g{i}"),
            Kind::Group,
            if i == 0 { None } else { Some(&parent) },
        );
    }
    register(&mut r, "leaf", Kind::Issue, Some("g19"));
    for (source, target) in [("g0", "leaf"), ("leaf", "g0"), ("leaf", "leaf")] {
        assert!(
            r.try_add_dep(source, target).is_err(),
            "{source} needs {target}"
        );
    }
    assert!(error(r.try_move("g0", Some("g19"))).contains("containment cycle"));
    assert!(error(r.try_move("g0", Some("leaf"))).contains("unfinished Group"));
    let mut value = current(Kind::Issue, Lifecycle::Undecided, Some("g19"));
    value.needs.insert(id("g0"));
    assert!(error(r.try_create("new", value)).contains("ancestor"));
    register(&mut r, "other", Kind::Group, None);
    register(&mut r, "otherchild", Kind::Issue, Some("other"));
    r.add_dep("g0", "otherchild");
    let cycle = "completion cycle";
    assert!(error(r.try_add_dep("other", "leaf")).contains(cycle));
    assert!(error(r.try_move("otherchild", Some("g19"))).contains(cycle));
    r.op("otherchild", Cancel);
    assert!(error(r.try_add_dep("otherchild", "leaf")).contains(cycle));
    assert!(r.view().is_valid());
}

#[test]
fn terminal_group_composition_and_working_subtree_moves() {
    let mut r = empty();
    for name in ["a", "b", "c"] {
        register(&mut r, name, Kind::Group, None);
    }
    register(&mut r, "child", Kind::Group, Some("a"));
    register(&mut r, "leaf", Kind::Issue, Some("child"));
    r.op("leaf", Start);
    r.op("c", Withdraw);
    assert!(error(r.try_move("child", Some("c"))).contains("adopted"));
    r.move_to("child", Some("b"));
    assert_eq!(r.current("leaf").parent, Some(id("child")));
    assert_eq!(effective(&r, "b"), Lifecycle::InProgress);
    r.move_to("child", None);
    r.op("leaf", Cancel);
    r.op("child", Cancel);
    r.move_to("child", Some("c"));
    assert!(error(r.try_move("leaf", None)).contains("unfinished Group"));
    assert!(
        error(r.try_create(
            "new",
            current(Kind::Group, Lifecycle::NotStarted, Some("child"))
        ))
        .contains("unfinished Group")
    );
    r.op("child", Reconsider);
    r.move_to("leaf", None);
}

#[test]
fn condition_edits_are_atomic_and_do_not_change_lifecycle_or_records() {
    let mut r = empty();
    let mut value = current(Kind::Issue, Lifecycle::NotStarted, None);
    value.condition = Some("exit 1".into());
    let created = r.try_create("item", value).unwrap();
    insert(&mut r.store, created);
    for operation in [None, Some(Start), Some(Complete)] {
        if let Some(operation) = operation {
            r.op("item", operation);
        }
        let before = r.current("item");
        let transitions_before = transitions(&r, "item");
        for command in [Some("exit 23".to_owned()), None] {
            let head = r.head("item");
            let record = r
                .store
                .set_condition(&id("item"), command.clone(), r.tick())
                .unwrap()
                .expect("a change");
            // The condition is its own record; lifecycle and every other field stay.
            assert_eq!(record.kind, RecordKind::Condition);
            assert_eq!(record.parents, BTreeSet::from([head]));
            insert(&mut r.store, record);
            assert_eq!(
                r.current("item"),
                Current {
                    condition: command,
                    ..before.clone()
                }
            );
            assert_eq!(transitions(&r, "item"), transitions_before);
        }
        assert!(
            r.store
                .set_condition(&id("item"), Some("  ".into()), r.tick())
                .is_err()
        );
        assert!(
            r.store
                .set_condition(&id("missing"), None, r.tick())
                .is_err()
        );
    }
}

#[test]
fn completed_work_moves_only_under_adopted_groups() {
    let mut r = empty();
    register(&mut r, "done", Kind::Issue, None);
    register(&mut r, "closed", Kind::Group, None);
    register(&mut r, "adopted", Kind::Group, None);
    register(&mut r, "pending", Kind::Group, None);
    r.op("pending", Withdraw);
    r.op("done", Start);
    r.op("done", Complete);
    r.op("closed", Complete);
    for name in ["done", "closed"] {
        assert!(error(r.try_move(name, Some("pending"))).contains("adopted"));
        r.move_to(name, Some("adopted"));
        r.move_to(name, None);
    }
    // A NotStarted Entity may still be placed under an unadopted Group.
    register(&mut r, "fresh", Kind::Issue, None);
    r.move_to("fresh", Some("pending"));
}

#[test]
fn a_cancelled_subgroup_with_completed_work_does_not_make_its_parent_working() {
    let mut r = empty();
    register(&mut r, "parent", Kind::Group, None);
    register(&mut r, "sub", Kind::Group, Some("parent"));
    register(&mut r, "work", Kind::Issue, Some("sub"));
    r.op("work", Start);
    r.op("work", Complete);
    assert_eq!(effective(&r, "parent"), Lifecycle::InProgress);
    r.op("sub", Cancel);
    assert_eq!(effective(&r, "parent"), Lifecycle::NotStarted);
    assert!(!r.view().working().contains(&id("parent")));
    r.op("parent", Withdraw);
    // Reconsidering leaves Completed work under an Undecided Group, which is allowed.
    r.op("sub", Reconsider);
    assert_eq!(effective(&r, "sub"), Lifecycle::Undecided);
    assert!(r.view().is_valid());
}

#[test]
fn reopen_requires_adopted_ancestors_even_when_the_parent_is_open() {
    let mut r = empty();
    register(&mut r, "g", Kind::Group, None);
    register(&mut r, "x", Kind::Issue, Some("g"));
    r.op("x", Start);
    r.op("x", Complete);
    r.op("g", Cancel);
    r.op("g", Reconsider);
    assert!(error(r.try_op("x", Reopen)).contains("adopted"));
    r.op("g", Accept);
    r.op("x", Reopen);
}

#[test]
fn complete_checks_only_the_entity_s_own_dependencies() {
    let mut r = empty();
    register(&mut r, "late", Kind::Issue, None);
    register(&mut r, "g", Kind::Group, None);
    register(&mut r, "work", Kind::Issue, Some("g"));
    register(&mut r, "sub", Kind::Group, Some("g"));
    register(&mut r, "done", Kind::Issue, Some("sub"));
    r.op("work", Start);
    r.op("done", Start);
    r.op("done", Complete);
    // A dependency added to the parent afterwards blocks new starts, not completion below it.
    r.add_dep("g", "late");
    register(&mut r, "next", Kind::Issue, Some("g"));
    assert!(error(r.try_op("next", Start)).contains("ancestor g"));
    r.op("work", Complete);
    r.op("sub", Complete);
    assert!(error(r.try_op("g", Complete)).contains("dependencies must be Completed"));
}

#[test]
fn accept_looks_at_every_descendant_for_started_work() {
    let mut r = empty();
    register(&mut r, "pending", Kind::Group, None);
    r.op("pending", Withdraw);
    register(&mut r, "plan", Kind::Group, None);
    register(&mut r, "sub", Kind::Group, Some("plan"));
    register(&mut r, "deep", Kind::Issue, Some("sub"));
    r.op("deep", Start);
    r.op("deep", Complete);
    // The Completed grandchild counts even under a Cancelled child Group, which does not
    // make `plan` working; the check is about started descendants, not the derived value.
    r.op("sub", Cancel);
    r.op("plan", Cancel);
    r.op("plan", Reconsider);
    r.move_to("plan", Some("pending"));
    assert!(error(r.try_op("plan", Accept)).contains("adopted ancestors"));
    r.op("pending", Accept);
    r.op("plan", Accept);
    assert_eq!(effective(&r, "plan"), Lifecycle::NotStarted);
}
