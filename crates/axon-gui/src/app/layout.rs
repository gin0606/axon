//! How the window lays its columns out for its width, the bar at the top of each column, the
//! panel that holds the roots and the filters when their column does not fit, and the way back
//! from a detail.
//!
//! Every width follows the same rules: each column starts with a bar of the same height that
//! names it and holds what acts on it; the button at the top left of the detail always steps
//! one back; the menu at the top left of the list opens the panel. Moving forward or back
//! between details or screens slides the incoming one a short way in from the side it comes
//! from, while a selection or a reload changes what is shown in place.

use super::{AxonApp, RegistryState, STATUS_MAX_HEIGHT, root_label, style::Palette, text};
use crate::Dismiss;
use crate::board::Filter;
use crate::project::ProjectRoot;
use axon::lifecycle::EntityId;
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{
    Animation, AnimationExt, AnyElement, Context, Div, ElementId, IntoElement, Render, Window,
    base::{StyledExt, TestSupportExt},
    div, ease_out_quint,
    prelude::*,
    px, quadratic,
};
use std::time::Duration;

/// Key context of the whole window, where Escape steps back.
pub const APP_CONTEXT: &str = "AxonApp";
/// The width of the left column, and the widest it gets as a panel over a narrow window.
pub const SIDEBAR_WIDTH: f32 = 200.;
pub const SIDEBAR_PANEL_WIDTH: f32 = 272.;
/// The narrowest window that shows the list and the detail side by side, and the one that
/// adds the left column to them.
pub const TWO_COLUMNS_MIN_WIDTH: f32 = 700.;
pub const THREE_COLUMNS_MIN_WIDTH: f32 = 980.;
/// How far below a threshold a narrowing window keeps its columns, so a width resting near it
/// does not switch back and forth.
pub const COLUMN_HYSTERESIS: f32 = 20.;
/// The narrowest the list and the detail pane get.
pub const LIST_MIN_WIDTH: f32 = 300.;
pub const DETAIL_MIN_WIDTH: f32 = 360.;
/// The height of the bar at the top of every column.
pub const BAR_HEIGHT: f32 = 44.;

/// How far, and how long, an incoming detail or screen slides.
const SHIFT: f32 = 24.;
const SHIFT_DURATION: Duration = Duration::from_millis(180);
const PANEL_OPEN_DURATION: Duration = Duration::from_millis(220);
const PANEL_CLOSE_DURATION: Duration = Duration::from_millis(160);

