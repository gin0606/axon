use super::*;
use Operation::*;
use proptest::prelude::*;

fn reference_heads_and_gaps(
    store: &Store,
) -> (
    BTreeMap<EntityId, BTreeSet<RecordId>>,
    BTreeMap<EntityId, BTreeSet<RecordId>>,
) {
    let mut records: BTreeMap<EntityId, BTreeSet<RecordId>> = BTreeMap::new();
    let mut referenced: BTreeMap<EntityId, BTreeSet<RecordId>> = BTreeMap::new();
    let mut gaps: BTreeMap<EntityId, BTreeSet<RecordId>> = BTreeMap::new();
    for (record_id, record) in store.records() {
        records
            .entry(record.entity.clone())
            .or_default()
            .insert(record_id.clone());
        for parent in &record.parents {
            if store.record(parent).is_some() {
                referenced
                    .entry(record.entity.clone())
                    .or_default()
                    .insert(parent.clone());
            } else {
                gaps.entry(record.entity.clone())
                    .or_default()
                    .insert(parent.clone());
            }
        }
    }
    let heads = records
        .into_iter()
        .map(|(entity, mut ids)| {
            if let Some(parents) = referenced.get(&entity) {
                ids.retain(|record| !parents.contains(record));
            }
            (entity, ids)
        })
        .collect();
    (heads, gaps)
}

// Enumerate short paths directly; generated stores have at most seven settled Entities.
fn reference_cycle_members(view: &View) -> BTreeSet<EntityId> {
    let settled: BTreeSet<_> = view.settled().map(|(entity, _)| entity.clone()).collect();
    let mut edges: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
    for entity in &settled {
        let mut needs = BTreeSet::new();
        for child in &settled {
            if view.current(child).unwrap().parent.as_ref() == Some(entity) {
                needs.insert(child.clone());
            }
        }
        let mut chain = vec![entity.clone()];
        let mut seen = BTreeSet::new();
        while let Some(next) = chain.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            let current = view.current(&next).unwrap();
            needs.extend(
                current
                    .needs
                    .iter()
                    .filter(|need| settled.contains(*need))
                    .cloned(),
            );
            if let Some(parent) = &current.parent
                && settled.contains(parent)
            {
                chain.push(parent.clone());
            }
        }
        edges.insert(entity.clone(), needs);
    }
    settled
        .into_iter()
        .filter(|start| {
            let mut frontier: Vec<_> = edges[start].iter().cloned().collect();
            let mut visited = BTreeSet::new();
            while let Some(next) = frontier.pop() {
                if &next == start {
                    return true;
                }
                if visited.insert(next.clone()) {
                    frontier.extend(edges[&next].iter().cloned());
                }
            }
            false
        })
        .collect()
}

