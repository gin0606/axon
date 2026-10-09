//! The filters, the shared list and the detail pane of the selected project.

use super::{AxonApp, StoreState, markdown, style::Palette, text};
use crate::board::{EntityDetail, Exclusion, Filter, Layout, Link, State, WaitKind};
use crate::{OpenSelectedEntity, SelectNextEntity, SelectPreviousEntity};
use axon::lifecycle::{EntityId, Kind, Label};
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    input::Input,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, DismissEvent, ElementId, FocusHandle, Focusable,
    HighlightStyle, Hsla, IntoElement, MouseButton, MouseDownEvent, ScrollStrategy, SharedString,
    WeakEntity, Window,
    base::{
        SelectableText, StyledExt, TestSupportExt, TextSelection, input as edit,
        text::{SelectionFormat, TextView, TextViewStyle},
    },
    div,
    prelude::*,
    px, relative, rems, uniform_list,
};
use std::ops::Range;

/// The deepest level that still indents further in the list.
const MAX_INDENT: usize = 8;
/// The room left of a top-level row's state mark, and the indent of each level below it.
const ROW_INSET: f32 = 8.;
const INDENT: f32 = 16.;

/// Key context of the list, where the arrow keys move the selection.
pub const LIST_CONTEXT: &str = "AxonList";

/// The element ID of the list row of `id`, and of a link to it inside one detail section.
pub fn entity_element(id: &EntityId) -> ElementId {
    ElementId::Name(format!("entity-{id}").into())
}

fn named(name: String) -> ElementId {
    ElementId::Name(name.into())
}

impl AxonApp {
    pub(super) fn update_filter(
        &mut self,
        change: impl FnOnce(&mut Filter),
        cx: &mut Context<Self>,
    ) {
        self.explorer.update_filter(change);
        self.scroll_list_to_top();
        cx.notify();
    }

    pub fn set_layout(&mut self, layout: Layout, cx: &mut Context<Self>) {
        self.explorer.set_layout(layout);
        self.scroll_list_to_top();
        cx.notify();
    }

