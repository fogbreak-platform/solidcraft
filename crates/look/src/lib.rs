//! Clean-room MIT OR Apache-2.0 Fogbreak look shim.
#![forbid(unsafe_code)]

pub fn install(ctx: &egui::Context) {
    solidcraft_theme::apply(ctx);
}

pub fn install_tables() {}
