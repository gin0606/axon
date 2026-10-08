//! The main window: the registered management roots and the filters on the left, the shared
//! list of the selected root in the middle, and the detail of the selected Entity on the right.
//! The window only reads the records of a root and never changes them; what it changes is the
//! list of registered roots in the application data directory. Disk access runs on the
//! background executor; each result is applied only while the request that produced it is
//! still the current one. Coming back to the window reads the records again, as the reload does.

mod explorer;
pub mod text;

pub use explorer::{LIST_CONTEXT, entity_element};

use crate::{
    board::{Board, Explorer},
    project::{InstanceLock, ProjectConnection, ProjectRoot, Registry, Requests, UpdateError},
};
use gpui_kit::component::{
    ActiveTheme, Disableable, Theme,
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FocusHandle, IntoElement, PathPromptOptions, Render,
    SharedString, UniformListScrollHandle, Window, base::TestSupportExt, div, prelude::*, px,
};
use std::sync::Arc;

/// The width of the left column.
pub const SIDEBAR_WIDTH: f32 = 200.;
/// The narrowest the list and the detail pane get.
pub const LIST_MIN_WIDTH: f32 = 300.;
pub const DETAIL_MIN_WIDTH: f32 = 360.;
/// The tallest the status of the selected root grows before it scrolls.
pub const STATUS_MAX_HEIGHT: f32 = 120.;

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
    /// The outcome of the last registration or unregistration that the screen does not
    /// otherwise show.
    notice: Option<String>,
    /// Counts the user's switches, so a result can tell whether one happened meanwhile.
    switches: u64,
    explorer: Explorer,
    search: Entity<InputState>,
    list_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
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
        Theme::sync_system_appearance(Some(window), cx);
        cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx)
        })
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
            notice: None,
            switches: 0,
            explorer: Explorer::default(),
            search,
            // A tab stop, so the arrow keys are reachable from the keyboard alone.
            list_focus: cx.focus_handle().tab_stop(true),
            list_scroll: UniformListScrollHandle::new(),
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
        if self.busy
            || self.registry_loads.pending().is_some()
            || self.store_loads.pending().is_some()
        {
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
        let lock = self.lock.clone();
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
            let Some(path) = path else {
                this.update(cx, |this, cx| {
                    this.busy = false;
                    this.notice =
                        failure.map(|error| format!("フォルダを選べませんでした。（{error}）"));
                    // Coming back from the dialog read nothing while it was open.
                    this.reload(cx);
                })
                .ok();
                return;
            };
            let outcome = cx
                .background_spawn(async move { reread_after(&lock, |lock| lock.register(&path)) })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
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
                        this.notice = Some(update_message(&error));
                        // A folder already registered is shown, unless the user switched.
                        let select = match &error {
                            UpdateError::Duplicate(root) if this.switches == switches => {
                                Some(root.clone())
                            }
                            _ => keep,
                        };
                        match reloaded {
                            Some(reloaded) => this.apply_registry(reloaded, select, cx),
                            // Coming back from the dialog read nothing while it was open.
                            None => this.reload(cx),
                        }
                    }
                }
                cx.notify();
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
                this.busy = false;
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
                cx.notify();
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let app = cx.entity().downgrade();
        let roots: Vec<ProjectRoot> = match &self.registry {
            RegistryState::Loaded(registry) => registry.roots().to_vec(),
            _ => Vec::new(),
        };
        let current = self.selected.clone();
        let switch_label = match self.selected() {
            Some(root) => format!("{} ▾", root.name()),
            None => "リポジトリなし".into(),
        };
        let loaded = matches!(self.registry, RegistryState::Loaded(_));
        let mut sidebar = div()
            .id("sidebar")
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("リポジトリ"),
            )
            .child(
                Button::new("project-switch")
                    .outline()
                    .w_full()
                    .label(switch_label)
                    .disabled(roots.is_empty())
                    .dropdown_menu(move |menu, _, _| {
                        roots.iter().fold(menu, |menu, root| {
                            let app = app.clone();
                            let chosen = root.clone();
                            menu.item(
                                PopupMenuItem::new(root_label(root))
                                    .checked(current.as_ref() == Some(root))
                                    .on_click(move |_, _, cx| {
                                        let root = chosen.clone();
                                        app.update(cx, |this, cx| this.select(root, cx)).ok();
                                    }),
                            )
                        })
                    }),
            )
            .child(
                Button::new("add-root")
                    .ghost()
                    .w_full()
                    .label("＋ リポジトリを登録")
                    .loading(self.busy)
                    .disabled(self.busy || !loaded)
                    .on_click(cx.listener(|this, _, _, cx| this.add_root(cx))),
            );
        if let Some(notice) = &self.notice {
            sidebar = sidebar.child(
                div()
                    .id("notice")
                    .text_sm()
                    .text_color(theme.danger)
                    .child(notice.clone())
                    .test_support(),
            );
        }
        if loaded {
            sidebar = sidebar.child(self.render_filters(cx));
        }
        sidebar.into_any_element()
    }

    /// What the selected root's store looks like, and the actions that state offers (kept
    /// apart so a long message never scrolls them out of reach).
    fn render_status(&self, cx: &mut Context<Self>) -> (AnyElement, Option<AnyElement>) {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let danger = theme.danger;
        let message = |text: SharedString| div().id("project-status").child(text);
        let reload = || {
            Button::new("reload")
                .outline()
                .label("再読み込み")
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
        };
        let status = div().flex().flex_col().gap_2();
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
                    .child(div().text_sm().text_color(danger).child(error.clone())),
                true,
            ),
            (RegistryState::Loaded(_), None, _) => (
                status.child(message(
                    "登録したリポジトリがありません。左の「＋ リポジトリを登録」から、.axon を持つフォルダ（管理 root）を選んでください。".into(),
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
                        "このリポジトリの記録を読み込めませんでした。空のリポジトリとしては扱いません。他のリポジトリには左のメニューから切り替えられます。".into(),
                    ))
                    .child(div().text_sm().text_color(danger).child(error.clone())),
                true,
            ),
        };
        let status = status
            .child(div().id("project-path").text_xs().text_color(muted).child(
                match self.selected() {
                    Some(root) => format!("管理 root: {root}"),
                    None => format!("登録の一覧の保存場所: {}", self.lock.data().dir().display()),
                },
            ))
            .into_any_element();
        let unregister = self.selected().is_some().then(|| {
            Button::new("remove-root")
                .ghost()
                .label("登録を解除")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| this.remove_selected(cx)))
        });
        let actions = (reloadable || unregister.is_some()).then(|| {
            div()
                .flex()
                .flex_row()
                .gap_2()
                .when(reloadable, |row| row.child(reload()))
                .children(unregister)
                .into_any_element()
        });
        (status, actions)
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