fn reference_violations(view: &View) -> BTreeSet<Violation> {
    let current: BTreeMap<_, _> = view
        .settled()
        .map(|(entity, value)| (entity.clone(), &value.current))
        .collect();
    let mut working = BTreeSet::new();
    loop {
        let next: BTreeSet<_> = current
            .iter()
            .filter(|(group, value)| {
                value.kind == Kind::Group
                    && value.lifecycle == Lifecycle::NotStarted
                    && current.iter().any(|(child, child_value)| {
                        child_value.parent.as_ref() == Some(group)
                            && (matches!(
                                child_value.lifecycle,
                                Lifecycle::InProgress | Lifecycle::Completed
                            ) || working.contains(child))
                    })
            })
            .map(|(entity, _)| entity.clone())
            .collect();
        if next == working {
            break;
        }
        working = next;
    }
    let mut result = BTreeSet::new();
    let mut add = |entity: &EntityId, kind| {
        result.insert(Violation {
            entity: entity.clone(),
            kind,
        });
    };
    for (entity, value) in &current {
        let mut ancestors = BTreeSet::new();
        let mut parent = value.parent.as_ref();
        while let Some(next) = parent {
            if !current.contains_key(next) || !ancestors.insert(next.clone()) {
                break;
            }
            parent = current[next].parent.as_ref();
        }
        if ancestors.contains(entity) {
            add(entity, ViolationKind::ContainmentCycle);
        }
        if let Some(parent) = &value.parent {
            if !view.is_known(parent) || current.get(parent).is_some_and(|p| p.kind != Kind::Group)
            {
                add(entity, ViolationKind::UnknownParent);
            }
            if current.get(parent).is_some_and(|p| p.is_terminal()) && !value.is_terminal() {
                add(entity, ViolationKind::OpenUnderTerminal);
            }
        }
        if (value.lifecycle == Lifecycle::InProgress || working.contains(entity))
            && ancestors
                .iter()
                .any(|ancestor| current[ancestor].lifecycle != Lifecycle::NotStarted)
        {
            add(entity, ViolationKind::UnadoptedAncestor);
        }
        if value.lifecycle == Lifecycle::Completed
            && value.needs.iter().any(|need| {
                current
                    .get(need)
                    .is_some_and(|dependency| dependency.lifecycle != Lifecycle::Completed)
            })
        {
            add(entity, ViolationKind::CompletedWithOpenDependency);
        }
        if value.needs.iter().any(|need| !view.is_known(need)) {
            add(entity, ViolationKind::UnknownDependency);
        }
    }
    for entity in reference_cycle_members(view) {
        add(&entity, ViolationKind::CompletionCycle);
    }
    result
}

fn check_graph(r: &Replica) {
    let view = r.view();
    let (heads, gaps) = reference_heads_and_gaps(&r.store);
    assert_eq!(
        view.known().cloned().collect::<BTreeSet<_>>(),
        heads.keys().cloned().collect()
    );
    assert_eq!(view.gaps(), &gaps);
    for (entity, expected) in heads {
        assert_eq!(view.heads(&entity), Some(&expected));
        assert!(view.is_known(&entity));
        assert_eq!(view.is_conflicted(&entity), expected.len() > 1);
        let value = if expected.len() == 1 {
            Some(&r.store.record(expected.first().unwrap()).unwrap().after)
        } else {
            None
        };
        assert_eq!(view.current(&entity), value);
    }
    assert_eq!(view.violations(), &reference_violations(&view));
}

