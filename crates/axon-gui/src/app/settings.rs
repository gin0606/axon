use super::{AxonApp, style::Palette};
use crate::{
    Language,
    i18n::SystemLanguage,
    project::InstanceLock,
    settings::{LanguagePreference, LoadError, SettingsFile},
};
use gpui_kit::base::{FocusTrapElement, StyledExt, TestSupportExt};
use gpui_kit::component::{
    Selectable,
    button::{Button, ButtonVariants},
};
use gpui_kit::{AnyElement, App, Context, FocusHandle, Window, div, prelude::*, px};
use std::sync::Arc;

pub(super) struct State {
    pub open: bool,
    file: SettingsFile,
    system: Language,
    preference: LanguagePreference,
    load_error: Option<LoadError>,
    generation: u64,
    save_error: Option<String>,
    previous_focus: Option<FocusHandle>,
    pub(super) focus: FocusHandle,
}

impl State {
    pub fn new(lock: Arc<InstanceLock>, cx: &mut App) -> Self {
        let file = SettingsFile::new(lock);
        let loaded = if cx.has_global::<crate::settings::Initial>() {
            cx.remove_global::<crate::settings::Initial>().0
        } else {
            file.load()
        };
        let preference = loaded.as_ref().copied().unwrap_or_default();
        Self {
            open: false,
            file,
            system: cx.global::<SystemLanguage>().0,
            preference,
            load_error: loaded.err(),
            generation: 0,
            save_error: None,
            previous_focus: None,
            focus: cx.focus_handle(),
        }
    }

    pub fn language(&self) -> Language {
        self.preference.resolve(self.system)
    }
}

impl AxonApp {
    pub(super) fn settings_button(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        Button::new(id)
            .ghost()
            .icon(gpui_kit::component::IconName::Settings)
            .tooltip(crate::tr!(self.language, "settings.title"))
            .accessibility_label(crate::tr!(self.language, "settings.title"))
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
    }

    pub fn language_preference(&self) -> LanguagePreference {
        self.settings.preference
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_settings_focus(window, cx);
        if window.focused(cx).is_none() {
            self.recover_lost_focus(window, cx);
        }
        let previous = window.focused(cx);
        let menu_origin = self.close_copy_menu(cx).and_then(|(focus, origin)| {
            previous
                .as_ref()
                .is_some_and(|previous| focus.contains(previous, window))
                .then_some(origin)
        });
        if !self.settings.open {
            self.settings.previous_focus = match menu_origin {
                Some(super::layout::FocusArea::Detail) => Some(self.detail_focus.clone()),
                Some(super::layout::FocusArea::List) => Some(self.list_focus.clone()),
                _ => previous,
            };
        }
        self.settings.open = true;
        window.focus(&self.settings.focus, cx);
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.open = false;
        self.restore_settings_focus(window, cx);
        cx.notify();
    }

