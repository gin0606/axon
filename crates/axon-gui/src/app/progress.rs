//! Changing the lifecycle from the detail pane: the state menu under the title lists every
//! transition that leads somewhere from the Entity's kind and lifecycle, says why the core
//! refuses one, and makes the chosen one. A choice closes the menu; the input that made it (a
//! second click of a double click, a held key, a press that began while the menu was open)
//! makes nothing more, so the next change is chosen anew.

use super::{AxonApp, named, text};
use crate::board::{Change, EntityDetail, Exclusion, Rejection, Section, State, Step};
use crate::project::ProjectId;
use axon::lifecycle::{EntityId, Kind, Operation};
use gpui_kit::component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, IntoElement, KeyUpEvent, Keystroke, MouseDownEvent,
    base::TestSupportExt, div, prelude::*, px,
};

/// The width of the state menu; a long reason wraps. It fits beside the narrowest list.
pub const STATE_MENU_WIDTH: f32 = 320.;

/// Guards against an input that chose from the state menu acting again behind it.
#[derive(Debug, Default)]
pub struct MenuGuard {
    /// The state menu is open.
    open: bool,
    /// The mouse press in progress began while the state menu was open.
    pressed_in_menu: bool,
    /// A transition was chosen from the keyboard and the key that chose it is not released
    /// yet: its repeats are dropped.
    swallow_held: bool,
}

impl AxonApp {
    /// Whether the state menu is open.
    pub fn is_state_menu_open(&self) -> bool {
        self.guard.open
    }

    /// Whether a click on a control that changes the records may act. The second click of a
    /// double click does not, nor a press that began while the state menu was open: the menu
    /// closed under it, so it was meant for the menu, not for what is behind.
    pub(super) fn accepts_click(&self, event: &ClickEvent) -> bool {
        match event {
            ClickEvent::Mouse(_) => event.click_count() <= 1 && !self.guard.pressed_in_menu,
            _ => event.click_count() <= 1,
        }
    }

    /// Notes, before anything under it sees the press, whether it began over an open menu.
    pub(super) fn note_press(&mut self, _: &MouseDownEvent) {
        self.guard.pressed_in_menu = self.guard.open;
        self.guard.swallow_held = false;
    }