    /// Scrolls the list back to its first row.
    pub(super) fn scroll_list_to_top(&self) {
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Scrolls the list to the selected row, when the list shows it.
    pub(super) fn reveal_selected(&self) {
        let listing = self.explorer.listing();
        if let Some(ix) = self
            .explorer
            .selected()
            .and_then(|id| listing.rows.iter().position(|row| &row.id == id))
        {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    /// Copies the ID of the selected row, as the copy button of the detail does, while the list
    /// itself has the focus and no text in the window is selected; selected text is left to the
    /// window, which copies it. A context menu open over the list has the focus, so this does
    /// not copy past it.
    fn copy_selected_id(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let applies = self.list_focus.is_focused(window)
            && TextSelection::selected_text(window, cx).trim().is_empty();
        match self.explorer.selected() {
            Some(id) if applies => cx.write_to_clipboard(ClipboardItem::new_string(id.to_string())),
            _ => cx.propagate(),
        }
    }

    /// Moves the selection through the list and keeps the selected row in view.
    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        self.explorer.step(step);
        self.reveal_selected();
        cx.notify();
    }

    pub fn reset_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.update_filter(|filter| *filter = Filter::default(), cx);
    }

    pub(super) fn render_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        let filter = self.explorer.filter();
        let heading = |text: &'static str| {
            div()
                .px_1()
                .pt_4()
                .pb_1()
                .text_xs()
                .text_color(palette.muted)
                .child(text)
        };
        let mut filters = div()
            .id("filters")
            .flex()
            .flex_col()
            .gap_1()
            .child(heading("状態"));
        for state in self.explorer.offered_states() {
            filters = filters.child(
                div()
                    .px_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        Checkbox::new(named(format!("state-{state:?}")))
                            .label(text::state(state))
                            .checked(filter.states.contains(&state))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.update_filter(|f| f.toggle_state(state, *checked), cx)
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.state(state))
                            .child(text::state_icon(state)),
                    ),
            );
        }
        filters = filters.child(heading("種類"));
        for kind in [Kind::Issue, Kind::Group] {
            filters = filters.child(
                div().px_1().child(
                    Checkbox::new(named(format!("kind-{kind:?}")))
                        .label(text::kind(kind))
                        .checked(filter.kinds.contains(&kind))
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.update_filter(|f| f.toggle_kind(kind, *checked), cx)
                        })),
                ),
            );
        }
        filters = filters.child(heading("label"));
        for label in Label::ALL {
            filters = filters.child(
                div().px_1().child(
                    Checkbox::new(named(format!("label-{}", label.name())))
                        .label(label.name())
                        .checked(filter.labels.contains(&label))
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.update_filter(|f| f.toggle_label(label, *checked), cx)
                        })),
                ),
            );
        }
        filters
            .child(
                div().pt_4().child(
                    Button::new("reset-filter")
                        .ghost()
                        .compact()
                        .w_full()
                        .label("絞り込みを初期値に戻す")
                        .on_click(cx.listener(|this, _, window, cx| this.reset_filter(window, cx))),
                ),
            )
            .into_any_element()
    }

    /// The search, the layout switch and the rows, once the selected project has been read.
    pub(super) fn render_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let board = self.explorer.board()?;
        if !matches!(self.store, StoreState::Loaded(_) | StoreState::Reloading(_)) {
            return None;
        }
        let palette = Palette::of(cx);
        let listing = self.explorer.listing();
        let layout = self.explorer.layout();
        let layout_button = |id: &'static str, label: &'static str, value: Layout| {
            let button = Button::new(id)
                .compact()
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_layout(value, cx)));
            if layout == value {
                button.primary()
            } else {
                button.ghost()
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
            .px_1()
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
                        Button::new("reload-list")
                            .outline()
                            .compact()
                            .label("再読み込み")
                            .on_click(cx.listener(|this, _, _, cx| this.reload_selected(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .items_center()
                    .child(layout_button("layout-tree", "階層", Layout::Tree))
                    .child(layout_button("layout-flat", "フラット", Layout::Flat))
                    .child(
                        div()
                            .id("list-count")
                            .pl_2()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(count),
                    ),
            );
        let body: AnyElement = if board.is_empty() {
            div()
                .id("list-empty")
                .px_1()
                .pt_4()
                .text_sm()
                .text_color(palette.muted)
                .child("このリポジトリにはまだ Issue・Group がありません。")
                .into_any_element()
        } else if listing.rows.is_empty() {
            div()
                .id("list-empty")
                .px_1()
                .pt_4()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .text_sm()
                .text_color(palette.muted)
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
            // Only the rows in view are built, so a frame costs the same however many
            // Entities the project holds. Every row has the same height for that.
            let rows = uniform_list(
                "entity-rows",
                listing.rows.len(),
                cx.processor(|this, range: Range<usize>, _, cx| this.render_rows(range, cx)),
            )
            .track_scroll(&self.list_scroll)
            .size_full();
            div()
                .id("entity-list")
                .track_focus(&self.list_focus)
                .capture_any_mouse_down(focus_before_menu(self.list_focus.clone()))
                .key_context(LIST_CONTEXT)
                .on_action(cx.listener(|this, _: &SelectNextEntity, _, cx| this.step(1, cx)))
                .on_action(cx.listener(|this, _: &SelectPreviousEntity, _, cx| this.step(-1, cx)))
                .on_action(cx.listener(|this, _: &OpenSelectedEntity, window, cx| {
                    this.open_selected(window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &edit::Copy, window, cx| {
                        this.copy_selected_id(window, cx)
                    }),
                )
                .rounded_md()
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .focus(|style| style.border_color(palette.signal))
                .flex_1()
                .min_h_0()
                .child(rows)
                .test_support()
                .into_any_element()
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

    /// The list rows in `range` of the listing. Both lines of a row are truncated, so every
    /// row has the height the list measures on the first. In the tree, a thin guide runs down
    /// each level of containment, so the rows of one Group read as one block.
    fn render_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(board) = self.explorer.board() else {
            return Vec::new();
        };
        let palette = Palette::of(cx);
        let selected = self.explorer.selected();
        let rows = self.explorer.listing().rows.get(range).unwrap_or_default();
        rows.iter()
            .map(|row| {
                let item = board
                    .item(&row.id)
                    .expect("a listed Entity is on the board");
                let meta = text::row_meta(item, row.matched);
                let id = row.id.clone();
                let is_selected = selected == Some(&row.id);
                let depth = row.depth.min(MAX_INDENT);
                let title_color = if row.matched {
                    palette.ink
                } else {
                    palette.muted
                };
                let mut line = div()
                    .id(entity_element(&row.id))
                    .relative()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .py(px(5.))
                    .pr_2()
                    // Deep trees stop indenting so the title keeps its room.
                    .pl(px(ROW_INSET + INDENT * depth as f32))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(palette.hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.list_focus, cx);
                        this.open_entity(id.clone(), cx);
                        this.keep_focus(window, cx);
                    }))
                    .children((0..depth).map(|level| {
                        div()
                            .absolute()
                            .top_0()
                            .bottom(px(-1.))
                            .left(px(ROW_INSET + INDENT * level as f32 + 6.))
                            .w(px(1.))
                            .bg(palette.rule)
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_color(if row.matched {
                                palette.state(item.state)
                            } else {
                                palette.state(item.state).opacity(0.55)
                            })
                            .child(text::state_icon(item.state)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(title_color)
                                    .when(item.kind == Kind::Group, |title| title.font_medium())
                                    .child(item.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.muted)
                                    .truncate()
                                    .child(meta),
                            ),
                    );
                if is_selected {
                    line = line.bg(palette.signal_wash).child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(4.))
                            .bottom(px(4.))
                            .w(px(2.))
                            .rounded_full()
                            .bg(palette.signal),
                    );
                }
                // The gap between rows, inside each row so the heights stay equal.
                Self::with_copy_menu(
                    div().pb_px().child(line.test_support()),
                    &row.id,
                    Some(&item.title),
                    MenuOrigin::List,
                    cx,
                )
            })
            .collect()
    }

    /// A clickable reference to another Entity, or its ID when the store does not hold it, whose
    /// context menu copies the ID and the title.
    fn render_link(&self, link: &Link, cx: &mut Context<Self>) -> AnyElement {
        let title = link.known.as_ref().map(|known| known.title.as_str());
        let target = self.render_link_target(link, cx);
        Self::with_copy_menu(
            div().min_w_0().child(target),
            &link.id,
            title,
            MenuOrigin::Detail,
            cx,
        )
    }

    fn render_link_target(&self, link: &Link, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        match &link.known {
            Some(known) => {
                let id = link.id.clone();
                div()
                    .id(entity_element(&link.id))
                    .flex()
                    .flex_row()
                    .gap_1p5()
                    .min_w_0()
                    .cursor_pointer()
                    .text_sm()
                    .hover(|style| style.text_color(palette.ink))
                    .text_color(palette.signal)
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_link(id.clone(), cx)))
                    .child(
                        div()
                            .flex_none()
                            .text_color(palette.state(known.state))
                            .child(text::state_icon(known.state)),
                    )
                    .child(div().min_w_0().child(known.title.clone()))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(format!(
                                "{}・{}",
                                text::kind(known.kind),
                                text::state(known.state)
                            )),
                    )
                    .test_support()
                    .into_any_element()
            }
            None => div()
                .id(entity_element(&link.id))
                .text_sm()
                .text_color(palette.muted)
                .child(format!("{}（記録にない ID）", link.id))
                .test_support()
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
        let mut section = section(id, heading.into(), Palette::of(cx));
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
        let palette = Palette::of(cx);
        let selection = cx.theme().colors.selection;
        self.prepared.next_frame();
        // Scoped to the Entity, so another one opens scrolled to its top.
        let mut pane = div()
            .id(named(format!("detail-{}", detail.id)))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_5()
            .px_6()
            .pt_5()
            .pb_8();

        // What it is, then where it belongs. The kind, the label and the ID head the column's
        // bar.
        let mut head = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("detail-title")
                    .text_size(px(22.))
                    .line_height(px(30.))
                    .font_semibold()
                    .child(
                        SelectableText::new("detail-title-text", detail.title.clone())
                            .selection_color(selection),
                    )
                    .test_support(),
            )
            .child(lifecycle_track(detail.state, palette));

        let mut state = String::new();
        if let Some(situation) = text::situation(detail.status) {
            state.push_str(situation);
        }
        if let Some(stored) = detail.stored
            && State::of(Some(stored)) != detail.state
        {
            state.push_str(&format!(
                "（子の進行から導出。保存値は{}）",
                text::lifecycle(stored)
            ));
        }
        if !state.is_empty() {
            head = head.child(
                div()
                    .id("detail-state")
                    .text_sm()
                    .text_color(palette.muted)
                    .child(state),
            );
        }
        let mut belongs = div()
            .id("detail-ancestors")
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .text_sm()
            .text_color(palette.muted)
            .child(div().text_xs().pr_1().child("所属"));
        if detail.ancestors.is_empty() {
            belongs = belongs.child("なし");
        } else {
            for (ix, ancestor) in detail.ancestors.iter().enumerate() {
                if ix > 0 {
                    belongs = belongs.child("›");
                }
                belongs = belongs.child(self.render_link(ancestor, cx));
            }
        }
        pane = pane
            .child(head.child(belongs))
            .children(self.render_filtered_out(cx));
        if detail.heads > 0 {
            pane = pane.child(callout("detail-conflict", palette.danger).child(format!(
                "衝突しています（head {} 件）。表示は最初の head の値です。",
                detail.heads
            )));
        }
        if !detail.violations.is_empty() {
            let mut violations = callout("detail-violations", palette.danger).gap_1();
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
            let mut waits = callout("detail-waits", palette.caution).gap_1().child(
                div()
                    .text_color(palette.caution)
                    .font_medium()
                    .child(text::waiting_for(detail.waiting_for)),
            );
            for (ix, wait) in detail.waits.iter().enumerate() {
                let mut line = div()
                    .id(("wait", ix))
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_x_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(text::wait(wait.kind)),
                    )
                    .child(self.render_link(&wait.entity, cx));
                if let (WaitKind::DescendantDependency, Some(via)) = (wait.kind, &wait.via) {
                    line = line
                        .child(div().text_color(palette.muted).child("→"))
                        .child(self.render_link(via, cx));
                }
                waits = waits.child(line);
            }
            pane = pane.child(waits);
        }

        pane = pane.child(section("detail-description", "本文".into(), palette).child(
            if detail.description.is_empty() {
                div()
                    .text_sm()
                    .text_color(palette.muted)
                    .child("本文はありません。")
                    .into_any_element()
            } else {
                div()
                    .id("detail-description-body")
                    .text_sm()
                    .line_height(relative(1.6))
                    .max_w(px(MEASURE))
                    .child(self.render_markdown("detail-description-text", &detail.description, cx))
                    .test_support()
                    .into_any_element()
            },
        ));
        if let Some(condition) = &detail.condition {
            pane = pane.child(
                section(
                    "detail-condition",
                    "再浮上条件（アプリは実行しません。満たしているものとして表示しています）"
                        .into(),
                    palette,
                )
                .child(
                    div()
                        .id("detail-condition-text")
                        .text_sm()
                        .max_w(px(MEASURE))
                        // A code block, so it reads as written and takes its place among the
                        // description and the Notes when a selection runs across them.
                        .child(self.render_markdown(
                            "detail-condition-code",
                            &markdown::code_block(condition),
                            cx,
                        ))
                        .test_support(),
                ),
            );
        }
        if detail.descendants.is_some() || !detail.children.is_empty() {
            let mut summary = format!("子 {} 件", detail.children.len());
            if let Some(descendants) = &detail.descendants {
                summary.push_str(&format!(
                    "、衝突していない子孫 {} 件（完了 {}、取りやめ {}）",
                    descendants.total, descendants.completed, descendants.cancelled
                ));
                if descendants.awaiting_confirmation {
                    summary.push_str("、完了確認待ち");
                }
            }
            let mut children = section("detail-children", summary.into(), palette);
            for child in &detail.children {
                children = children.child(self.render_link(child, cx));
            }
            pane = pane.child(children);
        }
        let mut dependencies = section("detail-dependencies", "依存先".into(), palette);
        if detail.dependencies.is_empty() {
            dependencies = dependencies.child(
                div()
                    .text_sm()
                    .text_color(palette.muted)
                    .child("依存先はありません。"),
            );
        }
        for dependency in &detail.dependencies {
            dependencies = dependencies.child(self.render_link(dependency, cx));
        }
        pane = pane.child(dependencies).children(self.render_links(
            "detail-dependents",
            "この仕事に依存している",
            &detail.dependents,
            cx,
        ));

        let mut notes = section(
            "detail-notes",
            format!("Note（{} 件）", detail.notes.len()).into(),
            palette,
        )
        .gap_3();
        for (ix, note) in detail.notes.iter().enumerate() {
            let mut head = text::time(note.at);
            if let Some(actor) = &note.actor {
                head.push_str(&format!("　{actor}"));
            }
            let mut entry = div()
                .id(("note", ix))
                .flex()
                .flex_col()
                .gap_1()
                .pl_3()
                .border_l_2()
                .border_color(palette.rule)
                .child(div().text_xs().text_color(palette.muted).child(head));
            if let Some(reason) = &note.reason {
                entry = entry.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(format!("理由: {reason}")),
                );
            }
            notes = notes.child(
                entry.child(
                    div()
                        .id(("note-body", ix))
                        .text_sm()
                        .line_height(relative(1.6))
                        .max_w(px(MEASURE))
                        .child(self.render_markdown(("note-text", ix), &note.body, cx))
                        .test_support(),
                ),
            );
        }
        pane = pane.child(notes);

        // A timeline: a dot per record on one thread, oldest first.
        let mut history = div().flex().flex_col();
        let last = detail.history.len().saturating_sub(1);
        for (ix, entry) in detail.history.iter().enumerate() {
            let mut head = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_x_2()
                .child(div().text_sm().child(text::record(&entry.kind)))
                .child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(text::time(entry.at)),
                );
            if let Some(actor) = &entry.actor {
                head = head.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(actor.clone()),
                );
            }
            if entry.concurrent_with_previous {
                head = head.child(
                    div()
                        .text_xs()
                        .text_color(palette.caution)
                        .child("直前の記録と並行"),
                );
            }
            if entry.parent_missing {
                head = head.child(
                    div()
                        .text_xs()
                        .text_color(palette.danger)
                        .child("前の記録が見つかりません"),
                );
            }
            let mut line = div()
                .id(("history", ix))
                .relative()
                .flex()
                .flex_col()
                .gap_0p5()
                .pl_5()
                .pb_3()
                // The thread, which stops at the last record.
                .when(ix < last, |line| {
                    line.child(
                        div()
                            .absolute()
                            .left(px(3.))
                            .top(px(12.))
                            .bottom_0()
                            .w(px(1.))
                            .bg(palette.rule),
                    )
                })
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(6.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(palette.muted),
                )
                .child(head);
            for change in &entry.changes {
                line = line.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(text::difference(change)),
                );
            }
            if let Some(reason) = &entry.reason {
                line = line.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(format!("理由: {reason}")),
                );
            }
            history = history.child(line);
        }
        pane.child(
            section("detail-history", "履歴".into(), palette)
                .gap_2()
                .child(history),
        )
        .into_any_element()
    }

    /// A description or a Note: Markdown whose text can be selected and copied as shown, whose
    /// web links open in the browser and whose images show their alternative text.
    fn render_markdown(
        &self,
        id: impl Into<ElementId>,
        source: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = Palette::of(cx);
        let theme = cx.theme();
        let style = TextViewStyle::default()
            .with_dark(theme.is_dark())
            .with_foreground(palette.ink)
            .with_muted_foreground(palette.muted)
            .with_link(palette.signal)
            .with_selection(theme.colors.selection)
            .with_code_background(palette.hover)
            .with_inline_code(HighlightStyle {
                background_color: Some(palette.hover),
                ..Default::default()
            })
            .with_border(palette.rule)
            .with_paragraph_gap(rems(0.75));
        TextView::markdown(id, self.prepared.get(source))
            .style(style)
            .selection_format(SelectionFormat::Plain)
            .plugin(markdown::ImageAlt)
            .image_source(markdown::no_image)
            .on_link_click(|url, event, _, cx| {
                if markdown::opens_link(url, event) {
                    cx.open_url(url);
                }
            })
            .into_any_element()
    }

    /// `trigger`, a wrapper of a row or a link inside `origin`, with a context menu that copies
    /// `id` and its title when there is one. The menu goes on a wrapper because a row or link
    /// observed for the tests cannot take one itself.
    fn with_copy_menu(
        trigger: gpui_kit::Div,
        id: &EntityId,
        title: Option<&str>,
        origin: MenuOrigin,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.clone();
        let title = title.map(str::to_owned);
        let app = cx.weak_entity();
        trigger
            .id(named(format!("copy-menu-{id}")))
            .context_menu(move |menu, window, cx| {
                // The menu takes the focus as it is built rather than when it is first drawn,
                // so a menu its row moved away from before then holds the focus too. Losing the
                // focus is not seen when nothing had it in the frame before, so the frame after
                // the first one the menu could be drawn in is checked as well.
                let focus = menu.focus_handle(cx);
                window.focus(&focus, cx);
                let opened = CopyMenu {
                    menu: cx.weak_entity(),
                    focus: focus.clone(),
                    origin,
                };
                app.update(cx, |app, cx| app.record_copy_menu(opened, cx))
                    .ok();
                let app = app.clone();
                window.on_next_frame(move |window, _| {
                    window.on_next_frame(move |window, cx| {
                        // Only this menu: another may have been opened since.
                        app.update(cx, |app, cx| {
                            app.recover_from_hidden_menu(Some(&focus), window, cx)
                        })
                        .ok();
                    })
                });
                copy_menu(menu, &id, title.as_deref())
            })
            .into_any_element()
    }

    /// Records `opened` as the last context menu. The one recorded before is closed, as it is
    /// either closed already or hidden still waiting to be closed, which no one would do once
    /// it is no longer recorded.
    fn record_copy_menu(&mut self, opened: CopyMenu, cx: &mut Context<Self>) {
        if let Some(menu) = self
            .copy_menu
            .replace(opened)
            .and_then(|before| before.menu.upgrade())
        {
            menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
    }

    /// Closes the last context menu and gives the focus back to where it was opened from,
    /// when the menu holds the focus but was not drawn. A menu is drawn only while its row or
    /// link is under the point right-clicked, so a read that moves them, or scrolls a row out
    /// of the list, hides it with the focus, and the keys would reach nothing. Closed, it does
    /// not come back and take the focus when the row returns to that place. When the focus
    /// cannot go back to the list or the detail it was opened from, since that is not shown,
    /// the window takes it.
    /// Only the menu with the focus `only` is closed, when it is given.
    pub(super) fn recover_from_hidden_menu(
        &mut self,
        only: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = self.copy_menu.take_if(|opened| {
            only.is_none_or(|only| *only == opened.focus)
                && opened.focus.is_focused(window)
                && !self.app_focus.contains(&opened.focus, window)
        }) else {
            return;
        };
        if let Some(menu) = opened.menu.upgrade() {
            menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
        let origin = match opened.origin {
            MenuOrigin::List => &self.list_focus,
            MenuOrigin::Detail => &self.detail_focus,
        };
        let target = if self.app_focus.contains(origin, window) {
            origin
        } else {
            &self.app_focus
        };
        window.focus(target, cx);
        // A focus moved while the window draws does not draw the window again by itself.
        window.on_next_frame(|window, _| window.refresh());
    }

    /// Why the selected Entity is not among the matches, when it is not.
    fn render_filtered_out(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let exclusions = self.explorer.selected_exclusions();
        if exclusions.is_empty() {
            return None;
        }
        let reasons: Vec<String> = exclusions.iter().map(exclusion).collect();
        Some(
            div()
                .id("detail-filtered-out")
                .pl_3()
                .border_l_2()
                .border_color(Palette::of(cx).caution)
                .text_sm()
                .text_color(Palette::of(cx).caution)
                .child(format!(
                    "現在の絞り込みに一致しないため、一覧には一致として表示されていません（{}）。",
                    reasons.join("。")
                ))
                .test_support()
                .into_any_element(),
        )
    }
}

/// Focuses `focus` on a right-click inside it, before a context menu there records the focus
/// to return to, as a left click would. Without this, closing a menu opened while nothing had
/// the focus would leave the keys reaching nothing.
pub(super) fn focus_before_menu(
    focus: FocusHandle,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.button == MouseButton::Right && !focus.contains_focused(window, cx) {
            window.focus(&focus, cx);
        }
    }
}

