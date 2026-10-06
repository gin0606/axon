//! Headless UI tests of the shared list and the detail pane on records written to independent
//! temporary projects through the library.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation,
    record::{Current, Entry, RecordId, new_entity_id},
};
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{StoreState, entity_element},
    board::{Layout, State, WaitKind},
    project::{AppData, ProjectConnection, ProjectId, WriteOutcome},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, base::Root, px, size,
};
use std::collections::BTreeSet;
use std::fs;

type Window = WindowHandle<Root>;

fn data() -> (tempfile::TempDir, AppData) {
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    (dir, data)
}

/// Writes records to one project as the CLI would, one update each.
struct Seed(ProjectConnection);

impl Seed {
    fn new(data: &AppData, name: &str) -> (ProjectId, Self) {
        let registry = data.create_project(name).unwrap();
        let project = registry.projects().last().unwrap().clone();
        (project.id.clone(), Self(data.connect(&project)))
    }

    fn write(&self, entry: impl FnOnce(&axon::lifecycle::record::Store, &str) -> Entry) -> Entry {
        let outcome = self.0.update(|header, records, _| {
            let entry = entry(records, &header.prefix);
            Ok((vec![entry.clone()], entry))
        });
        match outcome {
            WriteOutcome::Applied(entry) => entry,
            other => panic!("{other:?}"),
        }
    }

    fn create(
        &self,
        kind: Kind,
        lifecycle: Lifecycle,
        title: &str,
        label: Label,
        parent: Option<&EntityId>,
    ) -> EntityId {
        let entry = self.write(|records, prefix| {
            let id = new_entity_id(prefix).unwrap();
            let current = Current {
                kind,
                lifecycle,
                owner: None,
                title: title.into(),
                description: String::new(),
                label,
                condition: None,
                parent: parent.cloned(),
                needs: BTreeSet::new(),
            };
            Entry::Record(records.create(id, current, now()).unwrap())
        });
        entry.entity().clone()
    }

    fn perform(&self, id: &EntityId, operation: Operation) {
        self.write(|records, _| {
            Entry::Record(records.perform(id, operation, None, now()).unwrap())
        });
    }

    fn needs(&self, id: &EntityId, target: &EntityId) {
        self.write(|records, _| {
            Entry::Record(
                records
                    .add_dependency(id, target, None, now())
                    .unwrap()
                    .unwrap(),
            )
        });
    }

    /// Makes `id` conflicted: a Start and a Cancel both recorded on the same head, as two
    /// branches merged by Git would leave them. Returns the head of the Start branch.
    fn fork(&self, id: &EntityId) -> RecordId {
        let (_, stale, _) = self.0.load().unwrap();
        let started = self.write(|records, _| {
            Entry::Record(records.perform(id, Operation::Start, None, now()).unwrap())
        });
        self.write(|_, _| {
            Entry::Record(stale.perform(id, Operation::Cancel, None, now()).unwrap())
        });
        started.id().unwrap()
    }

    fn resolve(&self, id: &EntityId, chosen: &RecordId) {
        self.write(|records, _| Entry::Record(records.resolve(id, chosen, None, now()).unwrap()));
    }

    fn note(&self, id: &EntityId, body: &str) {
        self.write(|records, _| {
            Entry::Note(records.add_note(id, body.into(), None, now()).unwrap())
        });
    }
}

fn now() -> Context {
    Context {
        at: chrono::Utc::now(),
        recorder: None,
    }
}

fn open_sized(
    data: &AppData,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    let data = data.clone();
    let (window, app) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(width), px(height)),
            })),
            ..axon_gui::main_window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AxonApp::new(data, window, cx))
        })
        .expect("open the main window")
    });
    cx.run_until_parked();
    (window.downcast::<Root>().expect("Base Root"), app)
}

fn open(data: &AppData, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    open_sized(data, 1200., 760., cx)
}

fn with_window(
    handle: Window,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App),
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        f(window, cx)
    })
    .unwrap();
    cx.run_until_parked();
}

fn click(handle: Window, id: impl Into<ElementId>, cx: &mut TestAppContext) {
    let id = id.into();
    with_window(handle, cx, |window, cx| window.click(id, cx));
}

