//! Headless UI tests of restoring where the last run left off: the window, the root, the filter
//! and the layout, on temporary roots and a temporary data directory. "Restarting" closes the
//! window and opens the main window again as `main` does.

use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon::location::Location;
use axon_gui::{
    AxonApp, WINDOW_SIZE,
    app::WINDOW_SETTLE,
    board::{Layout, State},
    project::{AppData, InstanceLock, ProjectRoot, data::SESSION_FILE},
    session::{Placement, Session, SessionFile},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, point, px, size,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn data() -> (tempfile::TempDir, Arc<InstanceLock>) {
    let dir = tempfile::tempdir().unwrap();
    let lock = AppData::at(dir.path().join("data"))
        .unwrap()
        .lock_instance()
        .unwrap();
    (dir, Arc::new(lock))
}

/// A registered management root holding an Issue for each title.
fn root(lock: &InstanceLock, dir: &Path, name: &str, titles: &[&str]) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    Location::explicit(&root).unwrap().init("axon").unwrap();
    for title in titles {
        Location::explicit(&root)
            .and_then(|location| location.open())
            .unwrap()
            .update(|header, records, _| {
                let record = records.create(
                    new_entity_id(&header.prefix)?,
                    Current {
                        kind: Kind::Issue,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: (*title).into(),
                        description: String::new(),
                        label: Label::Feat,
                        condition: None,
                        parent: None,
                        needs: BTreeSet::new(),
                    },
                    Context {
                        at: chrono::Utc::now(),
                        recorder: None,
                    },
                )?;
                Ok((vec![Entry::Record(record)], ()))
            })
            .unwrap();
    }
    lock.register(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

/// Starts the application on `lock` as `main` does, after a link that arrives before the
/// registry is read when there is one.
fn start(
    lock: &Arc<InstanceLock>,
    link: Option<&Path>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    let app = cx.update(|cx| {
        let app = axon_gui::open_main_window(lock.clone(), cx).unwrap();
        if let Some(path) = link {
            let link = axon::app_link::open_link(path).unwrap();
            axon_gui::open_links(&app.downgrade(), vec![link], cx);
        }
        app
    });
    cx.run_until_parked();
    let window = cx.update(|cx| *cx.windows().last().unwrap());
    (window, app)
}

/// Closes the window, as quitting the application does.
fn close(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
}

fn with_window(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App),
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        f(window, cx)
    })
    .unwrap();
    cx.run_until_parked();
}

fn click(handle: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.click(id, cx));
}

fn selected(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<PathBuf> {
    cx.read(|cx| {
        app.read(cx)
            .selected()
            .map(|root| root.path().to_path_buf())
    })
}

fn titles(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let board = app.read(cx).explorer().board().expect("a board");
        board
            .items()
            .iter()
            .map(|item| item.title.clone())
            .collect()
    })
}

fn bounds(handle: AnyWindowHandle, cx: &mut TestAppContext) -> WindowBounds {
    cx.update_window(handle, |_, window, _| window.window_bounds())
        .unwrap()
}

fn saved(lock: &InstanceLock) -> Session {
    SessionFile::new(lock.data().clone()).load()
}

fn save(lock: &InstanceLock, session: &Session) {
    SessionFile::new(lock.data().clone())
        .save(1, session)
        .unwrap();
}

#[gpui_kit::test]
fn a_restart_opens_the_window_root_filter_and_layout_left_last(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(&lock, dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(&lock, dir.path(), "家計簿", &["予算を決める"]);
    let (window, app) = start(&lock, None, cx);
    assert_eq!(selected(&app, cx), Some(reading.clone()));

    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select(ProjectRoot::new(budget.clone()).unwrap(), cx);
            app.set_layout(Layout::Flat, cx);
        })
    });
    cx.run_until_parked();
    click(window, "state-Undecided", cx);
    click(window, "state-Completed", cx);
    click(window, "kind-Group", cx);
    click(window, "label-docs", cx);
    // Neither the search nor the open Entity is kept.
    with_window(window, cx, |window, cx| {
        let search = app.read(cx).search_input().clone();
        search.update(cx, |search, cx| search.focus(window, cx));
        window.input("予算", cx);
    });
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            let id = app.explorer().board().unwrap().items()[0].id.clone();
            app.open_entity(id, cx);
        })
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).explorer().selected().is_some()));
    // Changes are saved as they are made, so a crash keeps them.
    assert_eq!(saved(&lock).root.unwrap().path(), budget);
    assert_eq!(saved(&lock).layout, Layout::Flat);

    // The window's place is saved once it stops changing.
    cx.simulate_window_resize(window, size(px(900.), px(640.)));
    cx.run_until_parked();
    assert_eq!(saved(&lock).placement.unwrap().width, 1120.);
    cx.executor().advance_clock(WINDOW_SETTLE);
    cx.run_until_parked();
    let placement = saved(&lock).placement.unwrap();
    assert_eq!((placement.width, placement.height), (900., 640.));
    assert!(!placement.maximized);
    let origin = bounds(window, cx).get_bounds().origin;
    // A place that has not settled yet is saved as the window closes.
    cx.simulate_window_resize(window, size(px(950.), px(650.)));
    cx.run_until_parked();
    drop(app);
    close(window, cx);

    let (window, app) = start(&lock, None, cx);
    assert_eq!(
        bounds(window, cx),
        WindowBounds::Windowed(Bounds {
            origin,
            size: size(px(950.), px(650.)),
        })
    );
    assert_eq!(selected(&app, cx), Some(budget));
    assert_eq!(titles(&app, cx), ["予算を決める"]);
    cx.read(|cx| {
        let app = app.read(cx);
        let filter = app.explorer().filter();
        assert_eq!(
            filter.states,
            BTreeSet::from([
                State::NotStarted,
                State::InProgress,
                State::Completed,
                State::Conflicted
            ])
        );
        assert_eq!(filter.kinds, BTreeSet::from([Kind::Issue]));
        assert!(!filter.labels.contains(&Label::Docs));
        assert_eq!(filter.labels.len(), Label::ALL.len() - 1);
        assert_eq!(filter.query, "");
        assert_eq!(app.search_input().read(cx).value(), "");
        assert_eq!(app.explorer().layout(), Layout::Flat);
        assert_eq!(app.explorer().selected(), None);
    });
}

