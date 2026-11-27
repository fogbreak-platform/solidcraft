//! Clean-room window/view layout menu model types.

use egui::Color32;

#[derive(Clone, Debug, Default)]
pub struct LayoutText {
    pub heading: String,
    pub summary_single: String,
    pub summary: String,
    pub this_window: String,
    pub show: String,
    pub bring: String,
    pub windows: String,
    pub link: String,
    pub off: String,
    pub orientation: String,
    pub zoom: String,
    pub both: String,
    pub new_cells: String,
    pub detach: String,
    pub equal: String,
    pub reset: String,
    pub clean: String,
    pub save: String,
    pub saved: String,
    pub manage: String,
}

#[derive(Clone, Debug)]
pub struct LayoutView {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub accent: Color32,
}

#[derive(Clone, Debug)]
pub struct LayoutWindow {
    pub id: u32,
    pub name: String,
    pub display: String,
    pub views: Vec<LayoutView>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutMenu {
    pub text: LayoutText,
    pub saved: Vec<String>,
    pub matching_saved: Option<String>,
    pub design: String,
    pub window: u32,
    pub native: bool,
    pub visible: usize,
    pub link: String,
    pub can_link: bool,
    pub clean: bool,
    pub windows: Vec<LayoutWindow>,
}