/// Clicks the element `id` inside the element `scope`.
fn click_in(handle: Window, scope: &'static str, id: ElementId, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.within(scope).click(id, cx));
}

fn press(handle: Window, key: &str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.press(key, cx));
}

/// The listed rows as (title, depth, matched).
fn rows(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<(String, usize, bool)> {
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let board = explorer.board().expect("a board");
        explorer
            .listing()
            .rows
            .iter()
            .map(|row| {
                (
                    board.item(&row.id).unwrap().title.clone(),
                    row.depth,
                    row.matched,
                )
            })
            .collect()
    })
}

fn counts(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> (usize, usize) {
    cx.read(|cx| {
        let listing = app.read(cx).explorer().listing();
        (listing.matched, listing.context)
    })
}

fn detail_title(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| {
        app.read(cx)
            .explorer()
            .detail()
            .map(|detail| detail.as_ref().unwrap().title.clone())
    })
}

fn titles(rows: &[(String, usize, bool)]) -> Vec<&str> {
    rows.iter().map(|(title, _, _)| title.as_str()).collect()
}

struct Plan {
    group: EntityId,
    venue: EntityId,
    invite: EntityId,
}

/// A Group with a started Issue and a second Issue waiting for it, and a finished Issue.
fn plan(seed: &Seed) -> Plan {
    let group = seed.create(
        Kind::Group,
        Lifecycle::NotStarted,
        "読書会の準備",
        Label::Feat,
        None,
    );
    let venue = seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "会場を決める",
        Label::Chore,
        Some(&group),
    );
    let invite = seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "案内を送る",
        Label::Docs,
        Some(&group),
    );
    seed.needs(&invite, &venue);
    seed.note(&invite, "会場の返事を待ってから送る");
    seed.perform(&venue, Operation::Start);
    let done = seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "前回の振り返り",
        Label::Docs,
        None,
    );
    seed.perform(&done, Operation::Start);
    seed.perform(&done, Operation::Complete);
    Plan {
        group,
        venue,
        invite,
    }
}

#[gpui_kit::test]
fn the_list_filters_and_switches_between_tree_and_flat(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    plan(&seed);
    let (handle, app) = open(&data, cx);

    // The default: unfinished work as a tree. The Group is InProgress from its started child.
    assert_eq!(
        rows(&app, cx),
        [
            ("読書会の準備".into(), 0, true),
            ("会場を決める".into(), 1, true),
            ("案内を送る".into(), 1, true),
        ]
    );
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let board = explorer.board().unwrap();
        let group = &board.items()[0];
        assert_eq!(group.state, State::InProgress);
    });

    click(handle, "state-Completed", cx);
    assert_eq!(titles(&rows(&app, cx)).last(), Some(&"前回の振り返り"));
    assert_eq!(counts(&app, cx), (4, 0));

    // Only the InProgress state (Conflicted, not offered without a conflict, matches nothing):
    // the Group by its effective value, and its started child.
    for state in ["state-Undecided", "state-NotStarted", "state-Completed"] {
        click(handle, state, cx);
    }
    assert_eq!(titles(&rows(&app, cx)), ["読書会の準備", "会場を決める"]);
    click(handle, "kind-Issue", cx);
    assert_eq!(titles(&rows(&app, cx)), ["読書会の準備"]);
    click(handle, "kind-Issue", cx);

    // A search matching a child shows its unmatched parent for reference only.
    click(handle, "reset-filter", cx);
    click(handle, "search", cx);
    with_window(handle, cx, |window, cx| window.input("案内", cx));
    assert_eq!(
        rows(&app, cx),
        [
            ("読書会の準備".into(), 0, false),
            ("案内を送る".into(), 1, true),
        ]
    );
    assert_eq!(counts(&app, cx), (1, 1));

    click(handle, "layout-flat", cx);
    cx.read(|cx| assert_eq!(app.read(cx).explorer().layout(), Layout::Flat));
    assert_eq!(rows(&app, cx), [("案内を送る".into(), 0, true)]);
    assert_eq!(counts(&app, cx), (1, 0));

    // Labels combine with the rest.
    click(handle, "label-docs", cx);
    assert!(rows(&app, cx).is_empty());
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("reset-filter-empty").is_some())
    });
    click(handle, "reset-filter-empty", cx);
    assert_eq!(rows(&app, cx).len(), 3);
    cx.read(|cx| assert_eq!(app.read(cx).search_input().read(cx).value(), ""));
}

