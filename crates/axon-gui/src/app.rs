//! The main window: the registered management roots and the filters, the shared list of the
//! selected root, and the detail of the selected Entity, side by side as far as the width allows
//! (see [`layout`]).
//! The window only reads the records of a root and never changes them; what it changes is the
//! list of registered roots in the application data directory. Disk access runs on the
//! background executor; each result is applied only while the request that produced it is
//! still the current one. Coming back to the window reads the records again, as the reload does.

mod explorer;
mod layout;
mod markdown;
pub mod style;
pub mod text;

pub use explorer::{LIST_CONTEXT, entity_element};
pub use layout::{
    APP_CONTEXT, BAR_HEIGHT, COLUMN_HYSTERESIS, Columns, DETAIL_MIN_WIDTH, LIST_MIN_WIDTH,
    SIDEBAR_PANEL_WIDTH, SIDEBAR_WIDTH, THREE_COLUMNS_MIN_WIDTH, TWO_COLUMNS_MIN_WIDTH,
};

use crate::{
    board::{Board, Explorer},
    project::{InstanceLock, ProjectConnection, ProjectRoot, Registry, Requests, UpdateError},
};
use gpui_kit::component::{
    button::Button,
    input::{InputEvent, InputState},
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext, Context, Entity, FocusHandle, IntoElement,
    PathPromptOptions, PromptButton, PromptLevel, SharedString, UniformListScrollHandle, Window,
    div, prelude::*,
};
use std::{path::PathBuf, sync::Arc};
use style::Palette;

/// The button that confirms registering the root a link names.
pub const REGISTER_ANSWER: &str = "登録して開く";
/// The button that declines it.
pub const CANCEL_ANSWER: &str = "キャンセル";

/// The tallest the status of the selected root grows before it scrolls.
pub const STATUS_MAX_HEIGHT: f32 = 120.;

/// A root to open that a link asked for, and how many switches the user had made by then.
struct Request {
    path: PathBuf,
    switches: u64,
    /// The registry was read again for it after a read had failed.
    reread: bool,
}

/// What a registration adds: a folder the user chose, or the root a link named and the user
/// confirmed.
enum Target {
    Folder(PathBuf),
    Link(ProjectRoot),
}

/// The registry as last read from disk.
#[derive(Debug)]
pub enum RegistryState {
    Loading,
    /// The registry could not be read. It is not shown as empty and nothing is written to it.
    Failed(String),
    Loaded(Registry),
}

/// What the selected root's store looks like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// No root is selected.
    None,
    Loading,
    Loaded(Summary),
    /// The store is read again while what the last read gave stays on screen.
    Reloading(Summary),
    /// The store could not be read. It is not shown as empty.
    Failed(String),
}

/// What the main pane shows of a readable store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub entities: usize,
}

pub struct AxonApp {
    /// Held for as long as the window is open; the registry is changed through it.
    lock: Arc<InstanceLock>,
    registry: RegistryState,
    registry_loads: Requests<()>,
    selected: Option<ProjectRoot>,
    store_loads: Requests,
    store: StoreState,
    /// A folder is being chosen or the registry is being changed; another change is not
    /// started meanwhile.
    busy: bool,
    /// The folder dialog is open.
    choosing_folder: bool,
    /// The main window, where a link's registration is confirmed.
    window: AnyWindowHandle,
    /// The outcome of the last registration or unregistration that the screen does not
    /// otherwise show.
    notice: Option<String>,
    /// A root `axon gui` asked to open, waiting for the registry to settle.
    requested: Option<Request>,
    /// Counts the switches of root, the user's and those a link makes, so a result can tell
    /// whether one happened meanwhile.
    switches: u64,
    explorer: Explorer,
    search: Entity<InputState>,
    list_focus: FocusHandle,
    /// The window's own focus, which Escape reaches when the focused list leaves the screen.
    app_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    /// The detail column, which a click in it focuses and which outlives the Entity shown.
    detail_focus: FocusHandle,
    /// Descriptions and Notes as the detail renders them.
    prepared: markdown::Prepared,
    /// Which columns are shown, which panel is open and how the detail was reached.
    layout: layout::State,
}