/// How many columns the window lays side by side, which its width decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Columns {
    /// The list or the detail. The roots and the filters open as a panel over them.
    One,
    /// The list and the detail. The roots and the filters open as a panel over them.
    Two,
    /// The roots and the filters, the list and the detail.
    Three,
}
impl Columns {
    /// The columns for `width`. A window gains a column at its threshold and, narrowing from
    /// `previous`, keeps it until [`COLUMN_HYSTERESIS`] below.
    pub fn for_width(width: f32, previous: Option<Columns>) -> Self {
        let at = |width: f32| {
            if width >= THREE_COLUMNS_MIN_WIDTH {
                Self::Three
            } else if width >= TWO_COLUMNS_MIN_WIDTH {
                Self::Two
            } else {
                Self::One
            }
        };
        let fits = at(width);
        match previous {
            Some(previous) if previous > fits && at(width + COLUMN_HYSTERESIS) >= previous => {
                previous
            }
            _ => fits,
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    /// The columns of the last frame; none before the first.
    columns: Option<Columns>,
    panel: Panel,
    /// In one column, the detail of the selected Entity is shown instead of the list. Moving
    /// the selection with the arrow keys leaves the list on screen.
    detail_shown: bool,
    /// The Entities whose details lie under the one shown, oldest first: following a link keeps
    /// the shown one here, and going back returns to it.
    trail: Vec<EntityId>,
    motion: Motion,
}

#[derive(Default)]
struct Panel {
    open: bool,
    /// Closed, but still on screen while it slides out.
    closing: bool,
    /// Numbers each opening and closing, so each plays its own motion once.
    moves: u64,
}

/// The last move forward or back, numbered so each one plays once.
#[derive(Clone, Copy, Default)]
struct Motion {
    direction: Direction,
    scope: Scope,
    ix: u64,
}
impl Motion {
    /// How far right of its place the incoming detail or screen starts: from the right going
    /// forward, from the left going back.
    fn offset(self) -> Option<f32> {
        match self.direction {
            Direction::None => None,
            Direction::Forward => Some(SHIFT),
            Direction::Back => Some(-SHIFT),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Direction {
    /// Nothing to play, as when the window opens.
    #[default]
    None,
    Forward,
    Back,
}

/// What moved: in one column the whole screen between the list and a detail, or one detail
/// for another under the detail's bar.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Scope {
    #[default]
    Screen,
    Detail,
}

impl AxonApp {
    pub fn columns(&self) -> Columns {
        self.layout.columns.unwrap_or(Columns::Three)
    }
    /// Whether the roots and the filters are open as a panel over the list.
    pub fn is_panel_open(&self) -> bool {
        self.layout.panel.open && self.columns() != Columns::Three
    }
    /// Whether one column shows the detail rather than the list.
    pub fn is_detail_shown(&self) -> bool {
        self.layout.detail_shown && self.explorer.detail().is_some()
    }
    /// How far right of its place the detail or screen of the last move starts sliding in,
    /// or `None` before any move.
    pub fn slide_offset(&self) -> Option<f32> {
        self.layout.motion.offset()
    }

    /// Opens the panel and takes the focus off the list behind it, so the arrow keys leave
    /// it alone and Escape closes the panel.
    pub fn open_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = &mut self.layout.panel;
        panel.open = true;
        panel.closing = false;
        panel.moves += 1;
        window.focus(&self.app_focus, cx);
        cx.notify();
    }

    /// Leaves a detail that fills the only column for the list, whose top shows the notice.
    pub(super) fn reveal_notice(&mut self) {
        if self.columns() == Columns::One && self.is_detail_shown() {
            self.layout.detail_shown = false;
            self.moved(Direction::Back, Scope::Screen);
            self.reveal_selected();
        }
    }

    /// Closes the panel, which slides out unless the system asks for reduced motion.
    pub fn close_panel(&mut self, cx: &mut Context<Self>) {
        if !self.layout.panel.open {
            return;
        }
        let animate = !cx.reduce_motion() && self.columns() != Columns::Three;
        let panel = &mut self.layout.panel;
        panel.open = false;
        panel.closing = animate;
        panel.moves += 1;
        if animate {
            let moves = panel.moves;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PANEL_CLOSE_DURATION).await;
                this.update(cx, |this, cx| {
                    if this.layout.panel.moves == moves {
                        this.layout.panel.closing = false;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Opens a known Entity from the list. In one column its detail comes in over the list.
    pub fn open_entity(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.explorer.select(id);
        self.show_detail();
        self.reveal_selected();
        cx.notify();
    }

    /// In one column, shows the selected Entity's detail in place of the list, as Enter does.
    pub(super) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.explorer.selected().is_some() {
            self.show_detail();
            self.keep_focus(window, cx);
            cx.notify();
        }
    }

    /// The detail of the selection is reached from the list: nothing lies under it.
    fn show_detail(&mut self) {
        if self.columns() == Columns::One && !self.is_detail_shown() {
            self.moved(Direction::Forward, Scope::Screen);
        }
        self.layout.trail.clear();
        self.layout.detail_shown = true;
    }

    /// In one column the detail replaces the focused list, so the focus moves to the window,
    /// where Escape still steps back.
    pub(super) fn keep_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.columns() == Columns::One {
            window.focus(&self.app_focus, cx);
        }
    }

    /// Opens another Entity from the detail, over the one shown, which going back returns to.
    pub fn follow_link(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if let Some(shown) = self.explorer.selected().cloned()
            && shown != id
        {
            self.layout.trail.push(shown);
            self.moved(Direction::Forward, Scope::Detail);
        }
        self.explorer.select(id);
        self.reveal_selected();
        cx.notify();
    }

    /// Steps one back from the detail: to the detail a link was followed from, and from the
    /// first detail to the list in one column, or to no selection beside the list. Back on the
    /// list, the selected row stays in view and takes the focus so the arrow keys go on from
    /// it. An Entity a reload removed is passed over.
    pub fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(id) = self.layout.trail.pop() {
            if self
                .explorer
                .board()
                .is_some_and(|board| board.item(&id).is_some())
            {
                self.explorer.select(id);
                self.moved(Direction::Back, Scope::Detail);
                self.reveal_selected();
                cx.notify();
                return;
            }
        }
        if self.columns() == Columns::One {
            self.layout.detail_shown = false;
            self.moved(Direction::Back, Scope::Screen);
            self.reveal_selected();
            window.focus(&self.list_focus, cx);
        } else {
            self.explorer.deselect();
        }
        cx.notify();
    }

    /// Closes the detail, whatever lies under it.
    pub fn close_entity(&mut self, cx: &mut Context<Self>) {
        self.layout.trail.clear();
        self.explorer.deselect();
        cx.notify();
    }

    /// Escape: closes the panel, or else steps back from the detail on screen.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detail_on_screen = match self.columns() {
            Columns::One => self.is_detail_shown(),
            _ => self.explorer.detail().is_some(),
        };
        if self.is_panel_open() {
            self.close_panel(cx);
        } else if detail_on_screen {
            self.go_back(window, cx);
        }
    }

    fn moved(&mut self, direction: Direction, scope: Scope) {
        let ix = self.layout.motion.ix + 1;
        self.layout.motion = Motion {
            direction,
            scope,
            ix,
        };
    }

    /// Slides `element` in when the last move was of `scope` and has not played yet: from the
    /// right going forward, from the left going back, fading in. The system's reduced motion
    /// setting shows the end at once.
    fn moving(&self, scope: Scope, element: AnyElement) -> AnyElement {
        let motion = self.layout.motion;
        let Some(from) = motion.offset().filter(|_| motion.scope == scope) else {
            return element;
        };
        div()
            .id(ElementId::NamedInteger("moving".into(), motion.ix))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(element)
            .with_animation(
                ElementId::NamedInteger("move".into(), motion.ix),
                Animation::new(SHIFT_DURATION).with_easing(ease_out_quint()),
                move |element, delta| element.left(px(from * (1. - delta))).opacity(delta),
            )
            .into_any_element()
    }

    /// The roots and the filters: the left column, or with `panel` the panel over a narrower
    /// window, whose bar closes it where the menu opened it.
    fn render_sidebar(&self, panel: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
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
        let bar = bar(palette)
            .when_some(panel, |bar, _| {
                bar.child(
                    Button::new("close-panel")
                        .ghost()
                        .icon(IconName::Close)
                        .tooltip("閉じる")
                        .on_click(cx.listener(|this, _, _, cx| this.close_panel(cx))),
                )
            })
            .when(panel.is_none(), |bar| bar.pl_4())
            .child(bar_title("リポジトリ"));
        let unregister = self.selected().is_some().then(|| {
            Button::new("remove-root")
                .ghost()
                .compact()
                .label("登録を解除")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| this.remove_selected(cx)))
        });
        let mut body = div()
            .id("sidebar-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .pt_3()
            .pb_3()
            .child(
                Button::new("project-switch")
                    .outline()
                    .w_full()
                    .font_semibold()
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
                div()
                    .id("project-path")
                    // The body scrolls, so a line that hides its overflow must not shrink away.
                    .flex_none()
                    .px_1()
                    .text_xs()
                    .text_color(palette.muted)
                    .truncate()
                    .child(match self.selected() {
                        Some(root) => short_path(root.path()),
                        None => format!(
                            "登録の一覧の保存場所: {}",
                            short_path(self.lock.data().dir())
                        ),
                    })
                    .test_support(),
            )
            .children(unregister.map(|button| div().flex_none().flex().child(button)))
            .child(
                Button::new("add-root")
                    .ghost()
                    .w_full()
                    .label("＋ リポジトリを登録")
                    .loading(self.busy)
                    .disabled(self.busy || !loaded)
                    .on_click(cx.listener(|this, _, _, cx| this.add_root(cx))),
            );
        if loaded {
            body = body.child(self.render_filters(cx));
        }
        div()
            .id("sidebar")
            .w(px(panel.unwrap_or(SIDEBAR_WIDTH)))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.desk)
            .border_r_1()
            .border_color(palette.rule)
            .child(bar)
            .child(body)
            .test_support()
            .into_any_element()
    }

