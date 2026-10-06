//! Headless UI tests of the read-only window as a whole: Japanese text in the search field,
//! focus traversal with Tab and the menu bar. These run on GPUI's test platform and do not
//! exercise an OS input method.

use axon::lifecycle::{
    Context, Kind, Label, Lifecycle,
    record::{Current, Entry, new_entity_id},
};
use axon::location::Location;
use axon_gui::{
    AxonApp, MIN_WINDOW_SIZE,
    app::entity_element,
    project::{AppData, InstanceLock},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    Action, AppContext, Bounds, Entity, Focusable, MenuItem, Point, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions, base::Root, px, size,
};
use std::collections::BTreeSet;
use std::sync::Arc;

type Window = WindowHandle<Root>;

/// A registered management root holding one Issue, written through the core as the CLI would.
fn data_with_an_issue() -> (tempfile::TempDir, Arc<InstanceLock>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("読書会");
    std::fs::create_dir(&root).unwrap();
    let location = Location::explicit(&root).unwrap();
    location.init("axon").unwrap();
    location
        .open()
        .and_then(|mut store| {
            store.update(|header, records, _| {
                let record = records.create(
                    new_entity_id(&header.prefix)?,
                    Current {
                        kind: Kind::Issue,
                        lifecycle: Lifecycle::NotStarted,
                        owner: None,
                        title: "会場を決める".into(),
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
        })
        .unwrap();
    let lock = AppData::at(dir.path().join("data"))
        .unwrap()
        .lock_instance()
        .unwrap();
    lock.register(&root).unwrap();
    (dir, Arc::new(lock))
}

fn open(data: &Arc<InstanceLock>, cx: &mut TestAppContext) -> (Window, Entity<AxonApp>) {
    cx.update(axon_gui::init);
    let data = data.clone();
    let (window, app) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(760.)),
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

fn with_window<T>(
    handle: Window,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App) -> T,
) -> T {
    let value = cx
        .update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            f(window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    value
}

fn press(handle: Window, key: &str, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| window.press(key, cx));
}

fn search_focused(handle: Window, app: &Entity<AxonApp>, cx: &mut TestAppContext) -> bool {
    with_window(handle, cx, |window, cx| {
        app.read(cx)
            .search_input()
            .focus_handle(cx)
            .is_focused(window)
    })
}

fn list_focused(handle: Window, app: &Entity<AxonApp>, cx: &mut TestAppContext) -> bool {
    with_window(handle, cx, |window, cx| {
        app.read(cx).list_focus().is_focused(window)
    })
}

fn focus_search(handle: Window, app: &Entity<AxonApp>, cx: &mut TestAppContext) {
    with_window(handle, cx, |window, cx| {
        let search = app.read(cx).search_input().clone();
        search.update(cx, |search, cx| search.focus(window, cx));
    });
}

#[gpui_kit::test]
fn japanese_text_is_entered_in_the_search_field(cx: &mut TestAppContext) {
    let (_dir, data) = data_with_an_issue();
    let (handle, app) = open(&data, cx);
    focus_search(handle, &app, cx);
    with_window(handle, cx, |window, cx| window.input("会場", cx));
    with_window(handle, cx, |window, cx| {
        assert_eq!(app.read(cx).search_input().read(cx).value(), "会場");
        assert_eq!(window.find("search").value(), Some("会場"));
        assert_eq!(app.read(cx).explorer().filter().query, "会場");
        assert_eq!(app.read(cx).explorer().listing().matched, 1);
    });
    with_window(handle, cx, |window, cx| window.input("以外", cx));
    cx.read(|cx| assert_eq!(app.read(cx).explorer().listing().matched, 0));
}

#[gpui_kit::test]
fn tab_moves_the_focus_between_the_search_field_and_the_list(cx: &mut TestAppContext) {
    let (_dir, data) = data_with_an_issue();
    let (handle, app) = open(&data, cx);
    focus_search(handle, &app, cx);
    assert!(search_focused(handle, &app, cx));

    // Tab leaves the search field without typing into it and reaches the list.
    let mut presses = 0;
    while !list_focused(handle, &app, cx) {
        presses += 1;
        assert!(presses <= 10, "Tab never reached the list");
        press(handle, "tab", cx);
    }
    assert!(!search_focused(handle, &app, cx));
    cx.read(|cx| assert_eq!(app.read(cx).search_input().read(cx).value(), ""));

    // The arrow keys move the selection once the list has the focus.
    press(handle, "down", cx);
    with_window(handle, cx, |window, cx| {
        assert!(app.read(cx).explorer().selected().is_some());
        let row = app.read(cx).explorer().selected().unwrap().clone();
        assert!(window.find(entity_element(&row)).visible());
    });

    // Shift-Tab goes back the same way.
    for _ in 0..presses {
        press(handle, "shift-tab", cx);
    }
    assert!(search_focused(handle, &app, cx));
}

#[gpui_kit::test]
fn the_window_cannot_shrink_below_the_smallest_size(cx: &mut TestAppContext) {
    let min_size = cx.update(|cx| axon_gui::main_window_options(cx).window_min_size);
    assert_eq!(
        min_size,
        Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1)))
    );
}

#[test]
fn menu_bar_offers_quit_and_editing_commands() {
    let items: Vec<_> = axon_gui::menus()
        .into_iter()
        .flat_map(|menu| menu.items)
        .filter_map(|item| match item {
            MenuItem::Action { name, action, .. } => Some((name.to_string(), action.name())),
            _ => None,
        })
        .collect();
    let expected = [
        ("Axon を終了", axon_gui::Quit.name()),
        ("取り消す", "input::Undo"),
        ("やり直す", "input::Redo"),
        ("カット", "input::Cut"),
        ("コピー", "input::Copy"),
        ("ペースト", "input::Paste"),
        ("すべてを選択", "input::SelectAll"),
    ];
    let expected: Vec<_> = expected
        .into_iter()
        .map(|(name, action)| (name.to_string(), action))
        .collect();
    assert_eq!(items, expected);
}
