//! Derivation and presentation order do not depend on the order records were read in. The
//! store keys records by ID, so these tests guard against order-dependent state being added
//! to the derivation; the adapter's file listing is checked in its own fixtures.
use super::*;
use Operation::*;
use proptest::prelude::*;

fn expected_history(store: &Store, entity: &str) -> Vec<RecordId> {
    let records: BTreeMap<_, _> = store
        .records()
        .filter(|(_, record)| record.entity == id(entity))
        .collect();
    let mut remaining: BTreeSet<_> = records.keys().copied().collect();
    let mut listed = Vec::new();
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .copied()
            .filter(|candidate| {
                records[*candidate]
                    .parents
                    .iter()
                    .all(|parent| !records.contains_key(parent) || listed.contains(&parent))
            })
            .collect();
        assert!(!ready.is_empty(), "generated DAG must be acyclic");
        let next = listed
            .iter()
            .rev()
            .find_map(|parent| {
                ready
                    .iter()
                    .copied()
                    .find(|candidate| records[*candidate].parents.contains(*parent))
            })
            .unwrap_or_else(|| {
                ready
                    .iter()
                    .copied()
                    .min_by_key(|candidate| (!records[*candidate].parents.is_empty(), *candidate))
                    .unwrap()
            });
        remaining.remove(next);
        listed.push(next);
    }
    listed.into_iter().cloned().collect()
}

