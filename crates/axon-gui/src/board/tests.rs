use super::fixture::Fixture;
use super::*;
use axon::lifecycle::{Kind, Label, Lifecycle, Operation, Refusal, record::Entry};
use axon::read::{PrerequisiteOperation, Status};

fn ids(listing: &Listing) -> Vec<(EntityId, usize, bool)> {
    listing
        .rows
        .iter()
        .map(|row| (row.id.clone(), row.depth, row.matched))
        .collect()
}

fn matching(board: &Board, filter: &Filter) -> Vec<EntityId> {
    listing::listing(board, filter, Layout::Flat)
        .rows
        .into_iter()
        .map(|row| row.id)
        .collect()
}

#[test]
fn facets_are_ored_within_and_anded_across() {
    let mut f = Fixture::new();
    let undecided_bug = f.create(Kind::Issue, Lifecycle::Undecided, "u", Label::Bug, None);
    let ready_feat = f.issue("r", None);
    let group_docs = f.create(Kind::Group, Lifecycle::NotStarted, "g", Label::Docs, None);
    let done = f.issue("d", None);
    f.perform(&done, Operation::Start);
    f.perform(&done, Operation::Complete);
    let board = f.board();

    let mut filter = Filter::default();
    assert_eq!(
        matching(&board, &filter),
        [
            undecided_bug.clone(),
            ready_feat.clone(),
            group_docs.clone()
        ],
        "the default leaves out finished work"
    );

    filter.states = [State::Undecided, State::Completed].into();
    assert_eq!(
        matching(&board, &filter),
        [undecided_bug.clone(), done.clone()]
    );

    filter.toggle_label(Label::Feat, false);
    assert_eq!(matching(&board, &filter), [undecided_bug]);

    filter = Filter::default();
    filter.kinds = [Kind::Group].into();
    assert_eq!(matching(&board, &filter), [group_docs]);

    for clear in [
        |f: &mut Filter| f.states.clear(),
        |f: &mut Filter| f.kinds.clear(),
        |f: &mut Filter| f.labels.clear(),
    ] {
        let mut filter = Filter::default();
        clear(&mut filter);
        let listing = listing::listing(&board, &filter, Layout::Tree);
        assert_eq!(
            listing,
            Listing::default(),
            "clearing a facet matches nothing"
        );
    }
}

#[test]
fn search_is_literal_in_title_and_description() {
    let mut f = Fixture::new();
    let by_title = f.issue("本を選ぶ", None);
    let by_body = f.issue("会場", None);
    f.describe(&by_body, "候補の本を三冊");
    f.issue("Book", None);
    let board = f.board();

    let mut filter = Filter {
        query: "本".into(),
        ..Filter::default()
    };
    assert_eq!(matching(&board, &filter), [by_title, by_body]);
    filter.query = "book".into();
    assert!(matching(&board, &filter).is_empty(), "case-sensitive");
}

#[test]
fn a_group_is_filtered_by_its_effective_state() {
    let mut f = Fixture::new();
    let working = f.group("進行中の Group", None);
    let child = f.issue("着手した Issue", Some(&working));
    f.perform(&child, Operation::Start);
    let finished = f.group("子が完了した Group", None);
    let done = f.issue("完了した Issue", Some(&finished));
    let other = f.issue("残り", Some(&finished));
    f.perform(&done, Operation::Start);
    f.perform(&done, Operation::Complete);
    let idle = f.group("未着手の Group", None);
    f.issue("未着手の子", Some(&idle));
    let board = f.board();

    assert_eq!(board.item(&working).unwrap().state, State::InProgress);
    assert_eq!(board.item(&finished).unwrap().state, State::InProgress);
    assert_eq!(board.item(&idle).unwrap().state, State::NotStarted);
    let filter = Filter {
        states: [State::InProgress].into(),
        kinds: [Kind::Group].into(),
        ..Filter::default()
    };
    assert_eq!(matching(&board, &filter), [working, finished.clone()]);
    let detail = board.detail(&finished).unwrap();
    assert_eq!(detail.state, State::InProgress);
    assert_eq!(detail.stored, Some(Lifecycle::NotStarted));
    assert_eq!(detail.children.len(), 2);
    assert_eq!(detail.children[1].id, other);
}

