//! The main witnesses of `spec/record_integration.qnt`: what integration makes visible and
//! how it is repaired.
use super::*;
use Operation::*;

/// wConcurrentStartConflict: the same Issue started by different actors on two replicas.
#[test]
fn concurrent_starts_by_different_actors_conflict_with_both_owners_visible() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op_as("i1", Start, "actor-0");
    r1.op_as("i1", Start, "actor-1");
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.is_conflicted(&id("i1")));
    assert!(view.current(&id("i1")).is_none());
    let owners: BTreeSet<_> = view
        .heads(&id("i1"))
        .unwrap()
        .iter()
        .map(|h| r0.store.record(h).unwrap().after.owner.clone())
        .collect();
    assert_eq!(
        owners,
        BTreeSet::from([Some("actor-0".into()), Some("actor-1".into())])
    );
    // A conflicted Entity blocks every ordinary operation but not Notes or resolution.
    assert!(error(r0.try_op("i3", Start)).contains("i1"));
    r0.note("i1", "still conflicted");
}

/// wEqualValueConflict / wEqualValueResolved: the same actor starting the Issue on both
/// replicas (at different times) yields equal values that still conflict; resolving picks one.
#[test]
fn equal_valued_concurrent_records_conflict_and_resolve() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let mine = r0.op_as("i1", Start, "same");
    let theirs = r1.op_as("i1", Start, "same");
    assert_ne!(mine, theirs);
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.is_conflicted(&id("i1")));
    let values: Vec<_> = view
        .heads(&id("i1"))
        .unwrap()
        .iter()
        .map(|h| r0.store.record(h).unwrap().after.clone())
        .collect();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0], values[1]);
    let resolve = r0.resolve("i1", &mine);
    let record = r0.store.record(&resolve).unwrap();
    assert_eq!(record.parents, BTreeSet::from([mine.clone(), theirs]));
    assert_eq!(record.kind, RecordKind::Resolve { chosen: mine });
    assert_eq!(lifecycle(&r0.view(), "i1"), Lifecycle::InProgress);
    // Two Accepts of the same proposal conflict the same way.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::Undecided, None);
    r1.sync(&r0);
    r0.op("i4", Accept);
    r1.op("i4", Accept);
    r1.sync(&r0);
    assert!(r1.view().is_conflicted(&id("i4")));
}

/// wConcurrentResolvesAgree / wConcurrentResolvesDisagree / wResolveOfResolves: both sides
/// resolving the same conflict separately conflict again, and one more resolution converges.
#[test]
fn concurrent_resolutions_conflict_again_and_converge_with_one_more() {
    for agree in [true, false] {
        let mut r0 = Replica::new("r0");
        let mut r1 = Replica::new("r1");
        let start0 = r0.op_as("i1", Start, "actor-0");
        let start1 = r1.op_as("i1", Start, "actor-1");
        r0.sync(&r1);
        r1.sync(&r0);
        let res0 = r0.resolve("i1", &start0);
        let res1 = r1.resolve("i1", if agree { &start0 } else { &start1 });
        r0.sync(&r1);
        let view = r0.view();
        assert!(view.is_conflicted(&id("i1")));
        let heads = r0.heads("i1");
        assert_eq!(heads, BTreeSet::from([res0.clone(), res1.clone()]));
        assert!(
            heads
                .iter()
                .all(|h| matches!(r0.store.record(h).unwrap().kind, RecordKind::Resolve { .. }))
        );
        let last = r0.resolve("i1", &res1);
        let view = r0.view();
        assert!(view.is_settled(&id("i1")));
        assert_eq!(view.head(&id("i1")), Some(&last));
        let expected = if agree { "actor-0" } else { "actor-1" };
        assert_eq!(
            view.current(&id("i1")).unwrap().owner.as_deref(),
            Some(expected)
        );
        // Taking the final resolution on the other side converges without a further conflict.
        r1.sync(&r0);
        assert_eq!(r1.view(), view);
    }
}

/// wGap / wGapFilled / wSpuriousGapConflict with a rebase-style prefix: the Complete of
/// Start → Complete → Reopen arriving without Start is a gap and a false conflict; the rest
/// arriving later fills the gap.
#[test]
fn partial_arrival_reports_a_gap_that_later_records_fill() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let start = r0.op("i3", Start);
    let complete = r0.op("i3", Complete);
    let reopen = r0.op("i3", Reopen);
    r1.sync_one(&r0, &complete);
    r1.sync_one(&r0, &reopen);
    let view = r1.view();
    assert_eq!(
        view.gaps().get(&id("i3")),
        Some(&BTreeSet::from([start.clone()]))
    );
    assert!(view.is_conflicted(&id("i3")));
    assert_eq!(r1.heads("i3").len(), 2);
    assert!(r1.heads("i3").contains(&reopen));
    // The history still orders what it has; the record with the missing parent is unordered
    // relative to the base creation record.
    let history: Vec<_> = r1.store.history(&id("i3")).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[2].0, &reopen);
    r1.sync_one(&r0, &start);
    let view = r1.view();
    assert!(view.gaps().is_empty());
    assert!(view.is_settled(&id("i3")));
    assert_eq!(view.head(&id("i3")), Some(&reopen));
}