fn generated_dag(seed: u16, gap: usize, resolve: bool, branches: &[bool]) -> Store {
    let mut main = Replica::new("r0");
    main.op("i3", Start);
    let mut inner = Replica::from("inner", &main.store);
    main.clock.set(-100);
    main.op("i3", Release);
    inner.op_as("i3", Release, &format!("inner{seed}"));
    let created = main.store.history(&id("i3")).unwrap()[0].0.clone();
    let (outer, missing) = (0..256)
        .map(|index| {
            let mut outer = Replica::new("outer");
            let who = format!("outer{seed}-{index}");
            let missing = outer.op_as("i3", Start, &who);
            outer.op_as("i3", Release, &who);
            (outer, missing)
        })
        .find(|(outer, _)| outer.head("i3") < created)
        .unwrap();
    main.sync(&inner);
    main.sync(&outer);
    for (index, extend) in branches.iter().enumerate() {
        let mut branch = Replica::new("branch");
        let who = format!("branch{seed}-{index}");
        branch.op_as("i3", Start, &who);
        if *extend {
            branch.op_as("i3", Release, &who);
        }
        main.sync(&branch);
    }
    if resolve {
        let chosen = main.heads("i3").first().unwrap().clone();
        main.resolve("i3", &chosen);
    }
    let same_time = ctx(seed as i64, "note");
    for (index, (entity, body)) in [("i3", "z"), ("g0", "a"), ("i3", "a")]
        .into_iter()
        .enumerate()
    {
        let note = Note {
            entity: id(entity),
            nonce: format!("{:032x}", seed as u128 * 1000 + index as u128)
                .try_into()
                .unwrap(),
            at: same_time.at,
            recorder: same_time.recorder.clone(),
            reason: None,
            body: format!("{body}{seed}"),
        };
        main.store.insert(Entry::Note(note)).unwrap();
    }
    let timed_note = |body: String, at| Note {
        entity: id("i3"),
        nonce: "0123456789abcdef0123456789abcdef".try_into().unwrap(),
        at,
        recorder: ctx(40, "r0").recorder,
        reason: None,
        body,
    };
    let earlier = timed_note("earlier".into(), ctx(40, "r0").at);
    let earlier_id = RecordId::of(&encode(&Entry::Note(earlier.clone())).unwrap());
    let later = (0..64)
        .map(|index| timed_note(format!("later {index}"), ctx(50, "r0").at))
        .find(|note| RecordId::of(&encode(&Entry::Note(note.clone())).unwrap()) < earlier_id)
        .unwrap();
    main.store.insert(Entry::Note(earlier)).unwrap();
    main.store.insert(Entry::Note(later)).unwrap();
    let mut store = Store::new();
    for (record_id, entry) in main.store.entries() {
        if gap != 0 || *record_id != missing {
            store.insert(entry.clone()).unwrap();
        }
    }
    store
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_dags_preserve_causal_history_and_note_order(seed in any::<u16>(), shift in any::<usize>(), branches in prop::collection::vec(any::<bool>(), 0..4)) {
        for gap in 0..2 {
            for resolve in [false, true] {
                let store = generated_dag(seed, gap, resolve, &branches);
                let expected = expected_history(&store, "i3");
                let mut entries: Vec<_> = store.entries().map(|(_, entry)| entry.clone()).collect();
                let reference_view = store.view().unwrap();
                let notes: Vec<_> = store.notes().map(|(id, note)| (id.clone(), note.entity.clone(), note.at)).collect();
                let mut expected_notes = notes.clone();
                expected_notes.sort_by_key(|(id, _, at)| (*at, id.clone()));
                let actual_all: Vec<_> = store.all_notes().iter().map(|(id, note)| ((*id).clone(), note.entity.clone(), note.at)).collect();
                prop_assert_eq!(actual_all, expected_notes.clone());
                for entity in ["i3", "g0"] {
                    let actual: Vec<_> = store.notes_of(&id(entity)).iter().map(|(id, _)| (*id).clone()).collect();
                    let filtered: Vec<_> = expected_notes.iter().filter(|(_, owner, _)| owner == &id(entity)).map(|(id, _, _)| id.clone()).collect();
                    prop_assert_eq!(actual, filtered);
                }
                for record_id in &expected {
                    let record = store.record(record_id).unwrap();
                    for parent in &record.parents {
                        if store.contains(parent) {
                            prop_assert!(store.precedes(parent, record_id));
                            prop_assert!(expected.iter().position(|id| id == parent) < expected.iter().position(|id| id == record_id));
                        }
                    }
                }
                for before in &expected {
                    for after in &expected {
                        let mut pending: Vec<_> = store.record(after).unwrap().parents.iter().collect();
                        let mut seen = BTreeSet::new();
                        while let Some(parent) = pending.pop() {
                            if seen.insert(parent) && let Some(record) = store.record(parent) {
                                pending.extend(&record.parents);
                            }
                        }
                        prop_assert_eq!(store.precedes(before, after), before != after && seen.contains(before));
                    }
                }
                let reversed_time = expected.windows(2).any(|pair| {
                    let first = store.record(&pair[0]).unwrap();
                    let second = store.record(&pair[1]).unwrap();
                    first.at > second.at
                });
                prop_assert!(reversed_time);
                for _ in 0..3 {
                    let offset = shift % entries.len();
                    entries.rotate_left(offset);
                    entries.reverse();
                    let mut reordered = Store::new();
                    for entry in &entries {
                        reordered.insert(entry.clone()).unwrap();
                    }
                    prop_assert_eq!(reordered.view().unwrap(), reference_view.clone());
                    prop_assert_eq!(history_ids(&reordered, "i3"), expected.clone());
                    prop_assert_eq!(reordered.all_notes(), store.all_notes());
                    for entity in ["i3", "g0"] {
                        prop_assert_eq!(reordered.notes_of(&id(entity)), store.notes_of(&id(entity)));
                    }
                }
                prop_assert_eq!(history_ids(&store, "i3"), expected);
                prop_assert_eq!(reference_view.gaps().contains_key(&id("i3")), gap == 0);
            }
        }
    }
}

