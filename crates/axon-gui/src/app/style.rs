//! The window's palette. Records are read on cool grey paper in dark ink; the only saturated
//! colors are the states, so a glance at the list tells how the work stands. The palette
//! follows the system appearance and is written into the component theme, so inputs, buttons
//! and checkboxes share it.

use crate::board::State;
use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::{App, Hsla, Window, rgb, rgba};

/// The colors of one appearance.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The list, where most of the reading happens.
    pub paper: Hsla,
    /// The left column, a shade deeper than the paper.
    pub desk: Hsla,
    /// The detail pane, a shade lighter than the paper.
    pub sheet: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    /// Hairlines between columns, tree guides and the lifecycle track.
    pub rule: Hsla,
    /// Links, the selected row and keyboard focus.
    pub signal: Hsla,
    /// The selected row's background.
    pub signal_wash: Hsla,
    pub hover: Hsla,
    pub danger: Hsla,
    pub caution: Hsla,
    states: [Hsla; 6],
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        if cx.theme().is_dark() { DARK } else { LIGHT }.resolve()
    }

    /// The color a state is drawn in wherever it appears.
    pub fn state(&self, state: State) -> Hsla {
        let ix = State::ALL
            .iter()
            .position(|s| *s == state)
            .expect("every state is listed");
        self.states[ix]
    }
}

struct Spec {
    paper: u32,
    desk: u32,
    sheet: u32,
    ink: u32,
    muted: u32,
    rule: u32,
    signal: u32,
    signal_wash: u32,
    hover: u32,
    danger: u32,
    caution: u32,
    /// In the order of [`State::ALL`].
    states: [u32; 6],
}

impl Spec {
    fn resolve(&self) -> Palette {
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        let a = |hex: u32| -> Hsla { rgba(hex).into() };
        Palette {
            paper: c(self.paper),
            desk: c(self.desk),
            sheet: c(self.sheet),
            ink: c(self.ink),
            muted: c(self.muted),
            rule: c(self.rule),
            signal: c(self.signal),
            signal_wash: a(self.signal_wash),
            hover: a(self.hover),
            danger: c(self.danger),
            caution: c(self.caution),
            states: self.states.map(c),
        }
    }
}

const LIGHT: Spec = Spec {
    paper: 0xEEF1F3,
    desk: 0xE3E8EC,
    sheet: 0xF8F9FA,
    ink: 0x18232F,
    muted: 0x5B6977,
    rule: 0xCCD4DB,
    signal: 0x2D5DA8,
    signal_wash: 0x2D5DA81F,
    hover: 0x18232F0D,
    danger: 0xB8352F,
    caution: 0x9A6510,
    // 未判断, 未着手, 着手中, 完了, 取りやめ, 衝突
    states: [0x7F64AE, 0x4F6E8E, 0xC27410, 0x3B8551, 0x8A9098, 0xC8383A],
};

const DARK: Spec = Spec {
    paper: 0x161A1F,
    desk: 0x101317,
    sheet: 0x1C2127,
    ink: 0xDCE3EA,
    muted: 0x8693A1,
    rule: 0x2C343D,
    signal: 0x84AEEC,
    signal_wash: 0x84AEEC24,
    hover: 0xDCE3EA0F,
    danger: 0xF0807A,
    caution: 0xE8B25C,
    states: [0xB5A1E0, 0x8FAACA, 0xF0A944, 0x76C28C, 0x6F7883, 0xF27474],
};

/// Follows the system appearance and lays the palette over the component theme.
pub fn sync_appearance(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    let p = Palette::of(cx);
    let dark = cx.theme().is_dark();
    Theme::update(cx, |theme| {
        theme.radius = gpui_kit::px(5.);
        theme.shadow = false;
        let t = &mut theme.colors;
        t.background = p.paper;
        t.foreground = p.ink;
        t.border = p.rule;
        t.input = p.rule;
        t.muted = p.desk;
        t.muted_foreground = p.muted;
        t.ring = p.signal;
        t.caret = p.signal;
        t.selection = p.signal_wash.opacity(if dark { 2.5 } else { 2. });
        t.link = p.signal;
        t.link_hover = p.ink;
        t.link_active = p.ink;
        t.primary = p.ink;
        t.primary_hover = p.ink.opacity(0.88);
        t.primary_active = p.ink;
        t.primary_foreground = p.paper;
        t.button_primary = p.ink;
        t.button_primary_hover = p.ink.opacity(0.88);
        t.button_primary_active = p.ink;
        t.button_primary_foreground = p.paper;
        t.secondary = p.desk;
        t.secondary_hover = p.hover;
        t.secondary_active = p.rule;
        t.secondary_foreground = p.ink;
        t.accent = p.hover;
        t.accent_foreground = p.ink;
        t.list = p.paper;
        t.list_hover = p.hover;
        t.list_active = p.signal_wash;
        t.list_active_border = p.signal;
        t.popover = p.sheet;
        t.popover_foreground = p.ink;
        t.danger = p.danger;
        t.warning = p.caution;
        t.scrollbar_thumb = p.muted.opacity(0.45);
        t.scrollbar_thumb_hover = p.muted.opacity(0.7);
    });
}