    /// Drops Enter and Space while the press that chose a transition from the keyboard is
    /// held, so its repeats neither reopen the menu nor press what has the focus. Called for
    /// every keystroke before any binding or element sees it; another key or a release ends it.
    pub(super) fn intercept_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if !self.guard.swallow_held {
            return;
        }
        let activation =
            matches!(keystroke.key.as_str(), "enter" | "space") && !keystroke.modifiers.modified();
        if activation {
            cx.stop_propagation();
        } else {
            self.guard.swallow_held = false;
        }
    }
    pub(super) fn note_key_up(&mut self, _: &KeyUpEvent) {
        self.guard.swallow_held = false;
    }

    fn state_menu_changed(&mut self, open: bool, cx: &mut Context<Self>) {
        self.guard.open = open;
        cx.notify();
    }

    /// Makes the transition chosen from the state menu of the Entity in `project`, when that
    /// detail is still the one on screen; `by_keyboard` when a key chose it.
    pub fn choose_transition(
        &mut self,
        project: &ProjectId,
        change: Change,
        by_keyboard: bool,
        cx: &mut Context<Self>,
    ) {
        self.guard.swallow_held = by_keyboard;
        if self.explorer.project() != Some(project)
            || self.explorer.selected() != Some(change.entity())
        {
            return;
        }
        self.apply(change, cx);
    }

    /// The state menu and the edit button, side by side under the title.
    pub(super) fn render_actions(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.is_settling();
        let blocked = detail.progress.is_empty();
        let entries: Vec<MenuEntry> = detail
            .progress
            .iter()
            .map(|step| self.menu_entry(detail, step))
            .collect();
        let app = cx.entity().downgrade();
        let app_for_open = app.clone();
        let project = self.explorer.project().cloned();
        let entity = detail.id.clone();
        let trigger = Button::new("change-state")
            .primary()
            .outline()
            .compact()
            .label("状態を変更 ▾")
            .disabled(busy || blocked)
            .dropdown_menu(move |menu, _, _| {
                let Some(project) = project.clone() else {
                    return menu;
                };
                entries.iter().fold(
                    menu.min_w(px(STATE_MENU_WIDTH)).max_w(px(STATE_MENU_WIDTH)),
                    |menu, entry| {
                        let menu = if entry.operation == Operation::Cancel {
                            menu.separator()
                        } else {
                            menu
                        };
                        let app = app.clone();
                        let project = project.clone();
                        let change = Change::Transition {
                            entity: entity.clone(),
                            operation: entry.operation,
                            to: entry.to,
                        };
                        let entry = entry.clone();
                        menu.item(
                            PopupMenuItem::element(move |_, cx| entry.render(cx)).on_click(
                                move |_, window, cx| {
                                    let by_keyboard = window.last_input_was_keyboard();
                                    app.update(cx, |this, cx| {
                                        this.choose_transition(
                                            &project,
                                            change.clone(),
                                            by_keyboard,
                                            cx,
                                        )
                                    })
                                    .ok();
                                },
                            ),
                        )
                    },
                )
            })
            .on_open_change(move |open, _, cx| {
                app_for_open
                    .update(cx, |this, cx| this.state_menu_changed(*open, cx))
                    .ok();
            });
        div()
            .id("detail-actions")
            .flex()
            .flex_row()
            .justify_between()
            .items_center()
            .gap_2()
            .child(trigger)
            .child(
                Button::new("edit-entity")
                    .outline()
                    .compact()
                    .label("編集")
                    .disabled(detail.editable.is_err() || self.edit_draft().is_some() || busy)
                    .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
            )
            .into_any_element()
    }

    fn menu_entry(&self, detail: &EntityDetail, step: &Step) -> MenuEntry {
        let reason = step.check.as_ref().err().map(|rejection| {
            let mut reason = text::rejection(rejection, text::Doing::Transition(step.operation));
            let named: Vec<String> = related(rejection)
                .into_iter()
                .map(|id| self.title_of(id))
                .collect();
            if !named.is_empty() {
                reason.push_str(&format!("（{}）", named.join("、")));
            }
            reason
        });
        MenuEntry {
            operation: step.operation,
            to: step.to,
            label: text::step(detail.kind, step.operation),
            reason,
        }
    }

    /// The title of `id` in the records read, or its ID when they do not hold it.
    fn title_of(&self, id: &EntityId) -> String {
        self.explorer
            .board()
            .and_then(|board| board.item(id))
            .map_or_else(|| id.to_string(), |item| item.title.clone())
    }

    /// What the lifecycle offers beyond the menu: why the menu is closed, the final
    /// confirmation a Group waits for, the change being saved and what became of the last one.
    pub(super) fn render_progress(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut section = div().id("detail-progress").flex().flex_col().gap_1();
        if detail.progress.is_empty() {
            section = section.child(
                div()
                    .id("progress-blocked")
                    .text_sm()
                    .text_color(muted)
                    .child("衝突しているため状態を変更できません。CLI の axon resolve で解決してから変更してください。")
                    .test_support(),
            );
        }
        if detail.kind == Kind::Group
            && let Some(complete) = detail
                .progress
                .iter()
                .find(|step| step.operation == Operation::Complete)
        {
            // Whether the children have ended is told apart from completing the Group itself.
            let ended = |link: &crate::board::Link| {
                link.known
                    .as_ref()
                    .is_some_and(|known| matches!(known.state, State::Completed | State::Cancelled))
            };
            let hint = if detail.children.is_empty() {
                None
            } else if detail.children.iter().all(ended) {
                complete.check.is_ok().then_some(
                    "配下の仕事はすべて終了しています。Group 全体の成果を確認してから完了にしてください。",
                )
            } else {
                Some(
                    "配下の仕事がすべて終了すると、Group 全体を最終確認して完了にできます。配下の仕事の完了は Group の完了ではありません。",
                )
            };
            if let Some(hint) = hint {
                section = section.child(
                    div()
                        .id("group-confirmation")
                        .text_sm()
                        .text_color(muted)
                        .child(hint)
                        .test_support(),
                );
            }
        }
        section
            .children(self.render_saving(detail, Section::Progress, cx))
            .children(self.render_outcome(detail, Section::Progress, cx))
            .into_any_element()
    }

    /// Why the selected Entity is not among the matches, when it is not.
    pub(super) fn render_filtered_out(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let exclusions = self.explorer.selected_exclusions();
        if exclusions.is_empty() {
            return None;
        }
        let reasons: Vec<String> = exclusions.iter().map(exclusion).collect();
        Some(
            div()
                .id("detail-filtered-out")
                .text_sm()
                .text_color(cx.theme().warning)
                .child(format!(
                    "現在の絞り込みに一致しないため、一覧には一致として表示されていません（{}）。",
                    reasons.join("。")
                ))
                .test_support()
                .into_any_element(),
        )
    }
}

/// One transition as the state menu lists it.
#[derive(Clone, Debug)]
struct MenuEntry {
    operation: Operation,
    to: axon::lifecycle::Lifecycle,
    label: &'static str,
    /// Why the core refuses it on the records read; choosing it then says so again, with
    /// links to what is involved, and writes nothing.
    reason: Option<String>,
}

impl MenuEntry {
    fn render(&self, cx: &mut gpui_kit::App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let refused = self.reason.is_some();
        div()
            .id(named(format!("transition-{:?}", self.operation)))
            // The menu's own padding and gaps take the rest of its width.
            .w(px(STATE_MENU_WIDTH - 32.))
            .flex()
            .flex_col()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .when(refused, |line| line.text_color(muted))
                    .child(self.label)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("→ {}", text::lifecycle(self.to))),
                    ),
            )
            .children(self.reason.clone().map(|reason| {
                div()
                    .id("transition-reason")
                    .text_xs()
                    .text_color(muted)
                    .child(reason)
                    .test_support()
            }))
            .test_support()
            .into_any_element()
    }
}

/// The Entities a rejection names, each once.
fn related(rejection: &Rejection) -> Vec<&EntityId> {
    let mut found: Vec<&EntityId> = Vec::new();
    if let Rejection::Refused(refusal) = rejection {
        for id in refusal.related() {
            if !found.contains(&id) {
                found.push(id);
            }
        }
    }
    found
}

fn exclusion(exclusion: &Exclusion) -> String {
    match exclusion {
        Exclusion::State(state) => format!("状態「{}」を選んでいません", text::state(*state)),
        Exclusion::Kind(kind) => format!("種類「{}」を選んでいません", text::kind(*kind)),
        Exclusion::Label(label) => format!("label「{}」を選んでいません", label.name()),
        Exclusion::Query(query) => format!("タイトル・本文に「{query}」がありません"),
    }
}
