//! Headless UI tests of registering, switching and unregistering management roots, on
//! temporary roots and a temporary data directory. Disk access runs for real; folder choices
//! are answered by the test platform; "reopening" opens a new window on the same data.

use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon::location::Location;
use axon_gui::Startup;
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{RegistryState, StoreState, Summary},
    project::{AppData, InstanceError, InstanceLock, ProjectRoot, Registry, data::REGISTRY_FILE},
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowHandle, WindowOptions, base::Root, px, size,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::{fs, time::Duration};

type Window = WindowHandle<Root>;

/// A temporary directory holding the data directory `data` and the roots the test makes.
fn data() -> (tempfile::TempDir, Arc<InstanceLock>) {
    let dir = tempfile::tempdir().unwrap();
    let lock = AppData::at(dir.path().join("data"))
        .unwrap()
        .lock_instance()
        .unwrap();
    (dir, Arc::new(lock))
}

/// A management root initialized as `axon init` would, holding an Issue for each title.
fn root(dir: &Path, name: &str, titles: &[&str]) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    Location::explicit(&root).unwrap().init("axon").unwrap();
    for title in titles {
        write_issue(&root, title);
    }
    root
}

/// Writes an Issue to the store at `root` through the core, as the CLI would.
fn write_issue(root: &Path, title: &str) {
    Location::explicit(root)
        .and_then(|location| location.open())
        .unwrap()
        .update(|header, records, _| {
            let record = records.create(
                new_entity_id(&header.prefix)?,
                Current {
                    kind: Kind::Issue,
                    lifecycle: Lifecycle::NotStarted,
                    owner: None,
                    title: title.into(),
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

/// Opens the window with the system's reduced motion on.
fn open_sized(
    lock: &Arc<InstanceLock>,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (Window, Entity<AxonApp>) {
    // Every frame lays out where the panel ends up rather than where it slides.
    cx.update(|cx| cx.set_reduce_motion(true));
    let lock = lock.clone();
    let (window, app) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(width), px(height)),
            })),
            ..axon_gui::main_window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AxonApp::new(lock, window, cx))
        })
        .expect("open the main window")
    });
    cx.run_until_parked();
    (window.downcast::<Root>().expect("Base Root"), app)
}

fn open(lock: &Arc<InstanceLock>, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    open_sized(lock, 1200., 600., cx)
}

/// Initializes the app once, as `main` does, and opens its window.
fn start(lock: &Arc<InstanceLock>, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    open(lock, cx)
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

fn click(handle: Window, id: &'static str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.click(id, cx));
}

/// Clicks "＋ リポジトリを登録" and chooses `path` in the folder dialog.
fn add(handle: Window, path: Option<&Path>, cx: &mut TestAppContext) {
    click(handle, "add-root", cx);
    assert!(cx.did_prompt_for_paths(), "no folder dialog");
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files && !options.multiple);
        path.map(|path| vec![path.to_path_buf()])
    });
    cx.run_until_parked();
}

fn registered(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<ProjectRoot> {
    cx.read(|cx| match app.read(cx).registry() {
        RegistryState::Loaded(registry) => registry.roots().to_vec(),
        other => panic!("registry not loaded: {other:?}"),
    })
}

fn selected(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| app.read(cx).selected().map(ProjectRoot::name))
}

fn store(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> StoreState {
    cx.read(|cx| app.read(cx).store().clone())
}

fn notice(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.read(|cx| app.read(cx).notice().map(str::to_owned))
}

/// The titles of the Entities the window shows.
fn titles(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let board = app.read(cx).explorer().board().expect("a board");
        let mut titles: Vec<_> = board
            .items()
            .iter()
            .map(|item| item.title.clone())
            .collect();
        titles.sort();
        titles
    })
}

fn canonical(path: &Path) -> ProjectRoot {
    ProjectRoot::new(fs::canonicalize(path).unwrap()).unwrap()
}

/// Chooses the `index`-th root from the switch menu.
async fn switch_to(handle: Window, index: usize, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| {
        window.click("project-switch", cx);
        assert!(window.find("popup-menu").visible());
        window.within("popup-menu").click(index, cx);
    });
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
    cx.run_until_parked();
}

