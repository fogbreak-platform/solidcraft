//! Clean-room MIT OR Apache-2.0 Fogbreak egui widget shim.
#![forbid(unsafe_code)]

use egui::{Response, RichText, Ui};
use solidcraft_theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Normal,
    Small,
    Primary,
}

pub fn button(ui: &mut Ui, label: &str, kind: Button) -> Response {
    let t = Tokens::get();
    let mut btn = egui::Button::new(RichText::new(label).color(match kind {
        Button::Primary => egui::Color32::WHITE,
        _ => t.text,
    }));
    if kind == Button::Primary {
        btn = btn.fill(t.accent);
    }
    ui.add(btn)
}
