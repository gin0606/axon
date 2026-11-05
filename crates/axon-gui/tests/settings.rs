//! Language switching is kept in one UI test: component locale is process-wide.

use axon::lifecycle::{
    Context as RecordContext, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon_gui::{
    AxonApp, Language,
    project::{AppData, InstanceLock},
    settings::{self, LanguagePreference as Preference, LoadError, SETTINGS_FILE, SettingsFile},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Entity, Focusable, TestAppContext, Window, px, size,
};
use std::{fs, sync::Arc};

fn data() -> (tempfile::TempDir, Arc<InstanceLock>) {
    let dir = tempfile::tempdir().unwrap();
    let lock = Arc::new(
        AppData::at(dir.path().join("data"))
            .unwrap()
            .lock_instance()
            .unwrap(),
    );
    (dir, lock)
}

#[test]
fn preferences_are_independent_and_reads_never_repair_files() {
    let (_dir, lock) = data();
    let path = lock.data().dir().join(SETTINGS_FILE);
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::System);
    assert!(!path.exists());
    let roots = br#"{"format":1,"roots":[]}"#;
    let session = axon_gui::session::Session::default().encode();
    fs::write(lock.data().dir().join("roots.json"), roots).unwrap();
    fs::write(lock.data().dir().join("session.json"), &session).unwrap();
    let file = SettingsFile::new(lock.clone());
    for (generation, preference) in [
        (1, Preference::Japanese),
        (2, Preference::English),
        (3, Preference::System),
    ] {
        file.save(generation, preference).unwrap();
        assert_eq!(settings::load(lock.data()).unwrap(), preference);
    }
    assert_eq!(
        fs::read(lock.data().dir().join("roots.json")).unwrap(),
        roots
    );
    assert_eq!(
        fs::read(lock.data().dir().join("session.json")).unwrap(),
        session
    );
    assert_eq!(
        Preference::System.resolve(Language::Japanese),
        Language::Japanese
    );
    assert_eq!(
        Preference::System.resolve(Language::English),
        Language::English
    );
    assert_eq!(
        Preference::Japanese.resolve(Language::English),
        Language::Japanese
    );
    for bytes in [
        b"broken".as_slice(),
        br#"{"format":2,"language":"ja"}"#,
        br#"{"format":1,"language":"fr"}"#,
        br#"{"format":1,"language":"ja","future":true}"#,
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            settings::load(lock.data()),
            Err(LoadError::Invalid(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(
        settings::load(lock.data()),
        Err(LoadError::Unreadable(_))
    ));
}

#[test]
fn stale_writes_cannot_follow_a_newer_success_or_failure() {
    let (_dir, lock) = data();
    let file = Arc::new(SettingsFile::new(lock.clone()));
    file.save(2, Preference::English).unwrap();
    file.save(1, Preference::Japanese).unwrap();
    assert_eq!(file.load().unwrap(), Preference::English);
    let obstacle = lock.data().dir().join("settings.json.tmp");
    fs::create_dir(&obstacle).unwrap();
    assert!(file.save(4, Preference::Japanese).is_err());
    fs::remove_dir(&obstacle).unwrap();
    file.save(3, Preference::System).unwrap();
    assert_eq!(file.load().unwrap(), Preference::English);
    std::thread::scope(|scope| {
        for generation in 5..30 {
            let file = &file;
            scope.spawn(move || file.save(generation, Preference::Japanese).unwrap());
        }
        scope.spawn(|| file.save(30, Preference::System).unwrap());
    });
    assert_eq!(file.load().unwrap(), Preference::System);
}

fn start(
    lock: &Arc<InstanceLock>,
    system: Language,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<AxonApp>) {
    cx.update(|cx| {
        axon_gui::init_with_language(system, cx);
        cx.set_reduce_motion(true);
    });
    let app = cx.update(|cx| axon_gui::open_main_window(lock.clone(), cx).unwrap());
    cx.run_until_parked();
    let window = cx.update(|cx| *cx.windows().last().unwrap());
    (window, app)
}

fn with_window(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut Window, &mut App),
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        f(window, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn click(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
    with_window(window, cx, |window, cx| window.click(id, cx));
}

fn keyboard_activate(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
    for _ in 0..10 {
        let mut focused = false;
        with_window(window, cx, |window, cx| {
            window.press("tab", cx);
        });
        with_window(window, cx, |window, _| {
            focused = window.find(id).focused() == Some(true);
        });
        if focused {
            with_window(window, cx, |window, cx| window.press("enter", cx));
            return;
        }
    }
    panic!("{id} must be reachable by Tab");
}

fn close(window: AnyWindowHandle, app: Entity<AxonApp>, cx: &mut TestAppContext) {
    drop(app);
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
}

fn assert_load_failure(window: AnyWindowHandle, app: &Entity<AxonApp>, cx: &mut TestAppContext) {
    cx.read(|cx| {
        assert_eq!(app.read(cx).language_preference(), Preference::System);
        assert_eq!(app.read(cx).language(), Language::English);
    });
    with_window(window, cx, |window, _| {
        assert!(window.find("settings-warning").visible())
    });
    click(window, "open-settings", cx);
    with_window(window, cx, |window, _| {
        assert!(window.find("settings-load-error").visible());
        assert!(window.find("reload-settings").visible());
    });
}

#[gpui_kit::test]
fn switching_restart_and_recovery_preserve_viewing_and_explicit_preferences(
    cx: &mut TestAppContext,
) {
    let (dir, lock) = data();
    let root = dir.path().join("repository");
    fs::create_dir(&root).unwrap();
    axon::location::Location::explicit(&root)
        .unwrap()
        .init("axon")
        .unwrap();
    let ids = axon::location::Location::explicit(&root)
        .unwrap()
        .open()
        .unwrap()
        .update(|header, records, _| {
            let mut entries = Vec::new();
            let mut ids = Vec::new();
            for index in 0..40 {
                let id = new_entity_id(&header.prefix)?;
                let record = records.create(
                    id.clone(),
                    Current {
                        kind: Kind::Issue,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: format!("retained query {index:02}"),
                        description: if index == 0 {
                            "![](https://example.com/image.png)".into()
                        } else {
                            "A long paragraph to retain the detail position.\n\n".repeat(80)
                        },
                        label: Label::Feat,
                        condition: None,
                        parent: None,
                        needs: Default::default(),
                    },
                    RecordContext {
                        at: chrono::Utc::now(),
                        recorder: None,
                    },
                )?;
                entries.push(Entry::Record(record));
                ids.push(id);
            }
            Ok((entries, ids))
        })
        .unwrap();
    lock.register(&root).unwrap();
    let (window, app) = start(&lock, Language::Japanese, cx);
    assert!(!lock.data().dir().join(SETTINGS_FILE).exists());
    gpui_kit::VisualTestContext::from_window(window, cx).deactivate_window();
    cx.run_until_parked();
    cx.read(|cx| assert!(cx.active_window().is_none()));
    cx.update(|cx| cx.dispatch_action(&axon_gui::OpenSettings));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(cx.active_window(), Some(window)));
    click(window, "close-settings", cx);
    with_window(window, cx, |window, cx| {
        window.blur(cx);
        app.update(cx, |app, cx| app.open_settings(window, cx));
    });
    click(window, "close-settings", cx);
    with_window(window, cx, |window, cx| {
        assert!(app.read(cx).list_focus().is_focused(window));
        window.press("cmd-,", cx);
    });
    with_window(window, cx, |window, cx| window.press("escape", cx));
    with_window(window, cx, |window, cx| {
        assert!(app.read(cx).list_focus().is_focused(window))
    });
    with_window(window, cx, |window, cx| {
        window
            .within("entity-list")
            .right_click(axon_gui::app::entity_element(&ids[0]), cx);
    });
    with_window(window, cx, |window, cx| {
        assert!(window.find("popup-menu").visible());
        window.press("cmd-,", cx);
    });
    click(window, "close-settings", cx);
    with_window(window, cx, |window, cx| {
        assert!(window.try_find("popup-menu").is_none());
        assert!(app.read(cx).list_focus().is_focused(window));
    });
    app.update(cx, |app, cx| app.open_entity(ids[0].clone(), cx));
    cx.run_until_parked();
    for (choice, shown) in [
        (None, "（画像）"),
        (Some("language-en"), "(image)"),
        (Some("language-ja"), "（画像）"),
    ] {
        if let Some(choice) = choice {
            click(window, "open-settings", cx);
            click(window, choice, cx);
            click(window, "close-settings", cx);
        }
        with_window(window, cx, |window, cx| {
            let bounds = window.find("detail-description-body").bounds();
            window.drag(
                bounds.origin + gpui_kit::point(px(1.), px(2.)),
                bounds.bottom_right() - gpui_kit::point(px(1.), px(2.)),
                cx,
            );
            window.dispatch_action(Box::new(gpui_kit::base::input::Copy), cx);
        });
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(shown)
        );
    }
    with_window(window, cx, |window, cx| window.press("cmd-,", cx));
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
        "unchanged clipboard".into(),
    ));
    with_window(window, cx, |window, cx| window.press("cmd-c", cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("unchanged clipboard")
    );
    with_window(window, cx, |window, cx| window.press("escape", cx));
    with_window(window, cx, |window, cx| window.press("cmd-c", cx));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("（画像）")
    );
    with_window(window, cx, |window, cx| {
        let detail = app.read(cx).detail_focus().clone();
        window.focus(&detail, cx);
    });
    with_window(window, cx, |window, cx| {
        window.blur(cx);
        app.update(cx, |app, cx| app.open_settings(window, cx));
    });
    click(window, "close-settings", cx);
    with_window(window, cx, |window, cx| {
        assert!(app.read(cx).detail_focus().is_focused(window));
    });
    let search = cx.read(|cx| app.read(cx).search_input().clone());
    with_window(window, cx, |window, cx| {
        window
            .within("entity-list")
            .right_click(axon_gui::app::entity_element(&ids[0]), cx);
    });
    // Dismissal and a new focus can occur before the next frame.
    with_window(window, cx, |window, cx| {
        window.press("escape", cx);
        search.update(cx, |search, cx| search.focus(window, cx));
    });
    with_window(window, cx, |window, cx| {
        assert!(window.try_find("popup-menu").is_none());
        assert!(search.read(cx).focus_handle(cx).is_focused(window));
        window.press("cmd-,", cx);
    });
    click(window, "close-settings", cx);
    with_window(window, cx, |window, cx| {
        assert!(search.read(cx).focus_handle(cx).is_focused(window));
    });
    // Closing before settings ever draws must not leave its undrawn focus active.
    with_window(window, cx, |window, cx| {
        search.update(cx, |search, cx| search.focus(window, cx));
        app.update(cx, |app, cx| {
            app.open_settings(window, cx);
            app.open_root("relative".into(), cx);
        });
    });
    with_window(window, cx, |window, cx| {
        assert!(window.try_find("close-settings").is_none());
        assert!(search.read(cx).focus_handle(cx).is_focused(window));
    });
    app.update(cx, |app, cx| app.add_root(cx));
    assert!(cx.did_prompt_for_paths());
    with_window(window, cx, |window, cx| {
        search.update(cx, |search, cx| search.focus(window, cx));
    });
    gpui_kit::VisualTestContext::from_window(window, cx).deactivate_window();
    cx.run_until_parked();
    cx.update(|cx| cx.dispatch_action(&axon_gui::OpenSettings));
    cx.run_until_parked();
    cx.read(|cx| assert!(cx.active_window().is_none()));
    with_window(window, cx, |window, _| {
        window.find("close-settings");
    });
    let notified = std::rc::Rc::new(std::cell::Cell::new(false));
    let changed = notified.clone();
    let subscription = cx.update(|cx| cx.observe(&app, move |_, _| changed.set(true)));
    app.update(cx, |app, cx| app.open_root(root.clone(), cx));
    assert!(
        notified.get(),
        "a queued link must invalidate the visible settings"
    );
    drop(subscription);
    with_window(window, cx, |window, cx| {
        assert!(window.try_find("close-settings").is_none());
        assert!(search.read(cx).focus_handle(cx).is_focused(window));
    });
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    with_window(window, cx, |window, cx| {
        search.update(cx, |search, cx| {
            search.set_value("retained query", window, cx);
            search.focus(window, cx);
        })
    });
    click(window, "state-Completed", cx);
    app.update(cx, |app, cx| app.open_entity(ids[39].clone(), cx));
    cx.run_until_parked();
    with_window(window, cx, |window, cx| {
        window.scroll(
            "detail-title",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-800.))),
            cx,
        );
    });
    let mut detail_top = px(0.);
    with_window(window, cx, |window, _| {
        detail_top = window.find("detail-title").bounds().origin.y
    });
    assert!(detail_top < px(0.));
    let filter = cx.read(|cx| app.read(cx).explorer().filter().clone());
    with_window(window, cx, |window, cx| window.press("cmd-,", cx));
    click(window, "language-en", cx);
    cx.read(|cx| {
        assert_eq!(app.read(cx).language(), Language::English);
        assert_eq!(app.read(cx).search_input(), &search);
        assert_eq!(app.read(cx).explorer().filter(), &filter);
        assert_eq!(app.read(cx).explorer().selected(), Some(&ids[39]));
        assert_eq!(
            search.read(cx).presentation().placeholder().as_ref(),
            "Search title, description or ID"
        );
        let menu = cx.get_menus().unwrap();
        assert_eq!(menu[1].name.as_ref(), "Edit");
    });
    assert_eq!(&*gpui_kit::component::locale(), "en");
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::English);
    click(window, "close-settings", cx);
    with_window(window, cx, |window, _| {
        assert_eq!(window.find("detail-title").bounds().origin.y, detail_top);
        let last = window
            .within("entity-list")
            .find(axon_gui::app::entity_element(&ids[39]));
        assert!(last.visible());
        assert!(
            !window
                .within("entity-list")
                .try_find(axon_gui::app::entity_element(&ids[0]))
                .is_some_and(|row| row.visible())
        );
    });
    click(window, "open-settings", cx);
    with_window(window, cx, |window, _| {
        assert_eq!(
            window.find("language-system").label(),
            Some("Follow system")
        );
    });
    cx.simulate_window_resize(window, size(px(380.), px(460.)));
    cx.run_until_parked();
    for id in [
        "language-system",
        "language-ja",
        "language-en",
        "close-settings",
    ] {
        with_window(window, cx, |window, _| {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.right() <= window.viewport_size().width,
                "{id}: {bounds:?}"
            );
            assert!(
                bounds.bottom() <= window.viewport_size().height,
                "{id}: {bounds:?}"
            );
        });
    }
    click(window, "language-ja", cx);
    click(window, "language-system", cx);
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::System);
    cx.read(|cx| assert_eq!(app.read(cx).language(), Language::Japanese));
    let failure = lock.data().dir().join("settings.json.tmp");
    fs::create_dir(&failure).unwrap();
    click(window, "language-en", cx);
    click(window, "close-settings", cx);
    with_window(window, cx, |window, _| {
        assert!(window.find("settings-warning").visible());
        window.find("detail-title");
    });
    click(window, "warning-settings", cx);
    fs::remove_dir(&failure).unwrap();
    click(window, "retry-settings", cx);
    click(window, "language-system", cx);
    // The latest selection is saved before another event or executor turn can run.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.set_language_preference(Preference::English, window, cx);
            app.set_language_preference(Preference::Japanese, window, cx);
        })
    })
    .unwrap();
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::Japanese);
    drop(app);
    cx.update_window(window, |_, window, _| window.remove_window())
        .unwrap();
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::Japanese);
    cx.run_until_parked();
    let (window, app) = start(&lock, Language::English, cx);
    cx.read(|cx| assert_eq!(app.read(cx).language(), Language::Japanese));
    click(window, "open-settings", cx);
    click(window, "language-system", cx);
    cx.read(|cx| assert_eq!(app.read(cx).language(), Language::English));
    close(window, app, cx);
    let (window, app) = start(&lock, Language::Japanese, cx);
    cx.read(|cx| assert_eq!(app.read(cx).language(), Language::Japanese));
    close(window, app, cx);

    let path = lock.data().dir().join(SETTINGS_FILE);
    for bytes in [b"broken".as_slice(), br#"{"format":99,"language":"en"}"#] {
        fs::write(&path, bytes).unwrap();
        let (window, app) = start(&lock, Language::English, cx);
        assert_load_failure(window, &app, cx);
        click(window, "language-ja", cx);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        close(window, app, cx);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let (window, app) = start(&lock, Language::English, cx);
        click(window, "open-settings", cx);
        click(window, "language-ja", cx);
        click(window, "replace-settings", cx);
        assert_eq!(settings::load(lock.data()).unwrap(), Preference::Japanese);
        close(window, app, cx);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let (window, app) = start(&lock, Language::English, cx);
    assert_load_failure(window, &app, cx);
    click(window, "language-ja", cx);
    app.update(cx, |app, cx| app.save_settings(cx));
    cx.run_until_parked();
    assert!(path.is_dir());
    fs::remove_dir(&path).unwrap();
    let recovered = br#"{"format":1,"language":"en"}"#;
    fs::write(&path, recovered).unwrap();
    app.update(cx, |app, cx| app.save_settings(cx));
    cx.run_until_parked();
    assert_eq!(fs::read(&path).unwrap(), recovered);
    click(window, "reload-settings", cx);
    click(window, "language-ja", cx);
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::Japanese);
    let obstacle = lock.data().dir().join("settings.json.tmp");
    fs::create_dir(&obstacle).unwrap();
    click(window, "language-en", cx);
    cx.read(|cx| assert_eq!(app.read(cx).language(), Language::English));
    with_window(window, cx, |window, _| {
        window.find("settings-save-error");
    });
    fs::remove_dir(&obstacle).unwrap();
    keyboard_activate(window, "retry-settings", cx);
    assert_eq!(settings::load(lock.data()).unwrap(), Preference::English);
    for _ in 0..3 {
        with_window(window, cx, |_, _| {});
    }
    with_window(window, cx, |window, cx| {
        assert!(window.find("close-settings").visible());
        assert!(gpui_kit::base::active_focus_trap(window, cx).is_some());
    });
    app.update(cx, |app, cx| app.open_root("relative".into(), cx));
    cx.run_until_parked();
    with_window(window, cx, |window, _| {
        assert!(window.try_find("close-settings").is_none())
    });
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .notice()
                .unwrap()
                .starts_with("Could not open the linked folder.")
        )
    });
    click(window, "open-settings", cx);
    click(window, "language-ja", cx);
    cx.read(|cx| {
        assert!(
            !app.read(cx)
                .notice()
                .unwrap()
                .starts_with("Could not open the linked folder.")
        );
        assert_eq!(
            app.read(cx)
                .search_input()
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            "タイトル・本文・ID を検索"
        );
        assert_eq!(cx.get_menus().unwrap()[1].name.as_ref(), "編集");
    });
    close(window, app, cx);

    let (_startup_dir, startup_lock) = data();
    let (saved, effective, edit_menu) = match Language::system() {
        Language::Japanese => (Preference::English, Language::English, "Edit"),
        Language::English => (Preference::Japanese, Language::Japanese, "編集"),
    };
    SettingsFile::new(startup_lock.clone())
        .save(1, saved)
        .unwrap();
    let startup = axon_gui::Startup::Ready(Arc::try_unwrap(startup_lock).unwrap());
    cx.update(|cx| axon_gui::init_for_startup(&startup, cx));
    cx.read(|cx| assert_eq!(cx.get_menus().unwrap()[1].name.as_ref(), edit_menu));
    let initial_app = cx.update(|cx| axon_gui::open_startup_window(startup, cx).unwrap().unwrap());
    cx.read(|cx| assert_eq!(initial_app.read(cx).language_preference(), saved));
    let initial_window = cx.update(|cx| *cx.windows().last().unwrap());
    close(initial_window, initial_app, cx);

    // Exercise the actual production initializer and notice window while another lock lives.
    SettingsFile::new(lock.clone()).save(1, saved).unwrap();
    let before = fs::read(&path).unwrap();
    let startup = axon_gui::startup_on(Ok(lock.data().clone()));
    assert!(matches!(startup, axon_gui::Startup::Refused(_)));
    cx.update(|cx| axon_gui::init_for_startup(&startup, cx));
    cx.read(|cx| assert_eq!(cx.get_menus().unwrap()[1].name.as_ref(), edit_menu));
    assert!(
        cx.update(|cx| axon_gui::open_startup_window(startup, cx).unwrap())
            .is_none()
    );
    cx.run_until_parked();
    let notice_window = cx.update(|cx| *cx.windows().last().unwrap());
    with_window(notice_window, cx, |window, cx| {
        let view = gpui_kit::base::Root::read(window, cx)
            .view()
            .clone()
            .downcast::<axon_gui::Notice>()
            .unwrap();
        let expected =
            axon_gui::StartupRefusal::AlreadyRunning(lock.data().clone()).message(effective);
        assert_eq!(view.read(cx).message(), expected);
        window.remove_window();
    });
    assert_eq!(fs::read(&path).unwrap(), before);
    let locate = axon_gui::startup_on(Err(axon_gui::project::data::LocateError::Relative(
        "relative".into(),
    )));
    cx.update(|cx| axon_gui::init_for_startup(&locate, cx));
    cx.read(|cx| assert_eq!(Language::of(cx), Language::system()));
    assert!(
        cx.update(|cx| axon_gui::open_startup_window(locate, cx).unwrap())
            .is_none()
    );
    cx.run_until_parked();
    let notice_window = cx.update(|cx| *cx.windows().last().unwrap());
    with_window(notice_window, cx, |window, cx| {
        let view = gpui_kit::base::Root::read(window, cx)
            .view()
            .clone()
            .downcast::<axon_gui::Notice>()
            .unwrap();
        let expected = axon_gui::StartupRefusal::Locate(
            axon_gui::project::data::LocateError::Relative("relative".into()),
        )
        .message(Language::system());
        assert_eq!(view.read(cx).message(), expected);
        window.remove_window();
    });
}
