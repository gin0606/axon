//! Headless UI tests of the shared list and the detail pane on records written to temporary
//! management roots through the library and registered in a temporary data directory.

use axon::lifecycle::{
    Context, EntityId, Kind, Label, Lifecycle, Operation,
    record::{Current, Entry, RecordId, new_entity_id},
};
use axon::location::Location;
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{
        Columns, StoreState, Summary, THREE_COLUMNS_MIN_WIDTH, TWO_COLUMNS_MIN_WIDTH,
        entity_element, text,
    },
    board::{Layout, State, WaitKind},
    project::{AppData, InstanceLock, ProjectRoot},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowHandle, WindowOptions, base::Root, px, size,
};
use std::collections::BTreeSet;
use std::fs;
use std::sync::Arc;

type Window = WindowHandle<Root>;

/// A data directory with no registration yet, and the lock the window changes it through.
fn data() -> (tempfile::TempDir, Arc<InstanceLock>) {
    let dir = tempfile::tempdir().unwrap();
    let lock = AppData::at(dir.path().join("data"))
        .unwrap()
        .lock_instance()
        .unwrap();
    (dir, Arc::new(lock))
}

/// Writes records to one project through the core as the CLI would, one update each.
struct Seed(Location);

impl Seed {
    /// Initializes a management root named `name` beside the data directory, as `axon init`
    /// would, and registers it.
    fn new(data: &InstanceLock, name: &str) -> (ProjectRoot, Self) {
        let root = data.data().dir().parent().unwrap().join(name);
        fs::create_dir_all(&root).unwrap();
        let location = Location::explicit(&root).unwrap();
        location.init("axon").unwrap();
        let (_, registered) = data.register(&root).unwrap();
        (registered, Self(location))
    }

    fn write(&self, entry: impl FnOnce(&axon::lifecycle::record::Store, &str) -> Entry) -> Entry {
        self.0
            .open()
            .unwrap()
            .update(|header, records, _| {
                let entry = entry(records, &header.prefix);
                Ok((vec![entry.clone()], entry))
            })
            .unwrap()
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
        let (_, stale, _) = self.0.open().unwrap().read().unwrap();
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

/// Every file and directory under `dir` with its length and modification time.
fn snapshot(dir: &std::path::Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            pending.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        }
        found.push((path, metadata.len(), metadata.modified().unwrap()));
    }
    found.sort();
    found
}

fn now() -> Context {
    Context {
        at: chrono::Utc::now(),
        recorder: None,
    }
}

/// Opens the window with the system's reduced motion on, so every frame lays out where the
/// screens end up rather than where a transition is.
fn open_sized(
    data: &Arc<InstanceLock>,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    cx.update(|cx| cx.set_reduce_motion(true));
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

fn open(data: &Arc<InstanceLock>, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
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
fn an_id_from_the_cli_finds_its_row_which_starts_with_the_id(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open(&data, cx);

    // Every row's second line starts with the ID, ahead of what a narrow list truncates.
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        let board = explorer.board().unwrap();
        assert_eq!(explorer.listing().rows.len(), 3);
        for row in &explorer.listing().rows {
            let meta = text::row_meta(board.item(&row.id).unwrap(), row.matched);
            assert!(meta.starts_with(&format!("{} · ", row.id)), "{meta}");
        }
    });

    // The suffix the CLI accepts finds the Entity although its title and text do not hold it.
    let suffix = plan.invite.as_ref().rsplit_once('-').unwrap().1.to_owned();
    click(handle, "search", cx);
    with_window(handle, cx, |window, cx| window.input(&suffix, cx));
    click(handle, "layout-flat", cx);
    assert_eq!(rows(&app, cx), [("案内を送る".into(), 0, true)]);
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
    let (project, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let store = project.path().join(".axon");
    let before = snapshot(&store);
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

    // Along the dependency and back from its dependents.
    click_in(
        handle,
        "detail-dependencies",
        entity_element(&plan.venue),
        cx,
    );
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    click_in(
        handle,
        "detail-dependents",
        entity_element(&plan.invite),
        cx,
    );
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));

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
    with_window(handle, cx, |window, _| {
        assert!(window.find("detail-filtered-out").visible())
    });
    assert!(
        !rows(&app, cx)
            .iter()
            .any(|(title, _, matched)| title == "案内を送る" && *matched)
    );

    // The step back returns along the links followed, then closes the detail.
    click(handle, "close-detail", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("読書会の準備"));
    click(handle, "close-detail", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    cx.update(|cx| app.update(cx, |app, cx| app.close_entity(cx)));
    assert_eq!(detail_title(&app, cx), None);

    // Browsing and reading again leave the store exactly as it was.
    click(handle, "reload-list", cx);
    assert_eq!(snapshot(&store), before);
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
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Reloading(_))));
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
    let root = first.path().to_path_buf();
    fs::remove_file(root.join(".axon/header.json")).unwrap();
    reload(&app, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Failed(_))));
    assert!(offers_conflicted(handle, cx));
    assert!(!chosen_states(&app, cx).contains(&State::Conflicted));
}

