//! Headless UI tests of project creation, switching and restart on an independent data
//! directory. Disk access runs for real; "restart" opens a new window on the same directory.

use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::{RegistryState, StoreState, Summary},
    project::{AppData, ProjectId, Registry, Status},
};
use axon_gui::{
    Startup,
    project::{FaultPoint, InstanceError, Step},
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AppContext, Bounds, ElementId, Entity, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowHandle, WindowOptions, base::Root, px, size,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{fs, time::Duration};

type Window = WindowHandle<Root>;

fn data() -> (tempfile::TempDir, AppData) {
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data")).unwrap();
    (dir, data)
}

fn open_sized(
    data: &AppData,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (Window, Entity<AxonApp>) {
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
    open_sized(data, 920., 600., cx)
}

/// Initializes the app once, as `main` does, and opens its window.
fn start(data: &AppData, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    open(data, cx)
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

fn type_text(handle: Window, text: &str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.input(text, cx));
}

fn press(handle: Window, key: &str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.press(key, cx));
}

/// Opens the form, types `name` and presses Enter.
fn create(handle: Window, name: &str, cx: &mut TestAppContext) {
    click(handle, "new-project", cx);
    type_text(handle, name, cx);
    press(handle, "enter", cx);
}

fn registry(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Registry {
    cx.read(|cx| match app.read(cx).registry() {
        RegistryState::Loaded(registry) => registry.clone(),
        other => panic!("registry not loaded: {other:?}"),
    })
}

fn selected(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> Option<(String, Status)> {
    cx.read(|cx| {
        app.read(cx)
            .selected()
            .map(|project| (project.name.clone(), project.status))
    })
}

fn store(app: &Entity<AxonApp>, cx: &mut TestAppContext) -> StoreState {
    cx.read(|cx| app.read(cx).store().clone())
}

/// Chooses the `index`-th project from the switch menu.
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
async fn two_japanese_projects_are_created_switched_and_reopened(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (handle, app) = start(&data, cx);
    assert!(registry(&app, cx).projects().is_empty());
    assert_eq!(selected(&app, cx), None);
    assert_eq!(store(&app, cx), StoreState::None);

    create(handle, "読書会", cx);
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
    cx.read(|cx| assert!(!app.read(cx).is_form_open()));

    click(handle, "new-project", cx);
    type_text(handle, "家計簿", cx);
    click(handle, "create-project", cx);
    assert_eq!(selected(&app, cx), Some(("家計簿".into(), Status::Ready)));

    let created = registry(&app, cx);
    let [first, second] = created.projects() else {
        panic!("{created:?}");
    };
    let roots = [data.project_root(&first.id), data.project_root(&second.id)];
    assert_ne!(roots[0], roots[1]);
    for root in &roots {
        assert!(root.join(".axon/header.json").is_file(), "{root:?}");
    }

    switch_to(handle, 0, cx).await;
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);

    // A new window on the same data, as after a restart, lists the same projects.
    let (_, restarted) = open(&data, cx);
    assert_eq!(registry(&restarted, cx), created);
    assert_eq!(
        selected(&restarted, cx),
        Some(("読書会".into(), Status::Ready))
    );
    assert_eq!(store(&restarted, cx), EMPTY);
}

#[gpui_kit::test]
fn refused_names_keep_the_input_and_write_nothing(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (handle, app) = start(&data, cx);
    create(handle, "読書会", cx);

    click(handle, "new-project", cx);
    press(handle, "enter", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.is_form_open());
        assert_eq!(app.form_error(), Some("名前を入力してください。"));
    });
    type_text(handle, "読書会", cx);
    click(handle, "create-project", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.form_error(),
            Some("同じ名前のプロジェクトがすでにあります。")
        );
        assert_eq!(app.name_input().read(cx).value(), "読書会");
    });
    assert_eq!(registry(&app, cx).projects().len(), 1);
    assert_eq!(data.load_registry().unwrap().projects().len(), 1);

    click(handle, "cancel-create", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_form_open());
        assert_eq!(app.form_error(), None);
        assert_eq!(app.name_input().read(cx).value(), "");
    });
}

#[gpui_kit::test]
fn a_repeated_submission_creates_one_project(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let (handle, app) = start(&data, cx);
    click(handle, "new-project", cx);
    type_text(handle, "読書会", cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("enter", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.read(|cx| assert!(app.read(cx).is_busy()));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_busy());
        assert_eq!(app.form_error(), None);
    });
    assert_eq!(registry(&app, cx).projects().len(), 1);
    assert_eq!(data.load_registry().unwrap().projects().len(), 1);
}

