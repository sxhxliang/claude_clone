//! Theme state and the semantic palette used by the Claude clone.
//!
//! The application predates gpui-component's theme system and has many small
//! rendering helpers with no `App` argument.  The active semantic palette is
//! therefore kept in a small, lock-protected process-local snapshot.  Theme
//! changes replace that snapshot and refresh all windows; renders remain
//! allocation-free and keep the existing call sites simple.

use std::{
    path::PathBuf,
    sync::{OnceLock, RwLock},
};

use gpui::{BorrowAppContext as _, Hsla, hsla};
use gpui_component::{ActiveTheme as _, Colorize as _, Theme, ThemeMode, ThemeRegistry};
use serde::{Deserialize, Serialize};

pub const CLAUDE_THEME_NAME: &str = "Claude Clone";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ThemeSelection {
    #[default]
    Preset,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BackgroundScope {
    #[default]
    Window,
    Chat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BackgroundFit {
    Fill,
    Contain,
    #[default]
    Cover,
    ScaleDown,
    Original,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BackgroundSettings {
    pub(crate) asset: Option<String>,
    pub(crate) scope: BackgroundScope,
    pub(crate) fit: BackgroundFit,
    pub(crate) opacity: f32,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            asset: None,
            scope: BackgroundScope::Window,
            fit: BackgroundFit::Cover,
            opacity: 0.25,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ThemeColorOverrides {
    pub(crate) accent: Option<Hsla>,
    pub(crate) background: Option<Hsla>,
    pub(crate) sidebar: Option<Hsla>,
    pub(crate) surface: Option<Hsla>,
    pub(crate) text: Option<Hsla>,
    pub(crate) text_secondary: Option<Hsla>,
    pub(crate) text_muted: Option<Hsla>,
    pub(crate) border: Option<Hsla>,
    pub(crate) success: Option<Hsla>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ThemeSettings {
    pub(crate) selection: ThemeSelection,
    pub(crate) preset: String,
    pub(crate) custom_base: String,
    pub(crate) custom: ThemeColorOverrides,
    pub(crate) background: BackgroundSettings,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            selection: ThemeSelection::Preset,
            preset: CLAUDE_THEME_NAME.to_string(),
            custom_base: CLAUDE_THEME_NAME.to_string(),
            custom: ThemeColorOverrides::default(),
            background: BackgroundSettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Palette {
    accent: Hsla,
    background: Hsla,
    sidebar: Hsla,
    border: Hsla,
    text: Hsla,
    text_secondary: Hsla,
    text_muted: Hsla,
    success: Hsla,
    surface: Hsla,
}

impl Palette {
    fn claude() -> Self {
        Self {
            accent: hsla(15.0 / 360.0, 0.55, 0.52, 1.0),
            background: hsla(45.0 / 360.0, 0.18, 0.95, 1.0),
            sidebar: hsla(40.0 / 360.0, 0.13, 0.93, 1.0),
            border: hsla(40.0 / 360.0, 0.12, 0.85, 1.0),
            text: hsla(40.0 / 360.0, 0.05, 0.10, 1.0),
            text_secondary: hsla(40.0 / 360.0, 0.05, 0.42, 1.0),
            text_muted: hsla(40.0 / 360.0, 0.05, 0.62, 1.0),
            success: hsla(150.0 / 360.0, 0.5, 0.48, 1.0),
            surface: hsla(0.0, 0.0, 1.0, 1.0),
        }
    }

    fn from_component(theme: &Theme) -> Self {
        Self {
            accent: theme.primary,
            background: theme.background,
            sidebar: theme.sidebar,
            border: theme.border,
            text: theme.foreground,
            text_secondary: theme.secondary_foreground,
            text_muted: theme.muted_foreground,
            success: theme.success,
            surface: theme.group_box,
        }
    }

    fn apply_overrides(mut self, overrides: &ThemeColorOverrides) -> Self {
        if let Some(color) = overrides.accent {
            self.accent = color;
        }
        if let Some(color) = overrides.background {
            self.background = color;
        }
        if let Some(color) = overrides.sidebar {
            self.sidebar = color;
        }
        if let Some(color) = overrides.surface {
            self.surface = color;
        }
        if let Some(color) = overrides.text {
            self.text = color;
        }
        if let Some(color) = overrides.text_secondary {
            self.text_secondary = color;
        }
        if let Some(color) = overrides.text_muted {
            self.text_muted = color;
        }
        if let Some(color) = overrides.border {
            self.border = color;
        }
        if let Some(color) = overrides.success {
            self.success = color;
        }
        self
    }
}

static ACTIVE_PALETTE: OnceLock<RwLock<Palette>> = OnceLock::new();
static ACTIVE_BACKGROUND: OnceLock<RwLock<BackgroundSettings>> = OnceLock::new();

fn palette() -> Palette {
    *ACTIVE_PALETTE
        .get_or_init(|| RwLock::new(Palette::claude()))
        .read()
        .expect("theme palette lock poisoned")
}

fn set_palette(value: Palette) {
    *ACTIVE_PALETTE
        .get_or_init(|| RwLock::new(Palette::claude()))
        .write()
        .expect("theme palette lock poisoned") = value;
}

fn set_background(value: BackgroundSettings) {
    *ACTIVE_BACKGROUND
        .get_or_init(|| RwLock::new(BackgroundSettings::default()))
        .write()
        .expect("theme background lock poisoned") = value;
}

fn background() -> BackgroundSettings {
    ACTIVE_BACKGROUND
        .get_or_init(|| RwLock::new(BackgroundSettings::default()))
        .read()
        .expect("theme background lock poisoned")
        .clone()
}

pub(crate) fn apply(settings: &ThemeSettings, cx: &mut gpui::App) {
    set_background(settings.background.clone());
    let registry = ThemeRegistry::global(cx);
    let (theme_name, overrides) = match settings.selection {
        ThemeSelection::Preset => (&settings.preset, None),
        ThemeSelection::Custom => (&settings.custom_base, Some(&settings.custom)),
    };

    if theme_name == CLAUDE_THEME_NAME {
        Theme::change(ThemeMode::Light, None, cx);
        let mut palette = Palette::claude();
        if let Some(overrides) = overrides {
            palette = palette.apply_overrides(overrides);
        }
        set_palette(palette);
        apply_component_palette(&palette, cx);
        return;
    }

    let Some(config) = registry.themes().get(theme_name.as_str()).cloned() else {
        Theme::change(ThemeMode::Light, None, cx);
        let mut palette = Palette::claude();
        if let Some(overrides) = overrides {
            palette = palette.apply_overrides(overrides);
        }
        set_palette(palette);
        apply_component_palette(&palette, cx);
        return;
    };

    cx.update_global::<Theme, _>(|theme, _| theme.apply_config(&config));
    let mut palette = Palette::from_component(cx.theme());
    if let Some(overrides) = overrides {
        palette = palette.apply_overrides(overrides);
    }
    set_palette(palette);
    apply_component_palette(&palette, cx);
}

fn apply_component_palette(palette: &Palette, cx: &mut gpui::App) {
    cx.update_global::<Theme, _>(|theme, _| {
        theme.primary = palette.accent;
        theme.primary_hover = palette.accent.opacity(0.88);
        theme.primary_active = palette.accent.opacity(0.76);
        theme.primary_foreground = palette.surface;
        theme.accent = palette.accent;
        theme.background = palette.background;
        theme.sidebar = palette.sidebar;
        theme.sidebar_foreground = palette.text;
        theme.border = palette.border;
        theme.sidebar_border = palette.border;
        theme.title_bar = palette.background;
        theme.title_bar_border = palette.border;
        theme.foreground = palette.text;
        theme.group_box = palette.surface;
        theme.group_box_foreground = palette.text;
        theme.popover = palette.surface;
        theme.popover_foreground = palette.text;
        theme.muted_foreground = palette.text_muted;
        theme.secondary_foreground = palette.text_secondary;
        theme.input = palette.surface;
        theme.ring = palette.accent;
        theme.success = palette.success;
    });
}

pub(crate) fn palette_from_component(cx: &gpui::App) -> ThemeColorOverrides {
    let theme = cx.theme();
    ThemeColorOverrides {
        accent: Some(theme.primary),
        background: Some(theme.background),
        sidebar: Some(theme.sidebar),
        surface: Some(theme.group_box),
        text: Some(theme.foreground),
        text_secondary: Some(theme.secondary_foreground),
        text_muted: Some(theme.muted_foreground),
        border: Some(theme.border),
        success: Some(theme.success),
    }
}

pub(crate) fn asset_path(asset: &str) -> Option<PathBuf> {
    crate::store::theme_assets_dir().map(|dir| dir.join(asset))
}

pub fn accent() -> Hsla {
    palette().accent
}
pub fn bg_color() -> Hsla {
    palette().background
}
pub fn sidebar_bg() -> Hsla {
    palette().sidebar
}
pub fn border_color() -> Hsla {
    palette().border
}
pub fn text_color() -> Hsla {
    palette().text
}
pub fn text_2() -> Hsla {
    palette().text_secondary
}
pub fn text_3() -> Hsla {
    palette().text_muted
}
pub fn green() -> Hsla {
    palette().success
}
pub fn white_color() -> Hsla {
    palette().surface
}
pub fn chat_bg_color() -> Hsla {
    let background = background();
    if background.asset.is_some() && background.opacity > 0.0 {
        palette()
            .background
            .opacity(0.9 - background.opacity.clamp(0.0, 1.0) * 0.3)
    } else {
        palette().background
    }
}
pub fn chat_surface_color() -> Hsla {
    let background = background();
    if background.asset.is_some() && background.opacity > 0.0 {
        palette()
            .surface
            .opacity(0.96 - background.opacity.clamp(0.0, 1.0) * 0.16)
    } else {
        palette().surface
    }
}
pub fn hover_bg() -> Hsla {
    palette().text.opacity(0.06)
}
pub fn hover_surface() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.35)
}
pub fn pill_hover_bg() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.65)
}
pub fn setup_row_hover_bg() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.2)
}
pub fn recent_active_bg() -> Hsla {
    palette()
        .background
        .mix_oklab(palette().accent.opacity(0.18), 0.22)
}
pub fn user_bubble_bg() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.45)
}
pub fn timeline_line() -> Hsla {
    palette().border.opacity(0.75)
}
pub fn file_chip_bg() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.18)
}
pub fn file_chip_hover_bg() -> Hsla {
    palette().background.mix_oklab(palette().surface, 0.42)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_defaults_preserve_the_claude_theme() {
        let settings = ThemeSettings::default();
        assert_eq!(settings.selection, ThemeSelection::Preset);
        assert_eq!(settings.preset, CLAUDE_THEME_NAME);
        assert_eq!(settings.background.scope, BackgroundScope::Window);
        assert_eq!(settings.background.fit, BackgroundFit::Cover);
        assert_eq!(settings.background.opacity, 0.25);
        assert!(settings.background.asset.is_none());
    }

    #[test]
    fn missing_appearance_fields_deserialize_with_defaults() {
        let settings: ThemeSettings = serde_json::from_str("{}").expect("valid settings");
        assert_eq!(settings.preset, CLAUDE_THEME_NAME);
        assert!(settings.custom.accent.is_none());
        assert!(settings.background.asset.is_none());
    }

    #[test]
    fn semantic_overrides_only_replace_selected_colors() {
        let base = Palette::claude();
        let replacement = hsla(210.0 / 360.0, 0.8, 0.5, 1.0);
        let overrides = ThemeColorOverrides {
            accent: Some(replacement),
            ..ThemeColorOverrides::default()
        };
        let result = base.apply_overrides(&overrides);
        assert_eq!(result.accent, replacement);
        assert_eq!(result.background, base.background);
        assert_eq!(result.text, base.text);
    }

    #[test]
    fn appearance_round_trips_through_json() {
        let mut settings = ThemeSettings::default();
        settings.selection = ThemeSelection::Custom;
        settings.custom.accent = Some(hsla(0.5, 0.5, 0.5, 1.0));
        settings.background.asset = Some("background.png".to_string());
        settings.background.scope = BackgroundScope::Chat;
        settings.background.fit = BackgroundFit::Contain;
        settings.background.opacity = 0.6;

        let json = serde_json::to_string(&settings).expect("serialize appearance");
        let restored: ThemeSettings = serde_json::from_str(&json).expect("deserialize appearance");
        assert_eq!(restored.selection, ThemeSelection::Custom);
        assert_eq!(restored.background.asset.as_deref(), Some("background.png"));
        assert_eq!(restored.background.scope, BackgroundScope::Chat);
        assert_eq!(restored.background.fit, BackgroundFit::Contain);
        let restored_accent = restored.custom.accent.expect("restored accent");
        let original_accent = settings.custom.accent.expect("original accent");
        assert!((restored_accent.h - original_accent.h).abs() < 0.01);
        assert!((restored_accent.s - original_accent.s).abs() < 0.01);
        assert!((restored_accent.l - original_accent.l).abs() < 0.01);
    }
}