    /// External links and background notifications can close settings without a Window.
    pub(super) fn restore_settings_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.open {
            return;
        }
        let previous = self.settings.previous_focus.take();
        if self.settings.focus.is_focused(window)
            || self.settings.focus.contains_focused(window, cx)
        {
            if let Some(previous) = previous.filter(|focus| self.app_focus.contains(focus, window))
            {
                window.focus(&previous, cx);
            } else {
                window.blur(cx);
                self.recover_lost_focus(window, cx);
            }
        }
    }

    /// Keeps input, selection, filter and scroll handles, updating only localized values.
    fn apply_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.language = self.settings.language();
        crate::i18n::init(self.language, cx);
        cx.set_menus(crate::menus(self.language));
        self.search.update(cx, |search, cx| {
            search.set_placeholder(crate::tr!(self.language, "search.placeholder"), window, cx);
        });
        self.close_copy_menu(cx);
        cx.notify();
    }

    pub fn set_language_preference(
        &mut self,
        preference: LanguagePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if preference == self.settings.preference {
            return;
        }
        self.settings.preference = preference;
        self.apply_language(window, cx);
        // A damaged file is replaced only by the separate, explicitly labelled save action.
        if self.settings.load_error.is_none() {
            self.save_settings(cx);
        }
    }

    pub fn reload_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let loaded = self.settings.file.load();
        self.settings.preference = loaded.as_ref().copied().unwrap_or_default();
        self.settings.load_error = loaded.err();
        self.settings.save_error = None;
        self.apply_language(window, cx);
    }

    pub fn save_settings(&mut self, cx: &mut Context<Self>) {
        if matches!(self.settings.load_error, Some(LoadError::Unreadable(_))) {
            return;
        }
        self.settings.load_error = None;
        self.settings.generation += 1;
        // Finish before the next quit event so failures remain visible and can be retried.
        self.settings.save_error = self
            .settings
            .file
            .save(self.settings.generation, self.settings.preference)
            .err()
            .map(|error| error.to_string());
        cx.notify();
    }

    pub(super) fn settings_warning(&self) -> bool {
        self.settings.load_error.is_some() || self.settings.save_error.is_some()
    }

    pub(super) fn render_settings_warning(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.settings_warning().then(|| {
            div()
                .id("settings-warning")
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .border_t_1()
                .border_color(Palette::of(cx).rule)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(Palette::of(cx).danger)
                        .child(crate::tr!(self.language, "settings.warning")),
                )
                .child(
                    Button::new("warning-settings")
                        .outline()
                        .label(crate::tr!(self.language, "settings.title"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                        ),
                )
                .test_support()
                .into_any_element()
        })
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::of(cx);
        let language = self.language;
        let mut body = div()
            .id("settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(crate::tr!(language, "settings.language")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(palette.muted)
                    .child(crate::tr!(language, "settings.explanation")),
            );
        for (id, preference, label) in [
            (
                "language-system",
                LanguagePreference::System,
                crate::tr!(language, "settings.system"),
            ),
            (
                "language-ja",
                LanguagePreference::Japanese,
                "日本語".to_owned(),
            ),
            (
                "language-en",
                LanguagePreference::English,
                "English".to_owned(),
            ),
        ] {
            body = body.child(
                Button::new(id)
                    .outline()
                    .w_full()
                    .label(label)
                    .selected(self.settings.preference == preference)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_language_preference(preference, window, cx)
                    })),
            );
        }
        if let Some(error) = &self.settings.load_error {
            body = body
                .child(
                    div()
                        .id("settings-load-error")
                        .text_sm()
                        .text_color(palette.danger)
                        .child(crate::tr!(language, "settings.load_failed"))
                        .test_support(),
                )
                .child(div().text_xs().child(error.to_string()))
                .child(
                    Button::new("reload-settings")
                        .outline()
                        .label(crate::tr!(language, "common.reload"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.reload_settings(window, cx)),
                        ),
                );
            if matches!(error, LoadError::Invalid(_)) {
                body = body
                    .child(
                        div()
                            .text_sm()
                            .child(crate::tr!(language, "settings.replace_explanation")),
                    )
                    .child(
                        Button::new("replace-settings")
                            .outline()
                            .label(crate::tr!(language, "settings.replace"))
                            .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                    );
            } else {
                body = body.child(
                    div()
                        .text_sm()
                        .child(crate::tr!(language, "settings.unreadable")),
                );
            }
        }
        if let Some(error) = &self.settings.save_error {
            body = body
                .child(
                    div()
                        .id("settings-save-error")
                        .text_sm()
                        .text_color(palette.danger)
                        .child(crate::tr!(language, "settings.save_failed"))
                        .test_support(),
                )
                .child(div().text_xs().child(error.clone()))
                .child(
                    Button::new("retry-settings")
                        .outline()
                        .label(crate::tr!(language, "settings.retry"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                );
        }
        div()
            .focus_trap("settings", &self.settings.focus)
            // Keep the selection behind settings, without copying its hidden text.
            .on_action(|_: &gpui_kit::base::input::Copy, _, _| {})
            .absolute()
            .inset_0()
            .occlude()
            .bg(palette.paper)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(super::BAR_HEIGHT))
                    .flex_none()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(palette.rule)
                    .child(
                        Button::new("close-settings")
                            .ghost()
                            .label(crate::tr!(language, "common.back"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                            ),
                    )
                    .child(
                        div()
                            .font_semibold()
                            .child(crate::tr!(language, "settings.title")),
                    ),
            )
            .child(body)
            .test_support()
            .into_any_element()
    }
}