#[test]
fn view_history_and_note_order_are_independent_of_insertion_order() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    r0.op("i1", Start);
    r1.op_as("i1", Start, "other");
    r0.sync(&r1);
    let head = r0.heads("i1").first().unwrap().clone();
    r0.resolve("i1", &head);
    r0.op("i1", Complete);
    r0.op("g2", Complete);
    r0.move_to("g2", Some("g0"));
    r1.move_to("g0", Some("g2"));
    r0.sync(&r1);
    // Notes at one instant with different bodies, plus a gap from a missing parent.
    let same_time = r0.tick();
    for body in ["b", "a", "c"] {
        let note = r0
            .store
            .add_note(&id("i3"), body.into(), None, same_time.clone())
            .unwrap();
        r0.store.insert(Entry::Note(note)).unwrap();
    }
    let start = r0.op("i3", Start);
    r0.op("i3", Release);
    let mut entries: Vec<_> = r0.store.entries().map(|(_, e)| e.clone()).collect();
    let mut forward = Store::new();
    for entry in &entries {
        forward.insert(entry.clone()).unwrap();
    }
    let mut backward = Store::new();
    for entry in entries.iter().rev() {
        backward.insert(entry.clone()).unwrap();
    }
    let len = entries.len();
    entries.rotate_left(len / 3);
    entries.swap(0, len - 1);
    let mut shuffled = Store::new();
    for entry in &entries {
        shuffled.insert(entry.clone()).unwrap();
    }
    // Drop the Start of i3 on one copy to leave a gap; every order agrees on it too.
    let mut gapped: Vec<_> = r0
        .store
        .entries()
        .filter(|(id, _)| **id != start)
        .map(|(_, e)| e.clone())
        .collect();
    let mut gap_forward = Store::new();
    for entry in &gapped {
        gap_forward.insert(entry.clone()).unwrap();
    }
    gapped.reverse();
    let mut gap_backward = Store::new();
    for entry in &gapped {
        gap_backward.insert(entry.clone()).unwrap();
    }
    for (a, b) in [
        (&forward, &backward),
        (&forward, &shuffled),
        (&gap_forward, &gap_backward),
    ] {
        assert_eq!(a.view().unwrap(), b.view().unwrap());
        for entity in ["g0", "i1", "g2", "i3"] {
            assert_eq!(
                a.history(&id(entity)).unwrap(),
                b.history(&id(entity)).unwrap()
            );
            assert_eq!(a.notes_of(&id(entity)), b.notes_of(&id(entity)));
        }
        assert_eq!(a.all_notes(), b.all_notes());
    }
    let view = forward.view().unwrap();
    assert!(view.conflicted().is_empty());
    assert!(
        view.violations()
            .iter()
            .any(|v| v.kind == ViolationKind::ContainmentCycle)
    );
    assert!(!gap_forward.view().unwrap().gaps().is_empty());
    // Notes at the same instant are ordered by record ID, not by body or insertion.
    let notes = forward.notes_of(&id("i3"));
    assert_eq!(notes.len(), 3);
    let ids: Vec<_> = notes.iter().map(|(id, _)| (*id).clone()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert!(notes.iter().all(|(_, note)| note.at == same_time.at));
}

#[test]
fn history_is_causal_with_id_ties_and_time_never_orders_records() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    // r1's clock is far ahead: its Start looks later but is concurrent with r0's.
    let mine = r0.op("i1", Start);
    let theirs = r1.op_as("i1", Start, "other");
    r0.sync(&r1);
    assert!(!r0.store.precedes(&mine, &theirs));
    assert!(!r0.store.precedes(&theirs, &mine));
    let resolve = r0.resolve("i1", &mine);
    assert!(r0.store.precedes(&mine, &resolve));
    assert!(r0.store.precedes(&theirs, &resolve));
    let created = r0.store.history(&id("i1")).unwrap()[0].0.clone();
    assert!(r0.store.precedes(&created, &resolve));
    assert!(!r0.store.precedes(&resolve, &created));
    assert!(!r0.store.precedes(&resolve, &resolve));
    let order: Vec<_> = r0
        .store
        .history(&id("i1"))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id.clone())
        .collect();
    let (first, second) = if mine < theirs {
        (mine, theirs)
    } else {
        (theirs, mine)
    };
    assert_eq!(order, vec![created, first, second, resolve]);
    assert!(r0.store.history(&id("nope")).unwrap().is_empty());
}