#[gpui_kit::test]
fn clearing_every_state_matches_nothing(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    plan(&seed);
    let (handle, app) = open(&data, cx);
    // Every offered state; Conflicted is not offered without a conflict.
    for state in ["state-Undecided", "state-NotStarted", "state-InProgress"] {
        click(handle, state, cx);
    }
    assert!(rows(&app, cx).is_empty());
    assert_eq!(counts(&app, cx), (0, 0));
    cx.read(|cx| assert!(app.read(cx).explorer().board().unwrap().len() == 4));
}

#[gpui_kit::test]
fn the_detail_shows_relations_waits_notes_and_history(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open(&data, cx);

    click_in(handle, "entity-list", entity_element(&plan.invite), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let detail = explorer.detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.parent.as_ref().unwrap().id, plan.group);
        assert_eq!(detail.dependencies[0].id, plan.venue);
        assert_eq!(detail.waits.len(), 1);
        assert_eq!(detail.waits[0].kind, WaitKind::Dependency);
        assert_eq!(detail.notes[0].body, "会場の返事を待ってから送る");
        assert_eq!(
            detail
                .history
                .iter()
                .map(|h| h.kind.name())
                .collect::<Vec<_>>(),
            ["created", "dependency"]
        );
        assert!(explorer.selected_exclusions().is_empty());
    });

    // To the dependency from the waiting reasons, then to the parent from where it belongs.
    click_in(handle, "detail-waits", entity_element(&plan.venue), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.dependents[0].id, plan.invite);
        assert_eq!(detail.state, State::InProgress);
    });
    click_in(handle, "detail-ancestors", entity_element(&plan.group), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("読書会の準備"));
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.state, State::InProgress);
        assert_eq!(detail.stored, Some(Lifecycle::NotStarted));
        assert_eq!(
            detail
                .children
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>(),
            [plan.venue.clone(), plan.invite.clone()]
        );
    });
    click_in(handle, "detail-children", entity_element(&plan.invite), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));

    // A selection the filter leaves out stays open and says so.
    click(handle, "state-NotStarted", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    cx.read(|cx| assert!(!app.read(cx).explorer().selected_exclusions().is_empty()));
    assert!(
        !rows(&app, cx)
            .iter()
            .any(|(title, _, matched)| title == "案内を送る" && *matched)
    );

    click(handle, "close-detail", cx);
    assert_eq!(detail_title(&app, cx), None);
}

