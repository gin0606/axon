//! GPUI desktop shell for Axon.
//!
//! The GUI depends on the `axon` library (storage adapters and the re-exported core) and
//! converts core values into display text and GPUI elements here. Nothing from GPUI flows back
//! into the core crates. [`project`] holds the projects and [`board`] the records of one as
//! values, both without GPUI; [`app`] is the window.

pub mod app;
pub mod board;
pub mod project;

pub use app::AxonApp;

use axon::lifecycle::Label;
use gpui_kit::component::{
    ActiveTheme, Theme,
    button::Button,
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, Global, KeyBinding, Menu, MenuItem, OsAction, Render,
    SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, actions,
    base::input as edit, div, prelude::*, px, size,
};
use project::{AppData, InstanceError, InstanceLock, data::LocateError};

actions!(
    axon_gui,
    [
        Quit,
        FocusNextField,
        FocusPreviousField,
        SelectNextEntity,
        SelectPreviousEntity
    ]
);

/// Key context wrapping a multi-line editor, so Tab leaves it instead of indenting it.
pub(crate) const BODY_CONTEXT: &str = "AxonBody";

/// Smallest window that still shows the project column, the list and the detail pane (or the
/// workbench in its place) side by side.
pub const MIN_WINDOW_SIZE: (f32, f32) = (880., 520.);

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
        KeyBinding::new("down", SelectNextEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntity, Some(app::LIST_CONTEXT)),
    ]);
    cx.on_action(|_: &Quit, cx| request_quit(cx));
    cx.set_menus(menus());
}

/// The main window, which asks before anything unsaved in it is lost.
pub(crate) struct MainWindow {
    pub window: gpui_kit::AnyWindowHandle,
    pub app: gpui_kit::WeakEntity<AxonApp>,
}
impl Global for MainWindow {}

/// Quits the application, after the main window has asked about anything unsaved in it.
pub fn request_quit(cx: &mut App) {
    // The action may arrive while the main window is being updated, where it cannot be updated
    // again; asking waits until that ends.
    cx.defer(ask_and_quit);
}

fn ask_and_quit(cx: &mut App) {
    if let Some(main) = cx.try_global::<MainWindow>() {
        let (window, app) = (main.window, main.app.clone());
        let asked = window.update(cx, |_, window, cx| {
            app.update(cx, |this, cx| this.request_quit(window, cx))
        });
        if matches!(asked, Ok(Ok(()))) {
            return;
        }
    }
    cx.quit();
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
    let bounds = Bounds::centered(None, size(px(1120.), px(700.)), cx);
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

/// What a start found: the data to open, or why this instance must not open it.
pub enum Startup {
    Ready { data: AppData, lock: InstanceLock },
    Refused(String),
}

/// Locates the data directory and takes the single-instance lock. A second instance on the
/// same data is refused, so only one process ever writes it.
pub fn startup() -> Startup {
    startup_on(AppData::locate())
}

/// [`startup`] on a data directory already located, or the reason none was.
pub fn startup_on(data: Result<AppData, LocateError>) -> Startup {
    let data = match data {
        Ok(data) => data,
        Err(error) => {
            return Startup::Refused(format!("データの保存場所を決められません: {error}"));
        }
    };
    match data.lock_instance() {
        Ok(lock) => Startup::Ready { data, lock },
        Err(InstanceError::AlreadyRunning) => Startup::Refused(format!(
            "Axon はすでに起動しています。同じデータを二つのアプリから書き換えないよう、こちらは開きません。（データの保存場所: {}）",
            data.dir().display()
        )),
        Err(error @ InstanceError::Io(_)) => Startup::Refused(format!(
            "データの保存場所を使えません: {}: {error}",
            data.dir().display()
        )),
    }
}

/// Keeps the instance lock for as long as the application runs.
struct HeldInstanceLock(#[allow(dead_code)] InstanceLock);
impl Global for HeldInstanceLock {}

/// Opens the window [`startup`] calls for: the main window holding the lock, or a notice.
pub fn open_startup_window(startup: Startup, cx: &mut App) -> gpui_kit::Result<()> {
    match startup {
        Startup::Ready { data, lock } => {
            cx.set_global(HeldInstanceLock(lock));
            open_main_window(data, cx).map(|_| ())
        }
        Startup::Refused(message) => open_notice_window(message, cx).map(|_| ()),
    }
}

/// Opens the main window on `data`. The caller holds its instance lock.
pub fn open_main_window(data: AppData, cx: &mut App) -> gpui_kit::Result<Entity<AxonApp>> {
    let options = main_window_options(cx);
    let (_, app) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AxonApp::new(data, window, cx))
    })?;
    Ok(app)
}

/// A window that only explains why the application did not open its data.
pub struct Notice {
    message: SharedString,
}
impl Notice {
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl Render for Notice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("notice")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().id("notice-message").child(self.message.clone()))
            .child(
                Button::new("quit")
                    .outline()
                    .label("終了")
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

pub fn open_notice_window(message: String, cx: &mut App) -> gpui_kit::Result<Entity<Notice>> {
    let bounds = Bounds::centered(None, size(px(480.), px(200.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Axon".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (_, notice) = gpui_kit::open_window(options, cx, |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        cx.new(|_| Notice {
            message: message.into(),
        })
    })?;
    Ok(notice)
}

/// A draft editor: a title line, a label chosen from a menu, and a multi-line body.
pub struct Workbench {
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    label: Label,
    /// The text is being saved: it takes no typing until the save ends, so what is emptied
    /// afterwards is exactly what was saved.
    locked: bool,
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
            locked: false,
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

    pub fn set_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        self.locked = locked;
        cx.notify();
    }

    /// Empties the title and the body, keeping the label for the next draft.
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.title
            .update(cx, |title, cx| title.set_value("", window, cx));
        self.body
            .update(cx, |body, cx| body.set_value("", window, cx));
        cx.notify();
    }

    /// Whether neither the title nor the body holds anything typed.
    pub fn is_blank(&self, cx: &App) -> bool {
        self.title.read(cx).value().is_empty() && self.body.read(cx).value().is_empty()
    }
}

/// A button that shows `current` and opens a menu of every label, calling `select` with the
/// chosen one. The names are the spelling records and the CLI use.
pub(crate) fn label_button(
    id: &'static str,
    current: Label,
    select: impl Fn(Label, &mut App) + 'static,
) -> impl IntoElement {
    let select = std::rc::Rc::new(select);
    Button::new(id)
        .outline()
        .label(format!("label: {} ▾", current.name()))
        .dropdown_menu(move |menu, _, _| {
            Label::ALL.into_iter().fold(menu, |menu, label| {
                let select = select.clone();
                menu.item(
                    PopupMenuItem::new(label.name())
                        .checked(label == current)
                        .on_click(move |_, _, cx| select(label, cx)),
                )
            })
        })
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
                    .child(
                        Input::new(&self.title)
                            .id("title")
                            .readonly(self.locked)
                            .flex_1(),
                    )
                    .child(label_button("label", current, move |label, cx| {
                        workbench
                            .update(cx, |this, cx| this.select_label(label, cx))
                            .ok();
                    })),
            )
            .child(
                div()
                    .key_context(BODY_CONTEXT)
                    .flex_1()
                    .min_h(px(80.))
                    .child(Textarea::new(&self.body).readonly(self.locked).size_full()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Tab で次の欄へ、Shift-Tab で前の欄へ移動します。"),
            )
    }
}
