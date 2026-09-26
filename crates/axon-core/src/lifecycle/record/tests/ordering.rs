//! Derivation and presentation order do not depend on the order records were read in. The
//! store keys records by ID, so these tests guard against order-dependent state being added
//! to the derivation; the adapter's file listing is checked in its own fixtures.
use super::*;
use Operation::*;

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
    // A parent of another Entity, a Note as parent, and a cycle are corruption, not gaps.
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
    // A cycle among records cannot be built: a record's ID depends on its parents' IDs. The
    // derivation still counts the records it orders, so a set that somehow contains one is
    // reported instead of looping.
    let view = r.store.view().unwrap();
    assert!(view.is_valid());
}

#[test]
fn notes_are_ordered_by_time_before_id() {
    let r = Replica::new("r0");
    let make = |body: &str, seconds: i64| Note {
        entity: id("i3"),
        nonce: Nonce::try_from("0123456789abcdef0123456789abcdef").unwrap(),
        at: ctx(seconds, "r0").at,
        recorder: ctx(seconds, "r0").recorder,
        reason: None,
        body: body.into(),
    };
    let note_id = |note: &Note| RecordId::of(&encode(&Entry::Note(note.clone())).unwrap());
    // Find a pair whose later Note has the smaller ID, so that ID order alone would be wrong.
    let earlier = make("earlier", 40);
    let later = (0..64)
        .map(|n| make(&format!("later {n}"), 50))
        .find(|later| note_id(later) < note_id(&earlier))
        .expect("some body hashes below the earlier Note");
    let (earlier_id, later_id) = (note_id(&earlier), note_id(&later));
    assert!(later_id < earlier_id);
    let mut store = r.store.clone();
    store.insert(Entry::Note(later.clone())).unwrap();
    store.insert(Entry::Note(earlier.clone())).unwrap();
    let order: Vec<_> = store
        .notes_of(&id("i3"))
        .iter()
        .map(|(id, n)| (n.body.clone(), (*id).clone()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("earlier".into(), earlier_id.clone()),
            (later.body.clone(), later_id.clone())
        ]
    );
    let all: Vec<_> = store
        .all_notes()
        .iter()
        .map(|(id, _)| (*id).clone())
        .collect();
    assert_eq!(all, vec![earlier_id, later_id]);
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
    let order = |store: &Store| -> Vec<RecordId> {
        store
            .history(&id("i3"))
            .unwrap()
            .into_iter()
            .map(|(id, _)| id.clone())
            .collect()
    };
    let reversed = |store: &Store| {
        let mut copy = Store::new();
        let entries: Vec<_> = store.entries().map(|(_, e)| e.clone()).collect();
        for entry in entries.into_iter().rev() {
            copy.insert(entry).unwrap();
        }
        copy
    };
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

/// Chosen so that the IDs place another branch between the two records of the inner fork.
const FORK_ACTOR: &str = "fork1";

#[test]
fn history_moves_to_the_smallest_waiting_record_when_a_branch_ends() {
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
    r2.op_as("i3", Start, "c");
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
    // Each step follows the rule: the smallest child of the previous record whose parents
    // are listed, or else the smallest record whose parents are listed.
    let mut fallbacks_with_choice = 0;
    for i in 1..order.len() {
        let listed: BTreeSet<_> = order[..i].iter().map(|(id, _)| id).collect();
        let listable: Vec<_> = order[i..]
            .iter()
            .filter(|(_, parents)| parents.iter().all(|p| listed.contains(p)))
            .collect();
        let continuing: Vec<_> = listable
            .iter()
            .filter(|(_, parents)| parents.contains(&order[i - 1].0))
            .map(|(id, _)| id)
            .collect();
        let expected = if continuing.is_empty() {
            if listable.len() > 1 {
                fallbacks_with_choice += 1;
            }
            listable.iter().map(|(id, _)| id).min()
        } else {
            continuing.into_iter().min()
        };
        assert_eq!(expected, Some(&order[i].0), "position {i}");
    }
    assert!(fallbacks_with_choice >= 2, "{fallbacks_with_choice}");
    // Another branch comes between the two records that fork from the same record, so
    // resuming the nearest sibling would not give this order.
    let at = |record: &RecordId| order.iter().position(|(id, _)| id == record).unwrap();
    assert!(at(&sibling).abs_diff(at(&forked)) > 1, "{order:?}");
    // Parents always come first.
    for (i, (listed, _)) in order.iter().enumerate() {
        for (listed_after, _) in &order[i + 1..] {
            assert!(!r0.store.precedes(listed_after, listed));
        }
    }
}
