//! The filters, the shared list and the detail pane of the selected project.

use super::{AxonApp, StoreState, text};
use crate::board::{EntityDetail, Filter, Layout, Link, Section, State, WaitKind};
use crate::{SelectNextEntity, SelectPreviousEntity};
use axon::lifecycle::{EntityId, Kind, Label};
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::Input,
};
use gpui_kit::{
    AnyElement, Context, ElementId, Hsla, IntoElement, SharedString, Window, base::TestSupportExt,
    div, prelude::*, px,
};

/// The deepest level that still indents further in the list.
const MAX_INDENT: usize = 8;

/// Key context of the list, where the arrow keys move the selection.
pub const LIST_CONTEXT: &str = "AxonList";

/// The element ID of the list row of `id`, and of a link to it inside one detail section.
pub fn entity_element(id: &EntityId) -> ElementId {
    ElementId::Name(format!("entity-{id}").into())
}

pub(super) fn named(name: String) -> ElementId {
    ElementId::Name(name.into())
}

impl AxonApp {
    pub(super) fn update_filter(
        &mut self,
        change: impl FnOnce(&mut Filter),
        cx: &mut Context<Self>,
    ) {
        self.explorer.update_filter(change);
        self.list_scroll.set_offset(Default::default());
        cx.notify();
    }

    pub fn set_layout(&mut self, layout: Layout, cx: &mut Context<Self>) {
        self.explorer.set_layout(layout);
        self.list_scroll.set_offset(Default::default());
        cx.notify();
    }

    /// Opens a known Entity in the detail pane.
    pub fn open_entity(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let previous = self.explorer.selected().cloned();
        self.explorer.select(id);
        self.drop_picker();
        self.drop_outcome(previous);
        self.reveal_selected();
        cx.notify();
    }

    /// Scrolls the list to the selected row, when the list shows it.
    pub(super) fn reveal_selected(&self) {
        let listing = self.explorer.listing();
        if let Some(ix) = self
            .explorer
            .selected()
            .and_then(|id| listing.rows.iter().position(|row| &row.id == id))
        {
            self.list_scroll.scroll_to_item(ix);
        }
    }