#[gpui_kit::test]
fn a_session_that_cannot_be_read_starts_from_the_defaults(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(&lock, dir.path(), "読書会", &[]);
    root(&lock, dir.path(), "家計簿", &[]);
    let unreadable = br#"{"format":2,"from":"a newer build"}"#;
    fs::write(lock.data().dir().join(SESSION_FILE), unreadable).unwrap();
    let (window, app) = start(&lock, None, cx);
    // Starting alone writes nothing.
    assert_eq!(
        fs::read(lock.data().dir().join(SESSION_FILE)).unwrap(),
        unreadable
    );
    assert_eq!(selected(&app, cx), Some(reading));
    assert_eq!(
        bounds(window, cx).get_bounds().size,
        size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1))
    );
    cx.read(|cx| {
        let explorer = app.read(cx).explorer();
        assert_eq!(*explorer.filter(), Default::default());
        assert_eq!(explorer.layout(), Layout::Tree);
    });
    // Neither does closing a window that nothing changed in.
    drop(app);
    close(window, cx);
    let (_, app) = start(&lock, None, cx);
    assert_eq!(
        fs::read(lock.data().dir().join(SESSION_FILE)).unwrap(),
        unreadable
    );
    // The next change replaces the file with a readable one.
    cx.update(|cx| app.update(cx, |app, cx| app.set_layout(Layout::Flat, cx)));
    cx.run_until_parked();
    assert_eq!(saved(&lock).layout, Layout::Flat);
}

#[gpui_kit::test]
fn a_root_no_longer_registered_opens_the_first_one(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(&lock, dir.path(), "読書会", &[]);
    let budget = root(&lock, dir.path(), "家計簿", &[]);
    save(
        &lock,
        &Session {
            root: Some(ProjectRoot::new(budget.clone()).unwrap()),
            layout: Layout::Flat,
            ..Session::default()
        },
    );
    lock.unregister(&ProjectRoot::new(budget).unwrap()).unwrap();
    let (_, app) = start(&lock, None, cx);
    assert_eq!(selected(&app, cx), Some(reading.clone()));
    // The rest of the session is restored all the same.
    cx.read(|cx| assert_eq!(app.read(cx).explorer().layout(), Layout::Flat));
}

#[gpui_kit::test]
fn the_restored_root_survives_a_registry_that_could_not_be_read_at_first(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    root(&lock, dir.path(), "読書会", &[]);
    let budget = root(&lock, dir.path(), "家計簿", &[]);
    save(
        &lock,
        &Session {
            root: Some(ProjectRoot::new(budget.clone()).unwrap()),
            ..Session::default()
        },
    );
    let registry = lock
        .data()
        .dir()
        .join(axon_gui::project::data::REGISTRY_FILE);
    let roots = fs::read(&registry).unwrap();
    fs::write(&registry, b"{").unwrap();
    let (_, app) = start(&lock, None, cx);
    assert_eq!(selected(&app, cx), None);
    assert_eq!(saved(&lock).root.unwrap().path(), budget);

    fs::write(&registry, roots).unwrap();
    cx.update(|cx| app.update(cx, |app, cx| app.reload(cx)));
    cx.run_until_parked();
    assert_eq!(selected(&app, cx), Some(budget));
}