#[gpui_kit::test]
async fn an_unreadable_project_is_reported_and_others_stay_reachable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    data.create_project("読書会").unwrap();
    let registry = data.create_project("家計簿").unwrap();
    let broken = registry.projects()[0].id.clone();
    fs::remove_dir_all(data.project_root(&broken)).unwrap();

    let (handle, app) = start(&data, cx);
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert!(matches!(store(&app, cx), StoreState::Failed(_)));
    assert!(
        !data.project_root(&broken).exists(),
        "reading never recreates a missing store"
    );

    switch_to(handle, 1, cx).await;
    assert_eq!(selected(&app, cx), Some(("家計簿".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
}

// Which read finishes first depends on the scheduler's seed; several seeds cover both orders.
#[gpui_kit::test(iterations = 32)]
fn only_the_last_selected_project_is_shown(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    data.create_project("読書会").unwrap();
    let registry = data.create_project("家計簿").unwrap();
    let [readable, broken] = [0, 1].map(|ix| registry.projects()[ix].id.clone());
    fs::remove_dir_all(data.project_root(&broken)).unwrap();
    let (_, app) = start(&data, cx);

    // Both reads run; whichever finishes first, the result shown is the last selection's.
    for (first, last) in [(&readable, &broken), (&broken, &readable)] {
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
fn an_interrupted_creation_is_shown_and_can_be_finished(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    // What a creation that stopped before initializing the store leaves behind.
    let id = ProjectId::from_bits(0xabc);
    let registry = Registry::default()
        .with_creating(id.clone(), "読書会")
        .unwrap();
    fs::create_dir_all(data.project_root(&id)).unwrap();
    fs::write(data.dir().join("projects.json"), registry.encode()).unwrap();

    let (handle, app) = start(&data, cx);
    assert_eq!(
        selected(&app, cx),
        Some(("読書会".into(), Status::Creating))
    );
    assert_eq!(store(&app, cx), StoreState::Incomplete { error: None });
    with_window(handle, cx, |window, _| {
        assert_eq!(
            window.find("project-switch").label(),
            Some("読書会（作成未完了） ▾")
        );
    });

    click(handle, "retry-creation", cx);
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
    assert_eq!(
        data.load_registry().unwrap().get(&id).unwrap().status,
        Status::Ready
    );
}

#[gpui_kit::test]
fn an_unreadable_registry_is_not_an_empty_list(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    fs::create_dir_all(data.dir()).unwrap();
    let path = data.dir().join("projects.json");
    fs::write(&path, "{ broken").unwrap();

    let (handle, app) = start(&data, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));
    // Neither creating nor switching is offered on a list that was not read.
    click(handle, "new-project", cx);
    cx.read(|cx| assert!(!app.read(cx).is_form_open()));
    with_window(handle, cx, |window, cx| window.click("project-switch", cx));
    with_window(handle, cx, |window, _| {
        assert!(window.try_find("popup-menu").is_none())
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");

    // Once repaired, reloading reads it.
    fs::write(&path, Registry::default().encode()).unwrap();
    click(handle, "reload", cx);
    assert!(registry(&app, cx).projects().is_empty());
}

#[gpui_kit::test]
fn smallest_main_window_keeps_projects_and_the_list_usable(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    data.create_project("とても長い名前のプロジェクトでも窓に収まる")
        .unwrap();
    cx.update(axon_gui::init);
    let (handle, _app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    click(handle, "new-project", cx);
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [
            ElementId::from("project-switch"),
            "project-name".into(),
            "create-project".into(),
            "search".into(),
            "reload-list".into(),
            "detail-empty".into(),
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
fn a_failed_creation_keeps_the_name_and_registers_nothing(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    // A file where the project directories go makes creation fail before registering.
    fs::create_dir_all(data.dir()).unwrap();
    fs::write(
        data.dir().join("projects.json"),
        Registry::default().encode(),
    )
    .unwrap();
    fs::write(data.dir().join("projects"), "").unwrap();
    let (handle, app) = start(&data, cx);
    create(handle, " 読書会 ", cx);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_busy());
        assert!(app.is_form_open());
        assert_eq!(app.name_input().read(cx).value(), " 読書会 ");
        let error = app.form_error().unwrap();
        assert!(error.contains("何も登録されていない"), "{error}");
    });
    assert!(registry(&app, cx).projects().is_empty());
    assert!(data.load_registry().unwrap().projects().is_empty());

    fs::remove_file(data.dir().join("projects")).unwrap();
    click(handle, "create-project", cx);
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    cx.read(|cx| assert!(!app.read(cx).is_form_open()));
}

/// A project left in creation whose directory holds a file the store must not replace.
fn blocked_creation(data: &AppData) -> ProjectId {
    let id = ProjectId::from_bits(0xabc);
    let registry = data
        .load_registry()
        .unwrap()
        .with_creating(id.clone(), "読書会")
        .unwrap();
    let foreign = data.project_root(&id).join(".axon/notes.txt");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, "手書きのメモ").unwrap();
    fs::write(data.dir().join("projects.json"), registry.encode()).unwrap();
    id
}

#[gpui_kit::test]
fn a_failed_retry_explains_itself_and_keeps_existing_files(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    fs::create_dir_all(data.dir()).unwrap();
    let id = blocked_creation(&data);
    let (handle, app) = start(&data, cx);
    click(handle, "retry-creation", cx);
    assert_eq!(
        selected(&app, cx),
        Some(("読書会".into(), Status::Creating))
    );
    let StoreState::Incomplete { error: Some(error) } = store(&app, cx) else {
        panic!("{:?}", store(&app, cx));
    };
    assert!(error.contains("保存先を準備できませんでした"), "{error}");
    assert_eq!(
        fs::read_to_string(data.project_root(&id).join(".axon/notes.txt")).unwrap(),
        "手書きのメモ"
    );
}

#[gpui_kit::test(iterations = 8)]
fn a_retry_result_is_not_shown_on_another_project(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let registry = data.create_project("家計簿").unwrap();
    let other = registry.projects()[0].id.clone();
    let blocked = blocked_creation(&data);
    let (_, app) = start(&data, cx);
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select(blocked, cx);
            app.retry_creation(cx);
            assert!(app.is_busy());
            app.select(other, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(selected(&app, cx), Some(("家計簿".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
    cx.read(|cx| assert!(!app.read(cx).is_busy()));
}

#[gpui_kit::test]
fn a_long_name_and_an_error_fit_in_the_smallest_window(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    let registry = data.create_project(&"長".repeat(80)).unwrap();
    fs::remove_dir_all(data.project_root(&registry.projects()[0].id)).unwrap();
    cx.update(axon_gui::init);
    let (handle, app) = open_sized(&data, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1, cx);
    assert!(matches!(store(&app, cx), StoreState::Failed(_)));
    with_window(handle, cx, |window, _| {
        let viewport = window.viewport_size();
        for id in [ElementId::from("reload"), "detail-empty".into()] {
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
fn the_main_window_holds_the_instance_lock(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    cx.update(axon_gui::init);
    let Startup::Ready {
        data: located,
        lock,
    } = axon_gui::startup_on(Ok(data.clone()))
    else {
        panic!("the first start is refused");
    };
    cx.update(|cx| {
        axon_gui::open_startup_window(
            Startup::Ready {
                data: located,
                lock,
            },
            cx,
        )
    })
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

/// Data whose creation fails at `point` while the returned flag is set.
fn failing_at(point: FaultPoint) -> (tempfile::TempDir, AppData, Arc<AtomicBool>) {
    let (dir, data) = data();
    let failing = Arc::new(AtomicBool::new(true));
    let data = data.with_fault({
        let failing = failing.clone();
        move |at| {
            if at == point && failing.load(Ordering::SeqCst) {
                Err(std::io::Error::other("injected"))
            } else {
                Ok(())
            }
        }
    });
    (dir, data, failing)
}

#[gpui_kit::test]
fn a_creation_stopped_after_registration_is_selected_and_can_be_finished(cx: &mut TestAppContext) {
    cx.update(axon_gui::init);
    for point in [
        FaultPoint::Before(Step::Initialize),
        FaultPoint::Before(Step::Finish),
        FaultPoint::Replaced(Step::Register),
    ] {
        let (_dir, data, failing) = failing_at(point);
        let (handle, app) = open(&data, cx);
        create(handle, " 読書会 ", cx);
        assert_eq!(
            selected(&app, cx),
            Some(("読書会".into(), Status::Creating))
        );
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(!app.is_form_open(), "{point:?}");
            assert_eq!(app.form_error(), None);
            assert_eq!(app.name_input().read(cx).value(), "");
        });
        let StoreState::Incomplete { error: Some(error) } = store(&app, cx) else {
            panic!("{point:?}: {:?}", store(&app, cx));
        };
        assert!(error.contains("作成を再試行"), "{error}");

        failing.store(false, Ordering::SeqCst);
        click(handle, "retry-creation", cx);
        assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
        assert_eq!(store(&app, cx), EMPTY);
    }
}

#[gpui_kit::test]
fn a_ready_project_whose_save_was_not_confirmed_is_reported(cx: &mut TestAppContext) {
    let (_dir, data, _) = failing_at(FaultPoint::Replaced(Step::Finish));
    let (handle, app) = start(&data, cx);
    create(handle, "読書会", cx);
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.is_form_open());
        let notice = app.notice().expect("a notice");
        assert!(notice.contains("確認できませんでした"), "{notice}");
    });
}

#[gpui_kit::test]
fn a_creation_failing_after_a_switch_keeps_the_choice_and_says_so(cx: &mut TestAppContext) {
    let (_dir, data, _) = failing_at(FaultPoint::Before(Step::Initialize));
    let setup = AppData::at(data.dir().to_path_buf()).unwrap();
    setup.create_project("家計簿").unwrap();
    let chosen = setup.create_project("旅行").unwrap().projects()[1]
        .id
        .clone();
    let (handle, app) = start(&data, cx);
    click(handle, "new-project", cx);
    type_text(handle, "読書会", cx);
    cx.update_window(handle.into(), |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.update(|cx| app.update(cx, |app, cx| app.select(chosen, cx)));
    cx.run_until_parked();
    assert_eq!(selected(&app, cx), Some(("旅行".into(), Status::Ready)));
    cx.read(|cx| {
        let notice = app.read(cx).notice().expect("a notice").to_owned();
        assert!(notice.contains("読書会"), "{notice}");
    });
    let incomplete = registry(&app, cx).projects()[2].clone();
    assert_eq!(incomplete.status, Status::Creating);
    cx.update(|cx| app.update(cx, |app, cx| app.select(incomplete.id, cx)));
    assert!(matches!(
        store(&app, cx),
        StoreState::Incomplete { error: Some(_) }
    ));
}

#[gpui_kit::test]
fn a_project_chosen_during_creation_stays_chosen(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    data.create_project("家計簿").unwrap();
    let chosen = data.create_project("旅行").unwrap().projects()[1]
        .id
        .clone();
    let (handle, app) = start(&data, cx);
    click(handle, "new-project", cx);
    type_text(handle, "読書会", cx);
    cx.update_window(handle.into(), |_, window, cx| window.press("enter", cx))
        .unwrap();
    // Away and back to the project shown at submission is still a choice the user made.
    let first = registry(&app, cx).projects()[0].id.clone();
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select(chosen, cx);
            app.select(first, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(registry(&app, cx).projects().len(), 3);
    assert_eq!(selected(&app, cx), Some(("家計簿".into(), Status::Ready)));
}

#[gpui_kit::test(iterations = 8)]
fn a_retry_error_stays_with_its_project(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    fs::create_dir_all(data.dir()).unwrap();
    let blocked = blocked_creation(&data);
    // Another project left in creation, with nothing in its way.
    let other = ProjectId::from_bits(0xdef);
    let registry = data
        .load_registry()
        .unwrap()
        .with_creating(other.clone(), "家計簿")
        .unwrap();
    fs::create_dir_all(data.project_root(&other)).unwrap();
    fs::write(data.dir().join("projects.json"), registry.encode()).unwrap();

    let (_, app) = start(&data, cx);
    cx.update(|cx| {
        app.update(cx, |app, cx| {
            app.select(blocked.clone(), cx);
            app.retry_creation(cx);
            app.select(other.clone(), cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        selected(&app, cx),
        Some(("家計簿".into(), Status::Creating))
    );
    assert_eq!(store(&app, cx), StoreState::Incomplete { error: None });

    // Back on the blocked project, the reason is still there.
    cx.update(|cx| app.update(cx, |app, cx| app.select(blocked, cx)));
    let StoreState::Incomplete { error: Some(error) } = store(&app, cx) else {
        panic!("{:?}", store(&app, cx));
    };
    assert!(error.contains("保存先を準備できませんでした"), "{error}");
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
    let (_dir, data) = data();
    fs::create_dir_all(data.dir()).unwrap();
    let path = data.dir().join("projects.json");
    fs::write(&path, "{ broken").unwrap();
    let (handle, app) = start(&data, cx);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));

    // Coming back while the registry is read again starts no other read.
    let before = reads(&app, cx);
    cx.update(|cx| app.update(cx, |app, cx| app.reload(cx)));
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(reads(&app, cx), before + 1);
    cx.read(|cx| assert!(matches!(app.read(cx).registry(), RegistryState::Failed(_))));

    // Coming back reads what the reload would: the registry, then the project it holds.
    fs::write(&path, Registry::default().encode()).unwrap();
    data.create_project("読書会").unwrap();
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(selected(&app, cx), Some(("読書会".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
}

#[gpui_kit::test]
fn coming_back_while_a_project_is_created_starts_no_read(cx: &mut TestAppContext) {
    let (_dir, data) = data();
    data.create_project("読書会").unwrap();
    let (handle, app) = start(&data, cx);
    assert_eq!(store(&app, cx), EMPTY);
    click(handle, "new-project", cx);
    type_text(handle, "家計簿", cx);

    let before = reads(&app, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        app.update(cx, |app, cx| app.submit(window, cx))
    })
    .unwrap();
    request_activation(handle, cx);
    cx.run_until_parked();
    assert_eq!(selected(&app, cx), Some(("家計簿".into(), Status::Ready)));
    assert_eq!(store(&app, cx), EMPTY);
    assert_eq!(
        reads(&app, cx),
        before + 1,
        "only the created project is read"
    );
}
