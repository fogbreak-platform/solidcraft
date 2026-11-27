//! Shared editable dense data grid built atop [`Table`].
use super::{
    layout::{Text, text},
    table::{Column, Table, table_custom},
    table_cell_nav::{CellKey, cell_key, keep_tab_in_cell, land_in_cell, landing, landing_in_row},
};
use egui::{Response, Ui};
use solidcraft_theme::Tokens;
use std::borrow::Cow;

/// Editing behavior for one cell/column in a [`Grid`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellKind {
    Custom,
    Text,
    Expression,
    ReadOnly,
    Choice(Vec<String>),
}

/// Column definition for [`Grid`].
#[derive(Clone, Debug)]
pub struct GridColumn {
    pub id: String,
    pub label: String,
    pub width: f32,
    pub kind: CellKind,
    pub min_content_width: f32,
    pub flexible: bool,
    pub edit_on_click: bool,
    pub icon_only: bool,
}

impl GridColumn {
    pub fn new(id: impl Into<String>, label: impl Into<String>, width: f32, kind: CellKind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            width,
            kind,
            min_content_width: 0.0,
            flexible: false,
            edit_on_click: false,
            icon_only: false,
        }
    }

    pub fn min_content_width(mut self, width: f32) -> Self {
        self.min_content_width = width;
        self
    }

    pub fn flex(mut self) -> Self {
        self.flexible = true;
        self
    }

    pub fn edit_on_click(mut self) -> Self {
        self.edit_on_click = true;
        self
    }

    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }
}

/// Inline cell editor widget result returned by [`GridSource::editor`].
pub struct CellEditor {
    pub response: Response,
    pub taken: bool,
}

/// Row identity for a committed [`GridEdit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowRef<'a> {
    New(usize),
    Existing(&'a str),
}

/// One validated cell commit produced during a frame of [`Grid::show_with`].
#[derive(Clone, Debug)]
pub struct GridEdit {
    existing: Option<String>,
    new_index: usize,
    col: usize,
    text: String,
}

impl GridEdit {
    pub fn existing(row_key: impl Into<String>, col: usize, text: impl Into<String>) -> Self {
        Self { existing: Some(row_key.into()), new_index: 0, col, text: text.into() }
    }

    pub fn new_row(index: usize, col: usize, text: impl Into<String>) -> Self {
        Self { existing: None, new_index: index, col, text: text.into() }
    }

    pub fn row(&self) -> RowRef<'_> {
        match &self.existing {
            Some(key) => RowRef::Existing(key),
            None => RowRef::New(self.new_index),
        }
    }

    pub fn col(&self) -> usize {
        self.col
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Per-frame interactions emitted by [`Grid::show_with`].
#[derive(Clone, Debug, Default)]
pub struct GridOutput {
    pub add_row: bool,
    pub editing: Option<(String, usize, String)>,
    pub cancelled: Option<(String, usize)>,
    pub refused: Option<String>,
    pub notice: Option<String>,
    pub edits: Vec<GridEdit>,
}

/// Application model backing a [`Grid`].
pub trait GridSource {
    fn columns(&self) -> Vec<GridColumn>;
    fn row_count(&self) -> usize;
    fn row_key(&self, row: usize) -> Cow<'_, str>;
    fn text(&self, row: usize, col: usize) -> Cow<'_, str>;

    fn kind(&self, _row: usize, _col: usize) -> Option<CellKind> {
        None
    }

    fn message(&self, _row: usize, _col: usize) -> Option<(Cow<'_, str>, bool)> {
        None
    }

    fn set_cell(&mut self, _row: usize, _col: usize, _text: &str) -> Result<(), String> {
        Ok(())
    }

    fn add_row(&self) -> Option<Cow<'_, str>> {
        None
    }

    fn header(&mut self, _ui: &mut Ui, _col: usize) -> bool {
        false
    }

    fn draw(&mut self, _ui: &mut Ui, _row: usize, _col: usize) -> Option<Response> {
        None
    }

    fn editor(&mut self, ui: &mut Ui, _row: usize, _col: usize, id: egui::Id, text: &mut String) -> CellEditor {
        CellEditor { response: super::cell_editor(ui, id, text, "", false), taken: false }
    }

    fn drawn(&mut self, _ui: &Ui, _row: usize, _col: usize, _response: &Response, _editing: bool) {}
}

#[derive(Clone, Default)]
struct ActiveEdit {
    row_key: String,
    col: usize,
    draft: String,
    needs_focus: bool,
}

#[derive(Clone, Default)]
struct GridMemory {
    active: Option<ActiveEdit>,
    refused: Option<String>,
}

/// Builder configuring a dense editable [`Grid`].
pub struct Grid<'a> {
    id: &'a str,
    state_id: Option<egui::Id>,
    empty_label: &'a str,
    resize_id: Option<egui::Id>,
}

impl<'a> Grid<'a> {
    pub fn new(id: &'a str) -> Self {
        Self { id, state_id: None, empty_label: "", resize_id: None }
    }