const EMPTY: StoreState = StoreState::Loaded(Summary { entities: 0 });

#[gpui_kit::test]
async fn two_roots_are_registered_switched_and_kept_after_reopening(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["会場を決める", "本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &["予算を決める"]);
    let (handle, app) = start(&lock, cx);
    assert!(registered(&app, cx).is_empty());
    assert_eq!(selected(&app, cx), None);
    assert_eq!(store(&app, cx), StoreState::None);

    add(handle, Some(&reading), cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&app, cx), ["会場を決める", "本を選ぶ"]);
    add(handle, Some(&budget), cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(titles(&app, cx), ["予算を決める"]);
    assert_eq!(
        registered(&app, cx),
        [canonical(&reading), canonical(&budget)]
    );
    with_window(handle, cx, |window, _| {
        assert_eq!(window.find("project-switch").label(), Some("家計簿 ▾"));
    });

    switch_to(handle, 0, cx).await;
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&app, cx), ["会場を決める", "本を選ぶ"]);

    // A new window on the same data, as after a restart, lists the same roots.
    let (_, reopened) = open(&lock, cx);
    assert_eq!(registered(&reopened, cx), registered(&app, cx));
    assert_eq!(selected(&reopened, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&reopened, cx), ["会場を決める", "本を選ぶ"]);
    assert_eq!(
        lock.data().load_registry().unwrap().roots(),
        registered(&app, cx)
    );
}

#[gpui_kit::test]
fn duplicates_and_folders_without_a_store_are_refused(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    let saved = fs::read(lock.data().dir().join(REGISTRY_FILE)).unwrap();

    add(handle, Some(&reading.join(".")), cx);
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains("すでに登録"), "{refused}");
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    add(handle, Some(&plain), cx);
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains(".axon"), "{refused}");
    with_window(handle, cx, |window, _| {
        assert!(window.find("notice").visible())
    });
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
    assert_eq!(registered(&app, cx), [canonical(&reading)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(
        fs::read(lock.data().dir().join(REGISTRY_FILE)).unwrap(),
        saved
    );
}

#[gpui_kit::test]
fn cancelling_the_folder_dialog_changes_nothing(cx: &mut TestAppContext) {
    let (_dir, lock) = data();
    let (handle, app) = start(&lock, cx);
    add(handle, None, cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_busy());
        assert_eq!(app.notice(), None);
    });
    assert!(registered(&app, cx).is_empty());
    assert!(!lock.data().dir().join(REGISTRY_FILE).exists());
}

#[gpui_kit::test]
fn unregistering_keeps_the_files_and_selects_another_root(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["会場を決める"]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    add(handle, Some(&budget), cx);
    let before = fs::read_dir(reading.join(".axon/records")).unwrap().count();

    cx.update(|cx| app.update(cx, |app, cx| app.select(canonical(&reading), cx)));
    click(handle, "remove-root", cx);
    assert_eq!(registered(&app, cx), [canonical(&budget)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(store(&app, cx), EMPTY);
    assert_eq!(
        lock.data().load_registry().unwrap().roots(),
        [canonical(&budget)]
    );
    assert!(reading.join(".axon/header.json").is_file());
    assert_eq!(
        fs::read_dir(reading.join(".axon/records")).unwrap().count(),
        before
    );

    click(handle, "remove-root", cx);
    assert!(registered(&app, cx).is_empty());
    assert_eq!(store(&app, cx), StoreState::None);
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("remove-root").is_none())
    });
}

#[gpui_kit::test]
async fn a_missing_root_is_reported_and_others_stay_reachable(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["会場を決める"]);
    let budget = root(dir.path(), "家計簿", &["予算を決める"]);
    lock.register(&reading).unwrap();
    lock.register(&budget).unwrap();
    fs::remove_dir_all(&reading).unwrap();

    let (handle, app) = start(&lock, cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    let StoreState::Failed(error) = store(&app, cx) else {
        panic!("{:?}", store(&app, cx));
    };
    assert!(error.contains("missing"), "{error}");
    cx.read(|cx| assert!(app.read(cx).explorer().board().is_none()));
    assert!(!reading.exists(), "reading never recreates a missing root");

    switch_to(handle, 1, cx).await;
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(titles(&app, cx), ["予算を決める"]);
}