#[test]
fn corrupt_parent_links_are_rejected_by_derivation() {
    // A parent of another Entity and a Note as parent are corruption, not gaps. A cycle among
    // records cannot be built here: a record's ID depends on its parents' IDs.
    let mut r = Replica::new("r0");
    let foreign = r.head("g0");
    let mut record = r.store.record(&r.head("i3")).unwrap().clone();
    record.kind = RecordKind::Edit;
    record.parents = BTreeSet::from([foreign]);
    record.after.title = "cross".into();
    record.at = r.tick().at;
    let mut store = r.store.clone();
    insert(&mut store, record.clone());
    assert!(error(store.view()).contains("parent of g0"));
    let note = r.note("i3", "n");
    record.parents = BTreeSet::from([note]);
    let mut store = r.store.clone();
    insert(&mut store, record);
    assert!(error(store.view()).contains("Note as its parent"));
}

#[test]
fn history_lists_each_concurrent_branch_together_before_the_resolution() {
    let mut r0 = Replica::new("r0");
    let mut r1 = Replica::new("r1");
    let created = r0.head("i3");
    let mine = vec![r0.op("i3", Start), r0.op("i3", Release), r0.op("i3", Start)];
    let theirs = vec![
        r1.op_as("i3", Start, "other"),
        r1.op_as("i3", Release, "other"),
        r1.op_as("i3", Start, "other"),
    ];
    r0.sync(&r1);
    let (first, second) = if mine[0] < theirs[0] {
        (&mine, &theirs)
    } else {
        (&theirs, &mine)
    };
    let branches: Vec<_> = std::iter::once(created)
        .chain(first.iter().cloned())
        .chain(second.iter().cloned())
        .collect();
    let order = |store: &Store| history_ids(store, "i3");
    // Conflicted: one branch runs to its head before the other starts.
    assert_eq!(r0.heads("i3").len(), 2);
    assert_eq!(order(&r0.store), branches);
    assert_eq!(order(&reversed(&r0.store)), branches);
    // Merged by a resolution: it comes last, after both branches.
    let resolve = r0.resolve("i3", &mine[2]);
    let mut resolved = branches.clone();
    resolved.push(resolve);
    assert_eq!(order(&r0.store), resolved);
    assert_eq!(order(&reversed(&r0.store)), resolved);
    // The only switch between branches is the only place with no causal link to the previous.
    let unordered = resolved
        .windows(2)
        .filter(|pair| !r0.store.precedes(&pair[0], &pair[1]))
        .count();
    assert_eq!(unordered, 1);
}

fn history_ids(store: &Store, entity: &str) -> Vec<RecordId> {
    store
        .history(&id(entity))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id.clone())
        .collect()
}

fn reversed(store: &Store) -> Store {
    let mut copy = Store::new();
    let entries: Vec<_> = store.entries().map(|(_, e)| e.clone()).collect();
    for entry in entries.into_iter().rev() {
        copy.insert(entry).unwrap();
    }
    copy
}

/// Chosen so that the IDs place another branch between the two records of the inner fork and
/// the history returns to a fork with a choice at least twice.
const FORK_ACTOR: &str = "fork1";
const THIRD_BRANCH_ACTOR: &str = "c3";

