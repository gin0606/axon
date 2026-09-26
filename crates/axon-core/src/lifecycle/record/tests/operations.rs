//! Ordinary operations: conflicts stop them, violations are never added (lifecycle
//! operations included), waivers apply only inside a violation and never let an operation add
//! a violation, and each success is exactly one record.
use super::*;
use Operation::*;

fn conflicted_replica() -> Replica {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op_as("i1", Start, "a");
    r1.op_as("i1", Start, "b");
    r0.sync(&r1);
    r0
}

#[test]
fn every_operation_except_resolve_and_note_is_rejected_while_any_entity_is_conflicted() {
    let mut r = conflicted_replica();
    let blocked = "conflicted Entities block";
    assert!(error(r.try_op("i3", Start)).contains(blocked));
    assert!(
        error(r.try_create("i4", current(Kind::Issue, Lifecycle::NotStarted, None)))
            .contains(blocked)
    );
    assert!(error(r.store.write(&id("i3"), Some("x".into()), None, r.tick())).contains(blocked));
    assert!(error(r.try_move("i3", Some("g2"))).contains(blocked));
    assert!(error(r.try_add_dep("i3", "g2")).contains(blocked));
    assert!(error(r.try_remove_dep("i3", "g2")).contains(blocked));
    assert!(
        error(
            r.store
                .set_condition(&id("i3"), Some("true".into()), r.tick())
        )
        .contains(blocked)
    );
    assert!(error(r.store.convert(&id("i3"), Kind::Group, r.tick())).contains(blocked));
    assert!(
        error(r.store.import(
            &id("i3"),
            "t".into(),
            "d".into(),
            None,
            BTreeSet::new(),
            r.tick()
        ))
        .contains(blocked)
    );
    // The conflicted Entity itself is described as such by the read-side check.
    assert!(error(r.view().check_operation(&id("i1"), Release)).contains("conflicted"));
    // Notes and resolution proceed.
    r.note("i3", "waiting");
    r.note("i1", "conflicted but noted");
    let head = r.heads("i1").first().unwrap().clone();
    r.resolve("i1", &head);
    assert!(r.view().is_valid());
    r.op("i3", Start);
}

#[test]
fn resolve_requires_a_conflicted_entity_and_one_of_its_heads() {
    let mut r = conflicted_replica();
    let foreign = r.head("i3");
    assert!(error(r.store.resolve(&id("i1"), &foreign, None, r.tick())).contains("not a head"));
    assert!(error(r.store.resolve(&id("i3"), &foreign, None, r.tick())).contains("not conflicted"));
    assert!(error(r.store.resolve(&id("nope"), &foreign, None, r.tick())).contains("missing"));
    let head = r.heads("i1").first().unwrap().clone();
    let resolve = r.resolve("i1", &head);
    assert_eq!(
        r.store.record(&resolve).unwrap().reason.as_deref(),
        Some("pick")
    );
    // Resolution with parents that are really ancestor and descendant (a gap) is accepted.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i3", Start);
    let complete = r0.op("i3", Complete);
    r1.sync_one(&r0, &complete);
    r1.resolve("i3", &complete);
    r1.sync(&r0);
    assert!(r1.view().is_valid());
}

#[test]
fn moves_dependencies_and_registrations_that_add_a_violation_are_rejected() {
    let mut r = Replica::new("r0");
    // A completion cycle through containment: i3 depends on g0, then moves under g0.
    r.add_dep("i3", "g0");
    assert!(error(r.try_move("i3", Some("g0"))).contains("completion cycle"));
    r.remove_dep("i3", "g0");
    // Direct and indirect dependency cycles.
    r.add_dep("i1", "i3");
    assert!(error(r.try_add_dep("i3", "i1")).contains("completion cycle"));
    assert!(error(r.try_add_dep("i3", "g0")).contains("completion cycle"));
    assert!(error(r.try_add_dep("g0", "i1")).contains("descendant"));
    assert!(error(r.try_add_dep("i1", "g0")).contains("ancestor"));
    assert!(error(r.try_add_dep("i1", "i1")).contains("itself"));
    // Containment cycles and self-containment.
    r.move_to("g2", Some("g0"));
    assert!(error(r.try_move("g0", Some("g2"))).contains("containment cycle"));
    assert!(error(r.try_move("g0", Some("g0"))).contains("itself"));
    assert!(error(r.try_move("i3", Some("i1"))).contains("unfinished Group"));
    // Registration under a terminal Group, with an unknown dependency, or twice.
    r.op("i1", Cancel);
    r.move_to("g2", None);
    r.op("g0", Complete);
    assert!(
        error(r.try_create(
            "i4",
            current(Kind::Issue, Lifecycle::NotStarted, Some("g0"))
        ))
        .contains("unfinished Group")
    );
    let mut needs_unknown = current(Kind::Issue, Lifecycle::NotStarted, None);
    needs_unknown.needs.insert(id("ghost"));
    assert!(error(r.try_create("i4", needs_unknown)).contains("missing Entity ghost"));
    assert!(
        error(r.try_create("i3", current(Kind::Issue, Lifecycle::NotStarted, None)))
            .contains("duplicate")
    );
    assert!(
        error(r.try_create("i4", current(Kind::Issue, Lifecycle::InProgress, None)))
            .contains("Undecided or NotStarted")
    );
    // A registration that is fine adds no violation and one record.
    let before = r.store.len();
    r.create("i4", Kind::Issue, Lifecycle::Undecided, Some("g2"));
    assert_eq!(r.store.len(), before + 1);
    assert!(r.view().is_valid());
}