#[gpui_kit::test]
fn a_root_inside_a_git_repository_is_registered_and_read(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let repository = dir.path().join("repository");
    fs::create_dir(&repository).unwrap();
    let mut command = Command::new("git");
    // A Git hook running the tests sets variables that would point Git at another repository.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let status = command
        .args(["init", "-q"])
        .current_dir(&repository)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let inside = root(&repository, "sub", &["Git の中の記録"]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&inside), cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("sub"));
    assert_eq!(titles(&app, cx), ["Git の中の記録"]);
}

// Which read finishes first depends on the scheduler's seed; several seeds cover both orders.
#[gpui_kit::test(iterations = 32)]
fn only_the_last_selected_root_is_shown(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let (_, readable) = lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    let broken = root(dir.path(), "家計簿", &[]);
    let (_, missing) = lock.register(&broken).unwrap();
    fs::remove_dir_all(&broken).unwrap();
    let (_, app) = start(&lock, cx);

    // Both reads run; whichever finishes first, the result shown is the last selection's.
    for (first, last) in [(&readable, &missing), (&missing, &readable)] {
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.select(first.clone(), cx);
                app.select(last.clone(), cx);
            })
        });
        assert_eq!(store(&app, cx), StoreState::Loading);
        cx.run_until_parked();
        let shown = store(&app, cx);
        if last == &readable {
            assert_eq!(shown, EMPTY);
        } else {
            assert!(matches!(shown, StoreState::Failed(_)), "{shown:?}");
        }
    }
}

#[gpui_kit::test]
fn a_root_registered_while_another_was_chosen_leaves_the_choice(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let (_, first) = lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    let (_, second) = lock.register(&root(dir.path(), "家計簿", &[])).unwrap();
    let added = root(dir.path(), "旅行", &[]);
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(second.clone(), cx)));
    cx.simulate_path_prompt_response(|_| Some(vec![added.clone()]));
    cx.run_until_parked();
    assert_eq!(
        registered(&app, cx),
        [first, second.clone(), canonical(&added)]
    );
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
}

#[gpui_kit::test]
fn an_unreadable_registry_is_not_an_empty_list(cx: &mut TestAppContext) {
    let (_dir, lock) = data();
    let path = lock.data().dir().join(REGISTRY_FILE);
    fs::write(&path, "{ broken").unwrap();

    let (handle, app) = start(&lock, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));
    // Neither registering nor switching is offered on a list that was not read.
    click(handle, "add-root", cx);
    assert!(!cx.did_prompt_for_paths());
    with_window(handle, cx, |window, cx| window.click("project-switch", cx));
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");

    // Once repaired, reloading reads it.
    fs::write(&path, Registry::default().encode()).unwrap();
    click(handle, "reload", cx);
    assert!(registered(&app, cx).is_empty());
}