impl Render for AxonApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, foreground, border, muted, danger) = (
            theme.background,
            theme.foreground,
            theme.border,
            theme.muted_foreground,
            theme.danger,
        );
        let title = self
            .selected()
            .map(ProjectRoot::name)
            .unwrap_or_else(|| "Axon".into());
        let sidebar = self.render_sidebar(cx);
        let (status, action) = self.render_status(cx);
        let list = self.render_list(cx);
        let detail: AnyElement = match self.explorer.detail() {
            Some(Ok(detail)) => self.render_detail(detail, cx),
            Some(Err(error)) => div()
                .id("detail-error")
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_color(danger)
                        .child(format!("詳細を表示できません: {error}")),
                )
                .child(
                    Button::new("close-detail")
                        .ghost()
                        .compact()
                        .label("閉じる")
                        .on_click(cx.listener(|this, _, _, cx| this.close_entity(cx))),
                )
                .into_any_element(),
            // Nothing is open: the column stays empty, and points at the list while it has rows.
            None => div()
                .id("detail-empty")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .when(self.explorer.listing().matched > 0, |empty| {
                    empty.child("一覧から Issue・Group を選ぶと、ここに詳細を表示します。")
                })
                .test_support()
                .into_any_element(),
        };
        div()
            .id("axon-app")
            .size_full()
            .flex()
            .flex_row()
            .bg(background)
            .text_color(foreground)
            .child(sidebar)
            .child(
                div()
                    .id("project-pane")
                    .flex_1()
                    .min_w(px(LIST_MIN_WIDTH))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .child(
                        div()
                            .id("project-title")
                            .text_xl()
                            .min_w_0()
                            .truncate()
                            .child(title),
                    )
                    // Long errors scroll here instead of pushing the list out of a small window.
                    .child(
                        div()
                            .id("project-status-area")
                            .flex_none()
                            .max_h(px(STATUS_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .child(status),
                    )
                    .children(action)
                    .children(list),
            )
            .child(
                div()
                    .id("detail-pane")
                    .flex_1()
                    .min_w(px(DETAIL_MIN_WIDTH))
                    .h_full()
                    .border_l_1()
                    .border_color(border)
                    .child(detail),
            )
    }
}

/// How a root is listed: its name and, to tell roots of the same name apart, its path.
fn root_label(root: &ProjectRoot) -> String {
    format!("{} — {}", root.name(), root)
}

fn update_message(error: &UpdateError) -> String {
    match error {
        UpdateError::Unreachable { path, error } => format!(
            "選んだフォルダを開けないため登録しませんでした。（{}: {error}）",
            path.display()
        ),
        UpdateError::NotADirectory(path) => format!(
            "フォルダではないため登録しませんでした。（{}）",
            path.display()
        ),
        UpdateError::NotAStore(header) => format!(
            "Axon の保存先（.axon の header）がないフォルダは登録できません。.axon を持つ管理 root を選んでください。（{} がありません）",
            header.display()
        ),
        UpdateError::Unrepresentable(path) => format!(
            "パスに UTF-8 で表せない文字を含むフォルダは登録できません。（{}）",
            path.display()
        ),
        UpdateError::Duplicate(root) => format!("{root} はすでに登録しています。"),
        UpdateError::Unknown(root) => format!("{root} は登録されていません。"),
        UpdateError::Read(error) => {
            format!("登録したリポジトリの一覧を読み込めないため、変更しませんでした。（{error}）")
        }
        UpdateError::Save(error) => format!(
            "登録したリポジトリの一覧を保存できませんでした。一覧には読み直した結果を表示しています。（{error}）"
        ),
    }
}