/// A context menu opened over a row or a link.
pub(super) struct CopyMenu {
    menu: WeakEntity<PopupMenu>,
    /// The menu's focus, held so the menu is still known to have it once a read drops the menu
    /// with its row.
    focus: FocusHandle,
    origin: MenuOrigin,
}

/// The list or the detail column a context menu was opened from.
#[derive(Clone, Copy)]
enum MenuOrigin {
    List,
    Detail,
}

/// The items that copy the ID, the title, and both as `ID title`, as `axon list` lines them up.
fn copy_menu(menu: PopupMenu, id: &EntityId, title: Option<&str>) -> PopupMenu {
    let copy = |label: &'static str, text: String| {
        PopupMenuItem::new(label).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        })
    };
    let menu = menu.item(copy("ID をコピー", id.to_string()));
    match title {
        Some(title) => menu
            .item(copy("タイトルをコピー", title.to_owned()))
            .item(copy("ID とタイトルをコピー", format!("{id} {title}"))),
        None => menu,
    }
}

fn exclusion(exclusion: &Exclusion) -> String {
    match exclusion {
        Exclusion::State(state) => format!("状態「{}」を選んでいません", text::state(*state)),
        Exclusion::Kind(kind) => format!("種類「{}」を選んでいません", text::kind(*kind)),
        Exclusion::Label(label) => format!("label「{}」を選んでいません", label.name()),
        Exclusion::Query(query) => format!("タイトル・本文・ID に「{query}」がありません"),
    }
}