#[gpui_kit::test]
fn smallest_main_window_keeps_the_roots_and_the_list_usable(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let long = "とても長い名前のリポジトリでも窓に収まる".repeat(3);
    lock.register(&root(dir.path(), &long, &[])).unwrap();
    cx.update(axon_gui::init);
    let (handle, _app) = open_sized(&lock, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    // The roots are switched and registered from the panel a narrow window opens.
    with_window(handle, cx, |window, cx| window.click("open-panel", cx));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [
            ElementId::from("project-switch"),
            "add-root".into(),
            "remove-root".into(),
            "close-panel".into(),
        ] {
            let element = window.find(id.clone());
            assert!(element.visible(), "{id:?} is hidden");
            let bounds = element.bounds();
            assert!(
                bounds.size.width >= px(30.) && bounds.size.height >= px(20.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
    });
    with_window(handle, cx, |window, cx| window.click("close-panel", cx));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [
            ElementId::from("open-panel"),
            "search".into(),
            "reload-list".into(),
        ] {
            let element = window.find(id.clone());
            assert!(element.visible(), "{id:?} is hidden");
            let bounds = element.bounds();
            assert!(
                bounds.size.width >= px(30.) && bounds.size.height >= px(20.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
    });
}

#[gpui_kit::test]
fn a_long_name_and_an_error_fit_in_the_smallest_window(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let missing = root(dir.path(), &"長".repeat(80), &[]);
    lock.register(&missing).unwrap();
    fs::remove_dir_all(&missing).unwrap();
    cx.update(axon_gui::init);
    let (handle, app) = open_sized(&lock, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    assert!(matches!(store(&app, cx), StoreState::Failed(_)));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [ElementId::from("reload"), "open-panel".into()] {
            let bounds = window.find(id.clone()).bounds();
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{id:?} overflows the window: {bounds:?}"
            );
            assert!(bounds.size.height >= px(20.), "{id:?}: {bounds:?}");
        }
    });
}

#[gpui_kit::test]
fn a_refused_start_explains_itself(cx: &mut TestAppContext) {
    cx.update(axon_gui::init);
    let message = "Axon はすでに起動しています。";
    let notice = cx
        .update(|cx| axon_gui::open_notice_window(message.into(), cx))
        .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(notice.read(cx).message(), message));
    let handle = cx.update(|cx| cx.windows()[0]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("quit").visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_main_window_holds_the_instance_lock(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    cx.update(axon_gui::init);
    let Startup::Ready(lock) = axon_gui::startup_on(Ok(data.clone())) else {
        panic!("the first start is refused");
    };
    cx.update(|cx| axon_gui::open_startup_window(Startup::Ready(lock), cx))
        .unwrap();
    cx.run_until_parked();
    assert!(matches!(
        data.lock_instance(),
        Err(InstanceError::AlreadyRunning)
    ));
    let Startup::Refused(message) = axon_gui::startup_on(Ok(data.clone())) else {
        panic!("a second start is not refused");
    };
    assert!(message.contains("すでに起動しています"), "{message}");
}

/// Brings the window to the front, as coming back to the app does; the activation is
/// delivered once the caller lets the window run.
fn request_activation(handle: Window, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, _| window.activate_window())
        .unwrap();
}

fn reads(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> u64 {
    cx.read(|cx| app.read(cx).reads_started())
}

#[gpui_kit::test]
fn coming_back_reads_an_unreadable_registry_again(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let path = lock.data().dir().join(REGISTRY_FILE);
    fs::write(&path, "{ broken").unwrap();
    let (handle, app) = start(&lock, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));

    // Coming back while the registry is read again starts no other read.
    let before = reads(&app, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.reload(cx)));
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(reads(&app, cx), before + 1);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));

    // Coming back reads what the reload would: the registry, then the root it holds.
    fs::write(&path, Registry::default().encode()).unwrap();
    lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(store(&app, cx), EMPTY);
}

#[gpui_kit::test]
fn coming_back_while_a_folder_is_chosen_starts_no_read(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    let added = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    assert_eq!(store(&app, cx), EMPTY);

    let before = reads(&app, cx);
    click(handle, "add-root", cx);
    cx.read(|cx| assert!(app.read(cx).is_busy()));
    // The dialog closing brings the window back to the front.
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(reads(&app, cx), before);
    cx.simulate_path_prompt_response(|_| Some(vec![added.clone()]));
    cx.run_until_parked();
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(store(&app, cx), EMPTY);
    assert_eq!(
        reads(&app, cx),
        before + 1,
        "only the registered root is read"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_registration_that_cannot_be_saved_shows_the_list_on_disk(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let (dir, lock) = data();
    lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    let added = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    // Writable again however the test ends, so the temporary directory can be removed.
    struct ReadOnly(PathBuf);
    impl Drop for ReadOnly {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let data_dir = ReadOnly(lock.data().dir().to_path_buf());
    fs::set_permissions(&data_dir.0, fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::File::create(data_dir.0.join("probe")).is_ok();
    add(handle, Some(&added), cx);
    drop(data_dir);
    if writable {
        return; // Permissions do not apply to this user (root); nothing to observe.
    }
    let saved = notice(&app, cx).expect("a notice");
    assert!(saved.contains("保存できませんでした"), "{saved}");
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
    assert_eq!(
        registered(&app, cx),
        lock.data().load_registry().unwrap().roots()
    );
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
}

#[gpui_kit::test]
fn records_written_while_a_folder_is_chosen_show_after_cancelling(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["会場を決める"]);
    lock.register(&reading).unwrap();
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    // The CLI writes while the dialog is open; coming back to the window reads nothing yet.
    write_issue(&reading, "本を選ぶ");
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    request_activation(handle, cx);
    cx.run_until_parked();
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(titles(&app, cx), ["会場を決める", "本を選ぶ"]);
}

#[gpui_kit::test]
fn a_root_no_longer_registered_cannot_be_selected(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let (_, first) = lock.register(&root(dir.path(), "読書会", &[])).unwrap();
    let (_, second) = lock.register(&root(dir.path(), "家計簿", &[])).unwrap();
    let (handle, app) = start(&lock, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.select(second.clone(), cx)));
    click(handle, "remove-root", cx);
    // A menu opened before the unregistration still offers the removed root.
    cx.update(|cx| app.update(cx, |app, cx| app.select(second, cx)));
    cx.run_until_parked();
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(registered(&app, cx), [first]);
}

#[gpui_kit::test]
fn a_refused_change_shows_the_list_on_disk(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let (_, first) = lock.register(&reading).unwrap();
    let (_, second) = lock.register(&root(dir.path(), "家計簿", &[])).unwrap();
    let (handle, app) = start(&lock, cx);
    // The list changes on disk behind the window.
    lock.unregister(&first).unwrap();
    click(handle, "remove-root", cx);
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains("登録されていません"), "{refused}");
    assert_eq!(registered(&app, cx), std::slice::from_ref(&second));
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));

    lock.register(&reading).unwrap();
    lock.unregister(&first).unwrap();
    lock.register(&reading).unwrap();
    add(handle, Some(&reading), cx);
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains("すでに登録"), "{refused}");
    assert_eq!(registered(&app, cx), [second, first]);
}