/// wTerminalGroupGainsChild / wReopenRepairs / wGroupReopenedWithCompletedChild: a child
/// registered under a Group that the other side completed, repaired by Reopen. Reopening a
/// Group with a Completed child makes the Group effectively InProgress.
#[test]
fn child_flowing_into_a_completed_group_is_repaired_by_reopen() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Start);
    r0.op("i1", Complete);
    r0.op("g0", Complete);
    r1.create("i4", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    r1.sync(&r0);
    let view = r1.view();
    assert!(view.conflicted().is_empty());
    assert_eq!(
        r1.violations("i4"),
        BTreeSet::from([ViolationKind::OpenUnderTerminal])
    );
    assert_eq!(lifecycle(&view, "g0"), Lifecycle::Completed);
    // The child is in violation, so the terminal parent no longer blocks it, but a Completed
    // ancestor is not adopted: the repair is the Group's Reopen.
    assert!(error(r1.try_op("i4", Start)).contains("adopted"));
    r1.op("g0", Reopen);
    let view = r1.view();
    assert!(view.is_valid());
    assert_eq!(lifecycle(&view, "g0"), Lifecycle::NotStarted);
    assert_eq!(
        view.effective_lifecycle(&id("g0")),
        Some(Lifecycle::InProgress)
    );
    assert!(view.working().contains(&id("g0")));
    r1.op("i4", Start);
    assert!(r1.view().is_valid());
}

/// wContainmentCycle (crossMoves) with unfinished Groups: both sides move the other Group
/// under their own; detaching one repairs it.
#[test]
fn cross_moves_form_a_containment_cycle_repaired_by_detaching() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.move_to("g0", Some("g2"));
    r1.move_to("g2", Some("g0"));
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.conflicted().is_empty());
    assert!(
        r0.violations("g0")
            .contains(&ViolationKind::ContainmentCycle)
    );
    assert!(
        r0.violations("g2")
            .contains(&ViolationKind::ContainmentCycle)
    );
    assert!(r0.violations("i1").is_empty());
    // Walks over the cycle terminate.
    assert_eq!(view.ancestors(&id("i1")), vec![id("g0"), id("g2")]);
    assert!(view.ancestors(&id("g0")).contains(&id("g0")));
    assert_eq!(view.descendants(&id("g0")).len(), 2);
    r0.move_to("g2", None);
    assert!(r0.view().is_valid());
}

/// A containment cycle of NotStarted Groups above a started Issue: deriving the Groups'
/// progress terminates and marks every Group on the cycle.
#[test]
fn a_containment_cycle_above_a_started_issue_marks_every_group_on_it_as_working() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Start);
    r0.move_to("g0", Some("g2"));
    r1.move_to("g2", Some("g0"));
    r0.sync(&r1);
    let view = r0.view();
    assert!(
        r0.violations("g0")
            .contains(&ViolationKind::ContainmentCycle)
    );
    assert_eq!(view.working(), &BTreeSet::from([id("g0"), id("g2")]));
    for group in ["g0", "g2"] {
        assert_eq!(lifecycle(&view, group), Lifecycle::NotStarted);
        assert_eq!(
            view.effective_lifecycle(&id(group)),
            Some(Lifecycle::InProgress)
        );
    }
}

/// wCompletedWithOpenDep (reopenUnderDependent): one side completes the dependent while the
/// other reopens the dependency. Adding a dependency and completing do not mix silently.
#[test]
fn reopening_a_dependency_under_a_completed_dependent_is_a_violation_not_a_merge() {
    let mut shared = Replica::new("r0");
    shared.op("g2", Complete);
    shared.add_dep("i1", "g2");
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    r0.op("i1", Start);
    r0.op("i1", Complete);
    r1.op("g2", Reopen);
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.conflicted().is_empty());
    assert_eq!(
        r0.violations("i1"),
        BTreeSet::from([ViolationKind::CompletedWithOpenDependency])
    );
    // Repair by reopening the dependent; alternatively complete the dependency again.
    r0.op("i1", Reopen);
    assert!(r0.view().is_valid());
    // The other order (dependency added on one side, completion on the other) is a conflict
    // of the same Entity, never a silent combination.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("g2", Complete);
    r1.sync(&r0);
    r0.add_dep("i1", "g2");
    r1.op("i1", Start);
    r1.op("i1", Complete);
    r0.sync(&r1);
    assert!(r0.view().is_conflicted(&id("i1")));
}