impl AxonApp {
    pub fn new(lock: Arc<InstanceLock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("タイトル・本文・ID を検索"));
        cx.subscribe_in(
            &search,
            window,
            |this, search, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    let query = search.read(cx).value().to_string();
                    this.update_filter(|filter| filter.query = query, cx);
                }
            },
        )
        .detach();
        style::sync_appearance(window, cx);
        cx.observe_window_appearance(window, |_, window, cx| style::sync_appearance(window, cx))
            .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.reload_on_activation(cx);
            }
        })
        .detach();
        let mut this = Self {
            lock,
            registry: RegistryState::Loading,
            registry_loads: Requests::default(),
            selected: None,
            store_loads: Requests::default(),
            store: StoreState::None,
            busy: false,
            choosing_folder: false,
            window: window.window_handle(),
            notice: None,
            requested: None,
            switches: 0,
            explorer: Explorer::default(),
            search,
            // A tab stop, so the arrow keys are reachable from the keyboard alone.
            list_focus: cx.focus_handle().tab_stop(true),
            app_focus: cx.focus_handle(),
            list_scroll: UniformListScrollHandle::new(),
            detail_focus: cx.focus_handle(),
            prepared: markdown::Prepared::default(),
            layout: layout::State::default(),
        };
        this.reload_registry(cx);
        this
    }

    pub fn registry(&self) -> &RegistryState {
        &self.registry
    }
    /// The selected root, while it is registered.
    pub fn selected(&self) -> Option<&ProjectRoot> {
        let RegistryState::Loaded(registry) = &self.registry else {
            return None;
        };
        self.selected
            .as_ref()
            .filter(|root| registry.contains(root))
    }
    pub fn store(&self) -> &StoreState {
        &self.store
    }
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    pub fn is_choosing_folder(&self) -> bool {
        self.choosing_folder
    }
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
    pub fn explorer(&self) -> &Explorer {
        &self.explorer
    }
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search
    }
    /// How many reads of the registry and of stores the window has started.
    pub fn reads_started(&self) -> u64 {
        self.registry_loads.issued() + self.store_loads.issued()
    }
    /// The focus of the list, where the arrow keys move the selection.
    pub fn list_focus(&self) -> &FocusHandle {
        &self.list_focus
    }

    /// Reads the registry again, keeping the selection when it is still registered.
    fn reload_registry(&mut self, cx: &mut Context<Self>) {
        let ticket = self.registry_loads.begin(());
        let lock = self.lock.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { lock.data().load_registry() })
                .await
                .map_err(|error| error.to_string());
            this.update(cx, |this, cx| {
                if this.registry_loads.finish(&ticket) {
                    let keep = this.selected.clone();
                    this.apply_registry(result, keep, cx);
                    this.open_requested(cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Shows a registry read from disk, or why it could not be read, selecting `select` when
    /// it is registered and the first root otherwise.
    fn apply_registry(
        &mut self,
        result: Result<Registry, String>,
        select: Option<ProjectRoot>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(registry) => {
                let select = select
                    .filter(|root| registry.contains(root))
                    .or_else(|| registry.roots().first().cloned());
                self.registry = RegistryState::Loaded(registry);
                self.selected = select;
            }
            Err(error) => {
                self.registry = RegistryState::Failed(error);
                self.selected = None;
            }
        }
        self.show_selected(cx);
    }

    /// Switches to the registered root `root`; a root no longer registered, as a menu opened
    /// before an unregistration can offer, is ignored.
    pub fn select(&mut self, root: ProjectRoot, cx: &mut Context<Self>) {
        if !matches!(&self.registry, RegistryState::Loaded(registry) if registry.contains(&root)) {
            return;
        }
        // The list of the chosen root is what the user wants to see next.
        self.close_panel(cx);
        if self.selected.as_ref() != Some(&root) {
            self.switches += 1;
            // The outcome of the last change was about the window as it was before.
            self.notice = None;
        }
        self.selected = Some(root);
        self.show_selected(cx);
    }

    /// Reads the selected root's store again.
    pub fn reload_selected(&mut self, cx: &mut Context<Self>) {
        self.show_selected(cx);
    }

    /// Reads again what the reload would when the window comes back to the front. While a read
    /// or a change of the registry is already running, or a folder is being chosen, its result
    /// is shown instead and no read is stacked on it; a change made after that read began shows
    /// on the next return or reload.
    fn reload_on_activation(&mut self, cx: &mut Context<Self>) {
        if self.registry_busy() || self.store_loads.pending().is_some() {
            return;
        }
        self.reload(cx);
    }

    /// Shows the selected root, reading its store. A read still running for the previously
    /// selected root becomes stale. Reading the root on screen again keeps its list and detail
    /// until the result replaces them.
    fn show_selected(&mut self, cx: &mut Context<Self>) {
        self.store_loads.cancel();
        let root = self.selected().cloned();
        let same = self.explorer.project() == root.as_ref();
        let shown = match &self.store {
            StoreState::Loaded(summary) | StoreState::Reloading(summary) if same => Some(*summary),
            _ => None,
        };
        if !same {
            self.scroll_list_to_top();
            self.explorer.deselect();
        }
        if shown.is_none() {
            self.explorer.unload(root.as_ref());
        }
        self.store = match root {
            None => StoreState::None,
            Some(root) => {
                let ticket = self.store_loads.begin(root.clone());
                let connection = ProjectConnection::new(root);
                cx.spawn(async move |this, cx| {
                    // The rows are derived off the UI thread too; the window only filters them.
                    let result = cx
                        .background_spawn(async move {
                            connection
                                .load()
                                .map(|(_, records, view)| Board::new(records, view))
                        })
                        .await;
                    this.update(cx, |this, cx| {
                        if this.store_loads.finish(&ticket) {
                            this.store = match result {
                                Ok(board) => {
                                    let summary = Summary {
                                        entities: board.len(),
                                    };
                                    this.explorer.load(ticket.key(), board);
                                    StoreState::Loaded(summary)
                                }
                                Err(error) => {
                                    // Nothing of the root is shown, so neither is a selection
                                    // that a later read would bring back unasked.
                                    this.explorer.unload(Some(ticket.key()));
                                    this.explorer.deselect();
                                    StoreState::Failed(error.to_string())
                                }
                            };
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
                match shown {
                    Some(summary) => StoreState::Reloading(summary),
                    None => StoreState::Loading,
                }
            }
        };
        cx.notify();
    }

    /// Asks for a folder and registers the management root it is. Until the outcome and the
    /// registry on disk have been shown, another change does nothing. The new root is selected
    /// unless the user switched meanwhile.
    pub fn add_root(&mut self, cx: &mut Context<Self>) {
        if self.busy || !matches!(self.registry, RegistryState::Loaded(_)) {
            return;
        }
        self.busy = true;
        self.notice = None;
        // A switch from here on, while the dialog is open too, keeps the user's choice.
        let switches = self.switches;
        self.choosing_folder = true;
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("登録".into()),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (path, failure) = match chosen.await {
                Ok(Ok(Some(paths))) => (paths.into_iter().next(), None),
                Ok(Ok(None)) | Err(_) => (None, None),
                Ok(Err(error)) => (None, Some(error)),
            };
            this.update(cx, |this, cx| {
                this.choosing_folder = false;
                match path {
                    Some(path) => this.register(Target::Folder(path), switches, cx),
                    None => {
                        this.notice =
                            failure.map(|error| format!("フォルダを選べませんでした。（{error}）"));
                        // Coming back from the dialog read nothing while it was open.
                        this.reload(cx);
                        this.finish_change(cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens the management root at `path`, registering it first when it is not registered,
    /// as `axon gui` asks. A request that arrives while the registry is being read or changed
    /// waits for that; a later request replaces one still waiting. A registry that could not be
    /// read is read again first. The outcome of a change that finishes meanwhile stays on screen,
    /// and a root the user switches to meanwhile stays selected.
    pub fn open_root(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // Only the outcome of a change still running, or one a waiting link took over, is kept
        // for the link to add to.
        if !self.busy && self.requested.is_none() {
            self.notice = None;
        }
        self.requested = Some(Request {
            path,
            switches: self.switches,
            reread: false,
        });
        self.open_requested(cx);
    }

    /// Whether the registry is being read or changed.
    fn registry_busy(&self) -> bool {
        self.busy || self.registry_loads.pending().is_some()
    }

    /// Shows `message` after the outcome already on screen, which another change left there.
    fn add_notice(&mut self, message: String) {
        self.notice = Some(match self.notice.take() {
            Some(shown) => format!("{shown}\n{message}"),
            None => message,
        });
        self.reveal_notice();
    }

    /// Ends a change of the registry and carries out an [`open_root`](Self::open_root) request
    /// that waited for it. Every change that set `busy` ends here.
    fn finish_change(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.open_requested(cx);
        cx.notify();
    }

    /// Carries out the waiting [`open_root`](Self::open_root) request once the registry is
    /// neither being read nor changed. A registry that still cannot be read is not changed.
    fn open_requested(&mut self, cx: &mut Context<Self>) {
        if self.registry_busy() {
            return;
        }
        let Some(mut request) = self.requested.take() else {
            return;
        };
        if let RegistryState::Failed(_) = self.registry {
            if !request.reread {
                request.reread = true;
                self.requested = Some(request);
                self.reload_registry(cx);
                return;
            }
            self.add_notice(format!(
                "{} を開けませんでした。登録したリポジトリの一覧を読み込めません。",
                text::path(request.path.display())
            ));
            cx.notify();
            return;
        }
        self.busy = true;
        let switches = if self.switches == request.switches {
            self.close_panel(cx);
            self.switches += 1;
            self.switches
        } else {
            // The user chose a root after the link arrived: the link registers but keeps it.
            request.switches
        };
        self.confirm_then_register(request.path, switches, cx);
    }

    /// Opens the root a link names. A root already registered is selected; any other is
    /// registered only once the user confirms its path: a link can come from anything that
    /// opens URLs, and reading a root runs Git in it. Nothing on disk is looked at before that,
    /// and only the confirmed path itself is registered, never what it resolves to by then
    /// (`axon gui` sends a resolved path). The caller has set `busy`.
    fn confirm_then_register(&mut self, path: PathBuf, switches: u64, cx: &mut Context<Self>) {
        let root = match ProjectRoot::new(path) {
            Ok(root) => root,
            Err(error) => {
                self.add_notice(format!(
                    "リンクのフォルダを開けませんでした。（{}）",
                    text::path(error)
                ));
                self.finish_change(cx);
                return;
            }
        };
        let registered = match &self.registry {
            RegistryState::Loaded(registry) => registry
                .roots()
                .iter()
                .find(|registered| registered.path().as_os_str() == root.path().as_os_str())
                .cloned(),
            _ => None,
        };
        if let Some(registered) = registered {
            if self.switches == switches {
                self.selected = Some(registered);
                self.show_selected(cx);
            }
            self.finish_change(cx);
            return;
        }
        let detail = format!(
            "{}\n\nリンクでこのフォルダを開くよう求められました。リンクは axon gui のほか、Web ページやほかのアプリからも送れます。心当たりがなければ登録しないでください。登録すると、記録を読むためにこのフォルダで Git を実行します。",
            text::path(&root)
        );
        let asked = self.window.update(cx, |_, window, cx| {
            window.prompt(
                PromptLevel::Warning,
                "このフォルダを登録して開きますか？",
                Some(&detail),
                // Return does not register: Escape cancels and nothing is the default.
                &[
                    PromptButton::Cancel(CANCEL_ANSWER.into()),
                    PromptButton::new(REGISTER_ANSWER),
                ],
                cx,
            )
        });
        let Ok(answer) = asked else {
            self.add_notice(format!(
                "{} を登録してよいかを確かめられないため、登録しませんでした。",
                text::path(&root)
            ));
            self.finish_change(cx);
            return;
        };
        cx.spawn(async move |this, cx| {
            let register = matches!(answer.await, Ok(1));
            this.update(cx, |this, cx| {
                if register {
                    this.register(Target::Link(root), switches, cx);
                } else {
                    // Coming to the front read nothing while the link was being handled.
                    this.reload(cx);
                    this.finish_change(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Registers `target` and selects the root it is, unless the user switched since `switches`.
    /// The caller has set `busy`. For a link, a root already registered is selected without a
    /// notice, since opening it is what was asked.
    fn register(&mut self, target: Target, switches: u64, cx: &mut Context<Self>) {
        let lock = self.lock.clone();
        let open = matches!(target, Target::Link(_));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    reread_after(&lock, |lock| match &target {
                        Target::Folder(path) => lock.register(path),
                        Target::Link(root) => lock.register_exact(root),
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                let keep = this.selected.clone();
                match outcome {
                    Ok((registry, root)) => {
                        let select = if this.switches == switches {
                            Some(root)
                        } else {
                            keep
                        };
                        this.apply_registry(Ok(registry), select, cx);
                    }
                    Err((error, reloaded)) => {
                        match (&error, open) {
                            (UpdateError::Duplicate(_), true) => {}
                            (_, true) => this.add_notice(update_message(&error)),
                            (_, false) => this.notice = Some(update_message(&error)),
                        }
                        // A folder already registered is shown, unless the user switched.
                        let select = match &error {
                            UpdateError::Duplicate(root) if this.switches == switches => {
                                Some(root.clone())
                            }
                            _ => keep,
                        };
                        match reloaded {
                            Some(reloaded) => this.apply_registry(reloaded, select, cx),
                            // The selected root was not read while the change ran.
                            None => this.reload(cx),
                        }
                    }
                }
                this.finish_change(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Removes the selected root from the registry, leaving its files as they are.
    pub fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.selected().cloned() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.notice = None;
        let lock = self.lock.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn({
                    let root = root.clone();
                    async move { reread_after(&lock, |lock| lock.unregister(&root)) }
                })
                .await;
            this.update(cx, |this, cx| {
                let keep = this.selected.clone().filter(|selected| selected != &root);
                match outcome {
                    Ok(registry) => this.apply_registry(Ok(registry), keep, cx),
                    Err((error, reloaded)) => {
                        this.notice = Some(update_message(&error));
                        if let Some(reloaded) = reloaded {
                            this.apply_registry(reloaded, this.selected.clone(), cx);
                        }
                    }
                }
                this.finish_change(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        match self.registry {
            RegistryState::Failed(_) => self.reload_registry(cx),
            _ => self.reload_selected(cx),
        }
    }

    /// What the selected root's store looks like, and the actions that state offers (kept
    /// apart so a long message never scrolls them out of reach).
    fn render_status(&self, cx: &mut Context<Self>) -> (AnyElement, Option<AnyElement>) {
        let palette = Palette::of(cx);
        let danger = palette.danger;
        let message = |text: SharedString| div().id("project-status").text_sm().child(text);
        let reload = || {
            Button::new("reload")
                .outline()
                .label("再読み込み")
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
        };
        let status = div().flex().flex_col().gap_1();
        let (status, reloadable) = match (&self.registry, self.selected(), &self.store) {
            (RegistryState::Loading, _, _) => (
                status.child(message("登録したリポジトリの一覧を読み込み中…".into())),
                false,
            ),
            (RegistryState::Failed(error), _, _) => (
                status
                    .child(message(
                        "登録したリポジトリの一覧を読み込めませんでした。空の一覧として扱わず、書き換えもしません。".into(),
                    ))
                    .child(div().text_xs().text_color(danger).child(error.clone())),
                true,
            ),
            (RegistryState::Loaded(_), None, _) => (
                status.child(message(
                    "登録したリポジトリがありません。「＋ リポジトリを登録」から、.axon を持つフォルダ（管理 root）を選んでください。".into(),
                )),
                false,
            ),
            (_, Some(_), StoreState::Loading | StoreState::None) => {
                (status.child(message("読み込み中…".into())), false)
            }
            (_, Some(_), StoreState::Loaded(summary) | StoreState::Reloading(summary)) => {
                let count = if summary.entities == 0 {
                    "Issue・Group はまだありません。".to_string()
                } else {
                    format!("{} 件の Issue・Group があります。", summary.entities)
                };
                let text = if matches!(self.store, StoreState::Reloading(_)) {
                    format!("{count}（読み直し中…）")
                } else {
                    count
                };
                (status.child(message(text.into())), false)
            }
            (_, Some(_), StoreState::Failed(error)) => (
                status
                    .child(message(
                        "このリポジトリの記録を読み込めませんでした。空のリポジトリとしては扱いません。他のリポジトリには「リポジトリ」の欄から切り替えられます。".into(),
                    ))
                    .child(div().text_xs().text_color(danger).child(error.clone())),
                true,
            ),
        };
        let actions = reloadable.then(|| reload().into_any_element());
        (status.into_any_element(), actions)
    }
}

/// The registry read again after a change that failed, when the file may differ from what the
/// window shows: the change was made to the file, not to what the window held. A refused
/// folder says nothing of the file, and nothing is read for it.
type Reread = Option<Result<Registry, String>>;

/// Runs a change of the registry, reading the registry again after a failure that calls for it.
fn reread_after<T>(
    lock: &InstanceLock,
    change: impl FnOnce(&InstanceLock) -> Result<T, UpdateError>,
) -> Result<T, (UpdateError, Reread)> {
    change(lock).map_err(|error| {
        // A duplicate or an unknown root may come from a file that differs from what the window
        // shows.
        let reloaded = matches!(
            error,
            UpdateError::Read(_)
                | UpdateError::Save(_)
                | UpdateError::Duplicate(_)
                | UpdateError::Unknown(_)
        )
        .then(|| lock.data().load_registry().map_err(|e| e.to_string()));
        (error, reloaded)
    })
}

/// How a root is listed: its name and, to tell roots of the same name apart, its path.
fn root_label(root: &ProjectRoot) -> String {
    format!("{} — {}", root.name(), root)
}

fn update_message(error: &UpdateError) -> String {
    match error {
        UpdateError::Unreachable { path, error } => format!(
            "フォルダを開けないため登録しませんでした。（{}: {error}）",
            text::path(path.display())
        ),
        UpdateError::NotADirectory(path) => format!(
            "フォルダではないため登録しませんでした。（{}）",
            text::path(path.display())
        ),
        UpdateError::NotAStore(header) => format!(
            "Axon の保存先（.axon の header）がないフォルダは登録できません。.axon を持つ管理 root を選んでください。（{} がありません）",
            text::path(header.display())
        ),
        UpdateError::Unrepresentable(path) => format!(
            "パスに UTF-8 で表せない文字を含むフォルダは登録できません。（{}）",
            text::path(path.display())
        ),
        UpdateError::Duplicate(root) => format!("{} はすでに登録しています。", text::path(root)),
        UpdateError::Elsewhere { asked, resolved } => format!(
            "{} は今は {} を指しているため登録しませんでした。登録したいフォルダで axon gui を実行し直してください。",
            text::path(asked),
            text::path(resolved.display())
        ),
        UpdateError::Unknown(root) => format!("{} は登録されていません。", text::path(root)),
        UpdateError::Read(error) => {
            format!("登録したリポジトリの一覧を読み込めないため、変更しませんでした。（{error}）")
        }
        UpdateError::Save(error) => format!(
            "登録したリポジトリの一覧を保存できませんでした。一覧には読み直した結果を表示しています。（{error}）"
        ),
    }
}
