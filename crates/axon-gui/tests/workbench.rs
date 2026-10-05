//! Headless UI tests of the workbench: Japanese text, focus traversal, the label menu and the
//! smallest window. These run on GPUI's test platform and do not exercise an OS input method.

use axon::lifecycle::Label;
use axon_gui::{MIN_WINDOW_SIZE, Workbench};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    Action, AppContext, Bounds, ElementId, Entity, Focusable, MenuItem, Point, TestAppContext,
    WindowBounds, WindowHandle, WindowOptions, base::Root, px, size,
};
use std::time::Duration;

fn open(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (WindowHandle<Root>, Entity<Workbench>) {
    cx.update(axon_gui::init);
    let (window, workbench) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(width), px(height)),
            })),
            ..axon_gui::main_window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workbench::new(window, cx))
        })
        .expect("open the workbench")
    });
    cx.run_until_parked();
    (window.downcast::<Root>().expect("Base Root"), workbench)
}

fn body_id(workbench: &Entity<Workbench>, cx: &mut TestAppContext) -> ElementId {
    cx.read(|cx| ElementId::from(("input", workbench.read(cx).body().entity_id())))
}

fn press(handle: WindowHandle<Root>, key: &str, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| window.press(key, cx))
        .unwrap();
    cx.run_until_parked();
}

#[derive(Debug, PartialEq)]
enum Focus {
    Title,
    Label,
    Body,
}

fn focus(
    handle: WindowHandle<Root>,
    workbench: &Entity<Workbench>,
    cx: &mut TestAppContext,
) -> Focus {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let workbench = workbench.read(cx);
        let title = workbench.title().focus_handle(cx).is_focused(window);
        let body = workbench.body().focus_handle(cx).is_focused(window);
        let label = window.find("label").focused() == Some(true);
        match (title, label, body) {
            (true, false, false) => Focus::Title,
            (false, true, false) => Focus::Label,
            (false, false, true) => Focus::Body,
            other => panic!("expected exactly one focused field, got {other:?}"),
        }
    })
    .unwrap()
}

#[gpui_kit::test]
fn japanese_text_is_entered_in_title_and_multiline_body(cx: &mut TestAppContext) {
    let (handle, workbench) = open(cx, 720., 520.);
    assert_eq!(focus(handle, &workbench, cx), Focus::Title);
    cx.update_window(handle.into(), |_, window, cx| {
        window.input("日本語のタイトル", cx)
    })
    .unwrap();

    let body = body_id(&workbench, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.click(body.clone(), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(focus(handle, &workbench, cx), Focus::Body);
    cx.update_window(handle.into(), |_, window, cx| window.input("一行目", cx))
        .unwrap();
    press(handle, "enter", cx);
    cx.update_window(handle.into(), |_, window, cx| window.input("二行目", cx))
        .unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let workbench = workbench.read(cx);
        assert_eq!(workbench.title().read(cx).value(), "日本語のタイトル");
        assert_eq!(workbench.body().read(cx).value(), "一行目\n二行目");
        assert_eq!(window.find("title").value(), Some("日本語のタイトル"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn tab_moves_focus_through_every_field_and_leaves_the_body(cx: &mut TestAppContext) {
    let (handle, workbench) = open(cx, 720., 520.);
    assert_eq!(focus(handle, &workbench, cx), Focus::Title);
    for expected in [Focus::Label, Focus::Body, Focus::Title] {
        press(handle, "tab", cx);
        assert_eq!(focus(handle, &workbench, cx), expected);
    }
    for expected in [Focus::Body, Focus::Label, Focus::Title] {
        press(handle, "shift-tab", cx);
        assert_eq!(focus(handle, &workbench, cx), expected);
    }
    cx.read(|cx| {
        let workbench = workbench.read(cx);
        assert_eq!(workbench.title().read(cx).value(), "");
        assert_eq!(
            workbench.body().read(cx).value(),
            "",
            "Tab must not indent the body"
        );
    });
}

#[gpui_kit::test]
async fn label_menu_lists_core_labels_and_closes_after_a_choice(cx: &mut TestAppContext) {
    let (handle, workbench) = open(cx, 720., 520.);
    cx.read(|cx| assert_eq!(workbench.read(cx).label(), Label::Feat));
    let chosen = Label::Chore;
    let index = Label::ALL
        .iter()
        .position(|label| *label == chosen)
        .unwrap();

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("label", cx);
        assert!(window.find("popup-menu").visible());
        for (ix, label) in Label::ALL.iter().enumerate() {
            let item = window.within("popup-menu").find(ix);
            assert_eq!(item.label(), Some(label.name()), "{item:?}");
        }
        assert!(
            window
                .within("popup-menu")
                .try_find(Label::ALL.len())
                .is_none(),
            "extra menu item"
        );
        window.within("popup-menu").click(index, cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(1), |window, _| {
        window.try_find("popup-menu").is_none()
    })
    .await;
    cx.read(|cx| assert_eq!(workbench.read(cx).label(), chosen));
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("label").label(), Some("label: chore ▾"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn smallest_window_keeps_every_control_visible(cx: &mut TestAppContext) {
    let min_size = cx.update(|cx| axon_gui::main_window_options(cx).window_min_size);
    assert_eq!(
        min_size,
        Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1)))
    );
    let (handle, workbench) = open(cx, MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1);
    let body = body_id(&workbench, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let viewport = window.viewport_size();
        for id in [ElementId::from("title"), "label".into(), body] {
            let element = window.find(id.clone());
            assert!(element.visible(), "{id:?} is hidden");
            let bounds = element.bounds();
            assert!(
                bounds.size.width >= px(80.) && bounds.size.height >= px(20.),
                "{id:?} is too small to use: {bounds:?}"
            );
            assert!(
                bounds.bottom_right().x <= viewport.width
                    && bounds.bottom_right().y <= viewport.height,
                "{id:?} overflows the window: {bounds:?}"
            );
        }
    })
    .unwrap();
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
