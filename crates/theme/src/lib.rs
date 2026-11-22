//! Clean-room MIT OR Apache-2.0 Fogbreak theme tokens (Carmel Stone / Monterey Shale).
#![forbid(unsafe_code)]

use std::cell::Cell;

use egui::{Color32, Context, Stroke, Visuals};
use serde::{Deserialize, Serialize};
use solidcraft_color::AnalysisColors;

pub const CARMEL_STONE: Color32 = Color32::from_rgb(0xF7, 0xFA, 0xFC);
pub const MONTEREY_SHALE: Color32 = Color32::from_rgb(0x11, 0x18, 0x21);
pub const FOGBREAK_TEAL: Color32 = Color32::from_rgb(0x08, 0x7F, 0x9C);
pub const PANEL_LIGHT: Color32 = Color32::WHITE;
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x1A, 0x24, 0x31);
pub const BORDER_LIGHT: Color32 = Color32::from_rgb(0xCB, 0xD5, 0xE1);
pub const BORDER_DARK: Color32 = Color32::from_rgb(0x2B, 0x39, 0x4B);

thread_local! {
    static DARK: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorVision {
    #[default]
    Off,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccentRamp {
    Primary,
    Sketch,
    Solid,
    Render,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAccent {
    Model,
    Sketch,
    Assembly,
    Drawing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub bg: Color32,
    pub panel: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub section_cap: Color32,
    pub analysis: AnalysisColors,
}

impl Tokens {
    pub fn light() -> Self {
        Self {
            bg: CARMEL_STONE,
            panel: PANEL_LIGHT,
            border: BORDER_LIGHT,
            text: MONTEREY_SHALE,
            text_dim: Color32::from_rgb(0x47, 0x55, 0x69),
            accent: FOGBREAK_TEAL,
            viewport_top: CARMEL_STONE,
            viewport_bottom: Color32::from_rgb(0xE2, 0xE8, 0xF0),
            section_cap: FOGBREAK_TEAL,
            analysis: AnalysisColors::default(),
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: MONTEREY_SHALE,
            panel: PANEL_DARK,
            border: BORDER_DARK,
            text: CARMEL_STONE,
            text_dim: Color32::from_rgb(0x94, 0xA3, 0xB8),
            accent: FOGBREAK_TEAL,
            viewport_top: MONTEREY_SHALE,
            viewport_bottom: Color32::from_rgb(0x1A, 0x24, 0x31),
            section_cap: FOGBREAK_TEAL,
            analysis: AnalysisColors::default(),
        }
    }

    pub fn get() -> Self {
        if is_dark() { Self::dark() } else { Self::light() }
    }
}

pub fn is_dark() -> bool {
    DARK.with(Cell::get)
}

pub fn set_dark(dark: bool) {
    DARK.with(|d| d.set(dark));
}

pub fn apply(ctx: &Context) {
    let dark = is_dark();
    let t = Tokens::get();
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.window_fill = t.panel;
    visuals.panel_fill = t.bg;
    visuals.faint_bg_color = t.bg;
    visuals.extreme_bg_color = t.bg;
    visuals.selection.bg_fill = t.accent;
    visuals.selection.stroke = Stroke::new(1.0, t.text);
    visuals.widgets.noninteractive.bg_fill = t.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    visuals.widgets.inactive.bg_fill = t.panel;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    visuals.widgets.active.bg_fill = t.accent;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = t.accent;
    ctx.set_visuals(visuals);
}