/// wUnadoptedAncestor: a Group moved under an unadopted Group on one side while its Issue is
/// started on the other; repaired by adopting the ancestor.
#[test]
fn work_under_an_unadopted_ancestor_is_a_violation_repaired_by_accepting() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::Undecided, None);
    r1.sync(&r0);
    r0.move_to("g0", Some("g4"));
    r1.op("i1", Start);
    r0.sync(&r1);
    assert_eq!(
        r0.violations("i1"),
        BTreeSet::from([ViolationKind::UnadoptedAncestor])
    );
    assert_eq!(
        r0.violations("g0"),
        BTreeSet::from([ViolationKind::UnadoptedAncestor])
    );
    r0.op("g4", Accept);
    assert!(r0.view().is_valid());
}

/// wDuplicateCreation / wDuplicateResolved with the rejected side re-registered afterwards.
#[test]
fn duplicate_registration_is_a_conflict_and_the_discarded_side_registers_again() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    let theirs = r1.create("i4", Kind::Group, Lifecycle::Undecided, None);
    r0.sync(&r1);
    assert!(
        error(r0.try_create("i5", current(Kind::Issue, Lifecycle::NotStarted, None)))
            .contains("i4")
    );
    let resolve = r0.resolve("i4", &theirs);
    let record = r0.store.record(&resolve).unwrap();
    assert_eq!(record.parents.len(), 2);
    let view = r0.view();
    assert_eq!(view.current(&id("i4")).unwrap().kind, Kind::Group);
    assert!(view.is_valid());
    r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    assert!(r0.view().is_valid());
}

/// wCompleteVsRelease / wChoseNotStartedOverCompleted / wStartAfterResolve: choosing the
/// unfinished side of a completion conflict lets ordinary work continue with a Start, which
/// is not a reopening path.
#[test]
fn choosing_the_released_side_over_completion_allows_a_new_start() {
    let mut shared = Replica::new("r0");
    shared.op("i3", Start);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    r0.op("i3", Complete);
    let release = r1.op("i3", Release);
    r0.sync(&r1);
    assert!(r0.view().is_conflicted(&id("i3")));
    r0.resolve("i3", &release);
    assert_eq!(lifecycle(&r0.view(), "i3"), Lifecycle::NotStarted);
    let start = r0.op("i3", Start);
    let record = r0.store.record(&start).unwrap();
    let parent = r0.store.record(record.parents.first().unwrap()).unwrap();
    assert!(matches!(parent.kind, RecordKind::Resolve { .. }));
    assert_eq!(record.after.lifecycle, Lifecycle::InProgress);
    assert!(r0.view().is_valid());
}

/// wNotesFromBoth / wTextConflict: Notes from both sides are kept; concurrent text edits
/// conflict.
#[test]
fn notes_from_both_sides_survive_and_text_edits_conflict() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.note("i1", "from r0");
    r1.note("i1", "from r1");
    let edit0 = r0
        .store
        .write(&id("i1"), Some("left".into()), None, r0.tick())
        .unwrap()
        .unwrap();
    insert(&mut r0.store, edit0);
    let edit1 = r1
        .store
        .write(&id("i1"), None, Some("right".into()), r1.tick())
        .unwrap()
        .unwrap();
    insert(&mut r1.store, edit1);
    r0.sync(&r1);
    let bodies: Vec<_> = r0
        .store
        .notes_of(&id("i1"))
        .iter()
        .map(|(_, note)| note.body.clone())
        .collect();
    assert_eq!(bodies, vec!["from r0", "from r1"]);
    assert!(r0.view().is_conflicted(&id("i1")));
    assert!(
        r0.heads("i1")
            .iter()
            .all(|h| r0.store.record(h).unwrap().kind == RecordKind::Edit)
    );
}

/// Records that arrive without the registration of their parent or dependency (a cherry-pick),
/// or a parent converted to an Issue on the other side, are the unknown-parent and
/// unknown-dependency violations; detaching and removing the dependency repair them.
#[test]
fn missing_parent_or_dependency_registrations_are_violations_repaired_by_detaching() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let child = r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    let dependent = r0.add_dep("i3", "g4");
    r1.sync_one(&r0, &child);
    r1.sync_one(&r0, &dependent);
    let view = r1.view();
    assert!(view.conflicted().is_empty());
    assert!(!view.is_known(&id("g4")));
    assert_eq!(
        r1.violations("i5"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    assert_eq!(
        r1.violations("i3"),
        BTreeSet::from([ViolationKind::UnknownDependency])
    );
    r1.move_to("i5", None);
    r1.remove_dep("i3", "g4");
    assert!(r1.view().is_valid());
    // A parent converted to an Issue on one side while a child moved under it on the other.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let convert = r0
        .store
        .convert(&id("g2"), Kind::Issue, r0.tick())
        .unwrap()
        .unwrap();
    insert(&mut r0.store, convert);
    r1.move_to("i3", Some("g2"));
    r0.sync(&r1);
    assert_eq!(
        r0.violations("i3"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    assert!(r0.violations("g2").is_empty());
    r0.move_to("i3", None);
    assert!(r0.view().is_valid());
}