    /// The selected root's name, its state and its list.
    fn render_list_column(&self, columns: Columns, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        let title = self
            .selected()
            .map(ProjectRoot::name)
            .unwrap_or_else(|| "Axon".into());
        let (status, action) = self.render_status(cx);
        let list = self.render_list(cx);
        // Without the left column, the menu opens the panel that holds the roots and the
        // filters. A dot on it tells the filters differ from the defaults, since the list then
        // hides some Entities.
        let menu = (columns != Columns::Three).then(|| {
            let filtered = self.explorer.filter() != &Filter::default();
            div()
                .flex_none()
                .relative()
                .child(
                    Button::new("open-panel")
                        .ghost()
                        .icon(IconName::Menu)
                        .tooltip(if filtered {
                            "リポジトリと絞り込み（絞り込みを変更しています）"
                        } else {
                            "リポジトリと絞り込み"
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.open_panel(window, cx))),
                )
                .when(filtered, |menu| {
                    menu.child(
                        div()
                            .id("filter-changed")
                            .absolute()
                            .top(px(3.))
                            .right(px(3.))
                            .size(px(7.))
                            .rounded_full()
                            .bg(palette.signal)
                            .test_support(),
                    )
                })
        });
        let notice = self.notice.as_ref().map(|notice| {
            div()
                .id("notice")
                .pl_2()
                .border_l_2()
                .border_color(palette.danger)
                .text_sm()
                .text_color(palette.danger)
                .child(notice.clone())
                .test_support()
        });
        let bar = bar(palette)
            .when(menu.is_none(), |bar| bar.pl_4())
            .children(menu)
            .child(
                div()
                    .id("project-title")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(bar_title(title)),
            );
        div()
            .id("project-pane")
            .flex_1()
            .when(columns != Columns::One, |pane| {
                pane.min_w(px(LIST_MIN_WIDTH))
            })
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .child(
                        div()
                            .px_1()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_1()
                            // Long errors scroll here instead of pushing the list out of a
                            // small window.
                            .child(
                                div()
                                    .id("project-status-area")
                                    .flex_none()
                                    .max_h(px(STATUS_MAX_HEIGHT))
                                    .overflow_y_scroll()
                                    .text_color(palette.muted)
                                    .child(status),
                            )
                            .children(notice)
                            .children(action),
                    )
                    .children(list),
            )
            .test_support()
            .into_any_element()
    }

    /// The bar over the detail: the step back on the left, then what the Entity is.
    fn render_detail_bar(&self, columns: Columns, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        let mut bar = bar(palette).id("detail-bar");
        let Some(detail) = self.explorer.detail() else {
            return bar.into_any_element();
        };
        let (icon, label) = if !self.layout.trail.is_empty() {
            (IconName::ChevronLeft, "戻る")
        } else if columns == Columns::One {
            (IconName::ChevronLeft, "一覧")
        } else {
            (IconName::Close, "閉じる")
        };
        bar = bar.child(
            Button::new("close-detail")
                .ghost()
                .icon(icon)
                .label(label)
                .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
        );
        if let Ok(detail) = detail {
            bar = bar.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .pl_1()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(text::kind(detail.kind))
                    .child(detail.label.name())
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(palette.ink)
                            .truncate()
                            .child(detail.id.to_string()),
                    ),
            );
        }
        bar.test_support().into_any_element()
    }

    /// The detail of the selected Entity, why it cannot be shown, or the empty column.
    fn render_detail_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        match self.explorer.detail() {
            Some(Ok(detail)) => self.render_detail(detail, cx),
            Some(Err(error)) => div()
                .id("detail-error")
                .px_6()
                .py_5()
                .text_sm()
                .text_color(palette.danger)
                .child(format!("詳細を表示できません: {error}"))
                .into_any_element(),
            // Nothing is open: the column stays empty, and points at the list while it has rows.
            None => div()
                .id("detail-empty")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .text_sm()
                .text_color(palette.muted)
                .when(self.explorer.listing().matched > 0, |empty| {
                    empty.child("一覧から Issue・Group を選ぶと、ここに詳細を表示します。")
                })
                .test_support()
                .into_any_element(),
        }
    }

    fn render_detail_column(&self, columns: Columns, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        let body = self.render_detail_body(cx);
        div()
            .id("detail-pane")
            .flex_1()
            .when(columns != Columns::One, |pane| {
                pane.min_w(px(DETAIL_MIN_WIDTH))
                    .border_l_1()
                    .border_color(palette.rule)
            })
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(palette.sheet)
            .child(self.render_detail_bar(columns, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.moving(Scope::Detail, body)),
            )
            .test_support()
            .into_any_element()
    }

    /// The panel over the list, on a scrim over the whole window whose part beside the panel
    /// closes it. The panel slides in as the scrim darkens, and both reverse as it closes.
    fn render_panel(&self, window_width: f32, cx: &mut Context<Self>) -> AnyElement {
        // The panel leaves a strip of the window to click back to it.
        let width = SIDEBAR_PANEL_WIDTH.min(window_width - 56.);
        let panel = &self.layout.panel;
        let opening = panel.open;
        let animation = if opening {
            Animation::new(PANEL_OPEN_DURATION).with_easing(ease_out_quint())
        } else {
            Animation::new(PANEL_CLOSE_DURATION).with_easing(quadratic)
        };
        let shown = move |delta: f32| if opening { delta } else { 1. - delta };
        let dim = if cx.theme().is_dark() { 0.5 } else { 0.28 };
        // The scrim dims the whole window under the panel, so the dimming follows the panel in
        // step instead of showing first beside it.
        div()
            .id("panel-layer")
            .absolute()
            .inset_0()
            .occlude()
            .child(
                div()
                    .id("panel-scrim")
                    .absolute()
                    .inset_0()
                    .bg(gpui_kit::black().opacity(dim))
                    .on_click(cx.listener(|this, _, _, cx| this.close_panel(cx)))
                    .test_support()
                    .with_animation(
                        ElementId::NamedInteger("panel-scrim".into(), panel.moves),
                        animation.clone(),
                        move |scrim, delta| scrim.opacity(shown(delta)),
                    ),
            )
            .child(
                div()
                    .id("panel")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    // A click on the panel does not reach the scrim under it, which closes.
                    .occlude()
                    .child(self.render_sidebar(Some(width), cx))
                    .with_animation(
                        ElementId::NamedInteger("panel".into(), panel.moves),
                        animation,
                        move |sidebar, delta| sidebar.left(px(-width * (1. - shown(delta)))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for AxonApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::of(cx);
        let width: f32 = window.viewport_size().width.into();
        let columns = Columns::for_width(width, self.layout.columns);
        self.layout.columns = Some(columns);
        if self.explorer.detail().is_none() {
            // A later selection by the arrow keys leaves one column on the list, with nothing
            // under it.
            self.layout.detail_shown = false;
            self.layout.trail.clear();
        }
        if columns == Columns::Three {
            self.layout.panel.open = false;
            self.layout.panel.closing = false;
        }
        let one_detail = columns == Columns::One && self.is_detail_shown();
        let sidebar = (columns == Columns::Three).then(|| self.render_sidebar(None, cx));
        let list = (!one_detail).then(|| self.render_list_column(columns, cx));
        let detail =
            (columns != Columns::One || one_detail).then(|| self.render_detail_column(columns, cx));
        let screens: Vec<AnyElement> = if columns == Columns::One {
            let screen = detail.or(list).expect("one of the two is shown");
            vec![self.moving(Scope::Screen, screen)]
        } else {
            list.into_iter().chain(detail).collect()
        };
        let panel = (self.layout.panel.open || self.layout.panel.closing)
            .then(|| self.render_panel(width, cx));
        div()
            .id("axon-app")
            .key_context(APP_CONTEXT)
            .track_focus(&self.app_focus)
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .bg(palette.paper)
            .text_color(palette.ink)
            .children(sidebar)
            .children(screens)
            .children(panel)
    }
}

/// A path short enough for one line of the left column: the home directory as `~`, and only
/// the last two directories of a deeper path. The menu of roots shows each path in full.
fn short_path(path: &std::path::Path) -> String {
    let home = dirs::home_dir();
    let (prefix, rest) = match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => ("~/", rest),
        None => ("/", path.strip_prefix("/").unwrap_or(path)),
    };
    let parts: Vec<_> = rest.iter().map(|part| part.to_string_lossy()).collect();
    if parts.len() > 2 {
        format!("{prefix}…/{}", parts[parts.len() - 2..].join("/"))
    } else {
        format!("{prefix}{}", parts.join("/"))
    }
}

/// The bar at the top of a column.
fn bar(palette: Palette) -> Div {
    div()
        .flex_none()
        .h(px(BAR_HEIGHT))
        .px_2()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(palette.rule)
}

/// The name of a column in its bar.
fn bar_title(title: impl Into<gpui_kit::SharedString>) -> Div {
    div()
        .text_sm()
        .font_semibold()
        .truncate()
        .child(title.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_path_keeps_its_last_two_directories() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            short_path(&home.join("src/github.com/axon")),
            "~/…/github.com/axon"
        );
        assert_eq!(short_path(&home.join("notes")), "~/notes");
        assert_eq!(short_path(std::path::Path::new("/srv/a/b/c")), "/…/b/c");
        assert_eq!(short_path(std::path::Path::new("/srv/a")), "/srv/a");
    }

    #[test]
    fn a_window_gains_a_column_at_the_threshold_and_keeps_it_a_little_below() {
        use Columns::*;
        assert_eq!(Columns::for_width(THREE_COLUMNS_MIN_WIDTH, None), Three);
        assert_eq!(Columns::for_width(THREE_COLUMNS_MIN_WIDTH - 1., None), Two);
        assert_eq!(Columns::for_width(TWO_COLUMNS_MIN_WIDTH - 1., None), One);
        // Widening gains a column only at the threshold.
        assert_eq!(
            Columns::for_width(THREE_COLUMNS_MIN_WIDTH - 1., Some(Two)),
            Two
        );
        assert_eq!(
            Columns::for_width(TWO_COLUMNS_MIN_WIDTH - 1., Some(One)),
            One
        );
        assert_eq!(
            Columns::for_width(THREE_COLUMNS_MIN_WIDTH, Some(One)),
            Three
        );
        // Narrowing keeps it until the hysteresis below.
        let keep = THREE_COLUMNS_MIN_WIDTH - COLUMN_HYSTERESIS;
        assert_eq!(Columns::for_width(keep, Some(Three)), Three);
        assert_eq!(Columns::for_width(keep - 1., Some(Three)), Two);
        let keep = TWO_COLUMNS_MIN_WIDTH - COLUMN_HYSTERESIS;
        assert_eq!(Columns::for_width(keep, Some(Two)), Two);
        assert_eq!(Columns::for_width(keep - 1., Some(Two)), One);
        assert_eq!(Columns::for_width(keep - 1., Some(Three)), One);
    }
}