    /// Moves the selection through the list and keeps the selected row in view.
    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        let previous = self.explorer.selected().cloned();
        self.explorer.step(step);
        self.drop_picker();
        self.drop_outcome(previous);
        self.reveal_selected();
        cx.notify();
    }

    /// Closes the detail pane, showing the draft workbench there again.
    pub fn close_entity(&mut self, cx: &mut Context<Self>) {
        let previous = self.explorer.selected().cloned();
        self.explorer.deselect();
        self.drop_outcome(previous);
        if self.picker.take().is_some() {
            self.focus_list = true;
        }
        cx.notify();
    }

    pub fn reset_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.update_filter(|filter| *filter = Filter::default(), cx);
    }

    pub(super) fn render_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let filter = self.explorer.filter();
        let heading = |text: &'static str| div().pt_2().text_xs().text_color(muted).child(text);
        let mut filters = div()
            .id("filters")
            .flex()
            .flex_col()
            .gap_1()
            .child(heading("状態"));
        for state in self.explorer.offered_states() {
            filters = filters.child(
                Checkbox::new(named(format!("state-{state:?}")))
                    .label(text::state(state))
                    .checked(filter.states.contains(&state))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.update_filter(|f| f.toggle_state(state, *checked), cx)
                    })),
            );
        }
        filters = filters.child(heading("種類"));
        for kind in [Kind::Issue, Kind::Group] {
            filters = filters.child(
                Checkbox::new(named(format!("kind-{kind:?}")))
                    .label(text::kind(kind))
                    .checked(filter.kinds.contains(&kind))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.update_filter(|f| f.toggle_kind(kind, *checked), cx)
                    })),
            );
        }
        filters = filters.child(heading("label"));
        for label in Label::ALL {
            filters = filters.child(
                Checkbox::new(named(format!("label-{}", label.name())))
                    .label(label.name())
                    .checked(filter.labels.contains(&label))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.update_filter(|f| f.toggle_label(label, *checked), cx)
                    })),
            );
        }
        filters
            .child(
                div().pt_2().child(
                    Button::new("reset-filter")
                        .ghost()
                        .compact()
                        .label("絞り込みを初期値に戻す")
                        .on_click(cx.listener(|this, _, window, cx| this.reset_filter(window, cx))),
                ),
            )
            .into_any_element()
    }

    /// The search, the layout switch and the rows, once the selected project has been read.
    pub(super) fn render_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let board = self.explorer.board()?;
        if !matches!(self.store, StoreState::Loaded(_)) {
            return None;
        }
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let listing = self.explorer.listing();
        let layout = self.explorer.layout();
        let selected = self.explorer.selected().cloned();
        let layout_button = |id: &'static str, label: &'static str, value: Layout| {
            let button = Button::new(id)
                .compact()
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_layout(value, cx)));
            if layout == value {
                button.primary()
            } else {
                button.outline()
            }
        };
        let count = if listing.context > 0 {
            format!(
                "{} 件が一致 / 参考表示の親 {} 件",
                listing.matched, listing.context
            )
        } else {
            format!("{} 件が一致", listing.matched)
        };
        let header = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(Input::new(&self.search).id("search").flex_1())
                    .child(
                        Button::new("new-entity")
                            .outline()
                            .compact()
                            .label("＋ 作成")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.new_entity(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("reload-list")
                            .outline()
                            .compact()
                            .label("再読み込み")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(layout_button("layout-tree", "階層", Layout::Tree))
                    .child(layout_button("layout-flat", "フラット", Layout::Flat))
                    .child(
                        div()
                            .id("list-count")
                            .text_xs()
                            .text_color(muted)
                            .child(count),
                    ),
            );
        let body: AnyElement = if board.is_empty() {
            div()
                .id("list-empty")
                .text_sm()
                .text_color(muted)
                .child("このプロジェクトにはまだ Issue・Group がありません。")
                .into_any_element()
        } else if listing.rows.is_empty() {
            div()
                .id("list-empty")
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .text_color(muted)
                .child("絞り込みに一致する Issue・Group はありません。")
                .child(
                    Button::new("reset-filter-empty")
                        .outline()
                        .compact()
                        .label("絞り込みを初期値に戻す")
                        .on_click(cx.listener(|this, _, window, cx| this.reset_filter(window, cx))),
                )
                .into_any_element()
        } else {
            let mut rows = div()
                .id("entity-list")
                .track_focus(&self.list_focus)
                .key_context(LIST_CONTEXT)
                .on_action(cx.listener(|this, _: &SelectNextEntity, _, cx| this.step(1, cx)))
                .on_action(cx.listener(|this, _: &SelectPreviousEntity, _, cx| this.step(-1, cx)))
                .track_scroll(&self.list_scroll)
                .rounded_md()
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .focus(|style| style.border_color(theme.ring))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_px();
            for row in &listing.rows {
                let item = board
                    .item(&row.id)
                    .expect("a listed Entity is on the board");
                let is_selected = selected.as_ref() == Some(&row.id);
                let mut meta = vec![
                    text::kind(item.kind).to_string(),
                    text::state(item.state).to_string(),
                ];
                meta.extend(text::situation(item.status).map(str::to_string));
                meta.push(item.label.name().to_string());
                if item.invalid {
                    meta.push("構造の違反".into());
                }
                if !row.matched {
                    meta.push("参考表示（絞り込みに一致しない親）".into());
                }
                let id = row.id.clone();
                let color: Hsla = if row.matched { theme.foreground } else { muted };
                let mut line = div()
                    .id(entity_element(&row.id))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .py_1()
                    .pr_2()
                    // Deep trees stop indenting so the title keeps its room.
                    .pl(px(8. + 16. * row.depth.min(MAX_INDENT) as f32))
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(color)
                    .hover(|style| style.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.list_focus, cx);
                        this.open_entity(id.clone(), cx)
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.))
                            .child(text::state_icon(item.state)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().child(item.title.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(meta.join(" · ")),
                            ),
                    );
                if is_selected {
                    line = line.bg(theme.list_active);
                }
                rows = rows.child(line.test_support());
            }
            rows.into_any_element()
        };
        Some(
            div()
                .id("list-pane")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(header)
                .child(body)
                .into_any_element(),
        )
    }

    /// A clickable reference to another Entity, or its ID when the store does not hold it.
    pub(super) fn render_link(&self, link: &Link, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        match &link.known {
            Some(known) => {
                let id = link.id.clone();
                div()
                    .id(entity_element(&link.id))
                    .cursor_pointer()
                    .text_color(theme.link)
                    .hover(|style| style.text_color(theme.link_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_entity(id.clone(), cx)))
                    .child(format!(
                        "{} {}（{} · {}）",
                        text::state_icon(known.state),
                        known.title,
                        text::kind(known.kind),
                        text::state(known.state)
                    ))
                    .test_support()
                    .into_any_element()
            }
            None => div()
                .text_color(theme.muted_foreground)
                .child(format!("{}（記録にない ID）", link.id))
                .into_any_element(),
        }
    }

    fn render_links(
        &self,
        id: &'static str,
        heading: &'static str,
        links: &[Link],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if links.is_empty() {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        let mut section = div()
            .id(id)
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(muted).child(heading));
        for link in links {
            section = section.child(self.render_link(link, cx));
        }
        Some(section.into_any_element())
    }

    pub(super) fn render_detail(
        &self,
        detail: &EntityDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger, warning, border) = (
            theme.muted_foreground,
            theme.danger,
            theme.warning,
            theme.border,
        );
        let section_heading = |text: SharedString| div().text_xs().text_color(muted).child(text);
        // Scoped to the Entity, so another one opens scrolled to its top.
        let mut pane = div()
            .id(named(format!("detail-{}", detail.id)))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4();

        // Where it belongs, then what it is.
        let mut belongs = div()
            .id("detail-ancestors")
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .text_xs()
            .text_color(muted);
        if detail.ancestors.is_empty() {
            belongs = belongs.child("所属なし");
        } else {
            for (ix, ancestor) in detail.ancestors.iter().enumerate() {
                if ix > 0 {
                    belongs = belongs.child("›");
                }
                belongs = belongs.child(self.render_link(ancestor, cx));
            }
        }
        pane = pane
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{} · {} · {}",
                        text::kind(detail.kind),
                        detail.label.name(),
                        detail.id
                    )))
                    .child(
                        Button::new("close-detail")
                            .ghost()
                            .compact()
                            .label("閉じる")
                            .on_click(cx.listener(|this, _, _, cx| this.close_entity(cx))),
                    ),
            )
            .child(
                div()
                    .id("detail-title")
                    .text_xl()
                    .child(detail.title.clone())
                    .test_support(),
            )
            .child(self.render_actions(detail, cx))
            .children(self.render_edit_blocked(detail, cx));
        let editing = self.edit_draft();
        match editing {
            Some(draft) => pane = pane.child(self.render_edit(detail, draft, cx)),
            None => pane = pane.children(self.render_outcome(detail, Section::Text, cx)),
        }

        let mut state = format!(
            "{} {}",
            text::state_icon(detail.state),
            text::state(detail.state)
        );
        if let Some(stored) = detail.stored
            && State::of(Some(stored)) != detail.state
        {
            state.push_str(&format!(
                "（子の進行から導出。保存値は{}）",
                text::lifecycle(stored)
            ));
        }
        if let Some(situation) = text::situation(detail.status) {
            state.push_str(&format!(" · {situation}"));
        }
        pane = pane
            .child(div().id("detail-state").child(state))
            .child(belongs)
            .child(self.render_progress(detail, cx))
            // A creation found made after its publication stopped says so on its detail.
            .children(self.render_outcome(detail, Section::Create, cx))
            .children(self.render_filtered_out(cx));
        if detail.heads > 0 {
            pane = pane.child(
                div()
                    .id("detail-conflict")
                    .text_sm()
                    .text_color(danger)
                    .child(format!(
                        "衝突しています（head {} 件）。表示は最初の head の値です。解決は CLI の axon resolve で行います。",
                        detail.heads
                    )),
            );
        }
        if !detail.violations.is_empty() {
            let mut violations = div()
                .id("detail-violations")
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .text_color(danger);
            for (ix, (kind, links)) in detail.violations.iter().enumerate() {
                let mut line = div()
                    .id(("violation", ix))
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .child(text::violation(*kind));
                for link in links {
                    line = line.child(self.render_link(link, cx));
                }
                violations = violations.child(line);
            }
            pane = pane.child(violations);
        }
        if !detail.waits.is_empty() {
            let mut waits = div()
                .id("detail-waits")
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(warning)
                .child(div().text_sm().child(text::waiting_for(detail.waiting_for)));
            for (ix, wait) in detail.waits.iter().enumerate() {
                let mut line = div()
                    .id(("wait", ix))
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .text_sm()
                    .child(
                        div()
                            .text_color(muted)
                            .child(format!("{}:", text::wait(wait.kind))),
                    )
                    .child(self.render_link(&wait.entity, cx));
                if let (WaitKind::DescendantDependency, Some(via)) = (wait.kind, &wait.via) {
                    line = line.child("→").child(self.render_link(via, cx));
                }
                waits = waits.child(line);
            }
            pane = pane.child(waits);
        }

        pane = pane
            .child(self.render_structure(detail, cx))
            .children(self.render_create_inside(detail, cx));

        // Shown beside the edit form too: the recorded body is what a warning asks to check.
        pane = pane.child(
            div()
                .id("detail-description")
                .flex()
                .flex_col()
                .gap_1()
                .child(section_heading("本文".into()))
                .child(if detail.description.is_empty() {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("本文はありません。")
                } else {
                    div().text_sm().child(detail.description.clone())
                }),
        );
        if let Some(condition) = &detail.condition {
            pane = pane.child(
                div()
                    .id("detail-condition")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(section_heading(
                        "再浮上条件（アプリは実行しません。満たしているものとして表示しています）"
                            .into(),
                    ))
                    .child(div().text_sm().child(condition.clone())),
            );
        }
        if detail.descendants.is_some() || !detail.children.is_empty() {
            let mut summary = format!("子 {} 件", detail.children.len());
            if let Some(descendants) = &detail.descendants {
                summary.push_str(&format!(
                    " · 衝突していない子孫 {} 件（完了 {} · 取りやめ {}）",
                    descendants.total, descendants.completed, descendants.cancelled
                ));
                if descendants.awaiting_confirmation {
                    summary.push_str(" · 完了確認待ち");
                }
            }
            let mut children = div()
                .id("detail-children")
                .flex()
                .flex_col()
                .gap_1()
                .child(section_heading(summary.into()));
            for child in &detail.children {
                children = children.child(self.render_link(child, cx));
            }
            pane = pane.child(children);
        }
        pane = pane
            .child(self.render_dependencies(detail, cx))
            .children(self.render_links(
                "detail-dependents",
                "この仕事に依存している",
                &detail.dependents,
                cx,
            ));

        let mut notes = div()
            .id("detail-notes")
            .flex()
            .flex_col()
            .gap_2()
            .child(section_heading(
                format!("Note（{} 件）", detail.notes.len()).into(),
            ))
            .child(self.render_note_form(detail, cx));
        for (ix, note) in detail.notes.iter().enumerate() {
            let mut head = text::time(note.at);
            if let Some(actor) = &note.actor {
                head.push_str(&format!(" · {actor}"));
            }
            let mut entry = div()
                .id(("note", ix))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(border)
                .child(div().text_xs().text_color(muted).child(head));
            if let Some(reason) = &note.reason {
                entry = entry.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("理由: {reason}")),
                );
            }
            notes = notes.child(entry.child(div().text_sm().child(note.body.clone())));
        }
        pane = pane.child(notes);

        let mut history = div()
            .id("detail-history")
            .flex()
            .flex_col()
            .gap_1()
            .child(section_heading("履歴".into()));
        for (ix, entry) in detail.history.iter().enumerate() {
            let mut head = format!("{} {}", text::time(entry.at), text::record(&entry.kind));
            if let Some(actor) = &entry.actor {
                head.push_str(&format!(" · {actor}"));
            }
            if entry.concurrent_with_previous {
                head.push_str(" · 直前の記録と並行");
            }
            if entry.parent_missing {
                head.push_str(" · 前の記録が見つかりません");
            }
            let mut line = div()
                .id(("history", ix))
                .flex()
                .flex_col()
                .text_sm()
                .child(head);
            for change in &entry.changes {
                line = line.child(
                    div()
                        .pl_4()
                        .text_xs()
                        .text_color(muted)
                        .child(text::difference(change)),
                );
            }
            if let Some(reason) = &entry.reason {
                line = line.child(
                    div()
                        .pl_4()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("理由: {reason}")),
                );
            }
            history = history.child(line);
        }
        pane.child(history).into_any_element()
    }
}