#[gpui_kit::test]
fn an_empty_project_is_not_a_filter_without_matches(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    Seed::new(&data, "空のリポジトリ");
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
    // While the store is read again, the earlier read stays on screen.
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.store(), &StoreState::Reloading(Summary { entities: 4 }));
        assert!(app.explorer().board().is_some());
        let detail = app.explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 1);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 2);
        assert_eq!(detail.notes[1].body, "返事が来た");
    });

    // A store that can no longer be read (here: its header is gone) drops nothing silently.
    let root = cx.read(|cx| app.read(cx).selected().unwrap().path().to_path_buf());
    fs::remove_file(root.join(".axon/header.json")).unwrap();
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

/// Asserts that each element is drawn, large enough to use and inside the window.
fn usable(window: &mut gpui_kit::Window, ids: &[&'static str]) {
    let viewport = window.viewport_size();
    for id in ids {
        let element = window.find(*id);
        assert!(element.visible(), "{id:?} is hidden");
        let bounds = element.bounds();
        assert!(
            bounds.size.width >= px(20.) && bounds.size.height >= px(14.),
            "{id:?} is too small to use: {bounds:?}"
        );
        assert!(
            bounds.bottom_right().x <= viewport.width && bounds.bottom_right().y <= viewport.height,
            "{id:?} overflows the window: {bounds:?}"
        );
    }
}

/// The smallest window that shows the list and the detail side by side.
const TWO_COLUMNS: (f32, f32) = (TWO_COLUMNS_MIN_WIDTH, MIN_WINDOW_SIZE.1);

fn columns(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Columns {
    cx.read(|cx| app.read(cx).columns())
}

#[gpui_kit::test]
fn the_window_width_decides_the_columns(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    plan(&seed);
    for (width, expected) in [
        (THREE_COLUMNS_MIN_WIDTH, Columns::Three),
        (THREE_COLUMNS_MIN_WIDTH - 1., Columns::Two),
        (TWO_COLUMNS_MIN_WIDTH, Columns::Two),
        (TWO_COLUMNS_MIN_WIDTH - 1., Columns::One),
    ] {
        let (handle, app) = open_sized(&data, width, 600., cx);
        assert_eq!(columns(&app, cx), expected, "{width}");
        with_window(handle, cx, |window, _| {
            assert_eq!(
                window.try_find("sidebar").is_some(),
                expected == Columns::Three
            );
            assert_eq!(
                window.try_find("open-panel").is_some(),
                expected != Columns::Three
            );
            assert_eq!(
                window.try_find("detail-empty").is_some(),
                expected != Columns::One
            );
        });
        cx.update_window(handle.into(), |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn a_narrow_window_opens_the_roots_and_the_filters_as_a_panel(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    plan(&seed);
    let (handle, app) = open_sized(&data, TWO_COLUMNS.0, TWO_COLUMNS.1, cx);
    click(handle, "open-panel", cx);
    with_window(handle, cx, |window, _| {
        usable(
            window,
            &[
                "project-switch",
                "project-path",
                "add-root",
                "state-Undecided",
                "close-panel",
            ],
        );
    });

    // The filters change the list behind the panel, which stays open for the next one.
    click(handle, "kind-Issue", cx);
    click(handle, "kind-Group", cx);
    assert!(rows(&app, cx).is_empty());
    cx.read(|cx| assert!(app.read(cx).is_panel_open()));

    // The strip of window beside the panel, the close button and Escape all close it.
    click(handle, "panel-scrim", cx);
    cx.read(|cx| assert!(!app.read(cx).is_panel_open()));
    click(handle, "open-panel", cx);
    click(handle, "close-panel", cx);
    cx.read(|cx| assert!(!app.read(cx).is_panel_open()));
    click(handle, "open-panel", cx);
    press(handle, "escape", cx);
    cx.read(|cx| assert!(!app.read(cx).is_panel_open()));
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("sidebar").is_none())
    });
}

#[gpui_kit::test]
fn one_column_shows_the_detail_in_place_of_the_list(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    assert_eq!(columns(&app, cx), Columns::One);

    // A click opens the detail over the whole window, under a bar that goes back.
    click_in(handle, "entity-list", entity_element(&plan.invite), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("entity-list").is_none());
        usable(window, &["detail-bar", "close-detail", "detail-title"]);
        // The back button stands at the top left.
        let back = window.find("close-detail").bounds();
        assert!(
            back.origin.x < px(40.) && back.origin.y < px(60.),
            "{back:?}"
        );
    });

    // A link is pushed over the detail, and going back returns to it before the list.
    click_in(
        handle,
        "detail-dependencies",
        entity_element(&plan.venue),
        cx,
    );
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    click(handle, "close-detail", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));

    // Going back to the list keeps the selection, and the arrow keys go on from it without
    // leaving the list.
    click(handle, "close-detail", cx);
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("detail-pane").is_none());
        assert!(
            window
                .within("entity-list")
                .find(entity_element(&plan.invite))
                .visible()
        );
    });
    press(handle, "up", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("entity-list").is_some())
    });

    // Enter opens the selected row, and Escape goes back.
    press(handle, "enter", cx);
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("detail-title").is_some())
    });
    press(handle, "escape", cx);
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("entity-list").is_some())
    });
}