/// Answers the question whether to register the root a link names with "register".
fn confirm(cx: &mut TestAppContext) {
    assert!(cx.has_pending_prompt(), "no confirmation");
    cx.simulate_prompt_answer(axon_gui::app::REGISTER_ANSWER);
    cx.run_until_parked();
}

/// The link `axon gui` sends for `path`, which it has resolved.
fn link(path: &Path) -> String {
    axon::app_link::open_link(&fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .unwrap()
}

/// Hands `path` to the app as `axon gui` does, through the link the app receives.
fn open_link(app: &Entity<AxonApp>, path: &Path, cx: &mut TestAppContext) {
    let link = link(path);
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_link_that_starts_the_app_registers_and_opens_its_root(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &["予算を決める"]);
    lock.register(&reading).unwrap();
    cx.update(axon_gui::init);
    // The link arrives before the window has read the registry.
    let link = link(&budget);
    let app = cx.update(|cx| {
        let lock = lock.clone();
        let (_, app) =
            gpui_kit::open_window(axon_gui::main_window_options(cx), cx, |window, cx| {
                cx.new(|cx| AxonApp::new(lock, window, cx))
            })
            .unwrap();
        axon_gui::open_links(
            &app.downgrade(),
            vec!["https://example.com".into(), link],
            cx,
        );
        app
    });
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(
        registered(&app, cx),
        [canonical(&reading), canonical(&budget)]
    );
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(titles(&app, cx), ["予算を決める"]);
    assert_eq!(notice(&app, cx), None);
}

#[gpui_kit::test]
fn a_link_to_a_registered_root_switches_to_it_without_a_notice(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    add(handle, Some(&budget), cx);
    let saved = fs::read(lock.data().dir().join(REGISTRY_FILE)).unwrap();

    open_link(&app, &reading, cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&app, cx), ["本を選ぶ"]);
    assert_eq!(notice(&app, cx), None);
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
    assert_eq!(
        fs::read(lock.data().dir().join(REGISTRY_FILE)).unwrap(),
        saved
    );
}