#[test]
fn a_tree_shows_unmatched_ancestors_for_reference_only() {
    let mut f = Fixture::new();
    let root = f.group("root", None);
    let middle = f.group("middle", Some(&root));
    let leaf = f.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "leaf",
        Label::Bug,
        Some(&middle),
    );
    let sibling = f.issue("sibling", Some(&root));
    let alone = f.create(Kind::Issue, Lifecycle::Undecided, "alone", Label::Bug, None);
    let board = f.board();

    let filter = Filter {
        labels: [Label::Bug].into(),
        ..Filter::default()
    };
    let tree = listing::listing(&board, &filter, Layout::Tree);
    assert_eq!(
        ids(&tree),
        [
            (root.clone(), 0, false),
            (middle.clone(), 1, false),
            (leaf.clone(), 2, true),
            (alone.clone(), 0, true),
        ]
    );
    assert_eq!((tree.matched, tree.context), (2, 2));

    let flat = listing::listing(&board, &filter, Layout::Flat);
    assert_eq!(ids(&flat), [(leaf.clone(), 0, true), (alone, 0, true)]);
    assert_eq!((flat.matched, flat.context), (2, 0));

    // Everything matching: the tree keeps creation order among siblings.
    let all = listing::listing(&board, &Filter::default(), Layout::Tree);
    assert_eq!(
        all.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()[..4],
        [root, middle, leaf, sibling]
    );
    assert_eq!(all.context, 0);
}

#[test]
fn detail_gathers_relations_waits_notes_and_history() {
    let mut f = Fixture::new();
    let group = f.group("計画", None);
    let first = f.issue("会場を決める", Some(&group));
    let second = f.issue("案内を送る", Some(&group));
    f.needs(&second, &first);
    f.describe(&second, "日時と場所を書く");
    f.note(&second, "先に会場の返事を待つ");
    let board = f.board();

    let detail = board.detail(&second).unwrap();
    assert_eq!(detail.title, "案内を送る");
    assert_eq!(detail.description, "日時と場所を書く");
    assert_eq!(detail.status, Status::Blocked);
    assert_eq!(detail.parent.as_ref().unwrap().id, group);
    assert_eq!(
        detail
            .ancestors
            .iter()
            .map(|l| l.id.clone())
            .collect::<Vec<_>>(),
        [group]
    );
    assert_eq!(detail.dependencies, [board.link(&first)]);
    assert_eq!(detail.waiting_for, Some(PrerequisiteOperation::Start));
    assert_eq!(
        detail.waits,
        [Wait {
            kind: WaitKind::Dependency,
            entity: board.link(&first),
            via: None
        }]
    );
    assert_eq!(detail.notes.len(), 1);
    assert_eq!(detail.notes[0].body, "先に会場の返事を待つ");
    let kinds: Vec<_> = detail.history.iter().map(|h| h.kind.name()).collect();
    assert_eq!(kinds, ["created", "dependency", "edit"]);
    assert_eq!(
        detail.history[1].changes,
        [Difference::DependencyAdded(first.clone())]
    );
    assert_eq!(detail.history[2].changes, [Difference::Description]);

    let first = board.detail(&first).unwrap();
    assert_eq!(first.dependents, [board.link(&second)]);
    assert!(first.waits.is_empty());
    assert_eq!(first.status, Status::Ready);
}

#[test]
fn reading_never_runs_a_condition() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    let mut f = Fixture::new();
    let id = f.issue("条件つき", None);
    f.condition(&id, &format!("touch {}", marker.display()));
    let board = f.board();
    listing::listing(&board, &Filter::default(), Layout::Tree);
    let detail = board.detail(&id).unwrap();
    assert_eq!(
        detail.condition.as_deref(),
        Some(format!("touch {}", marker.display()).as_str())
    );
    assert_eq!(detail.status, Status::Ready, "taken as satisfied");
    assert!(!marker.exists());
}

#[test]
fn an_unknown_entity_has_no_detail_and_matches_nothing() {
    let board = Fixture::new().board();
    let id = EntityId::try_from("axon-none".to_string()).unwrap();
    assert!(board.is_empty());
    assert!(board.detail(&id).is_err());
    assert!(!board.matches(&Filter::default(), &id));
    assert_eq!(board.link(&id).known, None);
}

