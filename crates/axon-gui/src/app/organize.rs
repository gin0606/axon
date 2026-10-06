//! Writing changes from the detail pane and keeping what became of them, and the structural
//! changes: moving the Entity under a Group or out of one, adding and removing dependencies,
//! and converting its kind. The core decides every rule; the write runs on the background
//! executor under the store's lock, and the window reads the store again to show the result.

use super::{AxonApp, entity_element, named, text};
use crate::board::{Change, EntityDetail, Purpose, Rejection, Section, organize};
use crate::project::{ProjectId, WriteOutcome};
use axon::lifecycle::{EntityId, Refusal};
use gpui_kit::component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    input::Input,
};
use gpui_kit::{
    AnyElement, AppContext, Context, IntoElement, Window, base::TestSupportExt, div, prelude::*,
};
use std::cell::Cell;

/// The tallest the outcomes above the list grow before they scroll, so the list keeps room.
pub const OUTCOMES_MAX_HEIGHT: f32 = 120.;

/// The most candidates a picker lists; a search narrows the rest.
pub const PICKER_LIMIT: usize = 50;

/// A picker choosing a Group to move `entity` under, or a dependency to add to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub entity: EntityId,
    pub purpose: Purpose,
}

/// What became of a change, for the project it was made in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub project: ProjectId,
    pub change: Change,
    pub kind: OutcomeKind,
    /// The detail of its Entity has drawn it, so leaving that detail may forget it.
    pub seen: Cell<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeKind {
    /// The core refused the change; nothing was written.
    Rejected(Rejection),
    /// Nothing was written, for a reason other than the rules (the store could not be read,
    /// locked or written). The same change can be made again.
    NotApplied(String),
    /// Publication stopped midway, so whether the change was made is not known. The store is
    /// read again to tell.
    Unknown { error: String, found: Found },
    /// Nothing was written: the window stopped before asking, to say something first. The
    /// same change can be made again.
    Warning(String),
}

/// What a read after a publication of unknown outcome shows of the change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// No read of the project has succeeded since.
    Pending,
    Made,
    NotMade,
    /// The Entity is conflicted or gone, so the read cannot tell.
    Undetermined,
}

impl Outcome {
    pub(super) fn new(project: ProjectId, change: Change, kind: OutcomeKind) -> Self {
        Self {
            project,
            change,
            kind,
            seen: Cell::new(false),
        }
    }
    /// Whether the user may put the outcome away. A creation or a Note whose publication no read
    /// has told about stays, so the same one is not sent again by mistake.
    pub(super) fn is_dismissible(&self) -> bool {
        !(self.is_pending_creation()
            || self.is_pending() && matches!(self.change, Change::AddNote { .. }))
    }
    /// A creation whose publication of unknown outcome no read has told about yet: the same
    /// one is not sent again in its project until one does.
    pub(super) fn is_pending_creation(&self) -> bool {
        self.is_pending() && matches!(self.change, Change::Create { .. })
    }
    /// A publication of unknown outcome that no read has told about yet: kept until one does.
    pub(super) fn is_pending(&self) -> bool {
        matches!(
            self.kind,
            OutcomeKind::Unknown {
                found: Found::Pending,
                ..
            }
        )
    }
}

