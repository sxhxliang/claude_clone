//! Color palette for the Claude clone. Plain `Hsla` helpers (not tied to the
//! `gpui_component` theme) so the example reads close to the reference design.
use gpui::{Hsla, hsla};

pub fn accent() -> Hsla {
    hsla(15.0 / 360.0, 0.55, 0.52, 1.0)
}
pub fn bg_color() -> Hsla {
    hsla(45.0 / 360.0, 0.18, 0.95, 1.0)
}
pub fn sidebar_bg() -> Hsla {
    hsla(40.0 / 360.0, 0.13, 0.93, 1.0)
}
pub fn border_color() -> Hsla {
    hsla(40.0 / 360.0, 0.12, 0.85, 1.0)
}
pub fn text_color() -> Hsla {
    hsla(40.0 / 360.0, 0.05, 0.10, 1.0)
}
pub fn text_2() -> Hsla {
    hsla(40.0 / 360.0, 0.05, 0.42, 1.0)
}
pub fn text_3() -> Hsla {
    hsla(40.0 / 360.0, 0.05, 0.62, 1.0)
}
pub fn green() -> Hsla {
    hsla(150.0 / 360.0, 0.5, 0.48, 1.0)
}
pub fn white_color() -> Hsla {
    hsla(0.0, 0.0, 1.0, 1.0)
}
pub fn hover_bg() -> Hsla {
    hsla(0.0, 0.0, 0.0, 0.06)
}

/// Hover background for menu rows and generic list items.
pub fn hover_surface() -> Hsla {
    hsla(45.0 / 360.0, 0.18, 0.96, 1.0)
}
/// Hover background for suggestion pills.
pub fn pill_hover_bg() -> Hsla {
    hsla(45.0 / 360.0, 0.18, 0.92, 1.0)
}
/// Hover background for the "get set up" rows.
pub fn setup_row_hover_bg() -> Hsla {
    hsla(45.0 / 360.0, 0.18, 0.97, 1.0)
}
/// Background of the active conversation row in the sidebar recents list.
pub fn recent_active_bg() -> Hsla {
    hsla(45.0 / 360.0, 0.18, 0.88, 1.0)
}
/// Background of a user message bubble in the chat view.
pub fn user_bubble_bg() -> Hsla {
    hsla(40.0 / 360.0, 0.18, 0.91, 1.0)
}
/// Vertical connector line in tool-call timelines.
pub fn timeline_line() -> Hsla {
    hsla(40.0 / 360.0, 0.10, 0.78, 1.0)
}
/// Background of an inline file chip.
pub fn file_chip_bg() -> Hsla {
    hsla(40.0 / 360.0, 0.08, 0.94, 1.0)
}
/// Hover background of an inline file chip.
pub fn file_chip_hover_bg() -> Hsla {
    hsla(40.0 / 360.0, 0.10, 0.90, 1.0)
}
