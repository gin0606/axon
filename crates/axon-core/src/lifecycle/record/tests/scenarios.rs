//! The `run` tests of `spec/record_integration_test.qnt` on conflicts, gaps, terminal cycles,
//! mutual Completed dependencies, duplicate creation and gap conflicts, in the same order.
//! The runs on waivers, the ancestor chain and cycles are covered in `operations` and
//! `witnesses`.
use super::*;
use Operation::*;

/// withdrawOverWorkingGrandchildScenario: while a grandchild Issue is InProgress, the
/// grandparent Group cannot be withdrawn.
#[test]
fn withdraw_over_working_grandchild_is_rejected() {
    let mut r0 = Replica::new("r0");
    r0.move_to("g2", Some("g0"));
    r0.move_to("i1", Some("g2"));
    r0.op("i1", Start);
    let view = r0.view();
    assert!(view.is_valid());
    assert_eq!(
        view.effective_lifecycle(&id("g2")),
        Some(Lifecycle::InProgress)
    );
    assert_eq!(
        view.effective_lifecycle(&id("g0")),
        Some(Lifecycle::InProgress)
    );
    assert!(error(view.check_operation(&id("g0"), Withdraw)).contains("cannot be withdrawn"));
    assert!(error(r0.try_op("g0", Withdraw)).contains("cannot be withdrawn"));
}

/// resolveArrivesBeforeParentScenario: both replicas start Issue 1 as different actors;
/// replica 0 takes replica 1's record and resolves for its own. Replica 1 then receives only
/// the resolve record: the chosen parent is missing (a gap), yet the resolve record carries
/// the value, so the Entity is settled with replica 0's owner.
#[test]
fn resolve_record_arriving_before_its_parent_still_settles() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let start0 = r0.op_as("i1", Start, "actor-0");
    r1.op_as("i1", Start, "actor-1");
    r0.sync(&r1);
    assert!(r0.view().is_conflicted(&id("i1")));
    let resolve = r0.resolve("i1", &start0);
    r1.sync_one(&r0, &resolve);
    let view = r1.view();
    assert_eq!(view.gaps().get(&id("i1")), Some(&BTreeSet::from([start0])));
    assert!(view.is_settled(&id("i1")));
    assert_eq!(
        view.current(&id("i1")).unwrap().owner.as_deref(),
        Some("actor-0")
    );
    assert_eq!(lifecycle(&view, "i1"), Lifecycle::InProgress);
    assert!(view.conflicted().is_empty());
}

/// terminalCycleRepairScenario: a containment cycle between two terminal Groups. An Entity in
/// violation can be detached even though its parent is terminal.
#[test]
fn terminal_group_cycle_is_repaired_by_detaching() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("g2", Complete);
    r0.move_to("g2", Some("g0"));
    r1.op("i1", Cancel);
    r1.move_to("g0", Some("g2"));
    r1.op("g0", Complete);
    r1.sync(&r0);
    let view = r1.view();
    assert!(view.conflicted().is_empty());
    assert!(!view.is_valid());
    assert!(
        r1.violations("g0")
            .contains(&ViolationKind::ContainmentCycle)
    );
    assert!(
        r1.violations("g2")
            .contains(&ViolationKind::ContainmentCycle)
    );
    // The waiver frees g0 from its terminal parent, not the destination rule: moving under
    // a terminal Group elsewhere stays rejected; detaching is the repair.
    r1.create("g5", Kind::Group, Lifecycle::NotStarted, None);
    r1.op("g5", Complete);
    assert!(error(r1.try_move("g0", Some("g5"))).contains("unfinished Group"));
    r1.move_to("g0", None);
    assert!(r1.view().is_valid());
    assert_eq!(r1.current("g2").parent, Some(id("g0")));
}

/// mutualCompletedDepsRepairScenario: resolving three conflicts can leave two Completed
/// Entities depending on each other; the dependency of an Entity in violation can be removed
/// although it is Completed. Issue 1 cancelled on both sides with equal values still conflicts.
#[test]
fn mutual_completed_dependencies_are_repaired_by_removing_one() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Cancel);
    r0.op("g2", Complete);
    r0.add_dep("g0", "g2");
    let g0_completed = r0.op("g0", Complete);
    let i1_cancelled = r1.op("i1", Cancel);
    r1.op("g0", Complete);
    r1.add_dep("g2", "g0");
    let g2_completed = r1.op("g2", Complete);
    r1.sync(&r0);
    assert_eq!(
        r1.view().conflicted(),
        &BTreeSet::from([id("g0"), id("i1"), id("g2")])
    );
    assert!(error(r1.try_op("i3", Start)).contains("conflicted Entities block"));
    r1.resolve("i1", &i1_cancelled);
    r1.resolve("g0", &g0_completed);
    r1.resolve("g2", &g2_completed);
    let view = r1.view();
    assert!(view.conflicted().is_empty());
    assert!(
        r1.violations("g0")
            .contains(&ViolationKind::CompletionCycle)
    );
    assert!(
        r1.violations("g2")
            .contains(&ViolationKind::CompletionCycle)
    );
    assert!(view.in_violation(&id("g0")));
    r1.remove_dep("g0", "g2");
    assert!(r1.view().is_valid());
    assert_eq!(lifecycle(&r1.view(), "g0"), Lifecycle::Completed);
}

/// duplicateCreationResolvedScenario: the same ID registered on both sides conflicts, and one
/// lineage is chosen.
#[test]
fn duplicate_creation_is_resolved_by_choosing_one_lineage() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let created0 = r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.create("i4", Kind::Issue, Lifecycle::Undecided, Some("g2"));
    r0.sync(&r1);
    assert!(r0.view().is_conflicted(&id("i4")));
    assert_eq!(r0.heads("i4").len(), 2);
    r0.resolve("i4", &created0);
    let view = r0.view();
    assert!(view.is_settled(&id("i4")));
    assert_eq!(lifecycle(&view, "i4"), Lifecycle::NotStarted);
    assert_eq!(view.current(&id("i4")).unwrap().parent, None);
    assert!(view.is_valid());
}

/// gapConflictScenario: cherry-picking only the Complete of Start → Complete puts it beside
/// the base creation record as a second head. Choosing the newer one is right, and the value
/// does not change when the Start arrives later.
#[test]
fn gap_looks_like_a_conflict_and_resolves_to_the_newer_value() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let start = r0.op("i3", Start);
    let complete = r0.op("i3", Complete);
    r1.sync_one(&r0, &complete);
    let view = r1.view();
    assert!(view.is_conflicted(&id("i3")));
    assert_eq!(
        view.gaps().get(&id("i3")),
        Some(&BTreeSet::from([start.clone()]))
    );
    // The two heads are really ancestor and descendant; the set cannot tell.
    let heads = r1.heads("i3");
    let base = heads.iter().find(|h| **h != complete).unwrap();
    assert!(!r1.store.precedes(base, &complete));
    r1.resolve("i3", &complete);
    assert_eq!(lifecycle(&r1.view(), "i3"), Lifecycle::Completed);
    r1.sync(&r0);
    let view = r1.view();
    assert!(view.is_settled(&id("i3")));
    assert_eq!(lifecycle(&view, "i3"), Lifecycle::Completed);
    assert!(view.gaps().is_empty());
    assert!(r1.store.precedes(&start, &complete));
}
