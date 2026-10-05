//! GPUI desktop shell for Axon.
//!
//! The GUI depends on the `axon` library (storage adapters and the re-exported core) and
//! converts core values into display text and GPUI elements here. Nothing from GPUI flows back
//! into the core crates.

use axon::lifecycle::Label;
use gpui_kit::component::{
    ActiveTheme, Theme,
    button::Button,
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, KeyBinding, Menu, MenuItem, OsAction, Render,
    TitlebarOptions, Window, WindowBounds, WindowOptions, actions, base::input as edit, div,
    prelude::*, px, size,
};

actions!(axon_gui, [Quit, FocusNextField, FocusPreviousField]);

/// Key context wrapping the body editor, so Tab leaves the body instead of indenting it.
const BODY_CONTEXT: &str = "AxonBody";

/// Smallest window that still shows every control of the workbench.
pub const MIN_WINDOW_SIZE: (f32, f32) = (420., 320.);

/// Registers the components, key bindings and application menus. Call once before opening
/// windows.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new(
            "tab",
            FocusNextField,
            Some(&format!("{BODY_CONTEXT} > Input")),
        ),
        KeyBinding::new(
            "shift-tab",
            FocusPreviousField,
            Some(&format!("{BODY_CONTEXT} > Input")),
        ),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(menus());
}

/// The menu bar: the application menu and the standard editing commands that text inputs
/// handle.
pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Axon").items([MenuItem::action("Axon を終了", Quit)]),
        Menu::new("編集").items([
            MenuItem::os_action("取り消す", edit::Undo, OsAction::Undo),
            MenuItem::os_action("やり直す", edit::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("カット", edit::Cut, OsAction::Cut),
            MenuItem::os_action("コピー", edit::Copy, OsAction::Copy),
            MenuItem::os_action("ペースト", edit::Paste, OsAction::Paste),
            MenuItem::os_action("すべてを選択", edit::SelectAll, OsAction::SelectAll),
        ]),
    ]
}

/// Options of the main window: centered, titled, and never smaller than [`MIN_WINDOW_SIZE`].
pub fn main_window_options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(720.), px(520.)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1))),
        titlebar: Some(TitlebarOptions {
            title: Some("Axon".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Opens the main window with the workbench and focuses the title field.
pub fn open_main_window(cx: &mut App) -> gpui_kit::Result<Entity<Workbench>> {
    let options = main_window_options(cx);
    let (_, workbench) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Workbench::new(window, cx))
    })?;
    Ok(workbench)
}

/// A draft editor: a title line, a label chosen from a menu, and a multi-line body.
pub struct Workbench {
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    label: Label,
}

impl Workbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("タイトル"));
        let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("本文"));
        title.update(cx, |title, cx| title.focus(window, cx));
        Theme::sync_system_appearance(Some(window), cx);
        cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx)
        })
        .detach();
        Self {
            title,
            body,
            label: Label::Feat,
        }
    }

    pub fn title(&self) -> &Entity<InputState> {
        &self.title
    }

    pub fn body(&self) -> &Entity<TextareaState> {
        &self.body
    }

    pub fn label(&self) -> Label {
        self.label
    }

    fn select_label(&mut self, label: Label, cx: &mut Context<Self>) {
        self.label = label;
        cx.notify();
    }
}

impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let current = self.label;
        let workbench = cx.entity().downgrade();
        div()
            .id("workbench")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(|_: &FocusNextField, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPreviousField, window, cx| window.focus_prev(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(Input::new(&self.title).id("title").flex_1())
                    .child(
                        Button::new("label")
                            .outline()
                            .label(format!("種類: {} ▾", current.name()))
                            .dropdown_menu(move |menu, _, _| {
                                Label::ALL.into_iter().fold(menu, |menu, label| {
                                    let workbench = workbench.clone();
                                    menu.item(
                                        PopupMenuItem::new(label.name())
                                            .checked(label == current)
                                            .on_click(move |_, _, cx| {
                                                workbench
                                                    .update(cx, |this, cx| {
                                                        this.select_label(label, cx)
                                                    })
                                                    .ok();
                                            }),
                                    )
                                })
                            }),
                    ),
            )
            .child(
                div()
                    .key_context(BODY_CONTEXT)
                    .flex_1()
                    .min_h(px(80.))
                    .child(Textarea::new(&self.body).size_full()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Tab で次の欄へ、Shift-Tab で前の欄へ移動します。"),
            )
    }
}
