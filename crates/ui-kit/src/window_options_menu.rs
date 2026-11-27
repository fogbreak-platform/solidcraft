//! Clean-room window options menu for secondary/pop-out windows.

use egui::{Context, Id, Response, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardHeaders {
    Always,
    Hover,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopBar {
    Always,
    Edge,
}

#[derive(Clone, Copy, Debug)]
pub struct DisplayChoice<'a> {
    pub id: usize,
    pub name: &'a str,
    pub current: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WindowText {
    pub preview: String,
    pub show_here: String,
    pub window: String,
    pub keep_on_top: String,
    pub ribbon: String,
    pub auto_hide: String,
    pub auto_hide_unavailable: String,
    pub other_panels: String,
    pub workspace_panels: String,
    pub top_bar: String,
    pub always: String,
    pub edge: String,
    pub card_headers: String,
    pub on_hover: String,
    pub hidden: String,
    pub move_to_display: String,
    pub interface_size: String,
    pub rename: String,
    pub use_default: String,
    pub return_views: String,
    pub close_window: String,
}

pub struct WindowMenu<'a> {
    pub text: &'a WindowText,
    pub title: &'a str,
    pub displays: &'a [DisplayChoice<'a>],
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
    pub interface_sizes: &'a [u16],
    pub interface_size: u16,
    pub panels: bool,
    pub top_bar: TopBar,
    pub ribbon: bool,
    pub ribbon_auto_hide: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CardHeaders(CardHeaders),
    KeepOnTop(bool),
    InterfaceSize(u16),
    Panels(bool),
    TopBar(TopBar),
    Ribbon(bool),
    RibbonAutoHide(bool),
    MoveToDisplay(usize),
    UseAsDefault,
    ReturnViews,
    Close,
    Rename,
}

pub fn is_open(ctx: &Context, id: Id) -> bool {
    egui::Popup::is_id_open(ctx, id.with("menu"))
}

pub fn show(ui: &mut Ui, anchor: Response, context_open: bool, menu: &WindowMenu<'_>) -> Option<Action> {
    let popup_id = anchor.id.with("menu");
    if anchor.clicked() || context_open {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
    }
    let mut picked = None;
    egui::Popup::menu(&anchor)
        .id(popup_id)
        .frame(crate::menu::frame())
        .show(|ui| {
            crate::menu::heading(ui, menu.title);
            if crate::menu::button(ui, &menu.text.rename, "rename").clicked() {
                picked = Some(Action::Rename);
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.keep_on_top, if menu.keep_on_top { "check" } else { "" }).clicked() {
                picked = Some(Action::KeepOnTop(!menu.keep_on_top));
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.workspace_panels, if menu.panels { "check" } else { "" }).clicked() {
                picked = Some(Action::Panels(!menu.panels));
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.ribbon, if menu.ribbon { "check" } else { "" }).clicked() {
                picked = Some(Action::Ribbon(!menu.ribbon));
                ui.close();
            }
            if crate::menu::button_enabled(ui, &menu.text.auto_hide, if menu.ribbon_auto_hide { "check" } else { "" }, menu.ribbon).clicked() {
                picked = Some(Action::RibbonAutoHide(!menu.ribbon_auto_hide));
                ui.close();
            }
            crate::menu::separator(ui);
            crate::menu::heading(ui, &menu.text.top_bar);
            if crate::menu::button(ui, &menu.text.always, if menu.top_bar == TopBar::Always { "check" } else { "" }).clicked() {
                picked = Some(Action::TopBar(TopBar::Always));
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.edge, if menu.top_bar == TopBar::Edge { "check" } else { "" }).clicked() {
                picked = Some(Action::TopBar(TopBar::Edge));
                ui.close();
            }
            crate::menu::separator(ui);
            crate::menu::heading(ui, &menu.text.card_headers);
            for (choice, label) in [
                (CardHeaders::Always, &menu.text.always),
                (CardHeaders::Hover, &menu.text.on_hover),
                (CardHeaders::Hidden, &menu.text.hidden),
            ] {
                if crate::menu::button(ui, label, if menu.card_headers == choice { "check" } else { "" }).clicked() {
                    picked = Some(Action::CardHeaders(choice));
                    ui.close();
                }
            }
            if !menu.interface_sizes.is_empty() {
                crate::menu::separator(ui);
                crate::menu::heading(ui, &menu.text.interface_size);
                for &pct in menu.interface_sizes {
                    let label = format!("{pct}%");
                    if crate::menu::button(ui, &label, if menu.interface_size == pct { "check" } else { "" }).clicked() {
                        picked = Some(Action::InterfaceSize(pct));
                        ui.close();
                    }
                }
            }
            if menu.displays.len() > 1 {
                crate::menu::separator(ui);
                crate::menu::heading(ui, &menu.text.move_to_display);
                for d in menu.displays {
                    if crate::menu::button_enabled(ui, d.name, if d.current { "check" } else { "" }, !d.current).clicked() {
                        picked = Some(Action::MoveToDisplay(d.id));
                        ui.close();
                    }
                }
            }
            crate::menu::separator(ui);
            if crate::menu::button(ui, &menu.text.use_default, "").clicked() {
                picked = Some(Action::UseAsDefault);
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.return_views, "").clicked() {
                picked = Some(Action::ReturnViews);
                ui.close();
            }
            if crate::menu::button(ui, &menu.text.close_window, "close").clicked() {
                picked = Some(Action::Close);
                ui.close();
            }
        });
    picked
}