#[test]
fn an_unmatched_leaf_before_a_matching_sibling_is_not_shown() {
    let mut f = Fixture::new();
    let root = f.group("root", None);
    f.issue("a", Some(&root));
    let b = f.group("b", Some(&root));
    let b1 = f.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "b1",
        Label::Bug,
        Some(&b),
    );
    let c = f.group("c", Some(&root));
    let c1 = f.group("c1", Some(&c));
    let c11 = f.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "c11",
        Label::Bug,
        Some(&c1),
    );
    f.issue("c2", Some(&c));
    let board = f.board();
    let filter = Filter {
        labels: [Label::Bug].into(),
        ..Filter::default()
    };
    let tree = listing::listing(&board, &filter, Layout::Tree);
    assert_eq!(
        ids(&tree),
        [
            (root, 0, false),
            (b, 1, false),
            (b1, 2, true),
            (c, 1, false),
            (c1, 2, false),
            (c11, 3, true),
        ]
    );
    assert_eq!((tree.matched, tree.context), (2, 4));
}

#[test]
fn a_cycle_and_a_missing_parent_still_list_every_entity_once() {
    let mut f = Fixture::new();
    // Created before the cycle it ends up under.
    let below = f.create(Kind::Issue, Lifecycle::Undecided, "below", Label::Bug, None);
    let a = f.group("a", None);
    let b = f.group("b", None);
    let moved = f.parent_in(&f.store.clone(), &below, &a);
    f.insert_entry(moved);
    // Two copies each make one side of the cycle; merged, they close it.
    let fork = f.store.clone();
    let one = f.parent_in(&fork, &a, &b);
    let other = f.parent_in(&f.store.clone(), &b, &a);
    f.insert_entry(one);
    f.insert_entry(other);
    // A child whose parent's records are missing.
    let mut g = f.clone();
    let lost = g.group("lost", None);
    let orphan = g.issue("orphan", Some(&lost));
    let orphan_record = g
        .store
        .records()
        .find(|(_, r)| r.entity == orphan)
        .map(|(_, r)| r.clone())
        .unwrap();
    f.insert_entry(Entry::Record(orphan_record));
    let board = f.board();
    assert!(board.item(&lost).is_none());

    let all = Filter {
        states: State::ALL.into(),
        ..Filter::default()
    };
    let tree = listing::listing(&board, &all, Layout::Tree);
    let mut listed: Vec<_> = tree.rows.iter().map(|r| r.id.clone()).collect();
    assert_eq!(tree.rows.len(), board.len());
    assert_eq!(tree.matched, board.len());
    listed.sort();
    listed.dedup();
    assert_eq!(listed.len(), board.len(), "each Entity once");
    let depth = |id: &EntityId| tree.rows.iter().find(|r| &r.id == id).unwrap().depth;
    assert_eq!(depth(&orphan), 0);
    assert_eq!(depth(&a), 0, "the earliest member of the cycle starts it");
    assert_eq!(depth(&b), 1);
    assert_eq!(depth(&below), 1);

    // Searching for the Entity below the cycle keeps the cycle above it for reference.
    let search = Filter {
        query: "below".into(),
        ..all
    };
    let tree = listing::listing(&board, &search, Layout::Tree);
    assert_eq!(ids(&tree), [(a, 0, false), (below, 1, true)]);
}

#[test]
fn a_conflicted_entity_is_its_own_state() {
    let mut f = Fixture::new();
    let id = f.issue("競合", None);
    let fork = f.store.clone();
    let started = f.perform_in(&fork, &id, Operation::Start);
    let cancelled = f.perform_in(&fork, &id, Operation::Cancel);
    f.insert_entry(started);
    f.insert_entry(cancelled);
    let board = f.board();
    assert_eq!(board.item(&id).unwrap().state, State::Conflicted);
    assert!(board.matches(&Filter::default(), &id));
    let cancelled_only = Filter {
        states: [State::Cancelled].into(),
        ..Filter::default()
    };
    assert!(!board.matches(&cancelled_only, &id));
    let detail = board.detail(&id).unwrap();
    assert_eq!(detail.heads, 2);
    assert_eq!(detail.stored, None);
    assert_eq!(detail.state, State::Conflicted);
}

#[test]
fn a_stalled_group_says_what_it_waits_for() {
    let mut f = Fixture::new();
    let outside = f.issue("外の仕事", None);
    let group = f.group("計画", None);
    let undecided = f.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "未判断",
        Label::Feat,
        Some(&group),
    );
    let waiting = f.issue("待つ", Some(&group));
    f.needs(&waiting, &outside);
    let board = f.board();
    let detail = board.detail(&group).unwrap();
    assert!(detail.stalled);
    assert_eq!(detail.waiting_for, None);
    assert_eq!(
        detail.waits,
        [
            Wait {
                kind: WaitKind::DescendantDependency,
                entity: board.link(&waiting),
                via: Some(board.link(&outside)),
            },
            Wait {
                kind: WaitKind::UndecidedChild,
                entity: board.link(&undecided),
                via: None,
            },
        ]
    );
}