#[gpui_kit::test]
fn a_detail_slides_in_from_the_side_it_comes_from(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    // Where the incoming side starts, not a drawn frame: the slide runs on the wall clock, and
    // a slow machine draws its first frame after the slide has mostly played.
    let slide = |cx: &mut TestAppContext| cx.read(|cx| app.read(cx).slide_offset());
    assert_eq!(slide(cx), None);

    // Forward from the list, and along a link: from the right.
    click_in(handle, "entity-list", entity_element(&plan.invite), cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    assert!(slide(cx).is_some_and(|from| from > 0.), "{:?}", slide(cx));
    click_in(
        handle,
        "detail-dependencies",
        entity_element(&plan.venue),
        cx,
    );
    assert_eq!(detail_title(&app, cx).as_deref(), Some("会場を決める"));
    assert!(slide(cx).is_some_and(|from| from > 0.), "{:?}", slide(cx));

    // Back: from the left.
    click(handle, "close-detail", cx);
    assert_eq!(detail_title(&app, cx).as_deref(), Some("案内を送る"));
    assert!(slide(cx).is_some_and(|from| from < 0.), "{:?}", slide(cx));
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
    let list = [
        "search",
        "layout-tree",
        "layout-flat",
        "reload-list",
        "open-panel",
    ];
    let panel = ["project-switch", "state-Undecided", "close-panel"];
    for size in [MIN_WINDOW_SIZE, TWO_COLUMNS] {
        let (handle, app) = open_sized(&data, size.0, size.1, cx);
        with_window(handle, cx, |window, _| {
            usable(window, &list);
            let row = window.within("entity-list").find(entity_element(&id));
            assert!(row.visible());
            assert!(row.bounds().bottom_right().x <= window.viewport_size().width);
        });
        click_in(handle, "entity-list", entity_element(&id), cx);
        assert_eq!(detail_title(&app, cx).as_deref(), Some(long.as_str()));
        with_window(handle, cx, |window, _| usable(window, &["close-detail"]));
        if size == TWO_COLUMNS {
            with_window(handle, cx, |window, _| usable(window, &list));
        } else {
            click(handle, "close-detail", cx);
        }
        click(handle, "open-panel", cx);
        with_window(handle, cx, |window, _| usable(window, &panel));
        cx.update_window(handle.into(), |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
    }
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
    let (handle, app) = open_sized(&data, TWO_COLUMNS.0, TWO_COLUMNS.1, cx);
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

/// Brings the window to the front, as coming back to the app does.
fn activate(handle: Window, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
}

/// Sends the window to the back, as switching to another app does.
fn deactivate(handle: Window, cx: &mut TestAppContext) {
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    cx.run_until_parked();
}

fn reads(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> u64 {
    cx.read(|cx| app.read(cx).reads_started())
}

#[gpui_kit::test]
fn coming_back_to_the_window_reads_the_records_again(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let plan = plan(&seed);
    let (handle, app) = open(&data, cx);
    activate(handle, cx);
    click_in(handle, "entity-list", entity_element(&plan.invite), cx);

    // Written while the app is in the back, as the CLI would.
    let before = reads(&app, cx);
    deactivate(handle, cx);
    seed.note(&plan.invite, "返事が来た");
    seed.create(
        Kind::Issue,
        Lifecycle::NotStarted,
        "名札を作る",
        Label::Chore,
        None,
    );
    assert_eq!(reads(&app, cx), before, "leaving the window reads nothing");
    assert_eq!(rows(&app, cx).len(), 3);

    activate(handle, cx);
    assert_eq!(reads(&app, cx), before + 1);
    assert_eq!(titles(&rows(&app, cx)).last(), Some(&"名札を作る"));
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.title, "案内を送る");
        assert_eq!(detail.notes.len(), 2);
        assert_eq!(detail.notes[1].body, "返事が来た");
    });

    // A store that can no longer be read shows no earlier list.
    deactivate(handle, cx);
    let root = cx.read(|cx| app.read(cx).selected().unwrap().path().to_path_buf());
    fs::remove_file(root.join(".axon/header.json")).unwrap();
    activate(handle, cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(app.store(), StoreState::Failed(_)));
        assert!(app.explorer().board().is_none());
        assert!(app.explorer().selected().is_none());
    });
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("entity-list").is_none());
    });
}

