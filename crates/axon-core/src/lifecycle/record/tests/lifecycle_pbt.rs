use super::*;
use crate::lifecycle::{REASON_LIMIT, TITLE_LIMIT};
use proptest::prelude::*;

const OPS: [Operation; 8] = [
    Operation::Accept,
    Operation::Withdraw,
    Operation::Start,
    Operation::Release,
    Operation::Complete,
    Operation::Cancel,
    Operation::Reconsider,
    Operation::Reopen,
];

fn expected(kind: Kind, state: Lifecycle, op: Operation) -> Option<Lifecycle> {
    use Lifecycle::*;
    use Operation::*;
    match (kind, state, op) {
        (_, Undecided, Accept) => Some(NotStarted),
        (_, NotStarted, Withdraw) => Some(Undecided),
        (Kind::Issue, NotStarted, Start) => Some(InProgress),
        (Kind::Issue, InProgress, Release) => Some(NotStarted),
        (Kind::Issue, InProgress, Complete) | (Kind::Group, NotStarted, Complete) => {
            Some(Completed)
        }
        (Kind::Issue, Undecided | NotStarted | InProgress, Cancel)
        | (Kind::Group, Undecided | NotStarted, Cancel) => Some(Cancelled),
        (_, Cancelled, Reconsider) => Some(Undecided),
        (_, Completed, Reopen) => Some(NotStarted),
        _ => None,
    }
}

fn isolated(kind: Kind, state: Lifecycle) -> Replica {
    let mut r = Replica {
        store: Store::new(),
        clock: Cell::new(100),
        name: "r0".into(),
    };
    r.create("item", kind, Lifecycle::NotStarted, None);
    match state {
        Lifecycle::Undecided => {
            r.op("item", Operation::Withdraw);
        }
        Lifecycle::NotStarted => {}
        Lifecycle::InProgress => {
            r.op("item", Operation::Start);
        }
        Lifecycle::Completed => {
            if kind == Kind::Issue {
                r.op("item", Operation::Start);
            }
            r.op("item", Operation::Complete);
        }
        Lifecycle::Cancelled => {
            r.op("item", Operation::Cancel);
        }
    }
    r
}

