//! GPUI desktop shell for Axon: a viewer of the records, which it never changes.
//!
//! The GUI depends on the `axon` library (storage adapters and the re-exported core) and
//! converts core values into display text and GPUI elements here. Nothing from GPUI flows back
//! into the core crates. [`project`] holds the registered management roots and [`board`] the
//! records of one as values, both without GPUI; [`session`] keeps where the last run left off
//! for the next start; [`settings`] keeps display preferences; [`app`] is the window.

// The translation macro reads files itself; include the catalog in Cargo's dependency graph.
const _: &str = include_str!("../locales/ui.json");
rust_i18n::i18n!("locales", fallback = "en");

pub mod i18n;
pub use i18n::Language;
pub(crate) use i18n::tr;

pub mod app;
pub mod board;
pub mod project;
pub mod session;
pub mod settings;

pub use app::AxonApp;

use gpui_kit::component::{ActiveTheme, button::Button};
use gpui_kit::{
    App, AppContext, Bounds, Context, DisplayId, Entity, KeyBinding, Menu, MenuItem, OsAction,
    Pixels, Render, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions, actions,
    base::input as edit, div, point, prelude::*, px, size,
};
use project::{AppData, InstanceError, InstanceLock, data::LocateError};
use session::{Placement, SessionFile};
use std::sync::Arc;

actions!(
    axon_gui,
    [
        Quit,
        OpenSettings,
        SelectNextEntity,
        SelectPreviousEntity,
        OpenSelectedEntity,
        Dismiss
    ]
);

/// Smallest window. It shows one column at a time; wider windows show more side by side (see
/// [`app::Columns`]).
pub const MIN_WINDOW_SIZE: (f32, f32) = (380., 460.);

/// Registers components and actions using the system language. Production startup uses
/// [`init_for_startup`] to apply saved preferences before creating menus.
pub fn init(cx: &mut App) {
    init_with_language(Language::system(), cx);
}

/// Resolves saved preferences before constructing the first menu or window.
pub fn init_for_startup(startup: &Startup, cx: &mut App) {
    let system = Language::system();
    let loaded = startup.data().map(settings::load);
    let preference = loaded
        .as_ref()
        .and_then(|loaded| loaded.as_ref().ok())
        .copied()
        .unwrap_or_default();
    initialize(preference.resolve(system), system, cx);
    if let (Startup::Ready(_), Some(loaded)) = (startup, loaded) {
        cx.set_global(settings::Initial(loaded));
    }
}

/// Initializes components and menus with an injected system language. A main window also
/// applies any saved preference, as production startup does.
pub fn init_with_language(language: Language, cx: &mut App) {
    initialize(language, language, cx);
}