#[gpui_kit::test]
fn a_link_that_starts_the_app_opens_its_root_over_the_restored_one(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(&lock, dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(&lock, dir.path(), "家計簿", &["予算を決める"]);
    save(
        &lock,
        &Session {
            root: Some(ProjectRoot::new(reading).unwrap()),
            ..Session::default()
        },
    );
    let (_, app) = start(&lock, Some(&budget), cx);
    assert_eq!(selected(&app, cx), Some(budget.clone()));
    assert_eq!(titles(&app, cx), ["予算を決める"]);
    assert_eq!(saved(&lock).root.unwrap().path(), budget);
}

/// A plain window's place on the test platform's only display, 1920×1080 at the origin.
fn on_display(x: f32, y: f32, width: f32, height: f32, cx: &mut TestAppContext) -> Placement {
    let uuid = cx.update(|cx| cx.primary_display().unwrap().uuid().unwrap());
    Placement {
        x,
        y,
        width,
        height,
        display: Some(uuid.to_string()),
        maximized: false,
    }
}

fn restored(placement: &Placement, cx: &mut TestAppContext) -> WindowBounds {
    let (bounds, display) = cx.update(|cx| axon_gui::restored_bounds(Some(placement), cx));
    let primary = cx.update(|cx| cx.primary_display().unwrap().id());
    assert_eq!(display, Some(primary));
    bounds
}

#[gpui_kit::test]
fn a_maximized_window_opens_maximized(cx: &mut TestAppContext) {
    let placement = Placement {
        maximized: true,
        ..on_display(100., 80., 900., 600., cx)
    };
    assert_eq!(
        restored(&placement, cx),
        WindowBounds::Maximized(Bounds {
            origin: point(px(100.), px(80.)),
            size: size(px(900.), px(600.)),
        })
    );
}

#[gpui_kit::test]
fn a_full_screen_window_keeps_the_place_it_had_before(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    root(&lock, dir.path(), "読書会", &[]);
    let (window, app) = start(&lock, None, cx);
    cx.simulate_window_resize(window, size(px(900.), px(640.)));
    cx.executor().advance_clock(WINDOW_SETTLE);
    cx.run_until_parked();
    let before = saved(&lock).placement.unwrap();
    assert_eq!((before.width, before.height), (900., 640.));
    assert!(before.display.is_some());

    with_window(window, cx, |window, _| window.toggle_fullscreen());
    cx.simulate_window_resize(window, size(px(1920.), px(1080.)));
    cx.executor().advance_clock(WINDOW_SETTLE);
    cx.run_until_parked();
    assert_eq!(saved(&lock).placement, Some(before.clone()));
    drop(app);
    close(window, cx);
    assert_eq!(saved(&lock).placement, Some(before));
}

#[gpui_kit::test]
fn a_window_off_its_display_opens_centered_with_its_size(cx: &mut TestAppContext) {
    // The frame, 28 taller than the content, is centered on the 1920×1080 display.
    let centered = Bounds {
        origin: point(px(510.), px(226.)),
        size: size(px(900.), px(600.)),
    };
    for (x, y) in [(2400., 100.), (-1400., 100.), (100., 1070.), (100., -10.)] {
        assert_eq!(
            restored(&on_display(x, y, 900., 600., cx), cx),
            WindowBounds::Windowed(centered),
            "({x}, {y})"
        );
    }
    // A window partly off the display stays where it was while its title bar can be grabbed.
    assert_eq!(
        restored(&on_display(1800., 500., 900., 500., cx), cx),
        WindowBounds::Windowed(Bounds {
            origin: point(px(1800.), px(500.)),
            size: size(px(900.), px(500.)),
        })
    );
    // The size stays within the display, with room for the title bar, and above the smallest
    // window.
    assert_eq!(
        restored(&on_display(0., 0., 1e6, 1e6, cx), cx)
            .get_bounds()
            .size,
        size(px(1920.), px(1080. - axon_gui::TITLE_BAR_HEIGHT))
    );
    assert_eq!(
        restored(&on_display(0., 0., 10., 10., cx), cx)
            .get_bounds()
            .size,
        size(
            px(axon_gui::MIN_WINDOW_SIZE.0),
            px(axon_gui::MIN_WINDOW_SIZE.1)
        )
    );
}

#[gpui_kit::test]
fn a_window_on_a_display_not_connected_opens_centered_on_the_primary_one(cx: &mut TestAppContext) {
    {
        let placement = Placement {
            display: Some("00000000-0000-4000-8000-000000000000".into()),
            ..on_display(100., 100., 900., 600., cx)
        };
        let (bounds, display) = cx.update(|cx| axon_gui::restored_bounds(Some(&placement), cx));
        assert_eq!(display, None);
        assert_eq!(
            bounds,
            WindowBounds::Windowed(Bounds {
                origin: point(px(510.), px(226.)),
                size: size(px(900.), px(600.)),
            })
        );
    }
    // A place the platform named no display for is on the primary one.
    let unnamed = Placement {
        display: None,
        ..on_display(100., 100., 900., 600., cx)
    };
    assert_eq!(
        restored(&unnamed, cx),
        WindowBounds::Windowed(Bounds {
            origin: point(px(100.), px(100.)),
            size: size(px(900.), px(600.)),
        })
    );
    // A window larger than the primary display fits it, with room for the title bar.
    let large = Placement {
        display: Some("00000000-0000-4000-8000-000000000000".into()),
        ..on_display(0., 0., 2400., 1300., cx)
    };
    let (bounds, _) = cx.update(|cx| axon_gui::restored_bounds(Some(&large), cx));
    assert_eq!(
        bounds.get_bounds().size,
        size(px(1920.), px(1080. - axon_gui::TITLE_BAR_HEIGHT))
    );
}