    pub fn state(mut self, id: egui::Id) -> Self {
        self.state_id = Some(id);
        self
    }

    pub fn empty(mut self, empty: &'a str) -> Self {
        self.empty_label = empty;
        self
    }

    pub fn resizable(mut self, id: egui::Id) -> Self {
        self.resize_id = Some(id);
        self
    }

    /// Clears persisted active cell edit / refusal state for `id`.
    pub fn forget(ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|d| d.remove::<GridMemory>(id));
    }

    /// Records a refusal message displayed / returned by the grid on the next frame.
    pub fn refuse(ctx: &egui::Context, id: egui::Id, message: &str) {
        ctx.data_mut(|d| {
            let mut mem = d.get_temp::<GridMemory>(id).unwrap_or_default();
            mem.refused = Some(message.to_owned());
            d.insert_temp(id, mem);
        });
        ctx.request_repaint();
    }

    pub fn show_with<S: GridSource>(
        self,
        ui: &mut Ui,
        source: &mut S,
        extra: impl FnOnce(&mut Ui, &mut Table<'_>, &mut S),
    ) -> GridOutput {
        let mut out = GridOutput::default();
        let grid_id = self.state_id.unwrap_or_else(|| ui.make_persistent_id(("sc_grid", self.id)));
        let nav_id = grid_id.with("nav");
        let cols = source.columns();
        let mut mem = ui.ctx().data(|d| d.get_temp::<GridMemory>(grid_id)).unwrap_or_default();
        if let Some(msg) = mem.refused.take() {
            out.refused = Some(msg);
        }

        let col_defs: Vec<Column<'_>> = cols
            .iter()
            .map(|c| {
                let mut col = Column::new(&c.label, c.width).min_content_width(c.min_content_width);
                if c.flexible {
                    col = col.flex();
                }
                if c.icon_only {
                    col = col.icon_only();
                }
                col
            })
            .collect();

        table_custom(ui, self.id, &col_defs, self.empty_label, |ui, table| {
            if let Some(resize_key) = self.resize_id {
                table.headers_resizable(ui, resize_key, |ui, col| source.header(ui, col));
            } else {
                table.headers(ui, |ui, col| source.header(ui, col));
            }

            let row_count = source.row_count();
            for row_idx in 0..row_count {
                let rkey = source.row_key(row_idx).into_owned();
                let is_row_editing = mem.active.as_ref().is_some_and(|a| a.row_key == rkey) || landing_in_row(ui.ctx(), nav_id, &rkey);
                table.row_visible(ui, &rkey, false, is_row_editing, |row_ui| {
                    for (col_idx, col_def) in cols.iter().enumerate() {
                        let kind = source.kind(row_idx, col_idx).unwrap_or_else(|| col_def.kind.clone());
                        let is_active = mem.active.as_ref().is_some_and(|a| a.row_key == rkey && a.col == col_idx);
                        row_ui.cell(|ui| {
                            if let Some(custom_resp) = source.draw(ui, row_idx, col_idx) {
                                source.drawn(ui, row_idx, col_idx, &custom_resp, false);
                                return;
                            }
                            let cell_text = source.text(row_idx, col_idx).into_owned();
                            let message = source.message(row_idx, col_idx);
                            let has_err = message.as_ref().is_some_and(|(_, err)| *err);
                            match &kind {
                                CellKind::Custom | CellKind::ReadOnly => {
                                    let resp = text(ui, &cell_text, if has_err { Text::Error(&cell_text) } else { Text::Cell });
                                    if let Some((tip, _)) = &message {
                                        resp.clone().on_hover_text(tip.as_ref());
                                    }
                                    source.drawn(ui, row_idx, col_idx, &resp, false);
                                }
                                CellKind::Choice(options) => {
                                    let opt_refs: Vec<&str> = options.iter().map(String::as_str).collect();
                                    let mut pos = opt_refs.iter().position(|o| *o == cell_text).unwrap_or(0);
                                    let before = pos;
                                    let resp = super::fields::dropdown(ui, &format!("{}_c_{}_{}", self.id, row_idx, col_idx), &mut pos, &opt_refs);
                                    if pos != before
                                        && let Some(chosen) = opt_refs.get(pos)
                                    {
                                        match source.set_cell(row_idx, col_idx, chosen) {
                                            Ok(()) => out.edits.push(GridEdit::existing(&rkey, col_idx, *chosen)),
                                            Err(err) => out.refused = Some(err),
                                        }
                                    }
                                    source.drawn(ui, row_idx, col_idx, &resp, false);
                                }
                                CellKind::Text | CellKind::Expression => {
                                    let editor_id = grid_id.with(("cell", &rkey, col_idx));
                                    let start_nav_landed = !is_active && landing_in_row(ui.ctx(), nav_id, &rkey);
                                    if is_active {
                                        let active = mem.active.as_mut().expect("active checked");
                                        let result = source.editor(ui, row_idx, col_idx, editor_id, &mut active.draft);
                                        let resp = result.response;
                                        if active.needs_focus {
                                            resp.request_focus();
                                            active.needs_focus = false;
                                        }
                                        let landed = landing(ui, nav_id, &rkey, &col_def.id, &resp, &active.draft);
                                        keep_tab_in_cell(ui, &resp);
                                        source.drawn(ui, row_idx, col_idx, &resp, true);

                                        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                                        let enter = !result.taken && resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                                        let nav = if result.taken || landed { None } else { cell_key(ui, resp.has_focus() || resp.lost_focus()) };

                                        if esc {
                                            out.cancelled = Some((rkey.clone(), col_idx));
                                            mem.active = None;
                                        } else if enter || nav.is_some() || resp.lost_focus() {
                                            let commit_text = active.draft.clone();
                                            if commit_text != cell_text {
                                                match source.set_cell(row_idx, col_idx, &commit_text) {
                                                    Ok(()) => out.edits.push(GridEdit::existing(&rkey, col_idx, commit_text)),
                                                    Err(err) => out.refused = Some(err),
                                                }
                                            }
                                            mem.active = None;
                                            if let Some(direction) = nav {
                                                let (next_row, next_col) = next_editable_cell(source, &cols, row_idx, col_idx, direction);
                                                if let (Some(nr), Some(nc)) = (next_row, next_col) {
                                                    let nrkey = source.row_key(nr).into_owned();
                                                    let nc_id = cols[nc].id.clone();
                                                    mem.active = Some(ActiveEdit {
                                                        row_key: nrkey.clone(),
                                                        col: nc,
                                                        draft: source.text(nr, nc).into_owned(),
                                                        needs_focus: true,
                                                    });
                                                    land_in_cell(ui.ctx(), nav_id, &nrkey, &nc_id);
                                                }
                                            }
                                        } else {
                                            out.editing = Some((rkey.clone(), col_idx, active.draft.clone()));
                                        }
                                    } else {
                                        let shown_text = if cell_text.is_empty() { " " } else { cell_text.as_str() };
                                        let (rect, click_resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
                                        let text_rect = rect.shrink2(egui::vec2(Tokens::get().space_4, 0.0));
                                        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
                                            text(ui, shown_text, if has_err { Text::Error(shown_text) } else { Text::Cell });
                                        });
                                        if let Some((tip, _)) = &message {
                                            click_resp.clone().on_hover_text(tip.as_ref());
                                        }
                                        if start_nav_landed && landing(ui, nav_id, &rkey, &col_def.id, &click_resp, &cell_text) {
                                            mem.active = Some(ActiveEdit { row_key: rkey.clone(), col: col_idx, draft: cell_text, needs_focus: true });
                                        } else if (col_def.edit_on_click && click_resp.clicked()) || click_resp.double_clicked() {
                                            mem.active = Some(ActiveEdit { row_key: rkey.clone(), col: col_idx, draft: cell_text, needs_focus: true });
                                            ui.ctx().request_repaint();
                                        }
                                        source.drawn(ui, row_idx, col_idx, &click_resp, false);
                                    }
                                }
                            }
                        });
                    }
                });
            }

            extra(ui, table, source);
            if let Some(add_label) = source.add_row()
                && table.add_prompt(ui, &add_label, &add_label).clicked()
            {
                out.add_row = true;
            }
        });

        ui.ctx().data_mut(|d| d.insert_temp(grid_id, mem));
        out
    }
}

