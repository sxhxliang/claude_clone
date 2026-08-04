//! Color palette for the Claude clone. Plain `Hsla` helpers (not tied to the
//! `gpui_component` theme) so the example reads close to the reference design.
//!
//! Every helper resolves against the active theme mode. The resolved light/dark
//! flag lives in a process global instead of being threaded through `cx`, which
//! keeps the palette call sites zero-argument. Rendering happens on the main
//! thread, so a relaxed atomic is enough.
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, Hsla, Window, hsla};
use gpui_component::{Theme, ThemeMode};

use crate::models::AppThemeMode;

static DARK: AtomicBool = AtomicBool::new(false);

/// Switch the palette between its light and dark variants. Callers are
/// responsible for refreshing windows afterwards.
pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

/// Whether the palette currently resolves to its dark variant.
pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// Resolve `mode` (following the OS appearance for [`AppThemeMode::System`]),
/// then apply it to both this palette and the `gpui_component` theme so the
/// hand-rolled colors and the component library stay in step. Repaints every
/// window, which is what makes the settings window and the main window flip
/// together.
pub fn apply_mode(mode: AppThemeMode, window: Option<&mut Window>, cx: &mut App) {
    let resolved = match mode {
        AppThemeMode::Light => ThemeMode::Light,
        AppThemeMode::Dark => ThemeMode::Dark,
        // Prefer `window.appearance()` over the app-level value; on Linux the
        // latter can fail before a window exists.
        AppThemeMode::System => window
            .as_ref()
            .map(|window| window.appearance())
            .unwrap_or_else(|| cx.window_appearance())
            .into(),
    };

    set_dark(resolved.is_dark());
    Theme::change(resolved, window, cx);
    cx.refresh_windows();
}

/// Resolve a light/dark pair against the active mode. Exposed so modules with
/// their own local colors can stay in step with the shared palette.
pub fn pick(light: Hsla, dark: Hsla) -> Hsla {
    if is_dark() { dark } else { light }
}

pub fn accent() -> Hsla {
    pick(
        hsla(15.0 / 360.0, 0.55, 0.52, 1.0),
        hsla(15.0 / 360.0, 0.58, 0.585, 1.0),
    )
}
pub fn bg_color() -> Hsla {
    pick(
        hsla(45.0 / 360.0, 0.18, 0.95, 1.0),
        hsla(40.0 / 360.0, 0.055, 0.125, 1.0),
    )
}
pub fn sidebar_bg() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.13, 0.93, 1.0),
        hsla(40.0 / 360.0, 0.06, 0.095, 1.0),
    )
}
pub fn border_color() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.12, 0.85, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.255, 1.0),
    )
}
pub fn text_color() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.05, 0.10, 1.0),
        hsla(40.0 / 360.0, 0.06, 0.93, 1.0),
    )
}
pub fn text_2() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.05, 0.42, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.70, 1.0),
    )
}
pub fn text_3() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.05, 0.62, 1.0),
        hsla(40.0 / 360.0, 0.04, 0.52, 1.0),
    )
}
pub fn green() -> Hsla {
    pick(
        hsla(150.0 / 360.0, 0.5, 0.48, 1.0),
        hsla(150.0 / 360.0, 0.45, 0.56, 1.0),
    )
}

/// Pure white, for content that sits on top of the accent color, on media
/// lightboxes, or inside illustrations. Deliberately *not* theme-aware — use
/// [`surface`] for panels that should darken with the theme.
pub fn white_color() -> Hsla {
    hsla(0.0, 0.0, 1.0, 1.0)
}

/// An elevated surface raised above [`bg_color`]: message bubbles, the composer,
/// search fields, selected chips and floating controls.
pub fn surface() -> Hsla {
    pick(
        hsla(0.0, 0.0, 1.0, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.175, 1.0),
    )
}

pub fn hover_bg() -> Hsla {
    pick(hsla(0.0, 0.0, 0.0, 0.06), hsla(0.0, 0.0, 1.0, 0.08))
}

/// Hover background for menu rows and generic list items.
pub fn hover_surface() -> Hsla {
    pick(
        hsla(45.0 / 360.0, 0.18, 0.96, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.205, 1.0),
    )
}
/// Hover background for suggestion pills.
pub fn pill_hover_bg() -> Hsla {
    pick(
        hsla(45.0 / 360.0, 0.18, 0.92, 1.0),
        hsla(40.0 / 360.0, 0.055, 0.225, 1.0),
    )
}
/// Hover background for the "get set up" rows.
pub fn setup_row_hover_bg() -> Hsla {
    pick(
        hsla(45.0 / 360.0, 0.18, 0.97, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.195, 1.0),
    )
}
/// Background of the active conversation row in the sidebar recents list.
pub fn recent_active_bg() -> Hsla {
    pick(
        hsla(45.0 / 360.0, 0.18, 0.88, 1.0),
        hsla(40.0 / 360.0, 0.06, 0.245, 1.0),
    )
}
/// Background of a user message bubble in the chat view.
pub fn user_bubble_bg() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.18, 0.91, 1.0),
        hsla(40.0 / 360.0, 0.06, 0.215, 1.0),
    )
}
/// Vertical connector line in tool-call timelines.
pub fn timeline_line() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.10, 0.78, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.32, 1.0),
    )
}
/// Background of an inline file chip.
pub fn file_chip_bg() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.08, 0.94, 1.0),
        hsla(40.0 / 360.0, 0.05, 0.20, 1.0),
    )
}
/// Hover background of an inline file chip.
pub fn file_chip_hover_bg() -> Hsla {
    pick(
        hsla(40.0 / 360.0, 0.10, 0.90, 1.0),
        hsla(40.0 / 360.0, 0.055, 0.25, 1.0),
    )
}
