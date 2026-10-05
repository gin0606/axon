//! Recording work: creating an Issue or Group from the workbench, editing the title,
//! description and label of the Entity in the detail pane, and adding Notes to it. Drafts are
//! kept per project and Entity apart from the records read, so switching projects, opening
//! another Entity or reading the store again never drops what was typed; closing the window or
//! quitting with something unsaved asks first. Every write goes through the same single write
//! as the other changes, so a repeated click or key does not make a second one.

use super::{AxonApp, StoreState, text};
use crate::board::{Change, Edit, EntityDetail, NewEntity, Section};
use crate::project::ProjectId;
use crate::{BODY_CONTEXT, label_button};
use axon::lifecycle::{EntityId, Kind, Label, Lifecycle, Nonce, record::new_entity_id};
use gpui_kit::component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    input::{Input, InputState, Textarea, TextareaState},
};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, PromptButton, PromptLevel, Window,
    base::TestSupportExt, div, prelude::*, px,
};

/// The height of the description and Note editors in the detail pane.
pub const EDITOR_HEIGHT: f32 = 160.;

/// The text of an Entity as an edit started from it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Text {
    title: String,
    description: String,
    label: Label,
}

/// An unsaved edit of one Entity's title, description and label.
pub struct EditDraft {
    pub title: Entity<InputState>,
    pub body: Entity<TextareaState>,
    pub label: Label,
    /// The values the edit started from; only the fields that differ from them are written,
    /// so a change made elsewhere to another field meanwhile stays.
    base: Text,
}

impl EditDraft {
    fn text(&self, cx: &App) -> Text {
        Text {
            title: self.title.read(cx).value().to_string(),
            description: self.body.read(cx).value().to_string(),
            label: self.label,
        }
    }
    /// The fields typed differently from where the edit started.
    fn edit(&self, cx: &App) -> Edit {
        let text = self.text(cx);
        Edit {
            title: (text.title != self.base.title).then_some(text.title),
            description: (text.description != self.base.description).then_some(text.description),
            label: (text.label != self.base.label).then_some(text.label),
        }
    }
    fn is_changed(&self, cx: &App) -> bool {
        !self.edit(cx).is_empty()
    }
}

/// What the workbench held when it was submitted, to empty it once that is saved unless more
/// was typed since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Submitted {
    title: String,
    body: String,
}

/// The drafts the window keeps.
pub(super) struct Drafts {
    /// The kind and lifecycle the next creation uses.
    pub kind: Kind,
    pub lifecycle: Lifecycle,
    /// The Group to create inside, for the project it belongs to.
    pub parent: Option<(ProjectId, EntityId)>,
    /// Empty the workbench at the next render, when it still holds this.
    pub clear: Option<Submitted>,
    /// Open this Entity once the next read of the project on screen shows it.
    pub open_on_load: Option<(ProjectId, EntityId)>,
    pub edits: std::collections::HashMap<(ProjectId, EntityId), EditDraft>,
    pub notes: std::collections::HashMap<(ProjectId, EntityId), Entity<TextareaState>>,
    /// The user chose to discard what is unsaved and close.
    pub discard_confirmed: bool,
    /// A question about discarding is on screen.
    pub asking: bool,
    /// Why no ID could be allocated for the last creation.
    pub create_error: Option<String>,
    /// A creation saved after its project was left: the workbench was emptied for it.
    pub created_elsewhere: Option<String>,
}

impl Default for Drafts {
    fn default() -> Self {
        Self {
            kind: Kind::Issue,
            lifecycle: Lifecycle::Undecided,
            parent: None,
            clear: None,
            open_on_load: None,
            edits: Default::default(),
            notes: Default::default(),
            discard_confirmed: false,
            asking: false,
            create_error: None,
            created_elsewhere: None,
        }
    }
}