fn is_editable_kind(kind: &CellKind) -> bool {
    matches!(kind, CellKind::Text | CellKind::Expression)
}

fn next_editable_cell<S: GridSource>(
    source: &S,
    cols: &[GridColumn],
    row: usize,
    col: usize,
    key: CellKey,
) -> (Option<usize>, Option<usize>) {
    let rows = source.row_count();
    if rows == 0 || cols.is_empty() {
        return (None, None);
    }
    let editable = |r: usize, c: usize| -> bool {
        let kind = source.kind(r, c).unwrap_or_else(|| cols[c].kind.clone());
        is_editable_kind(&kind)
    };
    match key {
        CellKey::Down => ((row + 1 < rows).then_some(row + 1), Some(col)),
        CellKey::Up => (row.checked_sub(1), Some(col)),
        CellKey::Next => {
            let mut r = row;
            let mut c = col + 1;
            while r < rows {
                while c < cols.len() {
                    if editable(r, c) {
                        return (Some(r), Some(c));
                    }
                    c += 1;
                }
                r += 1;
                c = 0;
            }
            (None, None)
        }
        CellKey::Previous => {
            let mut r = row;
            let mut c = col;
            loop {
                if c > 0 {
                    c -= 1;
                    if editable(r, c) {
                        return (Some(r), Some(c));
                    }
                } else if r > 0 {
                    r -= 1;
                    c = cols.len();
                } else {
                    return (None, None);
                }
            }
        }
    }
}
