//! Keyboard navigation helpers for dense editable tables (Tab/Shift+Tab/Up/Down).
use egui::{Key, Modifiers, Response, Ui};

/// Logical cell movement requested from a focused table cell editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKey {
    Down,
    Up,
    Next,
    Previous,
}

#[derive(Clone)]
struct PendingLanding {
    row: String,
    col: String,
}

fn pending_id(table: egui::Id) -> egui::Id {
    table.with("sc_pending_cell_landing")
}

/// Schedules keyboard focus to land in `(row, col)` on the next frame.
pub fn land_in_cell(ctx: &egui::Context, table: egui::Id, row: &str, col: &str) {
    ctx.data_mut(|d| {
        d.insert_temp(pending_id(table), PendingLanding { row: row.to_owned(), col: col.to_owned() });
    });
    ctx.request_repaint();
}

/// Returns true while a pending keyboard landing targets `row`, keeping virtualized rows live.
pub fn landing_in_row(ctx: &egui::Context, table: egui::Id, row: &str) -> bool {
    ctx.data(|d| d.get_temp::<PendingLanding>(pending_id(table))).is_some_and(|p| p.row == row)
}

/// Applies a pending keyboard landing onto `(row, col)` by focusing `response` and selecting `text`.
/// Returns true on the landing pass so the caller ignores navigation keys from the departing cell.
pub fn landing(ui: &mut Ui, table: egui::Id, row: &str, col: &str, response: &Response, text: &str) -> bool {
    let key = pending_id(table);
    let matches = ui.ctx().data(|d| d.get_temp::<PendingLanding>(key)).is_some_and(|p| p.row == row && p.col == col);
    if !matches {
        return false;
    }
    ui.ctx().data_mut(|d| d.remove::<PendingLanding>(key));
    response.request_focus();
    let count = text.chars().count();
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
        let range = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(count));
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), response.id);
    }
    true
}

/// Keeps Tab/Shift+Tab inside the focused table cell so the caller can direct inter-cell movement.
pub fn keep_tab_in_cell(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(response.id, egui::EventFilter { tab: true, ..Default::default() });
        });
    }
}

/// Consumes arrow/tab cell navigation keys when the cell editor is active.
pub fn cell_key(ui: &mut Ui, active: bool) -> Option<CellKey> {
    if !active {
        return None;
    }
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::SHIFT, Key::Tab) {
            Some(CellKey::Previous)
        } else if i.consume_key(Modifiers::NONE, Key::Tab) {
            Some(CellKey::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(CellKey::Down)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(CellKey::Up)
        } else {
            None
        }
    })
}