fn initialize(language: Language, system: Language, cx: &mut App) {
    cx.set_global(i18n::SystemLanguage(system));
    i18n::init(language, cx);
    gpui_kit::init(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("down", SelectNextEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("up", SelectPreviousEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("enter", OpenSelectedEntity, Some(app::LIST_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(app::APP_CONTEXT)),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(menus(language));
}

/// The menu bar: the application menu and the standard editing commands that text inputs
/// handle.
pub fn menus(language: Language) -> Vec<Menu> {
    vec![
        Menu::new("Axon").items([
            MenuItem::action(crate::tr!(language, "menu.settings"), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(crate::tr!(language, "menu.quit"), Quit),
        ]),
        Menu::new(crate::tr!(language, "menu.edit")).items([
            MenuItem::os_action(
                crate::tr!(language, "menu.undo"),
                edit::Undo,
                OsAction::Undo,
            ),
            MenuItem::os_action(
                crate::tr!(language, "menu.redo"),
                edit::Redo,
                OsAction::Redo,
            ),
            MenuItem::separator(),
            MenuItem::os_action(crate::tr!(language, "menu.cut"), edit::Cut, OsAction::Cut),
            MenuItem::os_action(
                crate::tr!(language, "menu.copy"),
                edit::Copy,
                OsAction::Copy,
            ),
            MenuItem::os_action(
                crate::tr!(language, "menu.paste"),
                edit::Paste,
                OsAction::Paste,
            ),
            MenuItem::os_action(
                crate::tr!(language, "menu.select_all"),
                edit::SelectAll,
                OsAction::SelectAll,
            ),
        ]),
    ]
}

/// The size the main window first opens with.
pub const WINDOW_SIZE: (f32, f32) = (1120., 700.);

/// How much of a restored window's title bar must lie on one display for the window to open
/// where it was: its height, and this width or the whole bar when narrower.
pub const TITLE_BAR_GRIP: (f32, f32) = (80., 28.);

/// The room a restored window leaves for the title bar its frame adds above the content.
pub const TITLE_BAR_HEIGHT: f32 = 28.;

/// Options of the main window: centered, titled, and never smaller than [`MIN_WINDOW_SIZE`].
pub fn main_window_options(cx: &App) -> WindowOptions {
    restored_window_options(None, cx)
}

/// [`main_window_options`] with the window where `placement` left it (see [`restored_bounds`]).
pub fn restored_window_options(placement: Option<&Placement>, cx: &App) -> WindowOptions {
    let (bounds, display_id) = restored_bounds(placement, cx);
    WindowOptions {
        window_bounds: Some(bounds),
        display_id,
        window_min_size: Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1))),
        titlebar: Some(TitlebarOptions {
            title: Some("Axon".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The bounds and the display to open the main window with: the saved place on the saved
/// display (the primary one when the platform named none), maximized when it was, or the first
/// size centered on the primary display. A saved window on a display not connected now is
/// centered on the primary display, and one whose title bar is not on the visible part of its
/// display is centered on it; both keep their saved size, which never exceeds the display.
pub fn restored_bounds(
    placement: Option<&Placement>,
    cx: &App,
) -> (WindowBounds, Option<DisplayId>) {
    let Some(placement) = placement else {
        let first = size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1));
        return (
            WindowBounds::Windowed(Bounds::centered(None, first, cx)),
            None,
        );
    };
    let display = match placement.display.as_deref() {
        Some(saved) => cx.displays().into_iter().find(|display| {
            display
                .uuid()
                .is_ok_and(|uuid| uuid.to_string().eq_ignore_ascii_case(saved))
        }),
        None => cx.primary_display(),
    };
    let mut width = px(placement.width);
    let mut height = px(placement.height);
    // The display it opens on: the saved one, or the primary one it is centered on.
    if let Some(target) = display.clone().or_else(|| cx.primary_display()) {
        // The window's frame adds a title bar to the content; the menu bar and the Dock are
        // outside the visible bounds.
        let shown = target.visible_bounds().size;
        width = width.min(shown.width);
        height = height.min(shown.height - px(TITLE_BAR_HEIGHT));
    }
    let saved = Bounds {
        origin: point(px(placement.x), px(placement.y)),
        size: size(
            width.max(px(MIN_WINDOW_SIZE.0)),
            height.max(px(MIN_WINDOW_SIZE.1)),
        ),
    };
    let display_id = display.as_ref().map(|display| display.id());
    let bounds = match &display {
        Some(display) if reachable(saved, display.visible_bounds()) => saved,
        _ => {
            // The frame, title bar included, is what is centered; the window opens with the
            // content size at the frame's top left corner.
            let bar = px(TITLE_BAR_HEIGHT);
            let frame = size(saved.size.width, saved.size.height + bar);
            let frame = Bounds::centered(display_id, frame, cx);
            Bounds {
                origin: frame.origin,
                size: size(frame.size.width, frame.size.height - bar),
            }
        }
    };
    let bounds = if placement.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (bounds, display_id)
}

/// Whether the title bar of a window at `bounds` can be grabbed on `display`, the visible part
/// of a display in the same coordinates (see [`TITLE_BAR_GRIP`]).
pub fn reachable(bounds: Bounds<Pixels>, display: Bounds<Pixels>) -> bool {
    let bar = Bounds {
        origin: bounds.origin,
        size: size(bounds.size.width, px(TITLE_BAR_GRIP.1)),
    };
    let shown = bar.intersect(&display);
    shown.size.width >= bar.size.width.min(px(TITLE_BAR_GRIP.0))
        && shown.size.height >= bar.size.height
}

/// Where `window` is as a plain window: the top left corner of its frame, the size of its
/// content, which is what a window opens with, and its display. Whether it is maximized is
/// left to the caller.
pub fn windowed_placement(window: &Window, cx: &App) -> Placement {
    let origin = window.bounds().origin;
    let content = window.viewport_size();
    Placement {
        x: origin.x.into(),
        y: origin.y.into(),
        width: content.width.into(),
        height: content.height.into(),
        display: window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string()),
        maximized: false,
    }
}

/// What a start found: the data to open, or why this instance must not open it.
pub enum Startup {
    /// The lock, which also gives the data it was taken on.
    Ready(InstanceLock),
    Refused(StartupRefusal),
}

impl Startup {
    fn data(&self) -> Option<&AppData> {
        match self {
            Self::Ready(lock) => Some(lock.data()),
            Self::Refused(
                StartupRefusal::AlreadyRunning(data) | StartupRefusal::Io { data, .. },
            ) => Some(data),
            Self::Refused(StartupRefusal::Locate(_)) => None,
        }
    }
}

/// The reason startup was refused, retaining diagnostics for translation when displayed.
pub enum StartupRefusal {
    Locate(LocateError),
    AlreadyRunning(AppData),
    Io { data: AppData, error: InstanceError },
}

impl StartupRefusal {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Locate(error) => crate::tr!(language, "startup.locate", error = error),
            Self::AlreadyRunning(data) => {
                crate::tr!(language, "startup.running", path = data.dir().display())
            }
            Self::Io { data, error } => crate::tr!(
                language,
                "startup.io",
                path = data.dir().display(),
                error = error
            ),
        }
    }
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
        Err(error) => return Startup::Refused(StartupRefusal::Locate(error)),
    };
    match data.lock_instance() {
        Ok(lock) => Startup::Ready(lock),
        Err(InstanceError::AlreadyRunning) => {
            Startup::Refused(StartupRefusal::AlreadyRunning(data))
        }
        Err(error @ InstanceError::Io(_)) => Startup::Refused(StartupRefusal::Io { data, error }),
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

/// Opens the main window, which keeps `lock` for as long as it is open, as the last run left it.
/// Closing the window ends the application.
pub fn open_main_window(
    lock: Arc<InstanceLock>,
    cx: &mut App,
) -> gpui_kit::Result<Entity<AxonApp>> {
    let file = Arc::new(SessionFile::new(lock.data().clone()));
    let session = file.load();
    let options = restored_window_options(session.placement.as_ref(), cx);
    let (_, app) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AxonApp::restore(lock, file, session, window, cx))
    })?;
    Ok(app)
}

/// A window that only explains why the application did not open its data.
pub struct Notice {
    reason: StartupRefusal,
    language: Language,
}
impl Notice {
    pub fn message(&self) -> String {
        self.reason.message(self.language)
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
            .child(div().id("notice-message").child(self.message()))
            .child(
                Button::new("quit")
                    .outline()
                    .label(crate::tr!(self.language, "common.quit"))
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

pub fn open_notice_window(
    reason: StartupRefusal,
    cx: &mut App,
) -> gpui_kit::Result<Entity<Notice>> {
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
        cx.new(|cx| Notice {
            reason,
            language: Language::of(cx),
        })
    })?;
    Ok(notice)
}