#[test]
fn history_shows_each_change_in_its_direction() {
    let mut f = Fixture::new();
    let id = f.issue("振り返り", None);
    f.perform(&id, Operation::Start);
    f.perform(&id, Operation::Complete);
    let detail = f.board().detail(&id).unwrap();
    assert_eq!(
        detail
            .history
            .iter()
            .map(|h| h.changes.clone())
            .collect::<Vec<_>>(),
        [
            vec![],
            vec![Difference::Lifecycle(
                Lifecycle::NotStarted,
                Lifecycle::InProgress
            )],
            vec![Difference::Lifecycle(
                Lifecycle::InProgress,
                Lifecycle::Completed
            )],
        ]
    );
}

#[test]
fn a_conflicted_child_is_listed_under_its_group() {
    let mut f = Fixture::new();
    let group = f.group("計画", None);
    let child = f.issue("競合する子", Some(&group));
    let fork = f.store.clone();
    let started = f.perform_in(&fork, &child, Operation::Start);
    let cancelled = f.perform_in(&fork, &child, Operation::Cancel);
    f.insert_entry(started);
    f.insert_entry(cancelled);
    let board = f.board();
    let detail = board.detail(&group).unwrap();
    assert_eq!(detail.children, [board.link(&child)]);
}

#[test]
fn rearrangements_are_checked_and_made_by_the_core() {
    let mut f = Fixture::new();
    let outer = f.group("外", None);
    let inner = f.group("内", Some(&outer));
    let leaf = f.issue("葉", Some(&inner));
    let other = f.group("別", None);
    let board = f.board();

    let into_child = Change::Move {
        entity: outer.clone(),
        parent: Some(inner.clone()),
    };
    assert_eq!(
        into_child.check(&board),
        Err(Rejection::Refused(Refusal::ContainmentCycle {
            destination: inner.clone()
        }))
    );
    let on_ancestor = Change::AddDependency {
        entity: leaf.clone(),
        target: outer.clone(),
    };
    assert_eq!(
        on_ancestor.check(&board),
        Err(Rejection::Refused(Refusal::DependencyOnAncestor {
            entity: leaf.clone(),
            target: outer.clone()
        }))
    );

    // A move takes the subtree along and is what a later read shows.
    let mv = Change::Move {
        entity: inner.clone(),
        parent: Some(other.clone()),
    };
    assert_eq!(mv.check(&board), Ok(()));
    assert_eq!(mv.is_shown_by(&board), Some(false));
    let context = axon::lifecycle::Context {
        at: chrono::DateTime::from_timestamp(1_900_000_000, 0).unwrap(),
        recorder: None,
    };
    let record = mv.record(board.records(), context).unwrap().unwrap();
    f.insert_entry(Entry::Record(record));
    let board = f.board();
    assert_eq!(mv.is_shown_by(&board), Some(true));
    assert_eq!(board.item(&leaf).unwrap().parent, Some(inner.clone()));
    assert_eq!(
        ids(&listing::listing(&board, &Filter::default(), Layout::Tree))
            .into_iter()
            .map(|(id, depth, _)| (id, depth))
            .collect::<Vec<_>>(),
        [(outer, 0), (other, 0), (inner, 1), (leaf, 2)]
    );
}

#[test]
fn the_detail_says_which_structural_changes_the_core_accepts() {
    let mut f = Fixture::new();
    let group = f.group("会", None);
    let first = f.issue("一", Some(&group));
    let second = f.issue("二", Some(&group));
    f.needs(&second, &first);
    let finished = f.group("済", None);
    let inside = f.issue("中", Some(&finished));
    f.perform(&inside, Operation::Cancel);
    f.perform(&finished, Operation::Cancel);
    let board = f.board();

    let detail = board.detail(&group).unwrap();
    assert_eq!(detail.structure.detach, None, "nothing to take it out of");
    assert_eq!(
        detail.structure.convert,
        (
            Kind::Issue,
            Err(Rejection::Refused(Refusal::GroupWithChildren(vec![
                first.clone(),
                second.clone()
            ])))
        )
    );

    let detail = board.detail(&second).unwrap();
    assert_eq!(detail.structure.detach, Some(Ok(())));
    assert_eq!(detail.structure.convert, (Kind::Group, Ok(())));
    assert_eq!(detail.structure.removals, [Ok(())]);

    // Nothing leaves a finished Group.
    let detail = board.detail(&inside).unwrap();
    assert_eq!(
        detail.structure.detach,
        Some(Err(Rejection::Refused(Refusal::ParentClosed(
            finished.clone()
        ))))
    );
    assert_eq!(
        detail.structure.convert.1,
        Err(Rejection::Refused(Refusal::ConvertTerminal))
    );
}