impl AxonApp {
    pub fn create_kind(&self) -> Kind {
        self.drafts.kind
    }
    pub fn create_lifecycle(&self) -> Lifecycle {
        self.drafts.lifecycle
    }
    /// The Group the next creation in the project on screen goes inside.
    pub fn create_parent(&self) -> Option<&EntityId> {
        match (&self.drafts.parent, self.explorer.project()) {
            (Some((project, group)), Some(shown)) if project == shown => Some(group),
            _ => None,
        }
    }
    pub fn set_create_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.drafts.kind = kind;
        cx.notify();
    }
    pub fn set_create_lifecycle(&mut self, lifecycle: Lifecycle, cx: &mut Context<Self>) {
        self.drafts.lifecycle = lifecycle;
        cx.notify();
    }
    pub fn clear_create_parent(&mut self, cx: &mut Context<Self>) {
        self.drafts.parent = None;
        cx.notify();
    }

    /// Shows the workbench in place of the detail and puts the cursor in its title.
    pub fn new_entity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A Group chosen for an earlier creation is not carried into an unrelated one.
        self.drafts.parent = None;
        self.close_entity(cx);
        let title = self.workbench.read(cx).title().clone();
        title.update(cx, |title, cx| title.focus(window, cx));
    }

    /// Prepares the workbench to create an Entity inside `group` of the project on screen.
    pub fn create_inside(&mut self, group: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.explorer.project().cloned() else {
            return;
        };
        self.new_entity(window, cx);
        self.drafts.parent = Some((project, group));
    }

    /// The outcome of the last creation in the project on screen.
    pub fn create_outcome(&self) -> Option<&super::Outcome> {
        let project = self.explorer.project()?;
        self.outcomes_of(project)
            .iter()
            .find(|o| matches!(o.change, Change::Create { .. }))
    }

    /// Whether a publication of unknown outcome that no read has told about holds back a
    /// save: any change of `entity` in the project on screen, or, without one, a creation in
    /// any project, since the workbench is shared. Submitting again could make it twice.
    fn awaits_check(&self, entity: Option<&EntityId>) -> bool {
        match entity {
            Some(entity) => self.explorer.project().is_some_and(|project| {
                self.outcomes_of(project)
                    .iter()
                    .any(|o| o.is_pending() && o.change.entity() == entity)
            }),
            None => self
                .outcomes
                .values()
                .flatten()
                .any(|o| o.is_pending() && matches!(o.change, Change::Create { .. })),
        }
    }

    /// Creates an Entity in the project on screen from the workbench. A value the core refuses
    /// is answered at once and the workbench keeps it.
    pub fn create(&mut self, cx: &mut Context<Self>) {
        if self.is_saving() || self.awaits_check(None) {
            return;
        }
        let (Some(project), Some(board)) =
            (self.explorer.project().cloned(), self.explorer.board())
        else {
            return;
        };
        let entity = match board.fresh_id(new_entity_id) {
            Ok(entity) => entity,
            Err(error) => {
                self.drafts.create_error = Some(format!(
                    "新しい ID を決められなかったため作成できませんでした。何も記録していないので、もう一度作成できます。（{error}）"
                ));
                cx.notify();
                return;
            }
        };
        self.drafts.create_error = None;
        self.drafts.created_elsewhere = None;
        // An earlier creation's outcome is superseded by this one.
        if let Some(kept) = self.outcomes.get_mut(&project) {
            kept.retain(|o| !matches!(o.change, Change::Create { .. }));
        }
        let value = self.new_value(cx);
        self.apply(Change::Create { entity, value }, cx);
    }

    fn new_value(&self, cx: &App) -> NewEntity {
        let workbench = self.workbench.read(cx);
        NewEntity {
            kind: self.drafts.kind,
            lifecycle: self.drafts.lifecycle,
            title: workbench.title().read(cx).value().to_string(),
            description: workbench.body().read(cx).value().to_string(),
            label: workbench.label(),
            parent: self.create_parent().cloned(),
        }
    }

    /// The edit draft of the Entity in the detail pane.
    pub fn edit_draft(&self) -> Option<&EditDraft> {
        self.edits_key().and_then(|key| self.drafts.edits.get(&key))
    }
    /// The Note draft of the Entity in the detail pane.
    pub fn note_draft(&self) -> Option<&Entity<TextareaState>> {
        self.edits_key().and_then(|key| self.drafts.notes.get(&key))
    }
    fn edits_key(&self) -> Option<(ProjectId, EntityId)> {
        Some((
            self.explorer.project()?.clone(),
            self.explorer.selected()?.clone(),
        ))
    }

    /// Opens the edit form of the Entity in the detail pane, with its values, or with the
    /// draft left unsaved earlier.
    pub fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.edits_key() else {
            return;
        };
        let Some(Ok(detail)) = self.explorer.detail() else {
            return;
        };
        if !self.drafts.edits.contains_key(&key) {
            let base = Text {
                title: detail.title.clone(),
                description: detail.description.clone(),
                label: detail.label,
            };
            let title = cx.new(|cx| InputState::new(window, cx).placeholder("タイトル"));
            let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("本文"));
            title.update(cx, |input, cx| {
                input.set_value(base.title.clone(), window, cx)
            });
            body.update(cx, |input, cx| {
                input.set_value(base.description.clone(), window, cx)
            });
            self.drafts.edits.insert(
                key.clone(),
                EditDraft {
                    title,
                    body,
                    label: base.label,
                    base,
                },
            );
        }
        let title = self.drafts.edits[&key].title.clone();
        title.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Closes the edit form, discarding what it holds.
    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.edits_key() {
            self.drafts.edits.remove(&key);
            self.forget_section(&key, Section::Text);
            self.focus_list = true;
        }
        cx.notify();
    }

    pub fn set_edit_label(&mut self, label: Label, cx: &mut Context<Self>) {
        if let Some(key) = self.edits_key()
            && let Some(draft) = self.drafts.edits.get_mut(&key)
        {
            draft.label = label;
        }
        cx.notify();
    }

    /// Saves the fields of the edit form that differ from where it started, in one record. An
    /// unchanged form just closes.
    pub fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.edits_key() else {
            return;
        };
        if self.is_saving() || self.awaits_check(Some(&key.1)) {
            return;
        }
        let Some(draft) = self.drafts.edits.get(&key) else {
            return;
        };
        let edit = draft.edit(cx);
        if edit.is_empty() {
            self.cancel_edit(cx);
            return;
        }
        // A field changed in the records since the form opened would be overwritten from an
        // older value: say so once and take the records' value as the new start.
        let current = self
            .explorer
            .board()
            .and_then(|board| board.read().current(&key.1).cloned());
        if let Some(current) = current {
            let base = &draft.base;
            let mut moved = Vec::new();
            // A field the records already hold as typed overwrites nothing.
            if edit
                .title
                .as_ref()
                .is_some_and(|t| &current.title != t && current.title != base.title)
            {
                moved.push("タイトル");
            }
            if edit.description.as_ref().is_some_and(|d| {
                &current.description != d && current.description != base.description
            }) {
                moved.push("本文");
            }
            if edit
                .label
                .is_some_and(|l| current.label != l && current.label != base.label)
            {
                moved.push("label");
            }
            if !moved.is_empty() {
                let message = format!(
                    "編集を始めた後に、{}が別の場所で変更されました。詳細の記録を確かめ、もう一度「保存」すると入力した内容で上書きします。",
                    moved.join("・")
                );
                // Only the fields typed over start anew; an untouched one keeps its start, so
                // it stays out of the edit and the newer value under the lock stays.
                if let Some(draft) = self.drafts.edits.get_mut(&key) {
                    if edit.title.is_some() {
                        draft.base.title = current.title;
                    }
                    if edit.description.is_some() {
                        draft.base.description = current.description;
                    }
                    if edit.label.is_some() {
                        draft.base.label = current.label;
                    }
                }
                self.keep_outcome(super::Outcome::new(
                    key.0,
                    Change::Edit {
                        entity: key.1,
                        edit,
                    },
                    super::OutcomeKind::Warning(message),
                ));
                cx.notify();
                return;
            }
        }
        self.apply(
            Change::Edit {
                entity: key.1,
                edit,
            },
            cx,
        );
    }

    /// Opens the Note editor of the Entity in the detail pane, keeping a draft left earlier.
    pub fn start_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.edits_key() else {
            return;
        };
        let draft = self
            .drafts
            .notes
            .entry(key)
            .or_insert_with(|| {
                cx.new(|cx| TextareaState::new(window, cx).placeholder("Note の本文"))
            })
            .clone();
        draft.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Closes the Note editor, discarding what it holds.
    pub fn cancel_note(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.edits_key() {
            self.drafts.notes.remove(&key);
            self.forget_section(&key, Section::Notes);
            self.focus_list = true;
        }
        cx.notify();
    }

    /// Adds the Note draft of the Entity in the detail pane to it.
    pub fn add_note(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.edits_key() else {
            return;
        };
        if self.is_saving() || self.awaits_check(Some(&key.1)) {
            return;
        }
        let Some(draft) = self.drafts.notes.get(&key) else {
            return;
        };
        let body = draft.read(cx).value().to_string();
        self.apply(
            Change::AddNote {
                entity: key.1,
                body,
                nonce: Nonce::generate(),
            },
            cx,
        );
    }

    /// Called once a change is known to be in the records of `project`: what was typed for it
    /// is no longer a draft, unless more was typed since it was submitted.
    pub(super) fn made(&mut self, project: &ProjectId, change: &Change, cx: &App) {
        let key = (project.clone(), change.entity().clone());
        match change {
            Change::Create { entity, value } => {
                self.drafts.clear = Some(Submitted {
                    title: value.title.clone(),
                    body: value.description.clone(),
                });
                if self.explorer.project() != Some(project) {
                    self.drafts.created_elsewhere = Some(format!(
                        "「{}」を「{}」に作成しました。作成欄の入力は空にしました。",
                        value.title,
                        self.project_name(project)
                    ));
                }
                if self.explorer.project() == Some(project) && self.explorer.selected().is_none() {
                    self.drafts.open_on_load = Some((project.clone(), entity.clone()));
                }
            }
            Change::Edit { edit, .. } => {
                if let Some(draft) = self.drafts.edits.get_mut(&key) {
                    let saved = Text {
                        title: edit.title.clone().unwrap_or(draft.base.title.clone()),
                        description: edit
                            .description
                            .clone()
                            .unwrap_or(draft.base.description.clone()),
                        label: edit.label.unwrap_or(draft.base.label),
                    };
                    if draft.text(cx) == saved {
                        self.drafts.edits.remove(&key);
                        self.focus_list = true;
                    } else {
                        draft.base = saved;
                    }
                }
            }
            Change::AddNote { body, .. }
                if self
                    .drafts
                    .notes
                    .get(&key)
                    .is_some_and(|draft| draft.read(cx).value().as_ref() == body) =>
            {
                self.drafts.notes.remove(&key);
                self.focus_list = true;
            }
            _ => {}
        }
    }

    /// Empties the workbench once its creation is saved, unless more was typed since.
    pub(super) fn clear_saved_workbench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(submitted) = self.drafts.clear.take() else {
            return;
        };
        let workbench = self.workbench.clone();
        let unchanged = {
            let workbench = workbench.read(cx);
            workbench.title().read(cx).value().as_ref() == submitted.title
                && workbench.body().read(cx).value().as_ref() == submitted.body
        };
        if unchanged {
            workbench.update(cx, |workbench, cx| workbench.clear(window, cx));
        }
    }

    /// The name of `project`, or its ID while the registry does not show it.
    pub(super) fn project_name(&self, project: &ProjectId) -> String {
        match &self.registry {
            super::RegistryState::Loaded(registry) => registry
                .get(project)
                .map_or_else(|| project.to_string(), |p| p.name.clone()),
            _ => project.to_string(),
        }
    }

    /// Says on the workbench that a creation in a project not on screen was not saved, or
    /// may have been: its outcome waits in that project.
    pub(super) fn creation_left_elsewhere(
        &mut self,
        project: &ProjectId,
        change: &Change,
        kind: &super::OutcomeKind,
    ) {
        if let Change::Create { value, .. } = change {
            let name = self.project_name(project);
            let what = match kind {
                super::OutcomeKind::Unknown { .. } => format!(
                    "保存が途中で止まり、作成されたか確認できていません。「{name}」に切り替えて読み直すまで、作成はできません"
                ),
                _ => "作成できませんでした。何も記録していません".into(),
            };
            self.drafts.created_elsewhere = Some(format!(
                "「{}」の「{name}」への作成: {what}。「{name}」に切り替えると理由を確かめられます。作成欄の入力はそのまま残しています。",
                value.title
            ));
        }
    }

    /// Opens the Entity created last once the read shows it.
    pub(super) fn open_created(&mut self) {
        if self
            .drafts
            .open_on_load
            .as_ref()
            .is_some_and(|(project, _)| Some(project) == self.explorer.project())
            && let Some((_, entity)) = self.drafts.open_on_load.take()
            && self.explorer.selected().is_none()
        {
            self.explorer.select(entity);
            self.reveal_selected();
            // The workbench that had the focus is no longer drawn.
            self.focus_list = true;
        }
    }

    /// Whether anything typed is not saved: the workbench, an edit that changes something, or
    /// a Note draft, in any project.
    pub fn has_unsaved(&self, cx: &App) -> bool {
        !self.workbench.read(cx).is_blank(cx)
            || self.drafts.edits.values().any(|draft| draft.is_changed(cx))
            || self
                .drafts
                .notes
                .values()
                .any(|draft| !draft.read(cx).value().is_empty())
    }

    /// Whether the window may close now. With something unsaved it stays open and asks; the
    /// answer closes it or leaves everything as it was.
    pub(super) fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.drafts.discard_confirmed || !self.has_unsaved(cx) && !self.is_saving() {
            return true;
        }
        self.ask_to_discard(window, cx, |_, window, _| window.remove_window());
        false
    }

    /// Forgets what became of the last creation once the workbench is typed in again, unless
    /// no read has told yet what a publication of unknown outcome left.
    pub(super) fn forget_create_outcome(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.drafts.create_error.take().is_some();
        // What holds every creation back stays said until a read tells.
        if !self.awaits_check(None) {
            changed |= self.drafts.created_elsewhere.take().is_some();
        }
        if let Some(project) = self.explorer.project()
            && let Some(kept) = self.outcomes.get_mut(project)
        {
            let before = kept.len();
            kept.retain(|o| !matches!(o.change, Change::Create { .. }) || o.is_pending());
            changed |= kept.len() != before;
        }
        // Every keystroke lands here; the window is drawn again only when something went.
        if changed {
            cx.notify();
        }
    }

    /// Quits, after asking when something is unsaved.
    pub(crate) fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drafts.discard_confirmed || !self.has_unsaved(cx) && !self.is_saving() {
            cx.quit();
            return;
        }
        self.ask_to_discard(window, cx, |_, _, cx| cx.quit());
    }

    fn ask_to_discard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.drafts.asking {
            return;
        }
        self.drafts.asking = true;
        let (message, detail) = if self.has_unsaved(cx) {
            (
                "保存していない入力があります",
                "作成欄・編集・Note の下書きのうち、保存していないものは失われます。変更を保存中の場合は、記録されたかを確認できなくなります。破棄して終了しますか？",
            )
        } else {
            (
                "変更を保存中です",
                "いま終了すると、保存中の変更が記録されたかを画面で確認できなくなります。終了しますか？",
            )
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            message,
            Some(detail),
            // Return and Escape keep what was typed; discarding takes a deliberate choice.
            &[
                PromptButton::cancel("キャンセル"),
                PromptButton::new("破棄して終了"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let discard = answer.await.ok() == Some(1);
            this.update_in(cx, |this, window, cx| {
                this.drafts.asking = false;
                if discard {
                    this.drafts.discard_confirmed = true;
                    then(this, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The workbench with what a creation needs besides the text: where it goes, its kind and
    /// lifecycle, the button and what became of the last one.
    pub(super) fn render_create(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let project = self.selected().map(|project| project.name.clone());
        let ready = matches!(self.store, StoreState::Loaded(_)) && self.explorer.board().is_some();
        let pending = self.awaits_check(None);
        let busy = self.is_settling();
        let heading = match &project {
            Some(name) => format!("新しい Issue・Group（作成先: {name}）"),
            None => "新しい Issue・Group".into(),
        };
        let choice = |id: &'static str, label: &'static str, chosen: bool| {
            let button = Button::new(id).compact().label(label);
            if chosen {
                button.primary()
            } else {
                button.outline()
            }
        };
        let kind = self.drafts.kind;
        let lifecycle = self.drafts.lifecycle;
        let options = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(div().text_xs().text_color(muted).child("種類"))
            .child(
                choice("create-kind-issue", "Issue", kind == Kind::Issue)
                    .on_click(cx.listener(|this, _, _, cx| this.set_create_kind(Kind::Issue, cx))),
            )
            .child(
                choice("create-kind-group", "Group", kind == Kind::Group)
                    .on_click(cx.listener(|this, _, _, cx| this.set_create_kind(Kind::Group, cx))),
            )
            .child(div().text_xs().text_color(muted).child("状態"))
            .child(
                choice(
                    "create-undecided",
                    "未判断",
                    lifecycle == Lifecycle::Undecided,
                )
                .on_click(
                    cx.listener(|this, _, _, cx| {
                        this.set_create_lifecycle(Lifecycle::Undecided, cx)
                    }),
                ),
            )
            .child(
                choice(
                    "create-not-started",
                    "採用済み（未着手）",
                    lifecycle == Lifecycle::NotStarted,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_create_lifecycle(Lifecycle::NotStarted, cx)
                })),
            );
        let mut parent = div()
            .id("create-parent")
            .test_support()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(div().text_xs().text_color(muted).child("所属"));
        match self.create_parent() {
            Some(group) => {
                parent = parent.child(self.render_named(group, cx)).child(
                    Button::new("create-parent-clear")
                        .ghost()
                        .compact()
                        .label("外す")
                        .on_click(cx.listener(|this, _, _, cx| this.clear_create_parent(cx))),
                );
            }
            None => {
                parent = parent.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("なし（Group の詳細の「この Group の中に作成」で選べます）"),
                );
            }
        }
        let mut footer = div()
            .id("create-footer")
            .flex_none()
            .max_h(px(200.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .pb_4()
            .child(options)
            .child(parent)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("create-entity")
                            .primary()
                            .label("作成")
                            .loading(self.writes.pending().is_some_and(|ticket| {
                                let (project, change) = ticket.key();
                                matches!(change, Change::Create { .. })
                                    && Some(project) == self.explorer.project()
                            }))
                            .disabled(!ready || busy || pending)
                            .on_click(cx.listener(|this, event, _, cx| {
                                if this.accepts_click(event) {
                                    this.create(cx)
                                }
                            })),
                    )
                    .when(!ready, |row| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("プロジェクトを読み込むと作成できます。"),
                        )
                    }),
            );
        footer = footer.children(self.render_create_saving(cx));
        if let Some(error) = &self.drafts.create_error {
            footer = footer.child(
                div()
                    .id("create-error")
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone())
                    .test_support(),
            );
        }
        if let Some(notice) = &self.drafts.created_elsewhere {
            footer = footer.child(
                div()
                    .id("created-elsewhere")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(notice.clone())
                    .test_support(),
            );
        }
        if let Some(outcome) = self.create_outcome() {
            outcome.seen.set(true);
            let entity = outcome.change.entity().clone();
            // Until a read tells what it left, the outcome stays: dismissing it would let the
            // same creation be sent again.
            let dismissible = outcome.is_dismissible();
            footer = footer.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.render_outcome_kind("create-outcome", outcome, cx))
                    .when(dismissible, |column| {
                        column.child(
                            Button::new("dismiss-create-outcome")
                                .ghost()
                                .compact()
                                .label("閉じる")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.dismiss_outcome(&entity, cx)
                                })),
                        )
                    }),
            );
        }
        div()
            .id("create-pane")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(border)
                    .pb_2()
                    .child(div().text_sm().min_w_0().truncate().child(heading))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("一覧から選ぶと詳細を表示します"),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.workbench.clone()))
            .child(footer)
            .into_any_element()
    }

    /// The creation being saved, or another change holding it back.
    fn render_create_saving(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (project, change) = self.writes.pending()?.key();
        let own = matches!(change, Change::Create { .. });
        let message = if own && Some(project) == self.explorer.project() {
            "保存中…".to_owned()
        } else if own {
            "別のプロジェクトへの作成を保存中です。終わると作成できます。".to_owned()
        } else {
            "ほかの変更を保存中です。終わると作成できます。".to_owned()
        };
        Some(
            div()
                .id("create-saving")
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(message)
                .test_support()
                .into_any_element(),
        )
    }

    /// Under the title: why the text cannot be edited, when it cannot.
    pub(super) fn render_edit_blocked(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rejection = detail.editable.as_ref().err()?;
        if self.edit_draft().is_some() {
            return None;
        }
        Some(
            div()
                .id("edit-blocked")
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text::rejection(rejection, text::Doing::Edit))
                .test_support()
                .into_any_element(),
        )
    }

    /// The edit form of the Entity in the detail pane.
    pub(super) fn render_edit(
        &self,
        detail: &EntityDetail,
        draft: &EditDraft,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.is_settling() || self.awaits_check(Some(&detail.id));
        let app = cx.entity().downgrade();
        div()
            .id("edit-form")
            .test_support()
            .key_context(BODY_CONTEXT)
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_xs().text_color(muted).child("タイトル"))
            .child(Input::new(&draft.title).id("edit-title"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(label_button("edit-label", draft.label, move |label, cx| {
                        app.update(cx, |this, cx| this.set_edit_label(label, cx))
                            .ok();
                    })),
            )
            .child(div().text_xs().text_color(muted).child("本文"))
            .child(
                div()
                    .h(px(EDITOR_HEIGHT))
                    .child(Textarea::new(&draft.body).size_full()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("save-edit")
                            .primary()
                            .compact()
                            .label("保存")
                            .disabled(busy)
                            .on_click(cx.listener(|this, event, _, cx| {
                                if this.accepts_click(event) {
                                    this.save_edit(cx)
                                }
                            })),
                    )
                    .child(
                        Button::new("cancel-edit")
                            .ghost()
                            .compact()
                            .label("編集をやめる（変更を破棄）")
                            .disabled(self.is_saving_section(&detail.id, Section::Text))
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                    ),
            )
            .children(self.render_saving(detail, Section::Text, cx))
            .children(self.render_outcome(detail, Section::Text, cx))
            .into_any_element()
    }

    /// Keeps the workbench from being typed in while its creation is saved (`locked`), so what
    /// is emptied after it is exactly what was saved. A Note editor is read-only while its own
    /// save runs (see the Note form); the other forms are just drawn again.
    pub(super) fn lock_drafts(&mut self, change: &Change, locked: bool, cx: &mut Context<Self>) {
        match change {
            Change::Create { .. } => self
                .workbench
                .update(cx, |workbench, cx| workbench.set_locked(locked, cx)),
            Change::AddNote { .. } | Change::Edit { .. } => cx.notify(),
            _ => {}
        }
    }

    /// Forgets what became of the last change from `section` of the Entity `key` names, once
    /// its form is put away, unless no read has told yet what it left.
    fn forget_section(&mut self, key: &(ProjectId, EntityId), section: Section) {
        self.remove_outcome(&key.0, &key.1, |o| {
            o.change.section() == section && !o.is_pending()
        });
    }

    /// Whether the change being written is `entity`'s, made from `section`.
    fn is_saving_section(&self, entity: &EntityId, section: Section) -> bool {
        self.writes.pending().is_some_and(|ticket| {
            let (project, change) = ticket.key();
            Some(project) == self.explorer.project()
                && change.entity() == entity
                && change.section() == section
        })
    }

    /// The Note editor, or the button that opens it, with what became of the last Note.
    pub(super) fn render_note_form(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.is_settling() || self.awaits_check(Some(&detail.id));
        let form = match self.note_draft() {
            Some(draft) => div()
                .id("note-form")
                .test_support()
                .key_context(BODY_CONTEXT)
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div().h(px(EDITOR_HEIGHT)).child(
                        Textarea::new(draft)
                            .readonly(self.is_saving_section(&detail.id, Section::Notes))
                            .size_full(),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("add-note")
                                .primary()
                                .compact()
                                .label("Note を追加")
                                .disabled(busy)
                                .on_click(cx.listener(|this, event, _, cx| {
                                    if this.accepts_click(event) {
                                        this.add_note(cx)
                                    }
                                })),
                        )
                        .child(
                            Button::new("cancel-note")
                                .ghost()
                                .compact()
                                .label("やめる（下書きを破棄）")
                                .disabled(self.is_saving_section(&detail.id, Section::Notes))
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_note(cx))),
                        ),
                )
                .into_any_element(),
            None => Button::new("write-note")
                .outline()
                .compact()
                .label("Note を書く")
                .on_click(cx.listener(|this, _, window, cx| this.start_note(window, cx)))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(form)
            .children(self.render_saving(detail, Section::Notes, cx))
            .children(self.render_outcome(detail, Section::Notes, cx))
            .into_any_element()
    }

    /// For a Group, the way to create an Entity inside it, or why the core refuses one.
    pub(super) fn render_create_inside(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let check = detail.create_inside.as_ref()?;
        let group = detail.id.clone();
        let mut line = div()
            .id("create-inside-line")
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new("create-inside")
                    .outline()
                    .compact()
                    .label("この Group の中に作成")
                    .disabled(check.is_err())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.create_inside(group.clone(), window, cx)
                    })),
            );
        if let Err(rejection) = check {
            line = line.child(self.render_rejection(
                "create-inside-blocked",
                rejection,
                text::Doing::Create,
                cx,
            ));
        }
        Some(line.into_any_element())
    }
}