#[gpui_kit::test]
fn the_arrow_keys_move_through_the_list(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open(&data, cx);
    click_in(handle, "entity-list", entity_element(&plan.group), cx);
    press(handle, "down", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    press(handle, "down", cx);
    press(handle, "down", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    press(handle, "up", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
}

fn offers_conflicted(handle: Window, cx: &mut TestAppContext) -> bool {
    let mut offered = false;
    with_window(handle, cx, |window, _| {
        offered = window.try_find("state-Conflicted").is_some();
    });
    offered
}

fn chosen_states(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> BTreeSet<State> {
    cx.read(|cx| app.read(cx).explorer().filter().states.clone())
}

fn reload(app: &Entity<AxonApp>, cx: &mut TestAppContext) {
    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_conflicted_state_is_offered_only_while_a_conflict_exists(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let venue = seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "会場を決める",
        Label::Chore,
        None,
    );
    let (second, other) = Seed::new(&data, "家計簿");
    other.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "家計簿をつける",
        Label::Chore,
        None,
    );
    let (handle, app) = open(&data, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    assert!(!offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));

    // A conflict shows the choice, chosen, and lists the Entity.
    let started = seed.fork(&venue);
    reload(&app, cx);
    assert!(offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));
    assert_eq!(titles(&rows(&app, cx)), ["会場を決める"]);

    // Cleared, then resolved: the hidden choice is chosen again, so a conflict that appears
    // later is listed.
    click(handle, "state-Conflicted", cx);
    assert!(rows(&app, cx).is_empty());
    // Reading the same project again keeps the choice offered and cleared throughout.
    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    cx.read(|cx| assert_eq!(app.read(cx).store(), &StoreState::Loading));
    // Checked before the window, whose frame lets the read finish.
    assert!(!chosen_states(&app, cx).contains(&State::Conflicted));
    assert!(offers_conflicted(handle, cx));
    cx.run_until_parked();
    assert!(offers_conflicted(handle, cx));
    assert!(!chosen_states(&app, cx).contains(&State::Conflicted));
    assert!(rows(&app, cx).is_empty());
    seed.resolve(&venue, &started);
    reload(&app, cx);
    assert!(!offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));
    let invite = seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "案内を送る",
        Label::Docs,
        None,
    );
    seed.fork(&invite);
    reload(&app, cx);
    assert!(offers_conflicted(handle, cx));
    assert_eq!(titles(&rows(&app, cx)), ["会場を決める", "案内を送る"]);

    // A reset follows the same rule.
    click(handle, "state-Conflicted", cx);
    click(handle, "reset-filter", cx);
    assert!(offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));

    // A switch carries no project's conflicts over to another, nor a choice hidden there.
    click(handle, "state-Conflicted", cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(second.clone(), cx)));
    assert!(
        !offers_conflicted(handle, cx),
        "nothing is known yet of the project"
    );
    cx.run_until_parked();
    assert!(!offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    assert!(offers_conflicted(handle, cx));
    assert!(chosen_states(&app, cx).contains(&State::Conflicted));

    // A failed read of the same project keeps the choice as the last read left it.
    click(handle, "state-Conflicted", cx);
    let root = cx.read(|cx| app.read(cx).data().project_root(&first));
    fs::remove_dir_all(root.join(".axon/records")).unwrap();
    reload(&app, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Failed(_))));
    assert!(offers_conflicted(handle, cx));
    assert!(!chosen_states(&app, cx).contains(&State::Conflicted));
}

#[gpui_kit::test]
fn an_empty_project_is_not_a_filter_without_matches(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    Seed::new(&data, "空のプロジェクト");
    let (handle, app) = open(&data, cx);
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        assert!(explorer.board().unwrap().is_empty());
        assert!(explorer.listing().rows.is_empty());
    });
    with_window(handle, cx, |window, _| {
        // The empty project offers no filter reset: nothing is hidden.
        assert!(window.try_find("reset-filter-empty").is_none());
    });
}

#[gpui_kit::test]
fn reloading_keeps_the_selection_and_shows_new_records(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open(&data, cx);
    click_in(handle, "entity-list", entity_element(&plan.invite), cx);
    seed.note(&plan.invite, "返事が来た");

    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    // While the store is read again, nothing of the earlier read is shown as current.
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.store(), &StoreState::Loading);
        assert!(app.explorer().board().is_none());
        assert!(app.explorer().detail().is_none());
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 2);
        assert_eq!(detail.notes[1].body, "返事が来た");
    });

    // A record gone from the store (here: the whole store) drops nothing silently.
    let root = cx.read(|cx| {
        let app = app.read(cx);
        app.data().project_root(&app.selected().unwrap().id)
    });
    fs::remove_dir_all(root.join(".axon/records")).unwrap();
    click(handle, "reload-list", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(app.store(), StoreState::Failed(_)));
        assert!(
            app.explorer().board().is_none(),
            "a failed read is not an empty list"
        );
        assert!(app.explorer().selected().is_none());
    });
}

// Which read finishes first depends on the scheduler's seed; several seeds cover both orders.
#[gpui_kit::test(iterations = 32)]
fn switching_projects_never_mixes_their_records(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (first, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (second, other) = Seed::new(&data, "家計簿");
    other.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "家計簿をつける",
        Label::Chore,
        None,
    );
    let (handle, app) = open(&data, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(first.clone(), cx)));
    cx.run_until_parked();
    click_in(handle, "entity-list", entity_element(&plan.invite), cx);

    for (a, b, expected) in [
        (&first, &second, vec!["家計簿をつける"]),
        (
            &second,
            &first,
            vec!["読書会の準備", "会場を決める", "案内を送る"],
        ),
    ] {
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.select(a.clone(), cx);
                app.select(b.clone(), cx);
            })
        });
        cx.read(|cx| {
            let explorer = app.read(cx).explorer();
            assert!(explorer.board().is_none());
            assert!(
                explorer.selected().is_none(),
                "a switch drops the selection"
            );
        });
        cx.run_until_parked();
        assert_eq!(titles(&rows(&app, cx)), expected);
        assert_eq!(detail_title(&app, cx), None);
    }
}

