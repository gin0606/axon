//! GPUI desktop shell for Axon: a viewer of the records, which it never changes.
//!
//! The GUI depends on the `axon` library (storage adapters and the re-exported core) and
//! converts core values into display text and GPUI elements here. Nothing from GPUI flows back
//! into the core crates. [`project`] holds the registered management roots and [`board`] the
//! records of one as values, both without GPUI; [`app`] is the window.

pub mod app;
pub mod board;
pub mod project;

pub use app::AxonApp;

use gpui_kit::component::{ActiveTheme, button::Button};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, KeyBinding, Menu, MenuItem, OsAction, Render,
    SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, actions,
    base::input as edit, div, prelude::*, px, size,
};
use project::{AppData, InstanceError, InstanceLock, data::LocateError};
use std::sync::Arc;

actions!(
    axon_gui,
    [
        Quit,
        SelectNextEntity,
        SelectPreviousEntity,
        OpenSelectedEntity,
        Dismiss
    ]
);

/// Smallest window. It shows one column at a time; wider windows show more side by side (see
/// [`app::Columns`]).
pub const MIN_WINDOW_SIZE: (f32, f32) = (380., 460.);

/// Registers the components, key bindings and application menus. Call once before opening
/// windows.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("down", SelectNextEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("enter", OpenSelectedEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(app::APP_CONTEXT)),
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
    /// The lock, which also gives the data it was taken on.
    Ready(InstanceLock),
    Refused(String),
}

/// Locates the data directory and takes the single-instance lock. A second instance on the
/// same data is refused, so only one process ever changes the list of registered roots.
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
        Ok(lock) => Startup::Ready(lock),
        Err(InstanceError::AlreadyRunning) => Startup::Refused(format!(
            "Axon はすでに起動しています。登録したリポジトリの一覧を二つのアプリから書き換えないよう、こちらは開きません。（データの保存場所: {}）",
            data.dir().display()
        )),
        Err(error @ InstanceError::Io(_)) => Startup::Refused(format!(
            "データの保存場所を使えません: {}: {error}",
            data.dir().display()
        )),
    }
}

/// Opens the window [`startup`] calls for: the main window holding the lock, which it returns,
/// or a notice.
pub fn open_startup_window(
    startup: Startup,
    cx: &mut App,
) -> gpui_kit::Result<Option<Entity<AxonApp>>> {
    match startup {
        Startup::Ready(lock) => open_main_window(Arc::new(lock), cx).map(Some),
        Startup::Refused(message) => open_notice_window(message, cx).map(|_| None),
    }
}

/// Opens in `app` the management root that the last of `links` from `axon gui` names, and
/// brings the application to the front. Other links are ignored.
pub fn open_links(app: &WeakEntity<AxonApp>, links: Vec<String>, cx: &mut App) {
    // Each request replaces the one before, so only the last one matters.
    let Some(root) = links
        .iter()
        .filter_map(|link| axon::app_link::parse_open_link(link))
        .next_back()
    else {
        return;
    };
    let Ok(choosing) = app.update(cx, |app, cx| {
        app.open_root(root, cx);
        app.is_choosing_folder()
    }) else {
        return;
    };
    cx.activate(true);
    // A folder dialog that the link waits for stays in front of the window.
    if choosing {
        return;
    }
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    }
}

/// Opens the main window, which keeps `lock` for as long as it is open. Closing the window
/// ends the application.
pub fn open_main_window(
    lock: Arc<InstanceLock>,
    cx: &mut App,
) -> gpui_kit::Result<Entity<AxonApp>> {
    let options = main_window_options(cx);
    let (_, app) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AxonApp::new(lock, window, cx))
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
        app::style::sync_appearance(window, cx);
        cx.new(|_| Notice {
            message: message.into(),
        })
    })?;
    Ok(notice)
}