#[gpui_kit::test]
fn a_link_to_a_folder_without_a_store_is_refused(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);

    open_link(&app, &plain, cx);
    confirm(cx);
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains(".axon"), "{refused}");
    assert_eq!(registered(&app, cx), [canonical(&reading)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
}

#[gpui_kit::test]
fn a_link_waits_for_the_folder_dialog(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    assert!(cx.did_prompt_for_paths(), "no folder dialog");
    open_link(&app, &budget, cx);
    assert!(registered(&app, cx).is_empty());

    cx.simulate_path_prompt_response(|_| Some(vec![reading.clone()]));
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(
        registered(&app, cx),
        [canonical(&reading), canonical(&budget)]
    );
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
}

#[gpui_kit::test]
fn a_link_waits_for_an_unregistration(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    let link = link(&budget);
    cx.update(|cx| {
        app.update(cx, |app, cx| app.remove_selected(cx));
        axon_gui::open_links(&app.downgrade(), vec![link], cx);
    });
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(registered(&app, cx), [canonical(&budget)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
}

#[gpui_kit::test]
fn a_link_reads_an_unreadable_registry_again(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let path = lock.data().dir().join(REGISTRY_FILE);
    fs::write(&path, "{ broken").unwrap();
    let (_, app) = start(&lock, cx);

    // Still unreadable: nothing is registered and the link says why.
    open_link(&app, &reading, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains("読書会"), "{refused}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");

    // Repaired meanwhile: the link alone reads it and opens the root.
    fs::write(&path, Registry::default().encode()).unwrap();
    open_link(&app, &reading, cx);
    confirm(cx);
    assert_eq!(registered(&app, cx), [canonical(&reading)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(notice(&app, cx), None);
}

#[gpui_kit::test]
fn a_failed_waiting_link_adds_its_reason_to_the_one_shown(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let gone = root(dir.path(), "旅行", &[]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    open_link(&app, &gone, cx);
    fs::remove_dir_all(&gone).unwrap();

    cx.simulate_path_prompt_response(|_| Some(vec![plain.clone()]));
    cx.run_until_parked();
    confirm(cx);
    assert!(registered(&app, cx).is_empty());
    let shown = notice(&app, cx).expect("a notice");
    assert!(shown.contains("plain"), "{shown}");
    assert!(shown.contains("旅行"), "{shown}");
}

#[gpui_kit::test]
async fn a_root_chosen_after_the_link_arrived_stays_selected(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let trip = root(dir.path(), "旅行", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    add(handle, Some(&budget), cx);
    click(handle, "add-root", cx);
    open_link(&app, &trip, cx);
    switch_to(handle, 0, cx).await;

    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(
        registered(&app, cx),
        [canonical(&reading), canonical(&budget), canonical(&trip)]
    );
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
}

#[gpui_kit::test]
fn only_the_last_waiting_link_is_opened(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let trip = root(dir.path(), "旅行", &[]);
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    assert!(cx.did_prompt_for_paths(), "no folder dialog");
    open_link(&app, &reading, cx);
    let links = [&budget, &trip].map(|path| link(path));
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), links.to_vec(), cx));

    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(registered(&app, cx), [canonical(&trip)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("旅行"));
}

#[gpui_kit::test]
fn a_waiting_link_keeps_the_reason_a_registration_failed(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    let (handle, app) = start(&lock, cx);
    click(handle, "add-root", cx);
    open_link(&app, &reading, cx);

    cx.simulate_path_prompt_response(|_| Some(vec![plain.clone()]));
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(registered(&app, cx), [canonical(&reading)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains(".axon"), "{refused}");
}

fn active(handle: Window, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| cx.active_window() == Some(handle.into()))
}

fn deactivate(handle: Window, cx: &mut TestAppContext) {
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_link_brings_the_window_forward_unless_a_folder_dialog_waits(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);

    deactivate(handle, cx);
    open_link(&app, &reading, cx);
    assert!(active(handle, cx));

    // A change other than the dialog does not keep the window back.
    deactivate(handle, cx);
    let link = link(&budget);
    cx.update(|cx| {
        app.update(cx, |app, cx| app.remove_selected(cx));
        axon_gui::open_links(&app.downgrade(), vec![link], cx);
    });
    cx.run_until_parked();
    confirm(cx);
    assert!(active(handle, cx));

    // The folder dialog the link waits for stays in front.
    click(handle, "add-root", cx);
    deactivate(handle, cx);
    open_link(&app, &reading, cx);
    assert!(!active(handle, cx));
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    confirm(cx);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    cx.read(|cx| assert!(!app.read(cx).is_choosing_folder()));

    // Once the dialog is closed, a link brings the window forward again.
    deactivate(handle, cx);
    open_link(&app, &budget, cx);
    assert!(active(handle, cx));
}

#[gpui_kit::test]
fn a_failed_link_leaves_a_detail_that_hides_its_reason(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    lock.register(&reading).unwrap();
    cx.update(axon_gui::init);
    let (handle, app) = open_sized(&lock, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    let id = cx.read(|cx| {
        app.read(cx).explorer().board().unwrap().items()[0]
            .id
            .clone()
    });
    cx.update(|cx| app.update(cx, |app, cx| app.open_entity(id, cx)));
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).is_detail_shown()));

    open_link(&app, &plain, cx);
    confirm(cx);
    cx.read(|cx| assert!(!app.read(cx).is_detail_shown()));
    with_window(handle, cx, |window, _| {
        assert!(window.find("notice").visible())
    });
}

#[gpui_kit::test]
fn a_failed_link_keeps_a_detail_beside_the_list(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let plain = dir.path().join("plain");
    fs::create_dir(&plain).unwrap();
    lock.register(&reading).unwrap();
    cx.update(axon_gui::init);
    let (handle, app) = open_sized(&lock, 1200., 600., cx);
    let id = cx.read(|cx| {
        app.read(cx).explorer().board().unwrap().items()[0]
            .id
            .clone()
    });
    cx.update(|cx| app.update(cx, |app, cx| app.open_entity(id, cx)));
    cx.run_until_parked();

    open_link(&app, &plain, cx);
    confirm(cx);
    cx.read(|cx| assert!(app.read(cx).explorer().detail().is_some()));
    with_window(handle, cx, |window, _| {
        assert!(window.find("notice").visible());
        assert!(window.try_find("detail-pane").is_some());
    });
}

#[gpui_kit::test]
fn a_link_registers_a_new_root_only_when_the_user_agrees(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);

    open_link(&app, &budget, cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("キャンセル");
    cx.run_until_parked();
    assert_eq!(registered(&app, cx), [canonical(&reading)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert!(
        !lock
            .data()
            .load_registry()
            .unwrap()
            .contains(&canonical(&budget))
    );
    cx.read(|cx| assert!(!app.read(cx).is_busy()));

    // A root already registered opens without asking.
    open_link(&app, &budget, cx);
    confirm(cx);
    open_link(&app, &reading, cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
}

/// The detail of the confirmation on screen.
fn asked(cx: &mut TestAppContext) -> String {
    cx.pending_prompt().expect("a confirmation").1
}

#[gpui_kit::test]
fn a_link_that_starts_the_app_opens_a_registered_root_without_asking(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &[]);
    lock.register(&reading).unwrap();
    lock.register(&budget).unwrap();
    cx.update(axon_gui::init);
    let link = link(&budget);
    let app = cx.update(|cx| {
        let lock = lock.clone();
        let (_, app) =
            gpui_kit::open_window(axon_gui::main_window_options(cx), cx, |window, cx| {
                cx.new(|cx| AxonApp::new(lock, window, cx))
            })
            .unwrap();
        axon_gui::open_links(&app.downgrade(), vec![link], cx);
        app
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
    assert_eq!(notice(&app, cx), None);
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
}

#[gpui_kit::test]
fn the_confirmation_shows_the_path_it_registers_and_a_link_waits_for_it(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (_, app) = start(&lock, cx);

    open_link(&app, &reading, cx);
    assert!(asked(cx).starts_with(&canonical(&reading).to_string()));
    // A second link waits behind the question instead of asking at once.
    open_link(&app, &budget, cx);
    assert!(asked(cx).starts_with(&canonical(&reading).to_string()));
    cx.simulate_prompt_answer(axon_gui::app::CANCEL_ANSWER);
    cx.run_until_parked();
    assert!(asked(cx).starts_with(&canonical(&budget).to_string()));
    confirm(cx);
    assert_eq!(registered(&app, cx), [canonical(&budget)]);
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_path_that_resolves_elsewhere_is_not_registered(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let alias = fs::canonicalize(dir.path()).unwrap().join("alias");
    std::os::unix::fs::symlink(&reading, &alias).unwrap();
    let (_, app) = start(&lock, cx);

    // The question names the path the link gave, and that path alone is registered.
    let link = axon::app_link::open_link(&alias).unwrap();
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
    assert!(asked(cx).starts_with(&alias.display().to_string()));
    confirm(cx);
    assert!(registered(&app, cx).is_empty());
    let refused = notice(&app, cx).expect("a notice");
    assert!(refused.contains("指している"), "{refused}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_confirmation_escapes_what_could_pass_for_other_text(cx: &mut TestAppContext) {
    let (_dir, lock) = data();
    let (_, app) = start(&lock, cx);
    let link = axon::app_link::open_link(Path::new(
        "/tmp/a\n\nもう安全です\u{202e}\u{2028}\u{200b}\\b",
    ))
    .unwrap();
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
    let shown = asked(cx);
    let first = shown.lines().next().unwrap();
    assert_eq!(
        first,
        "/tmp/a\\n\\nもう安全です\\u{202e}\\u{2028}\\u{200b}\\\\b"
    );
    cx.simulate_prompt_answer(axon_gui::app::CANCEL_ANSWER);
    cx.run_until_parked();
    assert!(registered(&app, cx).is_empty());
}

#[gpui_kit::test]
fn a_root_waiting_for_the_answer_is_not_read(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &["本を選ぶ"]);
    let budget = root(dir.path(), "家計簿", &["予算を決める"]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);

    open_link(&app, &budget, cx);
    assert!(cx.has_pending_prompt());
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&app, cx), ["本を選ぶ"]);
    cx.simulate_prompt_answer(axon_gui::app::CANCEL_ANSWER);
    cx.run_until_parked();
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(titles(&app, cx), ["本を選ぶ"]);
}

#[gpui_kit::test]
async fn a_root_chosen_after_a_link_to_a_registered_root_stays_selected(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    add(handle, Some(&budget), cx);
    click(handle, "add-root", cx);
    open_link(&app, &budget, cx);
    switch_to(handle, 0, cx).await;

    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert_eq!(notice(&app, cx), None);
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
}

#[gpui_kit::test]
fn a_notice_escapes_the_path_a_link_gave(cx: &mut TestAppContext) {
    let (_dir, lock) = data();
    fs::write(lock.data().dir().join(REGISTRY_FILE), "{ broken").unwrap();
    let (_, app) = start(&lock, cx);
    let link = axon::app_link::open_link(Path::new("/x\n\n偽の案内\u{202e}\\y")).unwrap();
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
    let shown = notice(&app, cx).expect("a notice");
    assert!(shown.contains("/x\\n\\n偽の案内\\u{202e}\\\\y"), "{shown}");
    assert!(
        !shown.contains('\n') && !shown.contains('\u{202e}'),
        "{shown}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn whether_a_root_is_registered_is_decided_by_its_path_alone(cx: &mut TestAppContext) {
    let (dir, lock) = data();
    let reading = root(dir.path(), "読書会", &[]);
    let budget = root(dir.path(), "家計簿", &[]);
    let alias = fs::canonicalize(dir.path()).unwrap().join("alias");
    std::os::unix::fs::symlink(&reading, &alias).unwrap();
    let (handle, app) = start(&lock, cx);
    add(handle, Some(&reading), cx);
    add(handle, Some(&budget), cx);

    // Another name for a registered root is asked about like any other path.
    let link = axon::app_link::open_link(&alias).unwrap();
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer(axon_gui::app::CANCEL_ANSWER);
    cx.run_until_parked();
    assert_eq!(selected(&app, cx).as_deref(), Some("家計簿"));

    // A registered root that is gone for now is selected without asking, and shows why it
    // cannot be read.
    let gone = link_target(&reading);
    fs::remove_dir_all(&reading).unwrap();
    let link = axon::app_link::open_link(&gone).unwrap();
    cx.update(|cx| axon_gui::open_links(&app.downgrade(), vec![link], cx));
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(selected(&app, cx).as_deref(), Some("読書会"));
    assert!(matches!(store(&app, cx), StoreState::Failed(_)));
    assert_eq!(
        registered(&app, cx),
        [canonical_path(&gone), canonical(&budget)]
    );
}

fn link_target(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

fn canonical_path(path: &Path) -> ProjectRoot {
    ProjectRoot::new(path.to_path_buf()).unwrap()
}