#[gpui_kit::test]
fn reading_again_keeps_the_list_its_filter_selection_and_scroll(cx: &mut TestAppContext) {
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
    let (handle, app) = open_sized(&data, TWO_COLUMNS.0, TWO_COLUMNS.1, cx);
    click(handle, "layout-flat", cx);
    click(handle, "search", cx);
    with_window(handle, cx, |window, cx| window.input("仕事", cx));
    click_in(handle, "entity-list", entity_element(&ids[0]), cx);
    for _ in 1..ids.len() {
        press(handle, "down", cx);
    }
    seed.note(&ids[39], "書き足した");
    let listed = rows(&app, cx);

    let visible = |window: &mut gpui_kit::Window, id: &EntityId| {
        window
            .within("entity-list")
            .try_find(entity_element(id))
            .is_some_and(|row| row.visible())
    };
    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Reloading(_))));
    assert_eq!(rows(&app, cx), listed);
    with_window(handle, cx, |window, _| {
        // Checked in the frame drawn while the read runs.
        assert!(visible(window, &ids[39]));
        assert!(!visible(window, &ids[0]));
        assert!(window.try_find("detail-empty").is_none());
    });
    cx.run_until_parked();

    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(app.store(), StoreState::Loaded(_)));
        assert_eq!(app.explorer().layout(), Layout::Flat);
        assert_eq!(app.explorer().filter().query, "仕事");
        assert_eq!(app.explorer().selected(), Some(&ids[39]));
        let detail = app.explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 1);
    });
    assert_eq!(rows(&app, cx), listed);
    with_window(handle, cx, |window, _| {
        assert!(visible(window, &ids[39]));
        assert!(!visible(window, &ids[0]));
    });
}

#[gpui_kit::test]
fn coming_back_while_a_read_runs_starts_no_other(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    plan(&seed);

    // Brought to the front as it opens, while the registry is read: no read is stacked on it.
    cx.update(axon_gui::init);
    let (handle, app) = cx.update(|cx| {
        let data = data.clone();
        gpui_kit::open_window(axon_gui::main_window_options(cx), cx, |window, cx| {
            window.activate_window();
            cx.new(|cx| AxonApp::new(data, window, cx))
        })
        .unwrap()
    });
    let handle: Window = handle.downcast::<Root>().unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Loaded(_))));
    assert_eq!(reads(&app, cx), 2, "the registry and the store, once each");

    // Brought to the front while the reload runs.
    deactivate(handle, cx);
    let before = reads(&app, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    activate(handle, cx);
    assert_eq!(reads(&app, cx), before + 1);
    cx.read(|cx| assert!(matches!(app.read(cx).store(), StoreState::Loaded(_))));
}

#[gpui_kit::test]
fn reading_again_keeps_the_detail_scrolled(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (_, seed) = Seed::new(&data, "読書会");
    let id = seed.create(
        Kind::Issue,
        Lifecycle::Undecided,
        "会場を決める",
        Label::Feat,
        None,
    );
    for ix in 0..30 {
        seed.note(&id, &format!("Note {ix}: {}", "長い本文".repeat(40)));
    }
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    click_in(handle, "entity-list", entity_element(&id), cx);
    with_window(handle, cx, |window, cx| {
        let delta = gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1500.)));
        window.scroll("detail-title", delta, cx);
    });
    let title_top = |window: &mut gpui_kit::Window| window.find("detail-title").bounds().origin.y;
    let mut scrolled = px(0.);
    with_window(handle, cx, |window, _| scrolled = title_top(window));
    assert!(scrolled < px(0.), "the detail is scrolled: {scrolled:?}");

    seed.note(&id, "書き足した");
    cx.update(|cx| app.update(cx, |app, cx| app.reload_selected(cx)));
    with_window(handle, cx, |window, _| {
        // Checked in the frame drawn while the read runs.
        assert_eq!(title_top(window), scrolled);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let detail = app.read(cx).explorer().detail().unwrap().as_ref().unwrap();
        assert_eq!(detail.notes.len(), 31);
    });
    with_window(handle, cx, |window, _| {
        assert_eq!(title_top(window), scrolled)
    });
}