#[test]
fn waivers_work_only_for_entities_inside_a_violation() {
    // Valid store: Completed dependencies are fixed, terminal parents fix their children,
    // Completed dependents block Reopen.
    let mut r = Replica::new("r0");
    r.op("g2", Complete);
    r.add_dep("i3", "g2");
    r.op("i3", Start);
    r.op("i3", Complete);
    assert!(error(r.try_remove_dep("i3", "g2")).contains("fixed"));
    assert!(error(r.try_add_dep("i3", "i1")).contains("fixed"));
    assert!(error(r.try_op("g2", Reopen)).contains("i3"));
    r.op("i1", Cancel);
    r.op("g0", Complete);
    assert!(error(r.try_move("i1", None)).contains("unfinished Group"));
    assert!(error(r.try_op("i1", Reconsider)).contains("unfinished Group"));
    assert!(r.view().is_valid());
    // Inside a violation the three rules are waived only where the result adds no violation,
    // and adding a dependency to a Completed Entity is never waived.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Cancel);
    r0.op("g2", Complete);
    r0.add_dep("g0", "g2");
    let g0 = r0.op("g0", Complete);
    let i1 = r1.op("i1", Cancel);
    r1.op("g0", Complete);
    r1.add_dep("g2", "g0");
    let g2 = r1.op("g2", Complete);
    r1.sync(&r0);
    r1.resolve("i1", &i1);
    r1.resolve("g0", &g0);
    r1.resolve("g2", &g2);
    assert!(r1.view().in_violation(&id("g0")));
    assert!(error(r1.try_add_dep("g0", "i3")).contains("fixed"));
    // i1 is on the cycle (g0 waits for its child i1, i1 for g0's dependency g2, g2 for g0), so
    // it is in violation too; i3 is not and stays where the ordinary rules put it.
    assert_eq!(
        r1.violations("i1"),
        BTreeSet::from([ViolationKind::CompletionCycle])
    );
    assert!(r1.violations("i3").is_empty());
    // A waiver never lets a lifecycle operation add a violation, and the read-side check
    // agrees with the writer: Reconsider would leave an unfinished Issue under the Completed
    // g0, and reopening either Group would leave the other Completed with an unfinished
    // dependency it does not have yet.
    for (entity, operation, message) in [
        ("i1", Reconsider, "unfinished Group"),
        ("g0", Reopen, "reopened first"),
        ("g2", Reopen, "reopened first"),
    ] {
        let read = error(r1.view().check_operation(&id(entity), operation));
        assert!(read.contains(message), "{entity}: {read}");
        let write = error(r1.try_op(entity, operation));
        assert!(write.contains(message), "{entity}: {write}");
    }
    // The repair reduces the violations: a Completed Entity in violation drops a dependency.
    r1.remove_dep("g0", "g2");
    assert!(r1.view().is_valid());
    assert_eq!(lifecycle(&r1.view(), "g0"), Lifecycle::Completed);
    assert_eq!(lifecycle(&r1.view(), "i1"), Lifecycle::Cancelled);
}

