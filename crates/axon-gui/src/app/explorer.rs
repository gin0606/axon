//! The filters, the shared list and the detail pane of the selected project.

use crate::i18n::Language;

use super::layout::FocusArea;
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
        text::{MarkdownExtensions, SelectionFormat, TextView, TextViewStyle},
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
        let heading = |text: String| {
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
            .child(heading(crate::tr!(self.language, "filter.state")));
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
                            .label(text::state(self.language, state))
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
        filters = filters.child(heading(crate::tr!(self.language, "filter.kind")));
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
        filters = filters.child(heading("label".into()));
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
                        .label(crate::tr!(self.language, "filter.reset"))
                        .on_click(cx.listener(|this, _, window, cx| this.reset_filter(window, cx))),
                ),
            )
            .into_any_element()
    }

    /// Whether [`render_list`](Self::render_list) draws rows, which take the focus of the list.
    pub(super) fn shows_rows(&self) -> bool {
        self.explorer.board().is_some()
            && matches!(self.store, StoreState::Loaded(_) | StoreState::Reloading(_))
            && !self.explorer.listing().rows.is_empty()
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
        let layout_button = |id: &'static str, label: String, value: Layout| {
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
            crate::tr!(
                self.language,
                "list.count_context",
                matched = listing.matched,
                total = board.len(),
                context = listing.context
            )
        } else {
            crate::tr!(
                self.language,
                "list.count",
                matched = listing.matched,
                total = board.len()
            )
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
                            .w(px(100.))
                            .label(if matches!(self.store, StoreState::Reloading(_)) {
                                crate::tr!(self.language, "common.reloading")
                            } else {
                                crate::tr!(self.language, "common.reload")
                            })
                            .tooltip(crate::tr!(self.language, "common.reload"))
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
                    .child(layout_button(
                        "layout-tree",
                        crate::tr!(self.language, "layout.tree"),
                        Layout::Tree,
                    ))
                    .child(layout_button(
                        "layout-flat",
                        crate::tr!(self.language, "layout.flat"),
                        Layout::Flat,
                    ))
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
                .child(crate::tr!(self.language, "list.empty"))
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
                .child(crate::tr!(self.language, "list.no_matches"))
                .child(
                    Button::new("reset-filter-empty")
                        .outline()
                        .compact()
                        .label(crate::tr!(self.language, "filter.reset"))
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
                let meta = text::row_meta(self.language, item, row.matched);
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
                self.with_copy_menu(
                    div().pb_px().child(line.test_support()),
                    &row.id,
                    Some(&item.title),
                    FocusArea::List,
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
        self.with_copy_menu(
            div().min_w_0().child(target),
            &link.id,
            title,
            FocusArea::Detail,
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
                            .child(crate::tr!(
                                self.language,
                                "detail.link_meta",
                                kind = text::kind(known.kind),
                                state = text::state(self.language, known.state)
                            )),
                    )
                    .test_support()
                    .into_any_element()
            }
            None => div()
                .id(entity_element(&link.id))
                .text_sm()
                .text_color(palette.muted)
                .child(crate::tr!(self.language, "detail.missing", id = link.id))
                .test_support()
                .into_any_element(),
        }
    }

    fn render_links(
        &self,
        id: &'static str,
        heading: String,
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
            .child(lifecycle_track(self.language, detail.state, palette));

        let mut state = String::new();
        if let Some(situation) = text::situation(self.language, detail.status) {
            state.push_str(&situation);
        }
        if let Some(stored) = detail.stored
            && State::of(Some(stored)) != detail.state
        {
            state = crate::tr!(
                self.language,
                "detail.derived",
                situation = state,
                state = text::lifecycle(self.language, stored)
            )
            .trim_start()
            .to_owned();
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
            .text_color(palette.muted);
        if !detail.ancestors.is_empty() {
            for (ix, ancestor) in detail.ancestors.iter().enumerate() {
                if ix > 0 {
                    belongs = belongs.child("›");
                }
                belongs = belongs.child(self.render_link(ancestor, cx));
            }
        }
        pane = pane
            .when(!detail.ancestors.is_empty(), |pane| pane.child(belongs))
            .child(head)
            .children(self.render_filtered_out(cx));
        if detail.heads > 0 {
            pane = pane.child(callout("detail-conflict", palette.danger).child(crate::tr!(
                self.language,
                "detail.conflict",
                count = detail.heads
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
                    .child(text::violation(self.language, *kind));
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
                    .child(text::waiting_for(self.language, detail.waiting_for)),
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
                            .child(text::wait(self.language, wait.kind)),
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

        if !detail.description.trim().is_empty() {
            pane = pane.child(
                div()
                    .id("detail-description-body")
                    .text_sm()
                    .line_height(relative(1.6))
                    .max_w(px(MEASURE))
                    .child(self.render_markdown("detail-description-text", &detail.description, cx))
                    .test_support(),
            );
        }
        if let Some(condition) = &detail.condition {
            pane = pane.child(
                section(
                    "detail-condition",
                    crate::tr!(self.language, "detail.condition").into(),
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
            let summary = match &detail.descendants {
                Some(descendants) => crate::tr!(
                    self.language,
                    if descendants.awaiting_confirmation {
                        "detail.descendants_confirmable"
                    } else {
                        "detail.descendants"
                    },
                    children = detail.children.len(),
                    total = descendants.total,
                    completed = descendants.completed,
                    cancelled = descendants.cancelled
                ),
                None => crate::tr!(
                    self.language,
                    "detail.children",
                    count = detail.children.len()
                ),
            };
            let mut children = section("detail-children", summary.into(), palette);
            for child in &detail.children {
                children = children.child(self.render_link(child, cx));
            }
            pane = pane.child(children);
        }
        pane = pane
            .children(self.render_links(
                "detail-dependencies",
                crate::tr!(self.language, "detail.dependencies"),
                &detail.dependencies,
                cx,
            ))
            .children(self.render_links(
                "detail-dependents",
                crate::tr!(self.language, "detail.dependents"),
                &detail.dependents,
                cx,
            ));

        let mut notes = section(
            "detail-notes",
            crate::tr!(self.language, "detail.notes", count = detail.notes.len()).into(),
            palette,
        )
        .gap_3();
        for (ix, note) in detail.notes.iter().enumerate() {
            let mut head = text::time(note.at);
            if let Some(actor) = &note.actor {
                head = crate::tr!(self.language, "note.heading", time = head, actor = actor);
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
                entry = entry.child(div().text_xs().text_color(palette.muted).child(crate::tr!(
                    self.language,
                    "history.reason",
                    reason = reason
                )));
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
        pane = pane.when(!detail.notes.is_empty(), |pane| pane.child(notes));

        // A timeline: a dot per record on one thread, oldest first.
        let mut history = div().flex().flex_col();
        let last = detail.history.len().saturating_sub(1);
        for (ix, entry) in detail.history.iter().enumerate() {
            let mut head = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_x_2()
                .child(
                    div()
                        .text_sm()
                        .child(text::record(self.language, &entry.kind)),
                )
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
                        .child(crate::tr!(self.language, "history.parallel")),
                );
            }
            if entry.parent_missing {
                head = head.child(
                    div()
                        .text_xs()
                        .text_color(palette.danger)
                        .child(crate::tr!(self.language, "history.missing")),
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
                        .child(text::difference(self.language, change)),
                );
            }
            if let Some(reason) = &entry.reason {
                line = line.child(div().text_xs().text_color(palette.muted).child(crate::tr!(
                    self.language,
                    "history.reason",
                    reason = reason
                )));
            }
            history = history.child(line);
        }
        pane.child(
            section(
                "detail-history",
                crate::tr!(self.language, "history.title").into(),
                palette,
            )
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
        TextView::markdown(id, self.prepared.get(self.language, source))
            .style(style)
            .selection_format(SelectionFormat::Plain)
            .markdown_extensions(
                MarkdownExtensions::default()
                    // ImageAlt's text depends on the language, so a language change must
                    // re-parse source that has not changed.
                    .parser_revision(match self.language {
                        Language::Japanese => 1,
                        Language::English => 2,
                    })
                    .plugin(markdown::ImageAlt(self.language)),
            )
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
        &self,
        trigger: gpui_kit::Div,
        id: &EntityId,
        title: Option<&str>,
        origin: FocusArea,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.clone();
        let title = title.map(str::to_owned);
        let app = cx.weak_entity();
        trigger
            .id(named(format!("copy-menu-{id}")))
            .context_menu(move |menu, window, cx| {
                // The menu takes the focus as it is built rather than when it is first drawn,
                // so a menu its row moved away from before then holds the focus too.
                let focus = menu.focus_handle(cx);
                window.focus(&focus, cx);
                let opened = CopyMenu {
                    menu: cx.weak_entity(),
                    focus,
                    origin,
                };
                app.update(cx, |app, cx| app.record_copy_menu(opened, cx))
                    .ok();
                copy_menu(Language::of(cx), menu, &id, title.as_deref())
            })
            .into_any_element()
    }

    /// Records `opened` as the last context menu. The one recorded before is closed, as it is
    /// either closed already or hidden still waiting to be closed, which no one would do once
    /// it is no longer recorded.
    fn record_copy_menu(&mut self, opened: CopyMenu, cx: &mut Context<Self>) {
        self.close_copy_menu(cx);
        self.copy_menu = Some(opened);
    }

    pub(super) fn close_copy_menu(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(FocusHandle, FocusArea)> {
        let opened = self.copy_menu.take()?;
        if let Some(menu) = opened.menu.upgrade() {
            menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
        Some((opened.focus, opened.origin))
    }

    /// Closes the last context menu when it holds the focus but was not drawn, and tells the
    /// part it was opened from, where the focus goes back. A menu is drawn only while its row or
    /// link is under the point right-clicked, so a read that moves them, or scrolls a row out of
    /// the list, hides it with the focus. Closed, it does not come back and take the focus when
    /// the row returns to that place.
    pub(super) fn close_hidden_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FocusArea> {
        let hidden = (self.copy_menu.as_ref())
            .is_some_and(|opened| self.holds_undrawn_focus(&opened.focus, window));
        let opened = self.copy_menu.take_if(|_| hidden)?;
        if let Some(menu) = opened.menu.upgrade() {
            menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
        Some(opened.origin)
    }

    /// Why the selected Entity is not among the matches, when it is not.
    fn render_filtered_out(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let exclusions = self.explorer.selected_exclusions();
        if exclusions.is_empty() {
            return None;
        }
        let reasons: Vec<String> = exclusions
            .iter()
            .map(|value| exclusion(self.language, value))
            .collect();
        Some(
            div()
                .id("detail-filtered-out")
                .pl_3()
                .border_l_2()
                .border_color(Palette::of(cx).caution)
                .text_sm()
                .text_color(Palette::of(cx).caution)
                .child(crate::tr!(
                    self.language,
                    "detail.excluded",
                    reasons = reasons.join(&self.language.text("list.separator"))
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
    pub(super) focus: FocusHandle,
    /// The list or the detail column the menu was opened from.
    origin: FocusArea,
}

/// The items that copy the ID, the title, and both as `ID title`, as `axon list` lines them up.
fn copy_menu(language: Language, menu: PopupMenu, id: &EntityId, title: Option<&str>) -> PopupMenu {
    let copy = |label: String, text: String| {
        PopupMenuItem::new(label).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        })
    };
    let menu = menu.item(copy(crate::tr!(language, "copy.id"), id.to_string()));
    match title {
        Some(title) => menu
            .item(copy(crate::tr!(language, "copy.title"), title.to_owned()))
            .item(copy(
                crate::tr!(language, "copy.both"),
                format!("{id} {title}"),
            )),
        None => menu,
    }
}

fn exclusion(language: Language, exclusion: &Exclusion) -> String {
    match exclusion {
        Exclusion::State(state) => crate::tr!(
            language,
            "exclusion.state",
            state = text::state(language, *state)
        ),
        Exclusion::Kind(kind) => crate::tr!(language, "exclusion.kind", kind = text::kind(*kind)),
        Exclusion::Label(label) => crate::tr!(language, "exclusion.label", label = label.name()),
        Exclusion::Query(query) => crate::tr!(language, "exclusion.query", query = query),
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
fn lifecycle_track(language: Language, current: State, palette: Palette) -> AnyElement {
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
            stop.child(text::state(language, *state))
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
                .child(text::state(language, current)),
        );
    }
    track.into_any_element()
}
