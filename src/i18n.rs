use gpui::SharedString;

use crate::models::ChatMode;

pub(crate) const DEFAULT_LOCALE: &str = "en";
pub(crate) const EN_LOCALE: &str = "en";
pub(crate) const ZH_CN_LOCALE: &str = "zh-CN";

pub(crate) fn normalize_locale(locale: &str) -> &'static str {
    match locale
        .trim()
        .replace('_', "-")
        .to_ascii_lowercase()
        .as_str()
    {
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" => ZH_CN_LOCALE,
        _ => EN_LOCALE,
    }
}

pub(crate) fn set_locale(locale: &str) -> SharedString {
    let locale = normalize_locale(locale);
    rust_i18n::set_locale(locale);
    locale.into()
}

pub(crate) fn current_locale() -> &'static str {
    normalize_locale(&rust_i18n::locale())
}

pub(crate) fn language_name(locale: &str) -> SharedString {
    match normalize_locale(locale) {
        ZH_CN_LOCALE => crate::tr!("settings.language.zh_cn"),
        _ => crate::tr!("settings.language.en"),
    }
}

pub(crate) fn chat_mode_label(mode: ChatMode) -> SharedString {
    match mode {
        ChatMode::Chat => crate::tr!("chat_mode.chat"),
        ChatMode::Cowork => crate::tr!("chat_mode.cowork"),
        ChatMode::Code => crate::tr!("chat_mode.code"),
    }
}