#[gpui_kit::test]
fn the_smallest_window_keeps_the_list_and_a_long_detail_usable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let long = "とても長いタイトルが折り返されても詳細と一覧が窓に収まることを確かめる".repeat(3);
    let id = seed.create(Kind::Issue, Lifecycle::Undecided, &long, Label::Feat, None);
    for ix in 0..30 {
        seed.note(&id, &format!("Note {ix}: {}", "長い本文".repeat(40)));
    }
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    click_in(handle, "entity-list", entity_element(&id), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some(long.as_str()));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [
            ElementId::from("search"),
            "layout-tree".into(),
            "layout-flat".into(),
            "reload-list".into(),
            "close-detail".into(),
            "state-Undecided".into(),
            "project-switch".into(),
        ] {
            let element = window.find(id.clone());
            assert!(element.visible(), "{id:?} is hidden");
            let bounds = element.bounds();
            assert!(
                bounds.size.width >= px(20.) && bounds.size.height >= px(14.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
        let row = window.within("entity-list").find(entity_element(&id));
        assert!(row.visible());
        assert!(row.bounds().bottom_right().x <= viewport.width);
    });
}

#[gpui_kit::test]
fn the_row_chosen_with_the_keyboard_stays_in_view(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let ids: Vec<_> = (0..40)
        .map(|ix| {
            seed.create(
                Kind::Issue,
                Lifecycle::Undecided,
                &format!("仕事 {ix}"),
                Label::Feat,
                None,
            )
        })
        .collect();
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    click_in(handle, "entity-list", entity_element(&ids[0]), cx);
    for _ in 1..ids.len() {
        press(handle, "down", cx);
    }
    assert_eq!(detail_title(&app, cx).as_deref(), Some("仕事 39"));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        let row = window
            .within("entity-list")
            .find(entity_element(ids.last().unwrap()));
        assert!(row.visible(), "{:?}", row.bounds());
        assert!(
            row.bounds().bottom_right().y <= viewport.height,
            "{:?}",
            row.bounds()
        );
    });

    // A changed filter starts the list from its top again.
    click(handle, "layout-flat", cx);
    with_window(handle, cx, |window, _| {
        // The list starts below the layout switch.
        let top = window.find("layout-flat").bounds().bottom_right().y;
        let first = window.within("entity-list").find(entity_element(&ids[0]));
        assert!(first.visible());
        assert!(first.bounds().origin.y >= top, "{:?}", first.bounds());
    });
}

#[gpui_kit::test]
fn the_list_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let long = "一覧の行の高さを変えないほど長いタイトル".repeat(8);
    let ids: Vec<_> = (0..200)
        .map(|ix| {
            let title = if ix == 1 {
                long.clone()
            } else {
                format!("仕事 {ix}")
            };
            seed.create(Kind::Issue, Lifecycle::Undecided, &title, Label::Feat, None)
        })
        .collect();
    let (handle, app) = open(&data, cx);
    assert_eq!(rows(&app, cx).len(), ids.len());
    with_window(handle, cx, |window, _| {
        let list = window.within("entity-list");
        let first = list.find(entity_element(&ids[0])).bounds();
        let long = list.find(entity_element(&ids[1])).bounds();
        assert_eq!(first.size.height, long.size.height);
        assert!(list.try_find(entity_element(ids.last().unwrap())).is_none());
    });

    // Selecting a row out of view scrolls it in, and it opens like any other row.
    app.update(cx, |app, cx| app.open_entity(ids[150].clone(), cx));
    with_window(handle, cx, |window, _| {
        let list = window.within("entity-list");
        assert!(list.find(entity_element(&ids[150])).visible());
        assert!(list.try_find(entity_element(&ids[0])).is_none());
    });
    click_in(handle, "entity-list", entity_element(&ids[149]), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("仕事 149"));
}