fn generated_operations(replica: &mut Replica, steps: &[u8]) {
    for step in steps {
        let before = replica.view().violations().clone();
        let count = replica.store.len();
        let result = match step {
            0 => replica.try_op("i3", Start).map(Some),
            1 => replica.try_op("i3", Release).map(Some),
            2 => replica.try_op("i3", Complete).map(Some),
            3 => replica.try_add_dep("i3", "g2"),
            4 => replica.try_add_dep("g2", "i3"),
            5 => replica.try_move("i3", Some("g0")),
            6 => replica.try_move("i3", None),
            7 => replica
                .try_create("i4", current(Kind::Issue, Lifecycle::NotStarted, None))
                .map(Some),
            8 => replica.try_add_dep("i4", "g2"),
            _ => replica.try_move("i4", Some("g0")),
        };
        if let Ok(Some(record)) = result {
            insert(&mut replica.store, record);
            assert!(replica.view().violations().is_subset(&before));
        } else {
            assert_eq!(replica.store.len(), count);
        }
        check_graph(replica);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_operation_and_delivery_sequences(
        left in prop::collection::vec(0u8..10, 1..10),
        right in prop::collection::vec(0u8..10, 1..10),
        delivery in prop::collection::vec(any::<bool>(), 1..16),
        reverse in any::<bool>()
    ) {
        let mut r0 = Replica::new("r0");
        let mut r1 = Replica::new("r1");
        generated_operations(&mut r0, &left);
        generated_operations(&mut r1, &right);
        let mut arriving: Vec<_> = r0.store.entries()
            .filter(|(record, _)| !r1.store.contains(record))
            .map(|(record, entry)| (record.clone(), entry.clone())).collect();
        if reverse { arriving.reverse(); }
        let mut merged = Replica::from("r1", &r1.store);
        for (index, (record, entry)) in arriving.iter().enumerate() {
            if delivery[index % delivery.len()] {
                prop_assert_eq!(&merged.store.insert(entry.clone()).unwrap(), record);
                prop_assert!(merged.store.contains(record));
                check_graph(&merged);
            }
        }
        let expected: BTreeSet<_> = r0.store.entries().chain(r1.store.entries()).map(|(record, _)| record.clone()).collect();
        merged.sync(&r0);
        prop_assert_eq!(merged.store.entries().map(|(record, _)| record.clone()).collect::<BTreeSet<_>>(), expected);
        check_graph(&merged);
        for entity in ["i3", "g2", "i4"] {
            if merged.view().is_conflicted(&id(entity)) {
                let chosen = merged.heads(entity).first().unwrap().clone();
                merged.resolve(entity, &chosen);
                check_graph(&merged);
            }
        }
    }

    #[test]
    fn generated_partial_arrival_and_resolution(
        actor in "[a-z]{1,6}", same in any::<bool>(), duplicate in any::<bool>(),
        deliver_start in any::<bool>(), resolve_first in any::<bool>()
    ) {
        let mut r0 = Replica::new("r0");
        let mut r1 = Replica::new("r1");
        let entity = if duplicate { "i4" } else { "i3" };
        let left = if duplicate {
            r0.create(entity, Kind::Issue, Lifecycle::NotStarted, None)
        } else {
            r0.op_as(entity, Start, &actor)
        };
        let right = if duplicate {
            r1.create(entity, Kind::Issue, Lifecycle::NotStarted, None)
        } else {
            r1.op_as(entity, Start, if same { &actor } else { "other" })
        };
        prop_assert_ne!(&left, &right);
        r0.sync(&r1);
        check_graph(&r0);
        prop_assert_eq!(r0.heads(entity).len(), 2);
        if same || duplicate {
            prop_assert_eq!(&r0.store.record(&left).unwrap().after, &r0.store.record(&right).unwrap().after);
        }
        let chosen = if resolve_first { &left } else { &right };
        let resolved = r0.resolve(entity, chosen);
        check_graph(&r0);
        prop_assert_eq!(&r0.store.record(&resolved).unwrap().parents, &BTreeSet::from([left.clone(), right.clone()]));
        prop_assert_eq!(&r0.store.record(&resolved).unwrap().after, &r0.store.record(chosen).unwrap().after);
        prop_assert!(r0.view().is_valid());

        let mut source = Replica::new("r0");
        let start = source.op("i3", Start);
        let complete = source.op("i3", Complete);
        let mut target = Replica::new("r1");
        target.sync_one(&source, &complete);
        check_graph(&target);
        let view = target.view();
        prop_assert_eq!(view.gaps().get(&id("i3")), Some(&BTreeSet::from([start.clone()])));
        if deliver_start {
            target.sync_one(&source, &start);
        } else {
            target.resolve("i3", &complete);
            target.sync_one(&source, &start);
        }
        check_graph(&target);
        prop_assert!(target.view().gaps().is_empty());
        prop_assert_eq!(lifecycle(&target.view(), "i3"), Lifecycle::Completed);
    }






}

#[test]
fn cycles_reject_new_edges_and_allow_repair() {
    for reverse in [false, true] {
        for waiting in [false, true] {
            for chord in [false, true] {
                let mut r0 = Replica::new("r0");
                let mut r1 = Replica::new("r1");
                r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
                r1.sync(&r0);
                let (a, b, c) = if reverse {
                    ("g2", "i3", "i4")
                } else {
                    ("i4", "i3", "g2")
                };
                r0.add_dep(a, b);
                r0.add_dep(b, c);
                r1.add_dep(c, a);
                r0.sync(&r1);
                check_graph(&r0);
                assert_eq!(
                    reference_cycle_members(&r0.view()),
                    BTreeSet::from([id(a), id(b), id(c)])
                );
                if waiting {
                    r0.add_dep("i1", b);
                    check_graph(&r0);
                    assert!(!r0.view().in_violation(&id("i1")));
                }
                let before = r0.store.len();
                let rejected = if chord {
                    r0.try_add_dep(a, c)
                } else {
                    r0.try_add_dep(c, b)
                };
                assert!(rejected.is_err());
                assert_eq!(r0.store.len(), before);
                check_graph(&r0);
                r0.remove_dep(b, c);
                check_graph(&r0);
                assert!(r0.view().is_valid());
            }
        }
    }
}

#[test]
fn second_cycle_and_inherited_dependency() {
    let actor = "actor";

    for extra_waiter in [false, true] {
        let mut r0 = Replica::new("r0");
        let mut r1 = Replica::new("r1");
        r0.create("i4", Kind::Issue, Lifecycle::NotStarted, None);
        r0.create("i5", Kind::Issue, Lifecycle::NotStarted, None);
        r1.sync(&r0);
        let left = format!("left-{actor}");
        r0.op_as("i1", Start, &left);
        r1.op_as("i1", Start, if extra_waiter { &left } else { "right" });
        r0.add_dep("g2", "i3");
        r1.add_dep("i3", "g2");
        r0.sync(&r1);
        check_graph(&r0);
        assert!(r0.view().is_conflicted(&id("i1")));
        let chosen = r0.heads("i1").first().unwrap().clone();
        r0.resolve("i1", &chosen);
        r0.add_dep("g0", "i4");
        if extra_waiter {
            r0.add_dep("i5", "g2");
        }
        check_graph(&r0);
        let before = r0.store.len();
        assert!(r0.try_move("i4", Some("g0")).is_err());
        assert_eq!(r0.store.len(), before);
        r1.add_dep("i4", "g0");
        r0.sync(&r1);
        check_graph(&r0);
        assert!(r0.view().in_violation(&id("i4")));
        r0.add_dep("g2", "i4");
        let before = r0.store.len();
        assert!(r0.try_add_dep("i4", "g2").is_err());
        assert_eq!(r0.store.len(), before);
        r0.remove_dep("g2", "i3");
        r0.remove_dep("i4", "g0");
        check_graph(&r0);
        assert!(r0.view().is_valid());
    }
}

#[test]
fn waiver_only_repairs_affected_entity() {
    for initially_undecided in [false, true] {
        for switch_back in [false, true] {
            let mut r0 = Replica::new("r0");
            let mut r1 = Replica::new("r1");
            r0.op("i1", Cancel);
            r0.op("g0", Complete);
            r1.create(
                "i4",
                Kind::Issue,
                if initially_undecided {
                    Lifecycle::Undecided
                } else {
                    Lifecycle::NotStarted
                },
                Some("g0"),
            );
            r1.sync(&r0);
            check_graph(&r1);
            assert_eq!(
                r1.violations("i4"),
                BTreeSet::from([ViolationKind::OpenUnderTerminal])
            );
            if initially_undecided {
                r1.op("i4", Accept);
            } else {
                r1.op("i4", Withdraw);
            }
            if switch_back {
                if initially_undecided {
                    r1.op("i4", Withdraw);
                } else {
                    r1.op("i4", Accept);
                }
            }
            check_graph(&r1);
            assert_eq!(
                r1.violations("i4"),
                BTreeSet::from([ViolationKind::OpenUnderTerminal])
            );
            let before = r1.store.len();
            assert!(
                r1.try_create(
                    "i5",
                    current(Kind::Issue, Lifecycle::NotStarted, Some("g0"))
                )
                .is_err()
            );
            assert_eq!(r1.store.len(), before);
            r1.op("i4", Cancel);
            check_graph(&r1);
            assert!(r1.view().is_valid());
        }
    }
}