impl AxonApp {
    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }
    pub fn picker_query(&self) -> &gpui_kit::Entity<gpui_kit::component::input::InputState> {
        &self.picker_query
    }
    /// The outcome kept for the Entity open in the detail pane.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome_for(self.explorer.selected()?)
    }
    /// The outcome kept for `entity` of the project on screen.
    pub fn outcome_for(&self, entity: &EntityId) -> Option<&Outcome> {
        self.outcomes
            .get(self.explorer.project()?)?
            .iter()
            .find(|o| o.change.entity() == entity)
    }
    /// Every outcome kept for `project`, on screen or not, one per Entity.
    pub fn outcomes_of(&self, project: &ProjectId) -> &[Outcome] {
        self.outcomes.get(project).map_or(&[], Vec::as_slice)
    }
    /// Removes the outcome kept for `entity` of `project` that `forget` accepts.
    pub(super) fn remove_outcome(
        &mut self,
        project: &ProjectId,
        entity: &EntityId,
        forget: impl Fn(&Outcome) -> bool,
    ) {
        if let Some(kept) = self.outcomes.get_mut(project) {
            kept.retain(|o| o.change.entity() != entity || !forget(o));
        }
    }
    pub(super) fn keep_outcome(&mut self, outcome: Outcome) {
        let project = outcome.project.clone();
        self.remove_outcome(&project, outcome.change.entity(), |_| true);
        self.outcomes.entry(project).or_default().push(outcome);
    }
    /// Forgets the outcome kept for `entity` of the project on screen once it has been seen,
    /// unless no read has told yet what its publication of unknown outcome left.
    fn forget_outcome(&mut self, entity: &EntityId) {
        if let Some(project) = self.explorer.project().cloned() {
            self.remove_outcome(&project, entity, |o| o.seen.get() && !o.is_pending());
        }
    }
    /// Forgets the outcome kept for `entity`, whatever it is, once the user dismisses it.
    pub fn dismiss_outcome(&mut self, entity: &EntityId, cx: &mut Context<Self>) {
        if let Some(project) = self.explorer.project().cloned() {
            self.remove_outcome(&project, entity, |o| o.is_dismissible());
        }
        cx.notify();
    }
    /// Whether a change is being written, in any project.
    pub fn is_saving(&self) -> bool {
        self.writes.pending().is_some()
    }
    /// Whether the controls of the detail pane wait: a change is being written, or the
    /// project is read again and the detail shown is the one from before.
    pub(super) fn is_settling(&self) -> bool {
        self.is_saving() || matches!(self.store, super::StoreState::Loading)
    }

    /// Forgets the outcome of the Entity that was open (`previous`) once another one, or none,
    /// is open: it was seen there.
    pub(super) fn drop_outcome(&mut self, previous: Option<EntityId>) {
        if let Some(previous) = previous
            && Some(&previous) != self.explorer.selected()
        {
            self.forget_outcome(&previous);
        }
    }

    /// Reads the selected project again on request. The outcome shown for the open Entity is
    /// forgotten, as the new read shows what the store holds.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(entity) = self.explorer.selected().cloned() {
            self.forget_outcome(&entity);
        }
        self.reload_selected(cx);
    }

    /// Closes a picker that belongs to an Entity no longer in the detail pane.
    pub(super) fn drop_picker(&mut self) {
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| Some(&picker.entity) != self.explorer.selected())
        {
            self.picker = None;
            self.focus_list = true;
        }
    }

    /// Opens a picker for the Entity in the detail pane, with an empty search.
    pub fn open_picker(&mut self, purpose: Purpose, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entity) = self.explorer.selected().cloned() else {
            return;
        };
        self.picker = Some(Picker { entity, purpose });
        // The search takes the focus; a return to the list asked for earlier is moot.
        self.focus_list = false;
        self.picker_query.update(cx, |query, cx| {
            query.set_value("", window, cx);
            query.focus(window, cx);
        });
        cx.notify();
    }

    pub fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        self.focus_list = true;
        cx.notify();
    }

    /// Chooses `target` in the open picker.
    pub fn choose(&mut self, target: EntityId, cx: &mut Context<Self>) {
        if let Some(picker) = &self.picker {
            let change = picker.purpose.change(picker.entity.clone(), target);
            self.apply(change, cx);
        }
    }

    /// Makes `change` in the selected project. A change the core refuses on the records on
    /// screen is answered at once; otherwise it is written under the store's lock, where the
    /// core checks it again against the latest records. While one change is written, in any
    /// project, another does nothing.
    pub fn apply(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.is_saving() {
            return;
        }
        let Some(project) = self.selected().cloned() else {
            return;
        };
        let Some(board) = self.explorer.board() else {
            return;
        };
        if self.explorer.project() != Some(&project.id) {
            return;
        }
        if let Err(rejection) = change.check(board) {
            self.keep_outcome(Outcome::new(
                project.id,
                change,
                OutcomeKind::Rejected(rejection),
            ));
            cx.notify();
            return;
        }
        let ticket = self.writes.begin((project.id.clone(), change.clone()));
        self.lock_drafts(&change, true, cx);
        let connection = self.data.connect(&project);
        cx.spawn(async move |this, cx| {
            let (written, refused) = cx
                .background_spawn({
                    let change = change.clone();
                    async move {
                        // What the core refused, told apart from failing to read or write.
                        let mut refused = None;
                        let written = connection.update(|_, records, view| {
                            match change.entry(records, view, axon::context_now()) {
                                Ok(entry) => Ok((entry.into_iter().collect(), ())),
                                Err(error) => {
                                    let message = error.to_string();
                                    refused = Some(Rejection::from(error));
                                    Err(axon::Error::Invalid(message))
                                }
                            }
                        });
                        (written, refused)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.writes.finish(&ticket) {
                    return;
                }
                this.lock_drafts(&ticket.key().1, false, cx);
                cx.notify();
                let (project, change) = ticket.key().clone();
                // After a switch the result is kept for the project it was made in and shown
                // when that project is on screen again; nothing is read for it meanwhile.
                let on_screen = this.explorer.project() == Some(&project);
                let kind = match written {
                    // The records show a made change when the project is read again.
                    WriteOutcome::Applied(()) if !on_screen => {
                        this.remove_outcome(&project, change.entity(), |_| true);
                        this.made(&project, &change, cx);
                        return;
                    }
                    WriteOutcome::Applied(()) => {
                        this.remove_outcome(&project, change.entity(), |_| true);
                        this.made(&project, &change, cx);
                        if this
                            .picker
                            .as_ref()
                            .is_some_and(|p| &p.entity == change.entity())
                        {
                            this.picker = None;
                            this.focus_list = true;
                        }
                        this.reveal_on_load = true;
                        this.reload_held(cx);
                        return;
                    }
                    // The records under the lock refused what the screen allowed, so they
                    // differ from it: read them again to show why.
                    WriteOutcome::NotApplied(_) if let Some(rejection) = refused => {
                        OutcomeKind::Rejected(rejection)
                    }
                    WriteOutcome::NotApplied(error) => OutcomeKind::NotApplied(error.to_string()),
                    WriteOutcome::PublicationUnknown(error) => OutcomeKind::Unknown {
                        error: error.to_string(),
                        found: Found::Pending,
                    },
                };
                if !on_screen {
                    this.creation_left_elsewhere(&project, &change, &kind);
                }
                let reload = on_screen && !matches!(kind, OutcomeKind::NotApplied(_));
                this.keep_outcome(Outcome::new(project, change, kind));
                if reload {
                    this.reload_held(cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Reads the selected project again after a write, keeping the detail on screen until the
    /// new read replaces it, so what has the focus in it keeps it.
    fn reload_held(&mut self, cx: &mut Context<Self>) {
        self.held = self
            .explorer
            .detail()
            .and_then(|detail| detail.as_ref().ok())
            .cloned();
        self.reload_selected(cx);
    }

    /// Called when a read of the selected project is shown: tells what each publication of
    /// unknown outcome in it left.
    pub(super) fn loaded(&mut self, cx: &gpui_kit::App) {
        if std::mem::take(&mut self.reveal_on_load) {
            self.reveal_selected();
        }
        let (Some(board), Some(project)) = (self.explorer.board(), self.explorer.project()) else {
            return;
        };
        let project = project.clone();
        let mut made = Vec::new();
        for outcome in self.outcomes.get_mut(&project).into_iter().flatten() {
            if let OutcomeKind::Unknown { found, .. } = &mut outcome.kind
                && *found == Found::Pending
            {
                *found = match outcome.change.is_shown_by(board) {
                    Some(true) => Found::Made,
                    Some(false) => Found::NotMade,
                    None => Found::Undetermined,
                };
                if *found == Found::Made {
                    made.push(outcome.change.clone());
                }
                // A change found made closes its picker, as a saved one does.
                if *found == Found::Made
                    && self
                        .picker
                        .as_ref()
                        .is_some_and(|p| &p.entity == outcome.change.entity())
                {
                    self.picker = None;
                    self.focus_list = true;
                }
            }
        }
        for change in made {
            self.made(&project, &change, cx);
        }
        // A creation found made opens its Entity as a saved one does.
        self.open_created();
        self.drop_picker();
    }

    /// The outcome of the last change for the Entity in `detail`, in the `section` it was
    /// made from.
    pub(super) fn render_outcome(
        &self,
        detail: &EntityDetail,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let outcome = self
            .outcome_for(&detail.id)
            .filter(|o| o.change.section() == section)?;
        outcome.seen.set(true);
        let id = match section {
            Section::Progress => "progress-outcome",
            Section::Structure | Section::Dependencies => "structure-outcome",
            Section::Create => "create-outcome",
            Section::Text => "edit-outcome",
            Section::Notes => "note-outcome",
        };
        Some(self.render_outcome_kind(id, outcome, cx))
    }

    /// The outcomes of the project on screen that the detail pane does not show, whether no
    /// detail or another Entity's is open or the Entity is not in the records: each under a
    /// link to its Entity, until it is opened or dismissed.
    pub(super) fn render_other_outcomes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let project = self.explorer.project()?;
        // The Entity whose detail shows its outcome, if the detail is shown.
        let open = self
            .explorer
            .selected()
            .filter(|_| matches!(self.explorer.detail(), Some(Ok(_))));
        // The workbench shows what became of a creation while it is on screen.
        let workbench = self.explorer.selected().is_none();
        let others: Vec<&Outcome> = self
            .outcomes_of(project)
            .iter()
            .filter(|o| Some(o.change.entity()) != open)
            .filter(|o| !(workbench && matches!(o.change, Change::Create { .. })))
            .collect();
        if others.is_empty() {
            return None;
        }
        let border = cx.theme().danger;
        let mut list = div()
            .id("other-outcomes")
            .flex_none()
            .max_h(gpui_kit::px(OUTCOMES_MAX_HEIGHT))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border);
        for outcome in others {
            let entity = outcome.change.entity().clone();
            // A creation names an Entity only once it is in the records.
            let created = self
                .explorer
                .board()
                .is_some_and(|board| board.item(&entity).is_some());
            let link = match outcome.change {
                Change::Create { .. } if !created => {
                    div().child("新しい Issue・Group").into_any_element()
                }
                _ => self.render_named(&entity, cx),
            };
            let dismissible = outcome.is_dismissible();
            list = list.child(
                div()
                    .id(named(format!("other-outcome-{entity}")))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_sm()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .items_center()
                            .child(match outcome.change {
                                Change::Create { .. } => "作成の結果:",
                                _ => "変更の結果:",
                            })
                            .child(link)
                            .when(dismissible, |line| {
                                line.child(
                                    Button::new(named(format!("dismiss-outcome-{entity}")))
                                        .ghost()
                                        .compact()
                                        .label("閉じる")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.dismiss_outcome(&entity, cx)
                                        })),
                                )
                            }),
                    )
                    .child(self.render_outcome_kind("outcome-message", outcome, cx))
                    .test_support(),
            );
        }
        Some(list.test_support().into_any_element())
    }

    pub(super) fn render_outcome_kind(
        &self,
        id: &'static str,
        outcome: &Outcome,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let danger = cx.theme().danger;
        let message = |text: String| {
            div()
                .id(id)
                .text_sm()
                .text_color(danger)
                .child(text)
                .test_support()
                .into_any_element()
        };
        match &outcome.kind {
            OutcomeKind::Rejected(rejection) => {
                self.render_rejection(id, rejection, text::Doing::of(&outcome.change), cx)
            }
            OutcomeKind::Warning(text) => message(text.clone()),
            OutcomeKind::NotApplied(error) => message(format!(
                "保存できませんでした。変更は記録されていないので、もう一度操作できます。（{error}）"
            )),
            OutcomeKind::Unknown { error, found } => message(match found {
                Found::Pending => format!(
                    "保存が途中で止まり、変更が記録されたか確認できません。「再読み込み」で記録を読み直して確かめてください。自動ではやり直しません。（{error}）"
                ),
                Found::Made => format!(
                    "保存が途中で止まりましたが、読み直した記録には変更が反映されています。（{error}）"
                ),
                Found::NotMade => format!(
                    "保存が途中で止まり、読み直した記録には変更が反映されていません。必要ならもう一度操作してください。（{error}）"
                ),
                Found::Undetermined => format!(
                    "保存が途中で止まりました。読み直した記録ではこの仕事が衝突しているか見つからないため、変更が反映されたか判断できません。（{error}）"
                ),
            }),
        }
    }

    /// The rejection the outcome of the last change for `detail` shows, so the same reason is
    /// not repeated beside a button.
    fn shown_rejection(&self, detail: &EntityDetail) -> Option<&Rejection> {
        match self.outcome_for(&detail.id) {
            Some(Outcome {
                kind: OutcomeKind::Rejected(rejection),
                ..
            }) => Some(rejection),
            _ => None,
        }
    }

    /// Whether every structural change of the Entity is refused whatever is chosen: it or
    /// another Entity is conflicted.
    fn all_blocked(detail: &EntityDetail) -> bool {
        detail.structure.conflicted
    }

    /// What the write in progress means for the Entity in `detail`, in `section`: its own
    /// change being saved there, or another one holding every change back, said beside the
    /// state menu.
    pub(super) fn render_saving(
        &self,
        detail: &EntityDetail,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (project, change) = self.writes.pending()?.key();
        let own = Some(project) == self.explorer.project() && change.entity() == &detail.id;
        let shown_in = if own {
            change.section()
        } else {
            Section::Progress
        };
        if shown_in != section {
            return None;
        }
        Some(
            div()
                .id(match section {
                    Section::Progress => "progress-saving",
                    Section::Structure | Section::Dependencies => "structure-saving",
                    Section::Create => "create-saving",
                    Section::Text => "edit-saving",
                    Section::Notes => "note-saving",
                })
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if own {
                    "保存中…"
                } else if Some(project) == self.explorer.project() {
                    "ほかの仕事の変更を保存中です。終わると操作できます。"
                } else {
                    "別のプロジェクトの変更を保存中です。終わると操作できます。"
                })
                .test_support()
                .into_any_element(),
        )
    }

    /// Why the core refused a change, worded for what it was `doing`, with a link to each
    /// Entity it names.
    pub(super) fn render_rejection(
        &self,
        id: &'static str,
        rejection: &Rejection,
        doing: text::Doing,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let danger = theme.danger;
        let mut element = div().id(id).flex().flex_col().gap_1().text_sm().child(
            div()
                .text_color(danger)
                .child(text::rejection(rejection, doing)),
        );
        match rejection {
            Rejection::Refused(Refusal::NewViolations(violations)) => {
                // One line per Entity, so each is linked once.
                let mut entities: Vec<&EntityId> = Vec::new();
                for (entity, _) in violations {
                    if !entities.contains(&entity) {
                        entities.push(entity);
                    }
                }
                for entity in entities {
                    let kinds: Vec<_> = violations
                        .iter()
                        .filter(|(e, _)| e == entity)
                        .map(|(_, kind)| text::violation(*kind))
                        .collect();
                    element = element.child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1()
                            .child(format!("{}:", kinds.join("・")))
                            .child(self.render_named(entity, cx)),
                    );
                }
            }
            Rejection::Refused(refusal) => {
                for entity in refusal.related() {
                    element = element.child(self.render_named(entity, cx));
                }
            }
            Rejection::Other(_) => {}
        }
        element.test_support().into_any_element()
    }

    /// A link to the Entity `id` in the records read, or its ID alone while none are: without
    /// a read the records cannot tell whether it is there.
    pub(super) fn render_named(&self, id: &EntityId, cx: &mut Context<Self>) -> AnyElement {
        match self.explorer.board() {
            Some(board) => self.render_link(&board.link(id), cx),
            None => div().child(id.to_string()).into_any_element(),
        }
    }

    /// Where the Entity belongs and what kind it is, with the changes of both, the open picker
    /// and the outcome of the last change.
    pub(super) fn render_structure(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.is_settling();
        let structure = &detail.structure;
        let entity = detail.id.clone();

        let mut parent_line = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(div().text_xs().text_color(muted).child("所属"))
            .child(match &detail.parent {
                Some(parent) => self.render_link(parent, cx),
                None => div().text_sm().child("なし").into_any_element(),
            })
            .child(
                Button::new("move-entity")
                    .outline()
                    .compact()
                    .label("所属を変更")
                    .disabled(busy || Self::all_blocked(detail))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_picker(Purpose::Parent, window, cx)
                    })),
            );
        if let Some(detach) = &structure.detach {
            let change = Change::Move {
                entity: entity.clone(),
                parent: None,
            };
            parent_line = parent_line.child(
                Button::new("detach-entity")
                    .ghost()
                    .compact()
                    .label("所属から外す")
                    .disabled(busy || detach.is_err())
                    .on_click(cx.listener(move |this, event, _, cx| {
                        if this.accepts_click(event) {
                            this.apply(change.clone(), cx)
                        }
                    })),
            );
        }
        let (other, convertible) = &structure.convert;
        let convert = Change::Convert {
            entity: entity.clone(),
            kind: *other,
        };
        let kind_line = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(div().text_xs().text_color(muted).child("種類"))
            .child(div().text_sm().child(text::kind(detail.kind)))
            .child(
                Button::new("convert-entity")
                    .outline()
                    .compact()
                    .label(format!("{} に変換", text::kind(*other)))
                    .disabled(busy || convertible.is_err())
                    .on_click(cx.listener(move |this, event, _, cx| {
                        if this.accepts_click(event) {
                            this.apply(convert.clone(), cx)
                        }
                    })),
            );

        let mut section = div()
            .id("detail-structure")
            .flex()
            .flex_col()
            .gap_2()
            .child(parent_line);
        let shown = self.shown_rejection(detail);
        let detach_blocked = match &structure.detach {
            Some(Err(rejection)) => Some(rejection),
            _ => None,
        };
        if let Some(rejection) = detach_blocked.filter(|r| Some(*r) != shown) {
            section = section.child(self.render_rejection(
                "detach-blocked",
                rejection,
                text::Doing::Structure,
                cx,
            ));
        }
        section = section.child(kind_line);
        if let Err(rejection) = convertible
            && Some(rejection) != shown
            && Some(rejection) != detach_blocked
        {
            section = section.child(self.render_rejection(
                "convert-blocked",
                rejection,
                text::Doing::Structure,
                cx,
            ));
        }
        if let Some(picker) = self
            .picker
            .as_ref()
            .filter(|p| p.entity == detail.id && p.purpose == Purpose::Parent)
        {
            section = section.child(self.render_picker(picker, cx));
        }
        section = section
            .children(self.render_saving(detail, Section::Structure, cx))
            .children(self.render_outcome(detail, Section::Structure, cx));
        section.into_any_element()
    }

    fn render_picker(&self, picker: &Picker, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let busy = self.is_settling();
        let heading = match picker.purpose {
            Purpose::Parent => "移動先の Group を選ぶ（中の仕事も一緒に移動します）",
            Purpose::Dependency => "追加する依存先を選ぶ（選んだ仕事の完了を待ちます）",
        };
        let query = self.picker_query.read(cx).value().to_string();
        let (candidates, total) = match self.explorer.board() {
            Some(board) => {
                organize::candidates(board, &picker.entity, picker.purpose, &query, PICKER_LIMIT)
            }
            None => (Vec::new(), 0),
        };
        let mut list = div()
            .id("picker-candidates")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(gpui_kit::px(240.))
            .overflow_y_scroll();
        if candidates.is_empty() {
            list = list.child(div().text_sm().text_color(muted).child(
                match (picker.purpose, query.is_empty()) {
                    (_, false) => "検索に一致する候補はありません。",
                    (Purpose::Parent, true) => "選べる Group がありません。",
                    (Purpose::Dependency, true) => "選べる Issue・Group がありません。",
                },
            ));
        }
        for link in &candidates {
            let Some(known) = &link.known else {
                continue;
            };
            let target = link.id.clone();
            list = list.child(
                div()
                    .id(entity_element(&link.id))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .when(!busy, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(theme.list_hover))
                            .on_click(cx.listener(move |this, event, _, cx| {
                                if this.accepts_click(event) {
                                    this.choose(target.clone(), cx)
                                }
                            }))
                    })
                    .child(format!(
                        "{} {}（{} · {} · {}）",
                        text::state_icon(known.state),
                        known.title,
                        text::kind(known.kind),
                        text::state(known.state),
                        link.id
                    ))
                    .test_support(),
            );
        }
        if total > candidates.len() {
            list = list.child(div().text_xs().text_color(muted).child(format!(
                "ほか {} 件。検索で絞り込んでください。",
                total - candidates.len()
            )));
        }
        div()
            .id("picker")
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(heading))
                    .child(
                        Button::new("picker-cancel")
                            .ghost()
                            .compact()
                            .label("やめる")
                            .on_click(cx.listener(|this, _, _, cx| this.close_picker(cx))),
                    ),
            )
            .child(Input::new(&self.picker_query).id("picker-query"))
            .child(list)
            .test_support()
            .into_any_element()
    }

    /// The dependencies with a removal for each, and the picker to add one.
    pub(super) fn render_dependencies(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.is_settling();
        let mut section = div()
            .id("detail-dependencies")
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(muted).child("依存先"))
                    .child(
                        Button::new("add-dependency")
                            .outline()
                            .compact()
                            .label("依存先を追加")
                            .disabled(busy || Self::all_blocked(detail))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_picker(Purpose::Dependency, window, cx)
                            })),
                    ),
            );
        if detail.dependencies.is_empty() {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("依存先はありません。"),
            );
        }
        let mut blocked = None;
        for (dependency, removable) in detail.dependencies.iter().zip(&detail.structure.removals) {
            let change = Change::RemoveDependency {
                entity: detail.id.clone(),
                target: dependency.id.clone(),
            };
            if let Err(rejection) = removable {
                blocked.get_or_insert(rejection);
            }
            section = section.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.render_link(dependency, cx)),
                    )
                    .child(
                        Button::new(named(format!("remove-dependency-{}", dependency.id)))
                            .ghost()
                            .compact()
                            .label("解除")
                            .disabled(busy || removable.is_err())
                            .on_click(cx.listener(move |this, event, _, cx| {
                                if this.accepts_click(event) {
                                    this.apply(change.clone(), cx)
                                }
                            })),
                    ),
            );
        }
        // A reason the structure section or the last outcome already gives is not repeated.
        let structure = &detail.structure;
        let elsewhere = [
            structure.convert.1.as_ref().err(),
            structure.detach.as_ref().and_then(|d| d.as_ref().err()),
            self.shown_rejection(detail),
        ];
        if let Some(rejection) = blocked.filter(|r| !elsewhere.contains(&Some(*r))) {
            section = section.child(self.render_rejection(
                "removal-blocked",
                rejection,
                text::Doing::Structure,
                cx,
            ));
        }
        if let Some(picker) = self
            .picker
            .as_ref()
            .filter(|p| p.entity == detail.id && p.purpose == Purpose::Dependency)
        {
            section = section.child(self.render_picker(picker, cx));
        }
        section
            .children(self.render_saving(detail, Section::Dependencies, cx))
            .children(self.render_outcome(detail, Section::Dependencies, cx))
            .into_any_element()
    }
}