#[test]
fn history_returns_to_the_nearest_fork_when_a_branch_ends() {
    // Three branches from the creation, one of which forks again after its first record.
    let mut r0 = Replica::new("r0");
    r0.op("i3", Start);
    let mut fork = Replica::from("fork", &r0.store);
    let sibling = r0.op("i3", Release);
    let forked = fork.op_as("i3", Release, FORK_ACTOR);
    let mut r1 = Replica::new("r1");
    r1.op_as("i3", Start, "b");
    r1.op_as("i3", Release, "b");
    let mut r2 = Replica::new("r2");
    r2.op_as("i3", Start, THIRD_BRANCH_ACTOR);
    for other in [&fork, &r1, &r2] {
        r0.sync(other);
    }
    let order: Vec<_> = r0
        .store
        .history(&id("i3"))
        .unwrap()
        .into_iter()
        .map(|(id, record)| (id.clone(), record.parents.clone()))
        .collect();
    assert_eq!(order.len(), 7);
    // Each step follows the rule: the smallest listable child of the previous record, or else
    // of the most recently listed record that has one, or else the smallest listable record.
    let mut returns_with_choice = 0;
    for i in 1..order.len() {
        let listed: BTreeSet<_> = order[..i].iter().map(|(id, _)| id).collect();
        let listable: Vec<_> = order[i..]
            .iter()
            .filter(|(_, parents)| parents.iter().all(|p| listed.contains(p)))
            .collect();
        let children_of = |parent: &RecordId| -> Vec<&RecordId> {
            listable
                .iter()
                .filter(|(_, parents)| parents.contains(parent))
                .map(|(id, _)| id)
                .collect()
        };
        let continuing = children_of(&order[i - 1].0);
        let expected = if continuing.is_empty() {
            let resumed = order[..i - 1]
                .iter()
                .rev()
                .map(|(id, _)| children_of(id))
                .find(|children| !children.is_empty());
            if resumed.is_some() && listable.len() > 1 {
                returns_with_choice += 1;
            }
            resumed
                .unwrap_or_else(|| listable.iter().map(|(id, _)| id).collect())
                .into_iter()
                .min()
        } else {
            continuing.into_iter().min()
        };
        assert_eq!(expected, Some(&order[i].0), "position {i}");
    }
    assert!(returns_with_choice >= 2, "{returns_with_choice}");
    // The two records that fork from the same record are listed next to each other, although
    // the IDs place another branch between them.
    let at = |record: &RecordId| order.iter().position(|(id, _)| id == record).unwrap();
    assert_eq!(at(&sibling).abs_diff(at(&forked)), 1, "{order:?}");
    let (low, high) = (sibling.clone().min(forked.clone()), sibling.max(forked));
    assert!(
        order.iter().any(|(id, parents)| !parents.is_empty()
            && low < *id
            && *id < high
            && at(id) > at(&high)),
        "{order:?}"
    );
    // Parents always come first. Without merge records, a branch switch follows only the end
    // of a branch.
    for (i, (listed, _)) in order.iter().enumerate() {
        for (listed_after, _) in &order[i + 1..] {
            assert!(!r0.store.precedes(listed_after, listed));
        }
    }
    let ids: Vec<_> = order.iter().map(|(id, _)| id.clone()).collect();
    let switches: Vec<_> = ids
        .windows(2)
        .filter(|pair| !r0.store.precedes(&pair[0], &pair[1]))
        .map(|pair| pair[0].clone())
        .collect();
    let ends: Vec<_> = order
        .iter()
        .filter(|(id, _)| !order.iter().any(|(_, parents)| parents.contains(id)))
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(switches.len(), ends.len() - 1, "{order:?}");
    assert!(switches.iter().all(|end| ends.contains(end)), "{order:?}");
    assert_eq!(history_ids(&reversed(&r0.store), "i3"), ids);
}

