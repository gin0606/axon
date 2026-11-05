//! English UI coverage is isolated from the Japanese UI tests because component locale is
//! process-wide. This binary changes that locale only in its single UI test.

use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon::location::Location;
use axon_gui::{
    AxonApp, Language,
    app::{Columns, entity_element},
    project::AppData,
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use std::{collections::BTreeSet, sync::Arc};

#[gpui_kit::test]
fn english_startup_controls_menus_notifications_and_layout(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let location = Location::explicit(&root).unwrap();
    location.init("axon").unwrap();
    let id = location
        .open()
        .unwrap()
        .update(|header, records, _| {
            let id = new_entity_id(&header.prefix)?;
            let record = records.create(
                id.clone(),
                Current {
                    kind: Kind::Issue,
                    lifecycle: Lifecycle::NotStarted,
                    owner: None,
                    title: "記録のタイトル".into(),
                    description: "記録の本文\n\n![](image.png)".into(),
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
            Ok((vec![Entry::Record(record)], id))
        })
        .unwrap();
    let lock = Arc::new(
        AppData::at(dir.path().join("data"))
            .unwrap()
            .lock_instance()
            .unwrap(),
    );
    lock.register(&root).unwrap();
    cx.update(|cx| {
        axon_gui::init_with_language(Language::from_preferences(["fr-FR", "en-GB", "ja"]), cx);
        cx.set_reduce_motion(true);
    });
    assert_eq!(&*gpui_kit::component::locale(), "en");
    let menu_names: Vec<_> = cx
        .read(|cx| cx.get_menus().unwrap())
        .into_iter()
        .flat_map(|menu| menu.items)
        .filter_map(|item| match item {
            gpui_kit::OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        menu_names,
        [
            "Settings…",
            "Quit Axon",
            "Undo",
            "Redo",
            "Cut",
            "Copy",
            "Paste",
            "Select All"
        ]
    );
    let (handle, app) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1200.), px(760.)),
                })),
                ..axon_gui::main_window_options(cx)
            },
            cx,
            |window, cx| cx.new(|cx| AxonApp::new(lock.clone(), window, cx)),
        )
        .unwrap()
    });
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("reload-list").label(), Some("Reload"));
        assert_eq!(window.find("state-NotStarted").label(), Some("Not started"));
        assert_eq!(window.find("layout-tree").label(), Some("Tree"));
        window
            .within("entity-list")
            .right_click(entity_element(&id), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within("popup-menu").find(0usize).label(),
            Some("Copy ID")
        );
        assert_eq!(
            window.within("popup-menu").find(1usize).label(),
            Some("Copy title")
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("entity-list").click(entity_element(&id), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            app.read(cx)
                .explorer()
                .board()
                .unwrap()
                .item(&id)
                .unwrap()
                .title,
            "記録のタイトル"
        )
    });
    for (width, columns) in [(800., Columns::Two), (380., Columns::One)] {
        cx.simulate_window_resize(handle, size(px(width), px(460.)));
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(app.read(cx).columns(), columns));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            if columns == Columns::One {
                window.press("escape", cx);
            }
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for id in ["reload-list", "open-panel", "search"] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.right() <= window.viewport_size().width,
                    "{id}: {bounds:?}"
                );
                assert!(
                    bounds.bottom() <= window.viewport_size().height,
                    "{id}: {bounds:?}"
                );
            }
        })
        .unwrap();
    }
    // A failed link retains its reason and translates its wrapper, while the technical
    // diagnostic stays as returned by the path validator.
    app.update(cx, |app, cx| app.open_root("relative".into(), cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let notice = app.read(cx).notice().unwrap();
        assert!(notice.starts_with("Could not open the linked folder."));
        let error = axon_gui::project::ProjectRoot::new("relative".into()).unwrap_err();
        assert!(notice.contains(&error.to_string()), "{notice}");
        assert!(!notice.contains("%{"), "{notice}");
    });
    let refusal = axon_gui::StartupRefusal::AlreadyRunning(lock.data().clone());
    assert!(
        refusal
            .message(Language::English)
            .starts_with("Axon is already running.")
    );
    assert!(
        refusal
            .message(Language::Japanese)
            .starts_with("Axon はすでに起動しています。")
    );
    let japanese = cx.new_app();
    japanese.update(|cx| axon_gui::init_with_language(Language::Japanese, cx));
    assert_eq!(&*gpui_kit::component::locale(), "ja");
    for (key, expected) in [
        ("Input.Cut", "カット"),
        ("Input.Copy", "コピー"),
        ("Input.Paste", "ペースト"),
        ("Input.Select All", "すべてを選択"),
    ] {
        assert_eq!(
            gpui_kit::component::_rust_i18n_translate("ja", key),
            expected
        );
    }
}

#[test]
fn startup_refusals_retain_paths_and_diagnostics_in_both_languages() {
    use axon_gui::{
        StartupRefusal,
        project::data::{InstanceError, LocateError},
    };
    let dir = tempfile::tempdir().unwrap();
    let data = AppData::at(dir.path().join("data with 日本語")).unwrap();
    let path = data.dir().display().to_string();
    let locate = LocateError::Relative("relative 日本語".into());
    let locate_diagnostic = locate.to_string();
    let io = InstanceError::Io(std::io::Error::other("raw OS diagnostic 日本語"));
    let io_diagnostic = io.to_string();
    for (reason, expected) in [
        (
            StartupRefusal::AlreadyRunning(data.clone()),
            vec![path.clone()],
        ),
        (StartupRefusal::Locate(locate), vec![locate_diagnostic]),
        (
            StartupRefusal::Io { data, error: io },
            vec![path, io_diagnostic],
        ),
    ] {
        for language in [Language::English, Language::Japanese] {
            let message = reason.message(language);
            for argument in &expected {
                assert!(message.contains(argument), "{message}");
            }
            assert!(!message.contains("%{"), "{message}");
        }
    }
}