#[test]
fn pickers_offer_the_project_s_own_entities_by_purpose() {
    let mut f = Fixture::new();
    let group = f.group("読書会", None);
    let issue = f.issue("会場を決める", Some(&group));
    let other = f.group("家計簿", None);
    let lone = f.issue("案内を送る", None);
    f.needs(&lone, &issue);
    let board = f.board();
    let ids = |(links, total): (Vec<Link>, usize)| {
        assert_eq!(links.len(), total);
        links.into_iter().map(|l| l.id).collect::<Vec<_>>()
    };

    // Groups for a parent, without the Entity itself or the Group it is in already.
    assert_eq!(
        ids(organize::candidates(
            &board,
            &issue,
            Purpose::Parent,
            "",
            10
        )),
        std::slice::from_ref(&other)
    );
    assert_eq!(
        ids(organize::candidates(
            &board,
            &group,
            Purpose::Parent,
            "",
            10
        )),
        std::slice::from_ref(&other)
    );
    // Any Entity for a dependency, without those it has.
    assert_eq!(
        ids(organize::candidates(
            &board,
            &lone,
            Purpose::Dependency,
            "",
            10
        )),
        [group.clone(), other.clone()]
    );
    // The search narrows by title or ID.
    assert_eq!(
        ids(organize::candidates(
            &board,
            &lone,
            Purpose::Dependency,
            "家計",
            10
        )),
        std::slice::from_ref(&other)
    );
    assert_eq!(
        ids(organize::candidates(
            &board,
            &lone,
            Purpose::Dependency,
            group.as_ref(),
            10
        )),
        std::slice::from_ref(&group)
    );
}

#[test]
fn pickers_list_the_first_candidates_and_count_the_rest() {
    let mut f = Fixture::new();
    let issue = f.issue("仕事", None);
    let groups: Vec<_> = (0..5).map(|ix| f.group(&format!("g{ix}"), None)).collect();
    let board = f.board();
    let (shown, total) = organize::candidates(&board, &issue, Purpose::Parent, "", 2);
    assert_eq!(total, 5);
    assert_eq!(
        shown.into_iter().map(|l| l.id).collect::<Vec<_>>(),
        groups[..2]
    );
}

#[test]
fn a_read_tells_whether_each_kind_of_change_was_made() {
    let mut f = Fixture::new();
    let first = f.issue("一", None);
    let second = f.issue("二", None);
    let context = || axon::lifecycle::Context {
        at: chrono::DateTime::from_timestamp(1_900_000_000, 0).unwrap(),
        recorder: None,
    };
    for change in [
        Change::AddDependency {
            entity: second.clone(),
            target: first.clone(),
        },
        Change::RemoveDependency {
            entity: second.clone(),
            target: first.clone(),
        },
        Change::Convert {
            entity: second.clone(),
            kind: Kind::Group,
        },
        Change::Transition {
            entity: first.clone(),
            operation: Operation::Start,
            to: Lifecycle::InProgress,
        },
    ] {
        let board = f.board();
        assert_eq!(change.is_shown_by(&board), Some(false), "{change:?}");
        let record = change.record(board.records(), context()).unwrap().unwrap();
        f.insert_entry(Entry::Record(record));
        assert_eq!(change.is_shown_by(&f.board()), Some(true), "{change:?}");
    }
    let missing = Change::Convert {
        entity: EntityId::try_from("axon-gone".to_string()).unwrap(),
        kind: Kind::Group,
    };
    assert_eq!(missing.is_shown_by(&f.board()), None);
}

/// The operations the state menu offers for `id`, with whether the core accepts each.
fn offered(board: &Board, id: &EntityId) -> Vec<(Operation, bool)> {
    board
        .detail(id)
        .unwrap()
        .progress
        .iter()
        .map(|step| (step.operation, step.check.is_ok()))
        .collect()
}

