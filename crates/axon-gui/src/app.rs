//! The main window: project switching, creation and the filters on the left, the shared list of
//! the selected project in the middle, and the detail of the selected Entity on the right. The
//! window only reads the records of a project and never changes them. Disk access runs on the
//! background executor; each result is applied only while the request that produced it is
//! still the current one.

mod explorer;
pub mod text;

pub use explorer::{LIST_CONTEXT, entity_element};

use crate::{
    board::{Board, Explorer},
    project::{
        AppData, CreateError, NameError, Project, ProjectId, Registry, Requests, Status, Step,
        registry::NAME_LIMIT,
    },
};
use gpui_kit::component::{
    ActiveTheme, Disableable, Theme,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FocusHandle, IntoElement, Render, SharedString,
    UniformListScrollHandle, Window, base::TestSupportExt, div, prelude::*, px,
};
use std::collections::HashMap;

/// The width of the project column.
pub const SIDEBAR_WIDTH: f32 = 200.;
/// The narrowest the list and the detail pane get.
pub const LIST_MIN_WIDTH: f32 = 300.;
pub const DETAIL_MIN_WIDTH: f32 = 360.;
/// The tallest the status of the selected project grows before it scrolls.
pub const STATUS_MAX_HEIGHT: f32 = 120.;

/// The registry as last read from disk.
#[derive(Debug)]
pub enum RegistryState {
    Loading,
    /// The registry could not be read. It is not shown as empty and nothing is written to it.
    Failed(String),
    Loaded(Registry),
}

/// What the selected project's store looks like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// No project is selected.
    None,
    /// The project's creation has not finished; `error` is why the last attempt failed.
    Incomplete {
        error: Option<String>,
    },
    Loading,
    Loaded(Summary),
    /// The store could not be read. It is not shown as empty.
    Failed(String),
}

/// What the main pane shows of a readable store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub entities: usize,
}

pub struct AxonApp {
    data: AppData,
    registry: RegistryState,
    registry_loads: Requests<()>,
    selected: Option<ProjectId>,
    store_loads: Requests,
    store: StoreState,
    name: Entity<InputState>,
    form_open: bool,
    /// A creation or a retry is running; a second one is not started meanwhile.
    busy: bool,
    form_error: Option<String>,
    /// The outcome of the last creation or retry that the screen does not otherwise show.
    notice: Option<String>,
    /// Counts the user's switches, so a result can tell whether one happened meanwhile.
    switches: u64,
    /// Why the last attempt to finish each incomplete project failed, kept across switching.
    incomplete_errors: HashMap<ProjectId, String>,
    explorer: Explorer,
    search: Entity<InputState>,
    list_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
}