fn check_transition(r: &mut Replica, op: Operation) {
    let before = r.current("item");
    let old_head = r.head("item");
    let old_len = r.store.len();
    let expected = expected(before.kind, before.lifecycle, op);
    let read = r.view().check_operation(&id("item"), op);
    let write = r.try_op("item", op);
    assert_eq!(read.is_ok(), expected.is_some(), "{before:?} {op:?}");
    assert_eq!(write.is_ok(), expected.is_some(), "{before:?} {op:?}");
    match (expected, write) {
        (Some(next), Ok(record)) => {
            assert_eq!(record.kind, RecordKind::Transition(op));
            assert_eq!(record.parents, BTreeSet::from([old_head]));
            assert_eq!(record.after.lifecycle, next);
            assert_eq!(record.after.kind, before.kind);
            assert_eq!(record.after.parent, before.parent);
            assert_eq!(record.after.needs, before.needs);
            assert_eq!(record.after.title, before.title);
            assert_eq!(record.after.description, before.description);
            assert_eq!(record.after.condition, before.condition);
            assert_eq!(
                record.after.owner.as_deref(),
                if op == Operation::Start {
                    Some("r0")
                } else {
                    None
                }
            );
            insert(&mut r.store, record);
            assert_eq!(r.store.len(), old_len + 1);
        }
        (None, Err(_)) => {
            assert_eq!(r.store.len(), old_len);
            assert_eq!(r.current("item"), before);
        }
        _ => unreachable!(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_lifecycle_paths_match_reference_table(path in prop::collection::vec(0usize..8, 0..24)) {
        for kind in [Kind::Issue, Kind::Group] {
            for state in [Lifecycle::Undecided, Lifecycle::NotStarted, Lifecycle::InProgress, Lifecycle::Completed, Lifecycle::Cancelled] {
                if kind == Kind::Group && state == Lifecycle::InProgress { continue; }
                for op in OPS {
                    let mut r = isolated(kind, state);
                    check_transition(&mut r, op);
                }
            }
            let mut r = isolated(kind, Lifecycle::NotStarted);
            for step in &path { check_transition(&mut r, OPS[*step]); }
        }
    }

    #[test]
    fn generated_information_respects_state_and_record_boundaries(
        kind in prop_oneof![Just(Kind::Issue), Just(Kind::Group)],
        state in 0usize..5,
        title in "[a-zA-Z0-9]{1,24}",
        body in ".{1,32}",
        condition in "[a-z]{1,16}",
    ) {
        let states = [Lifecycle::Undecided, Lifecycle::NotStarted, Lifecycle::InProgress, Lifecycle::Completed, Lifecycle::Cancelled];
        let state = states[state];
        if kind == Kind::Group && state == Lifecycle::InProgress { return Ok(()); }
        let mut r = isolated(kind, state);
        let before = r.current("item");
        let old_len = r.store.len();
        let edit = r.store.write(&id("item"), Some(title.clone()), Some(body.clone()), r.tick());
        prop_assert_eq!(edit.is_ok(), matches!(state, Lifecycle::Undecided | Lifecycle::NotStarted | Lifecycle::InProgress));
        if let Ok(Some(record)) = edit {
            prop_assert_eq!(&record.kind, &RecordKind::Edit);
            prop_assert_eq!(&record.after, &Current {
                title,
                description: body.clone(),
                ..before.clone()
            });
            insert(&mut r.store, record);
            prop_assert_eq!(r.store.len(), old_len + 1);
        } else {
            prop_assert_eq!(r.store.len(), old_len);
            prop_assert_eq!(r.current("item"), before);
        }
        let old_len = r.store.len();
        let before_condition = r.current("item");
        let change = r.store.set_condition(&id("item"), Some(condition.clone()), r.tick()).unwrap().unwrap();
        prop_assert_eq!(&change.kind, &RecordKind::Condition);
        prop_assert_eq!(&change.after, &Current {
            condition: Some(condition.clone()),
            ..before_condition
        });
        insert(&mut r.store, change);
        prop_assert_eq!(r.store.len(), old_len + 1);
        prop_assert!(r.store.set_condition(&id("item"), Some(condition), r.tick()).unwrap().is_none());
        let context = r.tick();
        let note = format!("note {body}");
        let a = r.store.add_note(&id("item"), note.clone(), None, context.clone()).unwrap();
        let b = r.store.add_note(&id("item"), note, None, context).unwrap();
        prop_assert_ne!(&a.nonce, &b.nonce);
        let previous = r.current("item");
        let old_len = r.store.len();
        let a = r.store.insert(Entry::Note(a)).unwrap();
        let b = r.store.insert(Entry::Note(b)).unwrap();
        prop_assert_ne!(a, b);
        prop_assert_eq!(r.store.len(), old_len + 2);
        prop_assert_eq!(r.current("item"), previous);
    }

    #[test]
    fn generated_title_and_reason_boundaries(
        extra in 0usize..3,
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for len in [TITLE_LIMIT - 1, TITLE_LIMIT, TITLE_LIMIT + extra + 1] {
            let title = "字".repeat(len);
            prop_assert_eq!(r.store.write(&id("item"), Some(title), None, r.tick()).is_ok(), len <= TITLE_LIMIT);
        }
        for bad in ['\n', '\t', '\u{1b}', '\u{85}'] {
            let invalid = format!("a{bad}b");
            prop_assert!(r.store.write(&id("item"), Some(invalid.clone()), None, r.tick()).is_err());
            prop_assert!(r.store.perform(&id("item"), Operation::Start, Some(invalid.clone()), r.tick()).is_err());
            let mut value = current(Kind::Issue, Lifecycle::NotStarted, None);
            value.title = invalid;
            prop_assert!(r.try_create("invalid", value).is_err());
        }
        prop_assert!(r.store.perform(&id("item"), Operation::Start, Some("   ".into()), r.tick()).is_err());
        let reason = "r".repeat(REASON_LIMIT);
        prop_assert_eq!(r.store.perform(&id("item"), Operation::Start, Some(reason.clone()), r.tick()).unwrap().reason, Some(reason));
        prop_assert!(r.store.perform(&id("item"), Operation::Start, Some("r".repeat(REASON_LIMIT + extra + 1)), r.tick()).is_err());
        for len in [TITLE_LIMIT, TITLE_LIMIT + extra + 1] {
            let mut value = current(Kind::Issue, Lifecycle::NotStarted, None);
            value.title = "字".repeat(len);
            prop_assert_eq!(r.try_create(&format!("new{len}"), value).is_ok(), len <= TITLE_LIMIT);
        }
        let mut value = current(Kind::Issue, Lifecycle::NotStarted, None);
        value.title = "   ".into();
        prop_assert!(r.try_create("invalid", value).is_err());
    }

    #[test]
    fn generated_forest_prerequisites_and_group_work(
        depth in 2usize..5,
        unadopted in prop::option::of(0usize..4),
        unfinished_dependency in prop::option::of(0usize..5),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let name = format!("g{level}");
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            r.create(&name, Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        let parent = format!("g{}", depth - 1);
        r.move_to("item", Some(&parent));
        for level in 0..=depth {
            let dep = format!("d{level}");
            r.create(&dep, Kind::Issue, Lifecycle::NotStarted, None);
            if unfinished_dependency != Some(level) {
                r.op(&dep, Operation::Start);
                r.op(&dep, Operation::Complete);
            }
            let owner = if level == depth { "item".to_owned() } else { format!("g{level}") };
            r.add_dep(&owner, &dep);
        }
        if let Some(level) = unadopted.filter(|level| *level < depth) {
            r.op(&format!("g{level}"), Operation::Withdraw);
        }
        let may_start = unadopted.is_none_or(|level| level >= depth)
            && unfinished_dependency.is_none_or(|level| level > depth);
        let before = r.store.len();
        prop_assert_eq!(r.view().check_operation(&id("item"), Operation::Start).is_ok(), may_start);
        let start = r.try_op("item", Operation::Start);
        prop_assert_eq!(start.is_ok(), may_start);
        prop_assert_eq!(r.store.len(), before);
        if let Ok(start) = start {
            insert(&mut r.store, start);
            for level in 0..depth {
                let name = format!("g{level}");
                prop_assert_eq!(r.current(&name).lifecycle, Lifecycle::NotStarted);
                prop_assert_eq!(r.view().effective_lifecycle(&id(&name)), Some(Lifecycle::InProgress));
            }
            prop_assert_eq!(r.current("item").owner, Some("r0".into()));
            prop_assert_eq!(r.store.len(), before + 1);
        }
    }

    #[test]
    fn generated_unsettled_ancestors_reject_without_misclassification(
        depth in 2usize..5,
    ) {
        let mut source = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let name = format!("g{level}");
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            source.create(&name, Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        let parent = format!("g{}", depth - 1);
        let moved = source.move_to("item", Some(&parent));
        for missing in 0..depth {
            for conflict in [false, true] {
                let mut partial = Replica { store: Store::new(), clock: Cell::new(200), name: "r1".into() };
                let item = source.store.history(&id("item")).unwrap();
                for (record, _) in item { partial.sync_one(&source, record); }
                for level in 0..depth {
                    if level != missing {
                        let record = source.head(&format!("g{level}"));
                        partial.sync_one(&source, &record);
                    }
                }
                prop_assert!(partial.store.contains(&moved));
                if conflict {
                    let name = format!("g{missing}");
                    let mut left = Replica::from("r0", &source.store);
                    let mut right = Replica::from("r1", &source.store);
                    let a = left.store.set_condition(&id(&name), Some("true".into()), left.tick()).unwrap().unwrap();
                    let b = right.store.set_condition(&id(&name), Some("false".into()), right.tick()).unwrap().unwrap();
                    let a = insert(&mut left.store, a);
                    let b = insert(&mut right.store, b);
                    partial.sync_one(&left, &source.head(&name));
                    partial.sync_one(&left, &a);
                    partial.sync_one(&right, &b);
                    prop_assert!(partial.view().conflicted().contains(&id(&name)));
                } else {
                    prop_assert!(partial.view().violations().iter().any(|v| v.kind == ViolationKind::UnknownParent));
                }
                let old_len = partial.store.len();
                let message = error(partial.view().check_operation(&id("item"), Operation::Start));
                prop_assert!(
                    message.contains(if conflict && missing == depth - 1 { "unfinished Group" } else { "adopted" }),
                    "{message}"
                );
                prop_assert!(partial.try_op("item", Operation::Start).is_err());
                prop_assert_eq!(partial.store.len(), old_len);
            }
        }
    }

    #[test]
    fn generated_conversion_changes_only_kind_and_same_kind_is_noop(
        title in "[a-zA-Z0-9]{1,24}",
        state in prop_oneof![Just(Lifecycle::Undecided), Just(Lifecycle::NotStarted)],
        from_group in any::<bool>(),
        label in 0usize..Label::ALL.len(),
    ) {
        let kind = if from_group { Kind::Group } else { Kind::Issue };
        let other = if from_group { Kind::Issue } else { Kind::Group };
        let mut r = isolated(kind, state);
        r.create("anchor", Kind::Group, Lifecycle::NotStarted, None);
        r.create("dep", Kind::Group, Lifecycle::NotStarted, None);
        r.move_to("item", Some("anchor"));
        r.add_dep("item", "dep");
        let edit = r.store.write(&id("item"), Some(format!("edited {title}")), None, r.tick()).unwrap().unwrap();
        insert(&mut r.store, edit);
        let relabel = r.store.set_label(&id("item"), Label::ALL[label], r.tick()).unwrap();
        if let Some(record) = relabel {
            insert(&mut r.store, record);
        }
        prop_assert_eq!(r.current("item").label, Label::ALL[label]);
        let before = r.current("item");
        let old_len = r.store.len();
        prop_assert!(r.store.convert(&id("item"), kind, r.tick()).unwrap().is_none());
        prop_assert_eq!(r.store.len(), old_len);
        let record = r.store.convert(&id("item"), other, r.tick()).unwrap().unwrap();
        prop_assert_eq!(&record.kind, &RecordKind::Convert);
        prop_assert_eq!(&record.after, &Current { kind: other, ..before.clone() });
        insert(&mut r.store, record);
        prop_assert_eq!(r.store.len(), old_len + 1);
        prop_assert_eq!(r.current("item"), Current { kind: other, ..before });
    }

    #[test]
    fn generated_conflicts_block_ordinary_writes_but_allow_notes_and_resolution(
        left in "[a-z]{1,12}",
        right in "[A-Z]{1,12}",
    ) {
        let mut base = isolated(Kind::Issue, Lifecycle::NotStarted);
        base.create("g0", Kind::Group, Lifecycle::NotStarted, None);
        base.create("g2", Kind::Group, Lifecycle::NotStarted, None);
        base.op("g2", Operation::Complete);
        base.add_dep("item", "g2");
        prop_assert!(base.try_create("new", current(Kind::Issue, Lifecycle::NotStarted, None)).is_ok());
        prop_assert!(base.try_op("item", Operation::Start).is_ok());
        prop_assert!(base.try_move("item", Some("g0")).unwrap().is_some());
        prop_assert!(base.try_add_dep("item", "g0").unwrap().is_some());
        prop_assert!(base.try_remove_dep("item", "g2").unwrap().is_some());
        prop_assert!(base.store.convert(&id("item"), Kind::Group, base.tick()).unwrap().is_some());
        prop_assert!(base.store.set_label(&id("item"), Label::Bug, base.tick()).unwrap().is_some());
        let mut a = Replica::from("r0", &base.store);
        let mut b = Replica::from("r1", &base.store);
        let edit = a.store.write(&id("item"), Some(left), None, a.tick()).unwrap().unwrap();
        insert(&mut a.store, edit);
        let edit = b.store.write(&id("item"), Some(right), None, b.tick()).unwrap().unwrap();
        insert(&mut b.store, edit);
        a.sync(&b);
        prop_assert!(a.view().conflicted().contains(&id("item")));
        let old_len = a.store.len();
        prop_assert!(a.try_create("new", current(Kind::Issue, Lifecycle::NotStarted, None)).is_err());
        prop_assert!(a.try_op("item", Operation::Start).is_err());
        prop_assert!(a.store.write(&id("item"), Some("updated".into()), None, a.tick()).is_err());
        prop_assert!(a.store.set_label(&id("item"), Label::Bug, a.tick()).is_err());
        prop_assert!(a.try_move("item", Some("g0")).is_err());
        prop_assert!(a.try_add_dep("item", "g0").is_err());
        prop_assert!(a.try_remove_dep("item", "g2").is_err());
        prop_assert!(a.store.set_condition(&id("item"), Some("true".into()), a.tick()).is_err());
        prop_assert!(a.store.convert(&id("item"), Kind::Group, a.tick()).is_err());
        let value = Imported { title: "x".into(), description: "body".into(), label: Label::Bug, parent: None, needs: BTreeSet::new() };
        prop_assert!(a.store.import(&id("item"), value, a.tick()).is_err());
        prop_assert_eq!(a.store.len(), old_len);
        let note = a.store.add_note(&id("item"), "note".into(), None, a.tick()).unwrap();
        a.store.insert(Entry::Note(note)).unwrap();
        let chosen = a.heads("item").first().unwrap().clone();
        a.resolve("item", &chosen);
        prop_assert!(a.view().conflicted().is_empty());
        prop_assert_eq!(a.store.len(), old_len + 2);
        for kind in [Kind::Issue, Kind::Group] {
            for state in [Lifecycle::Undecided, Lifecycle::NotStarted, Lifecycle::InProgress, Lifecycle::Completed, Lifecycle::Cancelled] {
                if kind == Kind::Group && state == Lifecycle::InProgress { continue; }
                for op in OPS {
                    if expected(kind, state, op).is_none() { continue; }
                    let mut control = isolated(kind, state);
                    control.create("blocker", Kind::Issue, Lifecycle::NotStarted, None);
                    prop_assert!(control.try_op("item", op).is_ok());
                    let mut left = Replica::from("r0", &control.store);
                    let mut right = Replica::from("r1", &control.store);
                    let a = left.store.set_condition(&id("blocker"), Some("true".into()), left.tick()).unwrap().unwrap();
                    let b = right.store.set_condition(&id("blocker"), Some("false".into()), right.tick()).unwrap().unwrap();
                    insert(&mut left.store, a);
                    insert(&mut right.store, b);
                    left.sync(&right);
                    prop_assert!(left.view().conflicted().contains(&id("blocker")));
                    let old_len = left.store.len();
                    prop_assert!(left.try_op("item", op).is_err());
                    prop_assert_eq!(left.store.len(), old_len);
                }
            }
        }
    }

    #[test]
    fn generated_relation_changes_keep_other_fields_and_reject_cycles(
        depth in 2usize..5,
        target in 0usize..4,
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            r.create(&format!("g{level}"), Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        r.create("other", Kind::Group, Lifecycle::NotStarted, None);
        r.create("other-issue", Kind::Issue, Lifecycle::NotStarted, None);
        let parent = format!("g{}", depth - 1);
        r.move_to("item", Some(&parent));
        let ancestor = format!("g{}", target % depth);
        let before = r.current("item");
        let old_len = r.store.len();
        prop_assert!(r.try_add_dep("item", &ancestor).is_err());
        prop_assert!(r.try_add_dep("item", "item").is_err());
        prop_assert!(r.try_move("g0", Some(&parent)).is_err());
        prop_assert!(r.try_move("item", Some("item")).is_err());
        prop_assert!(r.try_move("item", Some("other-issue")).is_err());
        prop_assert_eq!(r.store.len(), old_len);
        prop_assert_eq!(r.current("item"), before.clone());

        let record = r.try_move("item", Some("other")).unwrap().unwrap();
        prop_assert_eq!(&record.kind, &RecordKind::Parent);
        prop_assert_eq!(&record.after, &Current { parent: Some(id("other")), ..before.clone() });
        insert(&mut r.store, record);
        prop_assert_eq!(r.store.len(), old_len + 1);
        let before = r.current("item");
        let record = r.try_add_dep("item", &ancestor).unwrap().unwrap();
        prop_assert_eq!(&record.kind, &RecordKind::Dependency);
        prop_assert_eq!(&record.after, &Current { needs: BTreeSet::from([id(&ancestor)]), ..before });
        insert(&mut r.store, record);
        let before = r.current("item");
        let record = r.try_remove_dep("item", &ancestor).unwrap().unwrap();
        prop_assert_eq!(&record.after, &Current { needs: BTreeSet::new(), ..before });
        insert(&mut r.store, record);
        prop_assert_eq!(r.store.len(), old_len + 3);
        prop_assert!(r.view().is_valid());
    }

    #[test]
    fn generated_completion_checks_own_dependency_after_start(
        ancestor_dependency_open in any::<bool>(),
        own_dependency_open in any::<bool>(),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        r.create("outer", Kind::Group, Lifecycle::NotStarted, None);
        r.create("inner", Kind::Group, Lifecycle::NotStarted, Some("outer"));
        r.move_to("item", Some("inner"));
        r.create("gate", Kind::Issue, Lifecycle::NotStarted, None);
        r.create("own", Kind::Issue, Lifecycle::NotStarted, None);
        r.op("item", Operation::Start);
        r.add_dep("outer", "gate");
        r.add_dep("item", "own");
        if !ancestor_dependency_open {
            r.op("gate", Operation::Start);
            r.op("gate", Operation::Complete);
        }
        if !own_dependency_open {
            r.op("own", Operation::Start);
            r.op("own", Operation::Complete);
        }
        let before = r.store.len();
        prop_assert_eq!(r.view().check_operation(&id("item"), Operation::Complete).is_ok(), !own_dependency_open);
        let result = r.try_op("item", Operation::Complete);
        prop_assert_eq!(result.is_ok(), !own_dependency_open);
        prop_assert_eq!(r.store.len(), before);
        if let Ok(record) = result {
            insert(&mut r.store, record);
            prop_assert_eq!(r.current("item").lifecycle, Lifecycle::Completed);
            prop_assert_eq!(r.store.len(), before + 1);
        }
    }

    #[test]
    fn generated_group_work_is_derived_and_requires_final_transition(
        depth in 2usize..5,
        cancelled in any::<bool>(),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            r.create(&format!("g{level}"), Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        r.move_to("item", Some(&format!("g{}", depth - 1)));
        let before: Vec<_> = (0..depth).map(|level| r.head(&format!("g{level}"))).collect();
        r.op("item", Operation::Start);
        for (level, head) in before.iter().enumerate() {
            let name = format!("g{level}");
            prop_assert_eq!(r.current(&name).lifecycle, Lifecycle::NotStarted);
            prop_assert_eq!(r.view().effective_lifecycle(&id(&name)), Some(Lifecycle::InProgress));
            prop_assert_eq!(&r.head(&name), head);
            for op in [Operation::Start, Operation::Release, Operation::Withdraw, Operation::Complete, Operation::Cancel] {
                prop_assert!(r.try_op(&name, op).is_err());
            }
        }
        r.op("item", if cancelled { Operation::Cancel } else { Operation::Complete });
        for (level, head) in before.iter().enumerate().rev() {
            let name = format!("g{level}");
            let old_len = r.store.len();
            let complete = r.try_op(&name, Operation::Complete).unwrap();
            prop_assert_eq!(&complete.parents, &BTreeSet::from([head.clone()]));
            insert(&mut r.store, complete);
            prop_assert_eq!(r.store.len(), old_len + 1);
            prop_assert_eq!(r.current(&name).lifecycle, Lifecycle::Completed);
        }
    }

    #[test]
    fn generated_terminal_information_rules_keep_notes_and_conditions_available(
        kind in prop_oneof![Just(Kind::Issue), Just(Kind::Group)],
        cancelled in any::<bool>(),
        condition in "[a-z]{1,12}",
    ) {
        let state = if cancelled { Lifecycle::Cancelled } else { Lifecycle::Completed };
        let mut r = isolated(kind, state);
        r.create("destination", Kind::Group, Lifecycle::NotStarted, None);
        r.create("dependency", Kind::Issue, Lifecycle::NotStarted, None);
        let old_len = r.store.len();
        let other_kind = if kind == Kind::Issue { Kind::Group } else { Kind::Issue };
        prop_assert!(r.store.write(&id("item"), Some("task".into()), None, r.tick()).is_err());
        prop_assert!(r.store.convert(&id("item"), other_kind, r.tick()).is_err());
        prop_assert_eq!(r.store.len(), old_len);
        let before = r.current("item");
        let condition_record = r.store.set_condition(&id("item"), Some(condition.clone()), r.tick()).unwrap().unwrap();
        prop_assert_eq!(&condition_record.after, &Current { condition: Some(condition.clone()), ..before.clone() });
        insert(&mut r.store, condition_record);
        prop_assert!(r.store.set_condition(&id("item"), Some(condition), r.tick()).unwrap().is_none());
        let moved = r.try_move("item", Some("destination")).unwrap().unwrap();
        insert(&mut r.store, moved);
        prop_assert_eq!(r.current("item").parent, Some(id("destination")));
        let note = r.store.add_note(&id("item"), "terminal note".into(), None, r.tick()).unwrap();
        r.store.insert(Entry::Note(note)).unwrap();
        prop_assert_eq!(r.current("item").lifecycle, state);
        if cancelled {
            let added = r.try_add_dep("item", "dependency").unwrap().unwrap();
            insert(&mut r.store, added);
        } else {
            prop_assert!(r.try_add_dep("item", "dependency").is_err());
        }
    }

    #[test]
    fn generated_import_is_one_record_with_final_value(
        title in "[a-z]{1,20}",
        description in ".{0,30}",
        label in 0usize..Label::ALL.len(),
        with_parent in any::<bool>(),
        with_dependency in any::<bool>(),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        r.create("destination", Kind::Group, Lifecycle::NotStarted, None);
        r.create("dependency", Kind::Issue, Lifecycle::NotStarted, None);
        let parent = with_parent.then(|| id("destination"));
        let needs = if with_dependency { BTreeSet::from([id("dependency")]) } else { BTreeSet::new() };
        let title = format!("imported {title}");
        let label = Label::ALL[label];
        let before = r.current("item");
        let old_len = r.store.len();
        let record = r.store.import(&id("item"), Imported { title: title.clone(), description: description.clone(), label, parent: parent.clone(), needs: needs.clone() }, r.tick()).unwrap().unwrap();
        prop_assert_eq!(&record.kind, &RecordKind::Import);
        prop_assert_eq!(&record.after, &Current { title: title.clone(), description: description.clone(), label, parent: parent.clone(), needs: needs.clone(), ..before });
        prop_assert_eq!(r.store.len(), old_len);
        insert(&mut r.store, record);
        prop_assert_eq!(r.store.len(), old_len + 1);
        let value = Imported { title, description, label, parent, needs };
        prop_assert!(r.store.import(&id("item"), value, r.tick()).unwrap().is_none());
    }

    #[test]
    fn generated_completed_dependents_must_reopen_first(
        length in 2usize..6,
        target in 0usize..5,
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for index in 0..length {
            let name = format!("d{index}");
            r.create(&name, Kind::Issue, Lifecycle::NotStarted, None);
            if index > 0 { r.add_dep(&name, &format!("d{}", index - 1)); }
            r.op(&name, Operation::Start);
            r.op(&name, Operation::Complete);
        }
        let target = target % length;
        let target_name = format!("d{target}");
        let old_len = r.store.len();
        prop_assert_eq!(r.try_op(&target_name, Operation::Reopen).is_ok(), target == length - 1);
        prop_assert_eq!(r.store.len(), old_len);
        for index in (target..length).rev() {
            let name = format!("d{index}");
            let old_len = r.store.len();
            let record = r.try_op(&name, Operation::Reopen).unwrap();
            insert(&mut r.store, record);
            prop_assert_eq!(r.current(&name).lifecycle, Lifecycle::NotStarted);
            prop_assert_eq!(r.store.len(), old_len + 1);
        }
    }

    #[test]
    fn generated_terminal_ancestor_fixes_descendants_until_reopened(
        depth in 2usize..5,
        cancel_leaf in any::<bool>(),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            r.create(&format!("g{level}"), Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        r.move_to("item", Some(&format!("g{}", depth - 1)));
        if cancel_leaf { r.op("item", Operation::Cancel); }
        else { r.op("item", Operation::Start); r.op("item", Operation::Complete); }
        for level in (0..depth).rev() { r.op(&format!("g{level}"), Operation::Complete); }
        let old_len = r.store.len();
        let revive = if cancel_leaf { Operation::Reconsider } else { Operation::Reopen };
        prop_assert!(r.try_op("item", revive).is_err());
        prop_assert!(r.try_move("item", None).is_err());
        prop_assert_eq!(r.store.len(), old_len);
        for level in 0..depth { r.op(&format!("g{level}"), Operation::Reopen); }
        r.op("item", revive);
        prop_assert!(r.view().is_valid());
    }

    #[test]
    fn generated_working_subtrees_move_only_under_adopted_ancestors(
        depth in 2usize..5,
        completed in any::<bool>(),
    ) {
        let mut r = isolated(Kind::Issue, Lifecycle::NotStarted);
        for level in 0..depth {
            let parent = (level > 0).then(|| format!("g{}", level - 1));
            r.create(&format!("g{level}"), Kind::Group, Lifecycle::NotStarted, parent.as_deref());
        }
        r.move_to("item", Some(&format!("g{}", depth - 1)));
        r.create("pending", Kind::Group, Lifecycle::Undecided, None);
        r.create("adopted", Kind::Group, Lifecycle::NotStarted, None);
        r.op("item", Operation::Start);
        if completed { r.op("item", Operation::Complete); }
        let old_len = r.store.len();
        prop_assert!(r.try_move("g0", Some("pending")).is_err());
        prop_assert_eq!(r.store.len(), old_len);
        r.move_to("g0", Some("adopted"));
        prop_assert_eq!(r.view().effective_lifecycle(&id("adopted")), Some(Lifecycle::InProgress));
        r.move_to("g0", None);
        if !completed { r.op("item", Operation::Complete); }
        for level in (1..depth).rev() { r.op(&format!("g{level}"), Operation::Complete); }
        r.op("g0", Operation::Cancel);
        r.op("g0", Operation::Reconsider);
        r.move_to("g0", Some("pending"));
        prop_assert!(r.try_op("g0", Operation::Accept).is_err());
        r.op("pending", Operation::Accept);
        r.op("g0", Operation::Accept);
        prop_assert!(r.view().is_valid());
    }

    #[test]
    fn generated_unsettled_dependencies_are_not_treated_as_completed(
        dependency in "[a-z]{1,8}",
    ) {
        let base = isolated(Kind::Issue, Lifecycle::NotStarted);
        let mut source = Replica::from("r0", &base.store);
        let target = format!("dep-{dependency}");
        source.create(&target, Kind::Issue, Lifecycle::NotStarted, None);
        source.op(&target, Operation::Start);
        source.op(&target, Operation::Complete);
        let relation = source.add_dep("item", &target);
        for conflicted in [false, true] {
            let mut partial = Replica::from("r1", &base.store);
            partial.sync_one(&source, &relation);
            if conflicted {
                for (record, _) in source.store.history(&id(&target)).unwrap() {
                    partial.sync_one(&source, record);
                }
                let mut left = Replica::from("r0", &source.store);
                let mut right = Replica::from("r1", &source.store);
                let a = left.store.set_condition(&id(&target), Some("true".into()), left.tick()).unwrap().unwrap();
                let b = right.store.set_condition(&id(&target), Some("false".into()), right.tick()).unwrap().unwrap();
                let a = insert(&mut left.store, a);
                let b = insert(&mut right.store, b);
                partial.sync_one(&left, &a);
                partial.sync_one(&right, &b);
                prop_assert!(partial.view().conflicted().contains(&id(&target)));
                prop_assert!(!partial.view().violations().iter().any(|v| v.entity == id("item") && v.kind == ViolationKind::UnknownDependency));
            } else {
                prop_assert!(partial.view().violations().iter().any(|v| v.entity == id("item") && v.kind == ViolationKind::UnknownDependency));
            }
            let old_len = partial.store.len();
            let read = error(partial.view().check_operation(&id("item"), Operation::Start));
            prop_assert!(read.contains("dependencies must be Completed"), "{read}");
            prop_assert!(partial.try_op("item", Operation::Start).is_err());
            prop_assert_eq!(partial.store.len(), old_len);
        }
    }
}