/// The longest a line of prose gets in the detail pane.
const MEASURE: f32 = 620.;

/// A titled block of the detail pane.
fn section(
    id: &'static str,
    heading: SharedString,
    palette: Palette,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div().id(id).flex().flex_col().gap_1p5().child(
        div()
            .text_xs()
            .font_medium()
            .text_color(palette.muted)
            .child(heading),
    )
}

/// A block that needs attention: a colored rule down its left.
fn callout(id: &'static str, color: Hsla) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .pl_3()
        .py_1()
        .border_l_2()
        .border_color(color)
        .text_sm()
        .text_color(color)
}

/// The lifecycle an Issue or a Group walks, with where this one stands lit in its state's
/// color. Cancelled and conflicted leave the track, which then dims and shows the state apart.
fn lifecycle_track(current: State, palette: Palette) -> AnyElement {
    const TRACK: [State; 4] = [
        State::Undecided,
        State::NotStarted,
        State::InProgress,
        State::Completed,
    ];
    let reached = TRACK.iter().position(|state| *state == current);
    let mut track = div()
        .id("detail-lifecycle")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pt_1()
        .text_sm();
    for (ix, state) in TRACK.iter().enumerate() {
        if ix > 0 {
            let passed = reached.is_some_and(|r| ix <= r);
            track = track.child(
                div()
                    .flex_1()
                    .max_w(px(48.))
                    .min_w(px(8.))
                    .h(px(1.))
                    .bg(if passed { palette.muted } else { palette.rule }),
            );
        }
        let (color, weight) = match reached {
            Some(r) if r == ix => (palette.state(*state), gpui_kit::FontWeight::SEMIBOLD),
            Some(r) if ix < r => (palette.muted, gpui_kit::FontWeight::NORMAL),
            _ => (palette.rule, gpui_kit::FontWeight::NORMAL),
        };
        let stop = div()
            .flex_none()
            .flex()
            .flex_row()
            .gap_1()
            .text_color(color)
            .font_weight(weight)
            .child(text::state_icon(*state));
        // Only the stop it stands on is named, so the track stays short in a narrow pane.
        track = track.child(if reached == Some(ix) {
            stop.child(text::state(*state))
        } else {
            stop
        });
    }
    if reached.is_none() {
        track = track.child(
            div()
                .flex_none()
                .ml_2()
                .flex()
                .flex_row()
                .gap_1()
                .font_semibold()
                .text_color(palette.state(current))
                .child(text::state_icon(current))
                .child(text::state(current)),
        );
    }
    track.into_any_element()
}