impl AxonApp {
    pub fn new(data: AppData, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("例: 読書会"));
        cx.subscribe_in(&name, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit(window, cx);
            }
        })
        .detach();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("タイトル・本文を検索"));
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
        let mut this = Self {
            data,
            registry: RegistryState::Loading,
            registry_loads: Requests::default(),
            selected: None,
            store_loads: Requests::default(),
            store: StoreState::None,
            name,
            form_open: false,
            busy: false,
            form_error: None,
            notice: None,
            switches: 0,
            incomplete_errors: HashMap::new(),
            explorer: Explorer::default(),
            search,
            // A tab stop, so the arrow keys are reachable from the keyboard alone.
            list_focus: cx.focus_handle().tab_stop(true),
            list_scroll: UniformListScrollHandle::new(),
        };
        this.reload_registry(cx);
        this
    }

    pub fn data(&self) -> &AppData {
        &self.data
    }
    pub fn registry(&self) -> &RegistryState {
        &self.registry
    }
    pub fn selected(&self) -> Option<&Project> {
        let RegistryState::Loaded(registry) = &self.registry else {
            return None;
        };
        registry.get(self.selected.as_ref()?)
    }
    pub fn store(&self) -> &StoreState {
        &self.store
    }
    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }
    pub fn is_form_open(&self) -> bool {
        self.form_open
    }
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    pub fn form_error(&self) -> Option<&str> {
        self.form_error.as_deref()
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
    /// The focus of the list, where the arrow keys move the selection.
    pub fn list_focus(&self) -> &FocusHandle {
        &self.list_focus
    }

    /// Reads the registry again, keeping the selection when it still exists.
    fn reload_registry(&mut self, cx: &mut Context<Self>) {
        let ticket = self.registry_loads.begin(());
        let data = self.data.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { data.load_registry() })
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

    /// Shows a registry read from disk, or why it could not be read.
    fn apply_registry(
        &mut self,
        result: Result<Registry, String>,
        select: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(registry) => {
                let select = select.filter(|id| registry.get(id).is_some()).or_else(|| {
                    registry
                        .projects()
                        .first()
                        .map(|project| project.id.clone())
                });
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

    /// Switches to the project `id`.
    pub fn select(&mut self, id: ProjectId, cx: &mut Context<Self>) {
        if self.selected.as_ref() != Some(&id) {
            self.switches += 1;
        }
        self.selected = Some(id);
        self.show_selected(cx);
    }

    /// Reads the selected project's store again.
    pub fn reload_selected(&mut self, cx: &mut Context<Self>) {
        self.show_selected(cx);
    }

    /// Shows the selected project, reading its store when it is ready. A read still running
    /// for the previously selected project becomes stale.
    fn show_selected(&mut self, cx: &mut Context<Self>) {
        self.store_loads.cancel();
        let project = self.selected().cloned();
        let project_id = project.as_ref().map(|project| &project.id);
        if self.explorer.project() != project_id {
            self.scroll_list_to_top();
            self.explorer.deselect();
        }
        self.explorer.unload(project_id);
        self.store = match project {
            None => StoreState::None,
            Some(project) if project.status == Status::Creating => {
                // No read follows, so nothing selected could be shown.
                self.explorer.deselect();
                StoreState::Incomplete {
                    error: self.incomplete_errors.get(&project.id).cloned(),
                }
            }
            Some(project) => {
                let ticket = self.store_loads.begin(project.id.clone());
                let connection = self.data.connect(&project);
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
                                    // Nothing of the project is shown, so neither is a selection
                                    // that a later read would bring back unasked.
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
                StoreState::Loading
            }
        };
        cx.notify();
    }

    pub fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.registry, RegistryState::Loaded(_)) {
            return;
        }
        self.form_open = true;
        self.notice = None;
        self.name.update(cx, |name, cx| name.focus(window, cx));
        cx.notify();
    }

    pub fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.form_open = false;
        self.form_error = None;
        self.name
            .update(cx, |name, cx| name.set_value("", window, cx));
        cx.notify();
    }

    /// Creates a project with the typed name. Until the result and the registry on disk have
    /// been shown, another creation or retry does nothing; a refused name keeps what was typed.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let RegistryState::Loaded(registry) = &self.registry else {
            return;
        };
        if self.busy || !self.form_open {
            return;
        }
        let name = self.name.read(cx).value().to_string();
        // Checked here for an immediate answer; the creation checks again against the disk.
        if let Err(error) = registry.check_name(&name) {
            self.form_error = Some(name_message(&error));
            cx.notify();
            return;
        }
        let data = self.data.clone();
        let switches = self.switches;
        self.busy = true;
        self.form_error = None;
        self.notice = None;
        self.registry_loads.cancel();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            // A failed creation is followed by a read of the registry, which decides what the
            // failure left: the change was made to the file, not to what the window held.
            let outcome = cx
                .background_spawn(async move {
                    data.create_project(&name).map_err(|error| {
                        let reloaded = data.load_registry().map_err(|error| error.to_string());
                        (error, reloaded)
                    })
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                // A project chosen while the creation ran stays chosen.
                let switched = this.switches != switches;
                match outcome {
                    Ok(registry) => {
                        let created = registry.projects().last().map(|p| p.id.clone());
                        this.finish_form(window, cx);
                        let select = if switched {
                            this.selected.clone()
                        } else {
                            created
                        };
                        this.apply_registry(Ok(registry), select, cx);
                    }
                    Err((error, reloaded)) => {
                        this.creation_failed(error, reloaded, switched, window, cx)
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form_open = false;
        self.form_error = None;
        self.name
            .update(cx, |name, cx| name.set_value("", window, cx));
    }

    /// Shows a failed creation by what the registry read after it holds: a project the failure
    /// registered is shown (to be finished when incomplete), otherwise the form keeps the name.
    fn creation_failed(
        &mut self,
        error: CreateError,
        reloaded: Result<Registry, String>,
        switched: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = self.selected.clone();
        let (reason, project) = match &error {
            CreateError::Name(name) => {
                self.form_error = Some(name_message(name));
                self.apply_registry(reloaded, keep, cx);
                return;
            }
            CreateError::Failed { reason, .. } => (reason.clone(), error.project().cloned()),
        };
        let registered = match (&reloaded, &project) {
            (Ok(registry), Some(id)) => registry.get(id).cloned(),
            _ => None,
        };
        match registered {
            Some(project) => {
                self.finish_form(window, cx);
                self.record_outcome(&project, &reason);
                if switched && project.status == Status::Creating {
                    // The project is not on screen; say so where the form was.
                    self.notice = Some(format!(
                        "「{}」の作成が途中で止まりました。メニューから選んで「作成を再試行」で続きから作成できます。",
                        project.name
                    ));
                }
                let select = if switched {
                    keep
                } else {
                    Some(project.id.clone())
                };
                self.apply_registry(reloaded, select, cx);
            }
            None if reloaded.is_ok() => {
                self.form_error = Some(format!(
                    "プロジェクトを作成できませんでした。何も登録されていないので、もう一度作成できます。（{reason}）"
                ));
                self.apply_registry(reloaded, keep, cx);
            }
            None if project.is_none() => {
                // Nothing was registered; the name stays for when the list can be read again.
                self.form_error = Some(format!("プロジェクトを作成できませんでした。（{reason}）"));
                self.apply_registry(reloaded, keep, cx);
            }
            None => {
                // Without a readable registry nothing more can be done from the form. The reason
                // stays with the project for when the list can be read again.
                if let Some(id) = &project {
                    self.incomplete_errors.insert(
                        id.clone(),
                        format!(
                            "作成が途中で止まりました。「作成を再試行」で、既存のデータを置き換えずに続きから作成できます。（{reason}）"
                        ),
                    );
                }
                self.finish_form(window, cx);
                self.notice = Some(format!(
                    "プロジェクトを作成できませんでした。登録されたかどうかは、一覧を読み込めるようになってから確認してください。（{reason}）"
                ));
                self.apply_registry(reloaded, keep, cx);
            }
        }
    }

    /// Records why creating or finishing `project` failed, as the registry now shows it.
    fn record_outcome(&mut self, project: &Project, reason: &str) {
        match project.status {
            Status::Creating => {
                self.incomplete_errors.insert(
                    project.id.clone(),
                    format!(
                        "作成が途中で止まりました。「作成を再試行」で、既存のデータを置き換えずに続きから作成できます。（{reason}）"
                    ),
                );
            }
            Status::Ready => {
                self.incomplete_errors.remove(&project.id);
                self.notice = Some(format!(
                    "「{}」は作成しましたが、一覧への保存が確実に残ったかを確認できませんでした。OS の異常終了などで「作成未完了」に戻った場合は「作成を再試行」で完了できます。（{reason}）",
                    project.name
                ));
            }
        }
    }

    /// Finishes the creation of the selected project, keeping a store already there.
    pub fn retry_creation(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.selected().cloned() else {
            return;
        };
        if self.busy || project.status != Status::Creating {
            return;
        }
        let data = self.data.clone();
        self.busy = true;
        self.notice = None;
        self.registry_loads.cancel();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let project_name = project.name;
            let id = project.id;
            let outcome = cx
                .background_spawn({
                    let id = id.clone();
                    async move {
                        data.finish_creation(&id).map_err(|error| {
                            let reloaded = data.load_registry().map_err(|error| error.to_string());
                            (error, reloaded)
                        })
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                let keep = this.selected.clone();
                match outcome {
                    Ok(registry) => {
                        this.incomplete_errors.remove(&id);
                        this.apply_registry(Ok(registry), keep, cx);
                    }
                    Err((error, reloaded)) => {
                        let reason = match &error {
                            CreateError::Failed { step, reason, .. } => {
                                format!("{}（{reason}）", step_message(*step))
                            }
                            CreateError::Name(error) => name_message(error),
                        };
                        match reloaded.as_ref().ok().and_then(|r| r.get(&id)).cloned() {
                            Some(project) if project.status == Status::Creating => {
                                this.incomplete_errors.insert(id, reason);
                            }
                            Some(project) => this.record_outcome(&project, &reason),
                            None => {
                                this.notice = Some(format!(
                                    "「{}」の作成を再試行できませんでした。{reason}",
                                    project_name
                                ))
                            }
                        }
                        this.apply_registry(reloaded, keep, cx);
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
        let projects: Vec<Project> = match &self.registry {
            RegistryState::Loaded(registry) => registry.projects().to_vec(),
            _ => Vec::new(),
        };
        let current = self.selected.clone();
        let switch_label = match self.selected() {
            Some(project) => format!("{} ▾", project_label(project)),
            None => "プロジェクトなし".into(),
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
                    .child("プロジェクト"),
            )
            .child(
                Button::new("project-switch")
                    .outline()
                    .w_full()
                    .label(switch_label)
                    .disabled(projects.is_empty())
                    .dropdown_menu(move |menu, _, _| {
                        projects.iter().fold(menu, |menu, project| {
                            let app = app.clone();
                            let id = project.id.clone();
                            menu.item(
                                PopupMenuItem::new(project_label(project))
                                    .checked(current.as_ref() == Some(&project.id))
                                    .on_click(move |_, _, cx| {
                                        let id = id.clone();
                                        app.update(cx, |this, cx| this.select(id, cx)).ok();
                                    }),
                            )
                        })
                    }),
            );
        if self.form_open {
            sidebar = sidebar
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("新しいプロジェクトの名前"),
                )
                .child(Input::new(&self.name).id("project-name"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            Button::new("create-project")
                                .primary()
                                .label("作成")
                                .loading(self.busy)
                                .disabled(self.busy || !loaded)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("cancel-create")
                                .ghost()
                                .label("やめる")
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.close_form(window, cx)),
                                ),
                        ),
                );
            if let Some(error) = &self.form_error {
                sidebar = sidebar.child(
                    div()
                        .id("form-error")
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error.clone()),
                );
            }
        } else {
            sidebar = sidebar.child(
                Button::new("new-project")
                    .ghost()
                    .w_full()
                    .label("＋ プロジェクトを作成")
                    .disabled(!loaded)
                    .on_click(cx.listener(|this, _, window, cx| this.open_form(window, cx))),
            );
        }
        if let Some(notice) = &self.notice {
            sidebar = sidebar.child(
                div()
                    .id("notice")
                    .text_sm()
                    .text_color(theme.danger)
                    .child(notice.clone()),
            );
        }
        if loaded {
            sidebar = sidebar.child(self.render_filters(cx));
        }
        sidebar.into_any_element()
    }

    /// What the selected project's store looks like, and the action that state offers (kept
    /// apart so a long message never scrolls it out of reach).
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
                .into_any_element()
        };
        let status = div().flex().flex_col().gap_2();
        let (status, action) = match (&self.registry, self.selected(), &self.store) {
            (RegistryState::Loading, _, _) => (
                status.child(message("プロジェクトの一覧を読み込み中…".into())),
                None,
            ),
            (RegistryState::Failed(error), _, _) => (
                status
                    .child(message(
                        "プロジェクトの一覧を読み込めませんでした。空の一覧として扱わず、書き換えもしません。".into(),
                    ))
                    .child(div().text_sm().text_color(danger).child(error.clone())),
                Some(reload()),
            ),
            (RegistryState::Loaded(_), None, _) => (
                status.child(message(
                    "プロジェクトがありません。左の「＋ プロジェクトを作成」から作成してください。".into(),
                )),
                None,
            ),
            (_, Some(_), StoreState::Incomplete { error }) => {
                let mut status = status.child(message(
                    "このプロジェクトの作成は完了していません。再試行すると、既存のデータを置き換えずに作成を続けます。".into(),
                ));
                if let Some(error) = error {
                    status = status.child(div().text_sm().text_color(danger).child(error.clone()));
                }
                (
                    status,
                    Some(
                        Button::new("retry-creation")
                            .primary()
                            .label("作成を再試行")
                            .loading(self.busy)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.retry_creation(cx)))
                            .into_any_element(),
                    ),
                )
            }
            (_, Some(_), StoreState::Loading | StoreState::None) => {
                (status.child(message("読み込み中…".into())), None)
            }
            (_, Some(_), StoreState::Loaded(summary)) => (
                status.child(message(
                    if summary.entities == 0 {
                        "Issue・Group はまだありません。".into()
                    } else {
                        format!("{} 件の Issue・Group があります。", summary.entities).into()
                    },
                )),
                None,
            ),
            (_, Some(_), StoreState::Failed(error)) => (
                status
                    .child(message(
                        "このプロジェクトの保存先を読み込めませんでした。空のプロジェクトとしては扱いません。他のプロジェクトには左のメニューから切り替えられます。".into(),
                    ))
                    .child(div().text_sm().text_color(danger).child(error.clone())),
                Some(reload()),
            ),
        };
        let status = status
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(match self.selected() {
                        Some(project) => {
                            format!("保存先: {}", self.data.project_root(&project.id).display())
                        }
                        None => format!("データの保存場所: {}", self.data.dir().display()),
                    }),
            )
            .into_any_element();
        (status, action)
    }
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
            .map(|project| project.name.clone())
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
            None if self.explorer.selected().is_some()
                && matches!(self.store, StoreState::Loading) =>
            {
                div()
                    .id("detail-loading")
                    .p_4()
                    .text_color(muted)
                    .child("読み込み中…")
                    .into_any_element()
            }
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

fn project_label(project: &Project) -> String {
    match project.status {
        Status::Ready => project.name.clone(),
        Status::Creating => format!("{}（作成未完了）", project.name),
    }
}

fn name_message(error: &NameError) -> String {
    match error {
        NameError::Empty => "名前を入力してください。".into(),
        NameError::TooLong => format!("名前は {NAME_LIMIT} 文字以内にしてください。"),
        NameError::ControlCharacter => {
            "名前に改行・制御文字・見えない書式文字は使えません。".into()
        }
        NameError::Duplicate => "同じ名前のプロジェクトがすでにあります。".into(),
    }
}

fn step_message(step: Step) -> &'static str {
    match step {
        Step::Read | Step::Directory | Step::Register => {
            "プロジェクトの一覧を読み書きできませんでした。"
        }
        Step::Initialize => {
            "保存先を準備できませんでした。「作成を再試行」で続きから作成できます。"
        }
        Step::Finish => {
            "保存先は作成しましたが、完了を記録できませんでした。「作成を再試行」で完了できます。"
        }
    }
}
