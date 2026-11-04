//! The startup language and explicitly localized application text.

use gpui_kit::{App, Global};
use std::sync::Once;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    English,
    Japanese,
}

impl Global for Language {}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Japanese => "ja",
        }
    }

    pub fn system() -> Self {
        Self::from_preferences(sys_locale::get_locales())
    }

    /// The first supported language, ignoring its region, script and ASCII case.
    pub fn from_preferences(locales: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        locales
            .into_iter()
            .find_map(|locale| {
                let primary = locale.as_ref().split(['-', '_']).next()?;
                if primary.eq_ignore_ascii_case("ja") {
                    Some(Self::Japanese)
                } else if primary.eq_ignore_ascii_case("en") {
                    Some(Self::English)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn of(cx: &App) -> Self {
        *cx.global::<Self>()
    }

    pub fn text(self, key: &str) -> String {
        rust_i18n::t!(key, locale = self.code()).into_owned()
    }
}

/// Component translations use rust-i18n's process-wide locale. Application text always
/// supplies its own locale; callers changing the component locale in tests must be isolated.
pub fn init(language: Language, cx: &mut App) {
    static EXTEND: Once = Once::new();
    EXTEND.call_once(|| {
        use gpui_kit::component as gpui_component;
        rust_i18n::extend!(gpui_component);
    });
    gpui_kit::component::set_locale(language.code());
    cx.set_global(language);
}

macro_rules! tr {
    ($language:expr, $key:expr $(, $name:ident = $value:expr)* $(,)?) => {
        rust_i18n::t!($key, locale = $language.code() $(, $name = $value)*).into_owned()
    };
}

pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn preference_order_regions_and_unknown_languages() {
        for (locales, expected) in [
            (vec!["ja-JP", "en-US"], Language::Japanese),
            (vec!["en-GB", "ja-JP"], Language::English),
            (vec!["fr-FR", "JA_jp", "en"], Language::Japanese),
            (vec!["de", "EN-au", "ja"], Language::English),
            (vec!["javanese", "eng", "fr"], Language::English),
            (vec!["", "ja"], Language::Japanese),
            (vec![], Language::English),
        ] {
            assert_eq!(Language::from_preferences(locales), expected);
        }
    }

    fn arguments(text: &str) -> BTreeSet<&str> {
        text.split("%{")
            .skip(1)
            .map(|rest| rest.split_once('}').expect("closed interpolation").0)
            .collect()
    }

    #[test]
    fn catalogs_have_both_languages_and_matching_arguments() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../locales/ui.json")).unwrap();
        for (key, locales) in catalog.as_object().unwrap() {
            if key == "_version" {
                continue;
            }
            let locales = locales.as_object().expect("translations for each key");
            assert_eq!(locales.len(), 2, "{key}");
            let english = locales["en"].as_str().expect("English translation");
            let japanese = locales["ja"].as_str().expect("Japanese translation");
            assert!(!english.is_empty() && !japanese.is_empty(), "{key}");
            assert_eq!(arguments(english), arguments(japanese), "{key}");
            assert_eq!(Language::English.text(key), english, "{key}");
            assert_eq!(Language::Japanese.text(key), japanese, "{key}");
        }
    }
}
