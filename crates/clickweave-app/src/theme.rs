use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Vec2, Visuals};
use egui_snarl::ui::SnarlStyle;

// =============================================================================
// Color Palette (n8n-inspired dark theme)
// =============================================================================

// Background colors
pub const BG_DARK: Color32 = Color32::from_rgb(26, 26, 26); // #1a1a1a - main canvas
pub const BG_PANEL: Color32 = Color32::from_rgb(36, 36, 36); // #242424 - panels
pub const BG_HEADER: Color32 = Color32::from_rgb(45, 45, 45); // #2d2d2d - headers
pub const BG_HOVER: Color32 = Color32::from_rgb(55, 55, 55); // hover state
pub const BG_ACTIVE: Color32 = Color32::from_rgb(65, 65, 65); // active state

// Accent colors
pub const ACCENT_CORAL: Color32 = Color32::from_rgb(255, 109, 90); // #ff6d5a - primary buttons
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(80, 200, 120); // success/active
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(76, 158, 232); // info/links

// Text colors
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(240, 240, 240); // #f0f0f0
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 160, 160); // #a0a0a0
pub const TEXT_MUTED: Color32 = Color32::from_rgb(100, 100, 100); // #646464

// Border colors
pub const BORDER_DARK: Color32 = Color32::from_rgb(50, 50, 50); // #323232
pub const BORDER_LIGHT: Color32 = Color32::from_rgb(70, 70, 70); // #464646

// Grid
pub const GRID_COLOR: Color32 = Color32::from_rgb(40, 40, 40); // subtle grid

// Node type colors
pub const NODE_START: Color32 = Color32::from_rgb(80, 200, 120); // green
pub const NODE_STEP: Color32 = Color32::from_rgb(76, 158, 232); // blue
pub const NODE_END: Color32 = Color32::from_rgb(220, 90, 90); // red

// =============================================================================
// Theme Application
// =============================================================================

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Start with dark visuals
    style.visuals = Visuals::dark();

    // Override background colors
    style.visuals.window_fill = BG_PANEL;
    style.visuals.panel_fill = BG_PANEL;
    style.visuals.extreme_bg_color = BG_DARK;
    style.visuals.faint_bg_color = Color32::from_rgb(40, 40, 40);
    style.visuals.code_bg_color = BG_DARK;

    // Hyperlinks
    style.visuals.hyperlink_color = ACCENT_BLUE;

    // Widget colors
    style.visuals.widgets.noninteractive.bg_fill = BG_PANEL;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_DARK);

    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 50);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_DARK);

    style.visuals.widgets.hovered.bg_fill = BG_HOVER;
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_LIGHT);

    style.visuals.widgets.active.bg_fill = BG_ACTIVE;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_CORAL);

    style.visuals.widgets.open.bg_fill = BG_ACTIVE;
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    // Selection
    style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(255, 109, 90, 60);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT_CORAL);

    // Window styling
    style.visuals.window_corner_radius = CornerRadius::same(8);
    style.visuals.window_shadow = Shadow::NONE;
    style.visuals.popup_shadow = Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(60),
    };

    // Spacing
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.window_margin = Margin::same(12);

    ctx.set_style(style);
}

// =============================================================================
// Panel Frames
// =============================================================================

pub fn sidebar_frame() -> Frame {
    Frame {
        fill: BG_PANEL,
        stroke: Stroke::new(1.0, BORDER_DARK),
        inner_margin: Margin::same(0),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::ZERO,
        shadow: Shadow::NONE,
    }
}

pub fn header_frame() -> Frame {
    Frame {
        fill: BG_HEADER,
        stroke: Stroke::new(1.0, BORDER_DARK),
        inner_margin: Margin {
            left: 16,
            right: 16,
            top: 32,
            bottom: 10,
        },
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::ZERO,
        shadow: Shadow::NONE,
    }
}

pub fn floating_toolbar_frame() -> Frame {
    Frame {
        fill: BG_PANEL,
        stroke: Stroke::new(1.0, BORDER_LIGHT),
        inner_margin: Margin::symmetric(16, 10),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::same(12),
        shadow: Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(80),
        },
    }
}

pub fn inspector_frame() -> Frame {
    Frame {
        fill: BG_PANEL,
        stroke: Stroke::new(1.0, BORDER_DARK),
        inner_margin: Margin::same(12),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::ZERO,
        shadow: Shadow::NONE,
    }
}

pub fn logs_drawer_frame() -> Frame {
    Frame {
        fill: BG_DARK,
        stroke: Stroke::new(1.0, BORDER_DARK),
        inner_margin: Margin::same(12),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius {
            nw: 12,
            ne: 12,
            sw: 0,
            se: 0,
        },
        shadow: Shadow {
            offset: [0, -4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(60),
        },
    }
}

// =============================================================================
// Snarl Graph Style
// =============================================================================

pub fn create_snarl_style() -> SnarlStyle {
    SnarlStyle::new()
}