#[test]
fn history_finishes_a_nested_fork_before_the_next_branch() {
    // C → S0 → {R0, RF} and C → SB → RB, with actors chosen so that SB's ID falls between R0
    // and RF: listing the smallest waiting record would put SB and RB between them.
    let build = |fork_actor: &str, other_actor: &str| {
        let mut r0 = Replica::new("r0");
        let created = r0.head("i3");
        let s0 = r0.op("i3", Start);
        let mut fork = Replica::from("fork", &r0.store);
        let r0_release = r0.op("i3", Release);
        let rf = fork.op_as("i3", Release, fork_actor);
        let mut other = Replica::new("r1");
        let sb = other.op_as("i3", Start, other_actor);
        let rb = other.op_as("i3", Release, other_actor);
        r0.sync(&fork);
        r0.sync(&other);
        let (first, second) = (r0_release.clone().min(rf.clone()), r0_release.max(rf));
        (r0.store, [created, s0, first, second, sb, rb])
    };
    let (store, [created, s0, first, second, sb, rb]) = (0..64)
        .flat_map(|f| (0..64).map(move |o| (format!("f{f}"), format!("o{o}"))))
        .map(|(f, o)| build(&f, &o))
        .find(|(_, [_, s0, first, second, sb, _])| s0 < sb && first < sb && sb < second)
        .expect("some actors place SB between the two records of the inner fork");
    let expected = vec![created, s0, first, second, sb, rb];
    assert_eq!(history_ids(&store, "i3"), expected);
    assert_eq!(history_ids(&reversed(&store), "i3"), expected);
    // Concurrent branches start only after R0 and RF, the ends of the inner fork.
    let unordered: Vec<_> = expected
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| !store.precedes(&pair[0], &pair[1]))
        .map(|(i, _)| i + 1)
        .collect();
    assert_eq!(unordered, vec![3, 4]);
}

#[test]
fn history_starts_with_the_creation_record_before_a_gap_root() {
    // C → S → R with S missing: R has a parent missing from the set. Actors are chosen so that
    // R's ID is below C's, so ID order alone would start the history before the creation.
    let (store, created, gap_root) = (0..256)
        .map(|n| {
            let mut r = Replica::new("r0");
            let created = r.head("i3");
            let start = r.op_as("i3", Start, &format!("a{n}"));
            let release = r.op_as("i3", Release, &format!("a{n}"));
            let kept: Vec<_> = r
                .store
                .entries()
                .filter(|(id, _)| **id != start)
                .map(|(_, e)| e.clone())
                .collect();
            let mut store = Store::new();
            for entry in kept {
                store.insert(entry).unwrap();
            }
            (store, created, release)
        })
        .find(|(_, created, gap_root)| gap_root < created)
        .expect("some actor gives the gap root a smaller ID than the creation");
    let expected = vec![created, gap_root];
    assert_eq!(history_ids(&store, "i3"), expected);
    assert_eq!(history_ids(&reversed(&store), "i3"), expected);
}

#[test]
fn history_returns_to_an_open_fork_before_a_gap_root() {
    // C → {A, B}, and C → S → R → R2 with S missing, so R is a gap root. Actors are chosen so
    // that R's ID is below A's and B's: the fork of C still finishes before the gap branch.
    let (store, expected) = (0..64)
        .flat_map(|b| (0..64).map(move |g| (format!("b{b}"), format!("g{g}"))))
        .map(|(b, g)| {
            let mut r0 = Replica::new("r0");
            let created = r0.head("i3");
            let a = r0.op("i3", Start);
            let mut other = Replica::new("r1");
            let b = other.op_as("i3", Start, &b);
            let mut gapped = Replica::new("r2");
            let start = gapped.op_as("i3", Start, &g);
            let gap_root = gapped.op_as("i3", Release, &g);
            let after_gap = gapped.op_as("i3", Start, &g);
            r0.sync(&other);
            let mut store = r0.store.clone();
            for (id, entry) in gapped.store.entries() {
                if *id != start && !store.contains(id) {
                    store.insert(entry.clone()).unwrap();
                }
            }
            let (first, second) = (a.clone().min(b.clone()), a.max(b));
            (store, vec![created, first, second, gap_root, after_gap])
        })
        .find(|(_, order)| order[3] < order[1])
        .expect("some actors give the gap root a smaller ID than both children of the creation");
    assert_eq!(history_ids(&store, "i3"), expected);
    assert_eq!(history_ids(&reversed(&store), "i3"), expected);
}