#[test]
fn the_menu_offers_what_leads_somewhere_from_the_kind_and_lifecycle() {
    use Operation::*;
    let mut f = Fixture::new();
    let undecided = f.create(Kind::Issue, Lifecycle::Undecided, "u", Label::Feat, None);
    let issue = f.issue("i", None);
    let group = f.group("g", None);
    let child = f.issue("c", Some(&group));
    let board = f.board();
    assert_eq!(
        offered(&board, &undecided),
        [(Accept, true), (Cancel, true)]
    );
    assert_eq!(
        offered(&board, &issue),
        [(Start, true), (Withdraw, true), (Cancel, true)]
    );
    // A Group is never started or released; it completes as the final confirmation, after
    // every child has ended.
    let detail = board.detail(&group).unwrap();
    assert_eq!(
        offered(&board, &group),
        [(Complete, false), (Withdraw, true), (Cancel, false)]
    );
    assert_eq!(
        detail.progress[0].check,
        Err(Rejection::Refused(Refusal::ChildrenNotEnded(vec![
            child.clone()
        ])))
    );

    f.perform(&child, Start);
    let board = f.board();
    assert_eq!(
        offered(&board, &child),
        [(Complete, true), (Release, true), (Cancel, true)]
    );
    assert_eq!(
        offered(&board, &group),
        [(Complete, false), (Withdraw, false), (Cancel, false)],
        "working through its child"
    );
    f.perform(&child, Complete);
    let board = f.board();
    assert_eq!(offered(&board, &child), [(Reopen, true)]);
    assert_eq!(
        offered(&board, &group),
        [(Complete, true), (Withdraw, false), (Cancel, true)]
    );
    f.perform(&group, Complete);
    let board = f.board();
    assert_eq!(offered(&board, &group), [(Reopen, true)]);
    // Under the finished Group the child stays as it is.
    assert_eq!(
        board.detail(&child).unwrap().progress[0].check,
        Err(Rejection::Refused(Refusal::ParentClosed(group.clone())))
    );
    f.perform(&issue, Cancel);
    assert_eq!(offered(&f.board(), &issue), [(Reconsider, true)]);
}

#[test]
fn each_offered_transition_agrees_with_what_the_core_writes() {
    let mut f = Fixture::new();
    let outer = f.create(Kind::Group, Lifecycle::Undecided, "o", Label::Feat, None);
    let inner = f.group("in", Some(&outer));
    let leaf = f.issue("l", Some(&inner));
    let dep = f.issue("d", None);
    let waiting = f.issue("w", None);
    f.needs(&waiting, &dep);
    let board = f.board();
    for id in [&outer, &inner, &leaf, &dep, &waiting] {
        for step in board.detail(id).unwrap().progress {
            let written = board
                .records()
                .perform(id, step.operation, None, axon::context_now())
                .map(|_| ())
                .map_err(Rejection::from);
            assert_eq!(step.check, written, "{id} {:?}", step.operation);
        }
    }
    let leaf_start = board.detail(&leaf).unwrap().progress[0].clone();
    assert_eq!(
        leaf_start.check,
        Err(Rejection::Refused(Refusal::AncestorsNotAdopted(vec![
            outer.clone()
        ])))
    );
    let waiting_start = board.detail(&waiting).unwrap().progress[0].clone();
    assert_eq!(
        waiting_start.check,
        Err(Rejection::Refused(Refusal::DependenciesNotCompleted(vec![
            dep.clone()
        ])))
    );
}

#[test]
fn a_filter_says_which_facets_leave_an_entity_out() {
    let mut f = Fixture::new();
    let done = f.create(
        Kind::Group,
        Lifecycle::NotStarted,
        "完了した会",
        Label::Docs,
        None,
    );
    f.perform(&done, Operation::Complete);
    let board = f.board();
    let mut filter = Filter::default();
    assert_eq!(
        board.exclusions(&filter, &done),
        [Exclusion::State(State::Completed)]
    );
    filter.kinds = [Kind::Issue].into();
    filter.toggle_label(Label::Docs, false);
    filter.query = "読書".into();
    assert_eq!(
        board.exclusions(&filter, &done),
        [
            Exclusion::State(State::Completed),
            Exclusion::Kind(Kind::Group),
            Exclusion::Label(Label::Docs),
            Exclusion::Query("読書".into())
        ]
    );
    assert!(!board.matches(&filter, &done));
    assert!(
        board
            .exclusions(&Filter::default(), &f.issue("x", None))
            .is_empty()
    );
}