/// The waivers still let lifecycle operations repair, as long as the result adds no
/// violation: under a terminal parent an unfinished Entity in violation can change while it
/// stays unfinished and can end, and a Completed dependency whose Completed dependents
/// already have an unfinished dependency can be reopened.
#[test]
fn waived_lifecycle_operations_that_add_no_violation_succeed() {
    // Terminal parent: i4 flowed under the Completed g0 on the other side.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Cancel);
    r0.op("g0", Complete);
    r1.create("i4", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    r1.sync(&r0);
    assert_eq!(
        r1.violations("i4"),
        BTreeSet::from([ViolationKind::OpenUnderTerminal])
    );
    r1.view().check_operation(&id("i4"), Withdraw).unwrap();
    r1.op("i4", Withdraw);
    r1.op("i4", Accept);
    assert_eq!(
        r1.violations("i4"),
        BTreeSet::from([ViolationKind::OpenUnderTerminal])
    );
    r1.op("i4", Cancel);
    assert!(r1.view().is_valid());
    // Completed dependents: i3 depends on g2, i4 on i3 and g2; one side completes i3 and i4,
    // the other reopens g2. Both are Completed with an unfinished dependency, so reopening
    // i3 despite its Completed dependent i4 adds nothing, and reopening i4 repairs the rest.
    let mut shared = Replica::new("r0");
    shared.op("g2", Complete);
    shared.add_dep("i3", "g2");
    let mut i4 = current(Kind::Issue, Lifecycle::NotStarted, None);
    i4.needs.extend([id("i3"), id("g2")]);
    let record = shared.try_create("i4", i4).unwrap();
    insert(&mut shared.store, record);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    r0.op("i3", Start);
    r0.op("i3", Complete);
    r0.op("i4", Start);
    r0.op("i4", Complete);
    r1.op("g2", Reopen);
    r0.sync(&r1);
    for completed in ["i3", "i4"] {
        assert_eq!(
            r0.violations(completed),
            BTreeSet::from([ViolationKind::CompletedWithOpenDependency]),
            "{completed}"
        );
    }
    assert_eq!(r0.view().completed_dependents(&id("i3")), vec![&id("i4")]);
    r0.view().check_operation(&id("i3"), Reopen).unwrap();
    r0.op("i3", Reopen);
    assert_eq!(
        r0.violations("i4"),
        BTreeSet::from([ViolationKind::CompletedWithOpenDependency])
    );
    assert!(r0.violations("i3").is_empty());
    r0.op("i4", Reopen);
    assert!(r0.view().is_valid());
    // With a Completed dependent that has no unfinished dependency yet (i5 depends on i3
    // only), reopening i3 would add a violation to i5, so the read side and the writer both
    // reject it until i5 is reopened.
    let mut shared = Replica::new("r0");
    shared.op("g2", Complete);
    shared.add_dep("i3", "g2");
    let mut i4 = current(Kind::Issue, Lifecycle::NotStarted, None);
    i4.needs.extend([id("i3"), id("g2")]);
    let record = shared.try_create("i4", i4).unwrap();
    insert(&mut shared.store, record);
    let mut i5 = current(Kind::Issue, Lifecycle::NotStarted, None);
    i5.needs.insert(id("i3"));
    let record = shared.try_create("i5", i5).unwrap();
    insert(&mut shared.store, record);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    for done in ["i3", "i4", "i5"] {
        r0.op(done, Start);
        r0.op(done, Complete);
    }
    r1.op("g2", Reopen);
    r0.sync(&r1);
    let before = r0.view().violations().clone();
    assert!(r0.view().in_violation(&id("i3")) && r0.view().in_violation(&id("i4")));
    assert!(!r0.view().in_violation(&id("i5")));
    assert!(error(r0.view().check_operation(&id("i3"), Reopen)).contains("reopened first"));
    assert!(error(r0.try_op("i3", Reopen)).contains("reopened first"));
    assert_eq!(r0.view().violations(), &before);
    r0.op("i5", Reopen);
    r0.op("i3", Reopen);
    r0.op("i4", Reopen);
    assert!(r0.view().is_valid());
    // The waiver reaches only an Entity in violation: i3 is Completed with nothing wrong, and
    // its Completed dependent i4 already has an unfinished dependency, yet i3 stays fixed.
    let mut shared = Replica::new("r0");
    shared.op("g2", Complete);
    let mut i4 = current(Kind::Issue, Lifecycle::NotStarted, None);
    i4.needs.extend([id("i3"), id("g2")]);
    let record = shared.try_create("i4", i4).unwrap();
    insert(&mut shared.store, record);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    for done in ["i3", "i4"] {
        r0.op(done, Start);
        r0.op(done, Complete);
    }
    r1.op("g2", Reopen);
    r0.sync(&r1);
    assert!(!r0.view().in_violation(&id("i3")));
    assert_eq!(
        r0.violations("i4"),
        BTreeSet::from([ViolationKind::CompletedWithOpenDependency])
    );
    assert!(error(r0.view().check_operation(&id("i3"), Reopen)).contains("reopened first"));
    assert!(error(r0.try_op("i3", Reopen)).contains("reopened first"));
    // A missing parent (the Cancel arrived without the Group's records) blocks nothing beyond
    // what the waiver allows: Reconsider adds no violation and passes.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let created = r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    let cancelled = r0.op("i5", Cancel);
    r1.sync_one(&r0, &created);
    r1.sync_one(&r0, &cancelled);
    assert_eq!(
        r1.violations("i5"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    r1.view().check_operation(&id("i5"), Reconsider).unwrap();
    r1.op("i5", Reconsider);
    assert_eq!(
        r1.violations("i5"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    r1.sync(&r0);
    assert!(r1.view().is_valid());
    // An Issue that gained children through an integration (converted on one side, given a
    // child on the other) cannot end while the child is unfinished: the read side anticipates
    // the violation the writer would reject.
    let mut shared = Replica::new("r0");
    shared.create("g9", Kind::Group, Lifecycle::NotStarted, None);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    let converted = r0
        .store
        .convert(&id("g9"), Kind::Issue, r0.tick())
        .unwrap()
        .unwrap();
    insert(&mut r0.store, converted);
    r1.create("c", Kind::Issue, Lifecycle::NotStarted, Some("g9"));
    r0.sync(&r1);
    assert_eq!(r0.current("g9").kind, Kind::Issue);
    assert_eq!(
        r0.violations("c"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    assert!(error(r0.view().check_operation(&id("g9"), Cancel)).contains("children"));
    assert!(error(r0.try_op("g9", Cancel)).contains("children"));
    // The writer's whole check is the rule and rejects what the read side does not
    // anticipate: with the child started, starting or withdrawing the Issue g9 would put
    // InProgress work under an unadopted ancestor.
    r0.op("c", Start);
    let before = r0.view().violations().clone();
    for operation in [Start, Withdraw] {
        r0.view().check_operation(&id("g9"), operation).unwrap();
        let rejected = error(r0.try_op("g9", operation));
        assert!(
            rejected.contains("would add a structural violation"),
            "{rejected}"
        );
        assert!(rejected.contains("c (unadopted ancestor)"), "{rejected}");
    }
    assert_eq!(r0.view().violations(), &before);
    r0.op("c", Release);
    r0.op("g9", Start);
    assert!(error(r0.view().check_operation(&id("g9"), Complete)).contains("children"));
    assert!(error(r0.try_op("g9", Complete)).contains("children"));
    r0.move_to("c", None);
    r0.op("g9", Complete);
    assert!(r0.view().is_valid());
}

/// A completion cycle is attributed only to the Entities on it. Entities that merely wait
/// on the cycle are neither in violation nor waived, a new cycle among them is rejected
/// because it adds members, and additions on the waiting side that close no cycle pass.
#[test]
fn a_new_cycle_among_entities_waiting_on_a_cycle_is_rejected() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r0.add_dep("i4", "g2");
    r0.add_dep("g0", "g2");
    r0.add_dep("g2", "i3");
    r1.add_dep("i3", "g2");
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.conflicted().is_empty());
    let cycle: BTreeSet<Violation> = ["g2", "i3"]
        .into_iter()
        .map(|member| Violation {
            entity: id(member),
            kind: ViolationKind::CompletionCycle,
        })
        .collect();
    assert_eq!(view.violations(), &cycle);
    for waiting in ["g0", "i1", "i4"] {
        assert!(!view.in_violation(&id(waiting)), "{waiting}");
    }
    // g0 waits for i4 as well: no new cycle. i4 waiting for g0 would close one through g0's
    // child i1 too, and the three new members are named.
    r0.add_dep("g0", "i4");
    assert_eq!(r0.view().violations(), &cycle);
    let rejected = error(r0.try_add_dep("i4", "g0"));
    for named in [
        "g0 (completion cycle)",
        "i1 (completion cycle)",
        "i4 (completion cycle)",
    ] {
        assert!(rejected.contains(named), "{rejected}");
    }
    // Registration under the waiting Group and a dependency between waiting Entities pass.
    r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    r0.add_dep("i1", "i4");
    assert_eq!(r0.view().violations(), &cycle);
    // Repairing the original cycle leaves the store valid: nothing else added a violation.
    r0.remove_dep("g2", "i3");
    assert!(r0.view().is_valid());
    // An ancestor depending on its own descendant (a move on one side, a dependency on the
    // other) is a cycle of one Entity: the descendant waits for itself through its ancestor's
    // dependency. The ancestor only waits on it.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.move_to("i3", Some("g0"));
    r1.add_dep("g0", "i3");
    r0.sync(&r1);
    assert_eq!(
        r0.violations("i3"),
        BTreeSet::from([ViolationKind::CompletionCycle])
    );
    assert!(r0.violations("g0").is_empty());
    r0.remove_dep("g0", "i3");
    assert!(r0.view().is_valid());
}

fn cycle_members(names: &[&str]) -> BTreeSet<Violation> {
    names
        .iter()
        .map(|member| Violation {
            entity: id(member),
            kind: ViolationKind::CompletionCycle,
        })
        .collect()
}

#[test]
fn a_new_edge_on_a_cycle_is_rejected_even_between_its_members() {
    // g2 -> i3 -> i4 -> g2. A chord g2 -> i4 adds no member, yet it would stay a cycle
    // after i3 -> i4 is removed, so it is rejected. An Entity off the cycle may still wait on
    // it, and removing an edge repairs the store.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.sync(&r0);
    r0.add_dep("i3", "i4");
    r0.add_dep("g2", "i3");
    r1.add_dep("i4", "g2");
    r0.sync(&r1);
    let cycle = cycle_members(&["g2", "i3", "i4"]);
    assert_eq!(r0.view().violations(), &cycle);
    let rejected = error(r0.try_add_dep("g2", "i4"));
    assert!(rejected.contains("g2 waiting on i4"), "{rejected}");
    r0.add_dep("i1", "i4");
    assert_eq!(r0.view().violations(), &cycle);
    // Only relations the change adds count: i3 keeps its own dependency on the cycle and may
    // move under g0, whose new edges close nothing.
    r0.move_to("i3", Some("g0"));
    assert_eq!(r0.view().violations(), &cycle);
    r0.remove_dep("i3", "i4");
    assert!(r0.view().is_valid());

    // Two separate cycles, g0 <-> i4 (with g0's child i1 through g0's dependency) and
    // g2 <-> i3. g2 -> i4 closes nothing and passes; i4 -> g2 would then close a cycle
    // between members of both and is rejected. Removing the two original back edges leaves
    // no cycle behind.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.sync(&r0);
    r0.add_dep("g2", "i3");
    r0.add_dep("g0", "i4");
    r1.add_dep("i3", "g2");
    r1.add_dep("i4", "g0");
    r0.sync(&r1);
    let all = cycle_members(&["g0", "i1", "g2", "i3", "i4"]);
    assert_eq!(r0.view().violations(), &all);
    r0.add_dep("g2", "i4");
    assert_eq!(r0.view().violations(), &all);
    let rejected = error(r0.try_add_dep("i4", "g2"));
    assert!(rejected.contains("i4 waiting on g2"), "{rejected}");
    r0.remove_dep("i3", "g2");
    assert_eq!(r0.view().violations(), &cycle_members(&["g0", "i1", "i4"]));
    r0.remove_dep("i4", "g0");
    assert!(r0.view().is_valid());

    // A move is held to the same rule. On g2 -> child i3 -> i4 -> g2, moving i4 under g2
    // would make the parent wait on its new child on the cycle; i1, off the cycle, moves in.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.sync(&r0);
    r0.move_to("i3", Some("g2"));
    r0.add_dep("i3", "i4");
    r1.add_dep("i4", "g2");
    r0.sync(&r1);
    let cycle = cycle_members(&["g2", "i3", "i4"]);
    assert_eq!(r0.view().violations(), &cycle);
    let rejected = error(r0.try_move("i4", Some("g2")));
    assert!(rejected.contains("g2 waiting on i4"), "{rejected}");
    r0.move_to("i1", Some("g2"));
    assert_eq!(r0.view().violations(), &cycle);
    r0.remove_dep("i4", "g2");
    assert!(r0.view().is_valid());

    // Only a descendant's new edge closes the cycle: on i1 -> i4 -> i3 -> i1 with g2 waiting
    // on i3, moving g0 under g2 makes its child i1 inherit the dependency on i3.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.sync(&r0);
    r0.add_dep("i1", "i4");
    r0.add_dep("i4", "i3");
    r0.add_dep("g2", "i3");
    r1.add_dep("i3", "i1");
    r0.sync(&r1);
    assert_eq!(r0.view().violations(), &cycle_members(&["i1", "i3", "i4"]));
    let rejected = error(r0.try_move("g0", Some("g2")));
    assert!(rejected.contains("i1 waiting on i3"), "{rejected}");
    // A dependency of g0 on i3 closes nothing through g0 itself, only through its child.
    let rejected = error(r0.try_add_dep("g0", "i3"));
    assert!(rejected.contains("i1 waiting on i3"), "{rejected}");
    r0.remove_dep("i3", "i1");
    r0.move_to("g0", Some("g2"));
    assert!(r0.view().is_valid());

    // The edges count per relation, not per pair. On i1 -> i3 (inherited from g0) -> i1, a
    // dependency of i1 on i3 repeats the pair yet would keep the cycle after g0's is removed.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.add_dep("g0", "i3");
    r1.add_dep("i3", "i1");
    r0.sync(&r1);
    assert_eq!(r0.view().violations(), &cycle_members(&["i1", "i3"]));
    let rejected = error(r0.try_add_dep("i1", "i3"));
    assert!(rejected.contains("i1 waiting on i3"), "{rejected}");
    r0.remove_dep("g0", "i3");
    assert!(r0.view().is_valid());
    // The same for a move: i1 already waits on i3 on its own; under g2, which depends on i3,
    // it would inherit that edge too.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.add_dep("g2", "i3");
    r0.add_dep("i1", "i3");
    r1.add_dep("i3", "i1");
    r0.sync(&r1);
    assert_eq!(r0.view().violations(), &cycle_members(&["i1", "i3"]));
    let rejected = error(r0.try_move("i1", Some("g2")));
    assert!(rejected.contains("i1 waiting on i3"), "{rejected}");
    r0.remove_dep("i3", "i1");
    assert!(r0.view().is_valid());

    // A registration too: i6 arrived without its parent g5 and is on a cycle with i3. g5
    // registered with a dependency on i3 would make i6 inherit it; without it, it passes.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r1.create("g5", Kind::Group, Lifecycle::NotStarted, None);
    let child = r1.create("i6", Kind::Issue, Lifecycle::NotStarted, Some("g5"));
    let back = r1.add_dep("i3", "i6");
    r0.sync_one(&r1, &child);
    r0.add_dep("i6", "i3");
    r0.sync_one(&r1, &back);
    assert!(
        r0.view()
            .violations()
            .is_superset(&cycle_members(&["i6", "i3"]))
    );
    let mut value = current(Kind::Group, Lifecycle::NotStarted, None);
    value.needs.insert(id("i3"));
    let rejected = error(r0.try_create("g5", value));
    assert!(rejected.contains("i6 waiting on i3"), "{rejected}");
    r0.create("g5", Kind::Group, Lifecycle::NotStarted, None);
    r0.remove_dep("i3", "i6");
    assert!(r0.view().is_valid());

    // An ancestor both parent chains share adds no relation: i1 inherits g2's dependency on
    // i3 under g0 and still does under the sibling g4, so the move passes.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, Some("g2"));
    r0.move_to("g0", Some("g2"));
    r0.add_dep("g2", "i3");
    r1.add_dep("i3", "i1");
    r0.sync(&r1);
    assert_eq!(r0.view().violations(), &cycle_members(&["i1", "i3"]));
    r0.move_to("i1", Some("g4"));
    assert_eq!(r0.view().violations(), &cycle_members(&["i1", "i3"]));
    r0.remove_dep("i3", "i1");
    assert!(r0.view().is_valid());

    // A registration whose parent chain closes the cycle: i3 arrived under g4 without it, on
    // i1 <-> i3, and g2 depends on i1. g4 registered under g2 would make i3 inherit that
    // dependency; unassigned, it passes.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r1.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let moved = r1.move_to("i3", Some("g4"));
    let back = r1.add_dep("i3", "i1");
    r0.add_dep("i1", "i3");
    r0.add_dep("g2", "i1");
    r0.sync_one(&r1, &moved);
    r0.sync_one(&r1, &back);
    assert!(
        r0.view()
            .violations()
            .is_superset(&cycle_members(&["i1", "i3"]))
    );
    let rejected = error(r0.try_create(
        "g4",
        current(Kind::Group, Lifecycle::NotStarted, Some("g2")),
    ));
    assert!(rejected.contains("i3 waiting on i1"), "{rejected}");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    r0.remove_dep("i1", "i3");
    assert!(r0.view().is_valid());

    // A cycle of one: on i3 <-> i4 with g2 depending on i3, moving i3 under g2's child g0
    // makes i3 inherit its own dependency. No other new edge is on a cycle.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
    r1.sync(&r0);
    r0.move_to("g0", Some("g2"));
    r0.add_dep("g2", "i3");
    r0.add_dep("i3", "i4");
    r1.add_dep("i4", "i3");
    r0.sync(&r1);
    assert_eq!(r0.view().violations(), &cycle_members(&["i3", "i4"]));
    let rejected = error(r0.try_move("i3", Some("g0")));
    assert!(rejected.contains("i3 waiting on i3"), "{rejected}");
    r0.remove_dep("i4", "i3");
    assert!(r0.view().is_valid());
}

#[test]
fn lifecycle_prerequisites_follow_containment_and_dependencies() {
    let mut r = Replica::new("r0");
    r.create("g4", Kind::Group, Lifecycle::Undecided, None);
    r.move_to("g0", Some("g4"));
    assert!(error(r.try_op("i1", Start)).contains("adopted"));
    r.op("g4", Accept);
    r.add_dep("g0", "i3");
    assert!(error(r.try_op("i1", Start)).contains("ancestor g0"));
    r.add_dep("i1", "g2");
    r.op("i3", Start);
    r.op("i3", Complete);
    assert!(error(r.try_op("i1", Start)).contains("dependencies must be Completed"));
    r.op("g2", Complete);
    r.op("i1", Start);
    let view = r.view();
    assert_eq!(
        view.effective_lifecycle(&id("g4")),
        Some(Lifecycle::InProgress)
    );
    assert_eq!(
        view.current(&id("i1")).unwrap().owner.as_deref(),
        Some("r0")
    );
    assert!(error(r.try_op("g0", Complete)).contains("children must be terminal"));
    assert!(error(r.try_op("g4", Withdraw)).contains("cannot be withdrawn"));
    assert!(error(r.try_op("g0", Start)).contains("not started directly"));
    r.op("i1", Complete);
    assert_eq!(r.current("i1").owner, None);
    r.op("g0", Complete);
    assert!(error(r.try_op("i1", Reopen)).contains("unfinished Group"));
    r.op("g0", Reopen);
    assert!(r.view().working().contains(&id("g0")));
    r.op("i1", Reopen);
    r.op("i1", Withdraw);
    r.op("i1", Cancel);
    r.op("i1", Reconsider);
    assert!(r.view().is_valid());
    // Groups without children, or with only Cancelled children, complete from NotStarted;
    // an unfinished Issue moving under an Undecided Group is fine, InProgress work is not.
    r.op("i1", Accept);
    r.op("i1", Cancel);
    r.op("g0", Complete);
    r.op("g0", Reopen);
    r.create("g5", Kind::Group, Lifecycle::Undecided, None);
    r.move_to("i1", Some("g5"));
    assert!(error(r.try_move("i3", Some("g5"))).contains("adopted"));
    r.move_to("g0", Some("g5"));
    r.move_to("g0", None);
    r.op("g5", Cancel);
    assert!(error(r.try_op("g5", Complete)).contains("cannot Complete"));
}

#[test]
fn text_condition_and_import_records_carry_the_value_after_and_skip_no_ops() {
    let mut r = Replica::new("r0");
    let edit = r
        .store
        .write(&id("i3"), Some("renamed".into()), None, r.tick())
        .unwrap()
        .unwrap();
    assert_eq!(edit.kind, RecordKind::Edit);
    assert_eq!(edit.parents, BTreeSet::from([r.head("i3")]));
    insert(&mut r.store, edit);
    assert!(
        r.store
            .write(&id("i3"), Some("renamed".into()), None, r.tick())
            .unwrap()
            .is_none()
    );
    assert!(
        error(r.store.write(&id("i3"), Some(" ".into()), None, r.tick())).contains("empty title")
    );
    assert!(
        error(
            r.store
                .write(&id("i3"), Some("a\nb".into()), None, r.tick())
        )
        .contains("line break")
    );
    let condition = r
        .store
        .set_condition(&id("i3"), Some("exit 0".into()), r.tick())
        .unwrap()
        .unwrap();
    assert_eq!(condition.kind, RecordKind::Condition);
    insert(&mut r.store, condition);
    assert!(
        r.store
            .set_condition(&id("i3"), Some("exit 0".into()), r.tick())
            .unwrap()
            .is_none()
    );
    assert!(
        error(r.store.set_condition(&id("i3"), Some(" ".into()), r.tick()))
            .contains("empty condition")
    );
    r.op("i3", Start);
    r.op("i3", Complete);
    assert!(
        error(
            r.store
                .write(&id("i3"), Some("late".into()), None, r.tick())
        )
        .contains("fixed")
    );
    // Import: one record with the final value, validated as the sequence of operations.
    let mut r = Replica::new("r0");
    r.add_dep("i3", "g2");
    let import = r
        .store
        .import(
            &id("i3"),
            "imported".into(),
            "desc".into(),
            Some(id("g0")),
            BTreeSet::from([id("i1")]),
            r.tick(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(import.kind, RecordKind::Import);
    assert_eq!(import.parents, BTreeSet::from([r.head("i3")]));
    assert_eq!(import.after.title, "imported");
    assert_eq!(import.after.parent, Some(id("g0")));
    assert_eq!(import.after.needs, BTreeSet::from([id("i1")]));
    let count = r.store.len();
    insert(&mut r.store, import);
    assert_eq!(r.store.len(), count + 1);
    assert!(r.view().is_valid());
    assert!(
        r.store
            .import(
                &id("i3"),
                "imported".into(),
                "desc".into(),
                Some(id("g0")),
                BTreeSet::from([id("i1")]),
                r.tick()
            )
            .unwrap()
            .is_none()
    );
    // A dependency swap that would cycle in one order succeeds because removals precede the
    // move; a genuine cycle is still rejected.
    let mut r = Replica::new("r0");
    r.add_dep("i3", "g0");
    let swapped = r
        .store
        .import(
            &id("i3"),
            "task".into(),
            "body\n日本語".into(),
            Some(id("g0")),
            BTreeSet::new(),
            r.tick(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(swapped.after.parent, Some(id("g0")));
    assert!(
        error(r.store.import(
            &id("i3"),
            "task".into(),
            "body\n日本語".into(),
            Some(id("g0")),
            BTreeSet::from([id("g0")]),
            r.tick()
        ))
        .contains("completion cycle")
    );
}

#[test]
fn notes_need_a_known_entity_and_a_body_and_each_addition_is_a_new_record() {
    let mut r = Replica::new("r0");
    assert!(error(r.store.add_note(&id("nope"), "x".into(), None, r.tick())).contains("missing"));
    assert!(
        error(r.store.add_note(&id("i3"), " \n".into(), None, r.tick())).contains("empty Note")
    );
    let context = r.tick();
    let first = r
        .store
        .add_note(&id("i3"), "same".into(), None, context.clone())
        .unwrap();
    let second = r
        .store
        .add_note(&id("i3"), "same".into(), None, context)
        .unwrap();
    assert_ne!(first.nonce, second.nonce);
    let a = r.store.insert(Entry::Note(first)).unwrap();
    let b = r.store.insert(Entry::Note(second)).unwrap();
    assert_ne!(a, b);
    assert_eq!(r.store.notes_of(&id("i3")).len(), 2);
    let view = r.view();
    assert!(view.heads(&id("i3")).unwrap().len() == 1);
    assert!(view.noted_only().is_empty());
    // A Note whose Entity has no records is kept and reported, not rejected.
    let mut other = Store::new();
    other.insert(r.store.get(&a).unwrap().clone()).unwrap();
    let view = other.view().unwrap();
    assert_eq!(view.noted_only(), &BTreeSet::from([id("i3")]));
    assert!(!view.is_known(&id("i3")));
}

#[test]
fn a_rejected_operation_produces_no_record_and_a_success_exactly_one() {
    let mut r = Replica::new("r0");
    let before = r.store.len();
    assert!(r.try_op("g0", Start).is_err());
    assert!(r.try_move("i3", Some("i1")).is_err());
    assert!(r.try_add_dep("i1", "g0").is_err());
    r.op("i3", Start);
    assert_eq!(r.store.len(), before + 1);
}

#[test]
fn entity_ids_have_an_eight_character_random_part() {
    let generated = new_entity_id("demo").unwrap();
    let text = generated.to_string();
    let (prefix, suffix) = text.rsplit_once('-').unwrap();
    assert_eq!(prefix, "demo");
    assert_eq!(suffix.len(), 8);
    assert!(
        suffix
            .bytes()
            .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
    );
    assert_ne!(new_entity_id("demo").unwrap(), generated);
    assert!(error(new_entity_id("Demo")).contains("EntityId"));
}

/// The waivers reach only the Entities attributed a violation. With a violation elsewhere in
/// the store, an Entity outside it keeps the fixed dependencies of Completed work, the fixed
/// composition of a terminal parent and the Reopen block by a Completed dependent.
#[test]
fn a_violation_elsewhere_does_not_waive_the_rules_for_other_entities() {
    let mut shared = Replica::new("r0");
    shared.op("g2", Complete);
    shared.add_dep("i3", "g2");
    shared.op("i3", Start);
    shared.op("i3", Complete);
    shared.op("i1", Cancel);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    r0.op("g0", Complete);
    r1.create("i4", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    r1.sync(&r0);
    let view = r1.view();
    assert!(view.conflicted().is_empty());
    assert_eq!(
        r1.violations("i4"),
        BTreeSet::from([ViolationKind::OpenUnderTerminal])
    );
    for outside in ["i3", "i1", "g2", "g0"] {
        assert!(!view.in_violation(&id(outside)), "{outside}");
    }
    assert!(error(r1.try_remove_dep("i3", "g2")).contains("fixed"));
    assert!(error(r1.try_move("i1", None)).contains("unfinished Group"));
    assert!(error(r1.try_op("i1", Reconsider)).contains("unfinished Group"));
    assert!(error(r1.try_op("g2", Reopen)).contains("reopened first"));
    // The Entity in violation may leave its terminal parent, but only for an unfinished
    // Group or no parent: the destination rule is not waived.
    assert!(error(r1.try_move("i4", Some("g2"))).contains("unfinished Group"));
    assert!(error(r1.try_move("i4", Some("i3"))).contains("unfinished Group"));
    r1.move_to("i4", None);
    assert!(r1.view().is_valid());
}

#[test]
fn registration_and_conversion_never_add_a_violation() {
    let mut r = Replica::new("r0");
    r.move_to("g2", Some("g0"));
    // Depending on the containment line at registration is rejected by name.
    for ancestor in ["g2", "g0"] {
        let mut on_line = current(Kind::Issue, Lifecycle::NotStarted, Some("g2"));
        on_line.needs.insert(id(ancestor));
        assert!(error(r.try_create("i4", on_line)).contains("ancestor"));
    }
    // A cycle that runs outside the containment line is caught only by the whole check:
    // i3 waits for g0, and the new child of g0 would wait for i3.
    let mut r = Replica::new("r0");
    r.add_dep("i3", "g0");
    let mut through_child = current(Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    through_child.needs.insert(id("i3"));
    let rejected = error(r.try_create("i4", through_child));
    assert!(rejected.contains("completion cycle"), "{rejected}");
    assert!(r.view().is_valid());
    // Registering the missing parent of an InProgress orphan as an Undecided Group would put
    // work under an unadopted ancestor.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    let started = r0.op("i5", Start);
    r1.sync_one(&r0, &r0.store.history(&id("i5")).unwrap()[0].0.clone());
    r1.sync_one(&r0, &started);
    assert_eq!(
        r1.violations("i5"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    let rejected = error(r1.try_create("g4", current(Kind::Group, Lifecycle::Undecided, None)));
    assert!(rejected.contains("unadopted ancestor"), "{rejected}");
    r1.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    assert!(r1.view().is_valid());
    // A dependency that does not close a cycle registers and leaves the store valid.
    let mut r = Replica::new("r0");
    r.move_to("g2", Some("g0"));
    r.add_dep("g0", "i3");
    let mut fine = current(Kind::Issue, Lifecycle::NotStarted, Some("g2"));
    fine.needs.insert(id("i3"));
    let record = r.try_create("i4", fine).unwrap();
    insert(&mut r.store, record);
    assert!(r.view().is_valid());
    // Conversion: a child record arriving before its parent's conversion leaves the child
    // under an Issue. Converting that Issue would make it a working Group under an
    // unadopted ancestor, adding a violation, so the conversion is rejected until the
    // ancestor is adopted.
    let mut shared = Replica::new("r0");
    shared.create("u", Kind::Group, Lifecycle::NotStarted, None);
    shared.create("i", Kind::Issue, Lifecycle::NotStarted, Some("u"));
    shared.op("i3", Start);
    let mut r0 = Replica::from("r0", &shared.store);
    let mut r1 = Replica::from("r1", &shared.store);
    let convert = r0
        .store
        .convert(&id("i"), Kind::Group, r0.tick())
        .unwrap()
        .unwrap();
    let converted = insert(&mut r0.store, convert);
    let moved = r0.move_to("i3", Some("i"));
    r1.op("u", Withdraw);
    r1.sync_one(&r0, &moved);
    assert_eq!(
        r1.violations("i3"),
        BTreeSet::from([
            ViolationKind::UnknownParent,
            ViolationKind::UnadoptedAncestor
        ])
    );
    assert!(error(r1.store.convert(&id("i"), Kind::Group, r1.tick())).contains("violation"));
    r1.sync_one(&r0, &converted);
    assert_eq!(
        r1.violations("i3"),
        BTreeSet::from([ViolationKind::UnadoptedAncestor])
    );
    assert_eq!(
        r1.violations("i"),
        BTreeSet::from([ViolationKind::UnadoptedAncestor])
    );
    r1.op("u", Accept);
    assert!(r1.view().is_valid());
}

#[test]
fn a_start_with_a_blank_actor_is_rejected_before_the_record_exists() {
    let r = Replica::new("r0");
    let context = Context {
        at: r.tick().at,
        recorder: Some(Recorder {
            actor: " ".into(),
            data: BTreeMap::new(),
        }),
    };
    assert!(error(r.store.perform(&id("i3"), Start, None, context)).contains("owner"));
}

/// An unknown or conflicted parent or dependency never counts as adopted or Completed, at
/// any distance up the parent chain, and is reported neither as an unknown parent (when
/// conflicted) nor as an unfinished dependency (when unsettled).
#[test]
fn unsettled_references_fail_the_prerequisites_without_being_misreported() {
    // Unknown dependency and unknown parent, through a cherry-pick without the registrations.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let child = r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    let dependent = r0.add_dep("i3", "g4");
    r1.sync_one(&r0, &child);
    r1.sync_one(&r0, &dependent);
    assert!(error(r1.try_op("i3", Start)).contains("dependencies must be Completed"));
    assert!(error(r1.try_op("i5", Start)).contains("adopted"));
    // A missing root two levels up: g4 under a root whose records never arrived.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("root", Kind::Group, Lifecycle::Undecided, None);
    let g4 = r0.create("g4", Kind::Group, Lifecycle::NotStarted, Some("root"));
    let i5 = r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    r1.sync_one(&r0, &g4);
    r1.sync_one(&r0, &i5);
    assert_eq!(
        r1.violations("g4"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    assert!(r1.violations("i5").is_empty());
    assert!(error(r1.try_op("i5", Start)).contains("adopted"));
    assert!(error(r1.view().check_operation(&id("i5"), Start)).contains("adopted"));
    // Started work does not move under a Group whose chain is broken either, until the
    // missing records arrive.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let moved = r0.move_to("g0", Some("g4"));
    r1.op("i3", Start);
    r1.sync_one(&r0, &moved);
    assert_eq!(
        r1.violations("g0"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    assert!(error(r1.try_move("i3", Some("g0"))).contains("adopted"));
    r1.sync(&r0);
    r1.move_to("i3", Some("g0"));
    assert!(r1.view().is_valid());
    // Complete, Reopen and the Accept of a Group with finished work below it fail the same
    // way under the broken chain: i1 is InProgress, i4 Completed and the Undecided g5 holds
    // the Completed i6, all under g0, whose parent record is missing on r1.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Start);
    r0.create("i4", Kind::Issue, Lifecycle::NotStarted, Some("g0"));
    r0.op("i4", Start);
    r0.op("i4", Complete);
    r0.create("g5", Kind::Group, Lifecycle::NotStarted, Some("g0"));
    r0.create("i6", Kind::Issue, Lifecycle::NotStarted, Some("g5"));
    r0.op("i6", Start);
    r0.op("i6", Complete);
    r0.op("g5", Cancel);
    r0.op("g5", Reconsider);
    r1.sync(&r0);
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, None);
    let moved = r0.move_to("g0", Some("g4"));
    r1.sync_one(&r0, &moved);
    assert_eq!(
        r1.violations("g0"),
        BTreeSet::from([ViolationKind::UnknownParent])
    );
    for (entity, operation) in [("i1", Complete), ("i4", Reopen), ("g5", Accept)] {
        let read = error(r1.view().check_operation(&id(entity), operation));
        assert!(read.contains("adopted"), "{entity}: {read}");
        let write = error(r1.try_op(entity, operation));
        assert!(write.contains("adopted"), "{entity}: {write}");
    }
    r1.sync(&r0);
    r1.op("i1", Complete);
    r1.op("i4", Reopen);
    r1.op("g5", Accept);
    assert!(r1.view().is_valid());
    // A conflicted grandparent: the read-side check fails and the parent is not "unknown".
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.create("root", Kind::Group, Lifecycle::NotStarted, None);
    r0.create("g4", Kind::Group, Lifecycle::NotStarted, Some("root"));
    r0.create("i5", Kind::Issue, Lifecycle::NotStarted, Some("g4"));
    r0.op("g2", Complete);
    r1.sync(&r0);
    r0.op("root", Withdraw);
    r1.op("root", Withdraw);
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.is_conflicted(&id("root")));
    assert!(error(view.check_operation(&id("i5"), Start)).contains("adopted"));
    assert!(!view.ancestors_adopted(&id("g4")));
    assert!(r0.violations("g4").is_empty());
    assert!(!view.is_valid());
    // A conflicted dependency is neither Completed nor an unfinished dependency.
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("g2", Complete);
    r0.add_dep("i3", "g2");
    r0.add_dep("i1", "g2");
    r0.op("i3", Start);
    r0.op("i3", Complete);
    r1.sync(&r0);
    r0.move_to("g2", Some("g0"));
    r1.move_to("g2", Some("g0"));
    r0.sync(&r1);
    let view = r0.view();
    assert!(view.is_conflicted(&id("g2")));
    assert!(!view.dependencies_completed(&id("i3")));
    assert!(r0.violations("i3").is_empty());
    assert!(
        error(view.check_operation(&id("i1"), Start)).contains("dependencies must be Completed")
    );
}
