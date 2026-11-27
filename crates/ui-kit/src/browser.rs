//! Clean-room tree browser host built over [`crate::tree`] rows.

use std::any::Any;
use std::sync::Arc;

use egui::{Context, Id, Pos2, Rect, Ui};

pub type NodeId = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selectable {
    #[default]
    No,
    Yes(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Before,
    Into,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    Only,
    Toggle,
    Range(Vec<NodeId>),
    RangeAdd(Vec<NodeId>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Begin,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rename {
    #[default]
    Never,
    Host,
}

#[derive(Default)]
pub enum NodeMenu<'a> {
    #[default]
    None,
    Host,
    Items(&'a [crate::Item]),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drops {
    pub before: bool,
    pub into: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub scope: u64,
    pub presentation: u64,
}

#[derive(Clone)]
pub struct Payload(Arc<dyn Any + Send + Sync>);

impl Payload {
    pub fn new<T: Send + Sync + 'static>(value: T) -> Self {
        Self(Arc::new(value))
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

#[derive(Clone)]
pub struct Dragged {
    pub source: Source,
    pub ids: Vec<NodeId>,
    pub data: Option<Payload>,
}

#[derive(Clone)]
pub enum Event {
    Select { id: NodeId, how: How },
    Click(NodeId),
    Open(NodeId),
    RenameRequested(NodeId),
    Rename { id: NodeId, name: String },
    Fold { id: NodeId, open: bool },
    Eye { id: NodeId, visible: bool },
    Activate(NodeId),
    NameAction(NodeId),
    Menu { id: NodeId, at: Pos2 },
    Move { id: NodeId, zone: Zone, source: Source, data: Option<Payload> },
    Action { id: NodeId, action: String },
    Revealed(NodeId),
    EyeGesture(Gesture),
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Select { id: a, how: h1 }, Self::Select { id: b, how: h2 }) => a == b && h1 == h2,
            (Self::Click(a), Self::Click(b))
            | (Self::Open(a), Self::Open(b))
            | (Self::RenameRequested(a), Self::RenameRequested(b))
            | (Self::Activate(a), Self::Activate(b))
            | (Self::NameAction(a), Self::NameAction(b))
            | (Self::Revealed(a), Self::Revealed(b)) => a == b,
            (Self::Rename { id: a, name: n1 }, Self::Rename { id: b, name: n2 }) => a == b && n1 == n2,
            (Self::Fold { id: a, open: o1 }, Self::Fold { id: b, open: o2 }) => a == b && o1 == o2,
            (Self::Eye { id: a, visible: v1 }, Self::Eye { id: b, visible: v2 }) => a == b && v1 == v2,
            (Self::Menu { id: a, at: p1 }, Self::Menu { id: b, at: p2 }) => a == b && p1 == p2,
            (Self::Action { id: a, action: s1 }, Self::Action { id: b, action: s2 }) => a == b && s1 == s2,
            (Self::EyeGesture(a), Self::EyeGesture(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Default)]
pub struct Node<'a> {
    pub id: NodeId,
    pub key: &'a str,
    pub row: crate::Row<'a>,
    pub select: Selectable,
    pub rename: Rename,
    pub drag: Option<u32>,
    pub drops: Drops,
    pub menu: NodeMenu<'a>,
    pub accept: Option<&'a dyn Fn(&Dragged, Zone) -> bool>,
}

#[derive(Default)]
pub struct RowOut {
    pub rect: Option<Rect>,
    pub label_rect: Option<Rect>,
    pub widget: Option<Id>,
    pub hovered: bool,
    pub response: Option<egui::Response>,
    pub on_control: bool,
    pub drag_started: bool,
    pub fold_at: Option<Pos2>,
    pub eye_at: Option<Pos2>,
    pub eye_rect: Option<Rect>,
    pub radio_at: Option<Pos2>,
    pub clicked: bool,
    pub double: bool,
    pub secondary: Option<Pos2>,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug)]
pub struct DrawnRow {
    pub label: String,
    pub rect: Rect,
    pub fold: Option<Rect>,
    pub open: Option<bool>,
}

#[derive(Default)]
pub struct Output {
    pub drawn: Vec<DrawnRow>,
    pub pointer_owned: bool,
    pub events: Vec<Event>,
    pub content_height: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    pub compact: bool,
    pub eye_left: bool,
}

#[derive(Clone, Default)]
pub struct BrowserState {
    pub eye_drag_active: bool,
    pub active_drag: Option<Dragged>,
    last_click_id: Option<NodeId>,
    last_click_time: Option<f64>,
    anchor: Option<NodeId>,
    prev_selectable: Vec<NodeId>,
    curr_selectable: Vec<NodeId>,
    focused: bool,
}

impl BrowserState {
    pub fn focused(&self) -> bool {
        self.focused
    }
}

/// Cancel an active drag for `scope`, if any.
pub fn cancel_drag(_ctx: &Context, _scope: u64) {}

pub fn end_gestures(ctx: &Context, state: &mut BrowserState, source: Source) -> Vec<Event> {
    let mut events = Vec::new();
    if state.eye_drag_active && !ctx.input(|i| i.pointer.primary_down()) {
        state.eye_drag_active = false;
        events.push(Event::EyeGesture(Gesture::End));
    }
    if state.active_drag.as_ref().is_some_and(|d| d.source == source) && !ctx.input(|i| i.pointer.any_down()) {
        state.active_drag = None;
    }
    events
}

pub struct Browser<'a> {
    state: &'a mut BrowserState,
    style: Style,
    scope: u64,
    presentation: u64,
    active: bool,
    scroll: bool,
    shrink: bool,
    events_per_row: bool,
    keyboard: bool,
}

impl<'a> Browser<'a> {
    pub fn new(state: &'a mut BrowserState, style: Style, scope: u64) -> Self {
        Self {
            state,
            style,
            scope,
            presentation: 0,
            active: true,
            scroll: true,
            shrink: false,
            events_per_row: true,
            keyboard: false,
        }
    }

    pub fn presentation(mut self, presentation: u64) -> Self {
        self.presentation = presentation;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn scroll(mut self, scroll: bool) -> Self {
        self.scroll = scroll;
        self
    }

    pub fn shrink(mut self, shrink: bool) -> Self {
        self.shrink = shrink;
        self
    }

    pub fn events_per_row(mut self, events_per_row: bool) -> Self {
        self.events_per_row = events_per_row;
        self
    }

    pub fn keyboard(mut self, keyboard: bool) -> Self {
        self.keyboard = keyboard;
        self
    }

    pub fn show(self, ui: &mut Ui, draw: impl FnOnce(&mut Rows<'_>)) -> Output {
        let mut output = Output::default();
        let source = Source { scope: self.scope, presentation: self.presentation };
        if self.state.eye_drag_active && !ui.input(|i| i.pointer.primary_down()) {
            self.state.eye_drag_active = false;
            output.events.push(Event::EyeGesture(Gesture::End));
        }
        self.state.prev_selectable = std::mem::take(&mut self.state.curr_selectable);
        self.state.focused = false;
        let paint_rows = |ui: &mut Ui| {
            let top = ui.cursor().top();
            let mut rows = Rows {
                ui,
                state: self.state,
                source,
                style: self.style,
                output: &mut output,
                events_per_row: self.events_per_row,
            };
            draw(&mut rows);
            let bottom = ui.min_rect().bottom();
            output.content_height = (bottom - top).max(0.0);
        };
        if self.scroll {
            egui::ScrollArea::vertical()
                .auto_shrink([false, self.shrink])
                .show(ui, paint_rows);
        } else {
            paint_rows(ui);
        }
        let _ = (self.active, self.keyboard);
        if !ui.input(|i| i.pointer.any_down()) {
            self.state.active_drag = None;
        }
        output
    }
}

pub struct Rows<'a> {
    ui: &'a mut Ui,
    state: &'a mut BrowserState,
    source: Source,
    style: Style,
    output: &'a mut Output,
    events_per_row: bool,
}

impl Rows<'_> {
    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }

    pub fn next_visible(&self) -> bool {
        let cursor = self.ui.cursor();
        let h = solidcraft_theme::Tokens::get().control_height.max(1.0);
        let probe = egui::Rect::from_min_size(cursor.min, vec2(cursor.width().max(1.0), h));
        self.ui.is_visible() && self.ui.clip_rect().intersects(probe)
    }

    pub fn pass(&mut self, id: NodeId, select: Selectable) {
        if matches!(select, Selectable::Yes(_)) {
            self.state.curr_selectable.push(id);
        }
    }

    pub fn skip(&mut self, _id: NodeId, _select: Selectable) {}

    pub fn drag_with(&mut self, ids: Vec<NodeId>, data: Option<Payload>) {
        self.state.active_drag = Some(Dragged { source: self.source, ids, data });
    }

    pub fn row(&mut self, mut node: Node<'_>) -> RowOut {
        node.row.compact = self.style.compact;
        node.row.eye_left = self.style.eye_left;
        let was_eye = node.row.eye;
        let was_fold = node.row.fold;
        let label = node.row.label.to_owned();
        let row_id = self.ui.make_persistent_id(("browser_row", self.source.scope, self.source.presentation, node.id));
        let resp = crate::tree_row(self.ui, row_id, &node.row);
        let mut events = Vec::new();

        if let Some(rect) = resp.rect {
            let fold_rect = resp.fold_at.map(|c| Rect::from_center_size(c, egui::vec2(16.0, rect.height())));
            self.output.drawn.push(DrawnRow {
                label,
                rect,
                fold: fold_rect,
                open: was_fold,
            });
            if self.ui.rect_contains_pointer(rect) {
                self.output.pointer_owned = true;
            }
        }

        if resp.fold {
            events.push(Event::Fold { id: node.id, open: !was_fold.unwrap_or(false) });
        }
        if resp.eye {
            if self.ui.input(|i| i.pointer.primary_down()) && !self.state.eye_drag_active {
                self.state.eye_drag_active = true;
                events.push(Event::EyeGesture(Gesture::Begin));
            }
            events.push(Event::Eye { id: node.id, visible: !was_eye.unwrap_or(true) });
        }
        if resp.radio {
            events.push(Event::Activate(node.id));
        }
        if resp.action {
            events.push(Event::NameAction(node.id));
        }
        for (idx, r) in resp.actions.iter().enumerate() {
            if r.clicked()
                && let Some(act) = node.row.actions.get(idx)
            {
                events.push(Event::Action { id: node.id, action: act.target.to_string() });
            }
        }
        if let Some(at) = resp.secondary {
            events.push(Event::Menu { id: node.id, at });
        }

        let mut slow_rename = false;
        if resp.clicked {
            let now = self.ui.input(|i| i.time);
            if node.rename == Rename::Host
                && self.state.last_click_id == Some(node.id)
                && self.state.last_click_time.is_some_and(|t0| now - t0 >= 0.45 && now - t0 <= 1.8)
            {
                slow_rename = true;
                events.push(Event::RenameRequested(node.id));
            }
            self.state.last_click_id = Some(node.id);
            self.state.last_click_time = Some(now);
            if resp.double {
                events.push(Event::Open(node.id));
            } else {
                match node.select {
                    Selectable::Yes(_) => {
                        let mods = self.ui.input(|i| i.modifiers);
                        let how = if mods.shift {
                            let ids = if let Some(anchor) = self.state.anchor
                                && let Some(a_idx) = self.state.prev_selectable.iter().position(|x| *x == anchor)
                                && let Some(b_idx) = self.state.prev_selectable.iter().position(|x| *x == node.id)
                            {
                                let (lo, hi) = if a_idx <= b_idx { (a_idx, b_idx) } else { (b_idx, a_idx) };
                                self.state.prev_selectable[lo..=hi].to_vec()
                            } else {
                                vec![node.id]
                            };
                            if mods.command || mods.ctrl { How::RangeAdd(ids) } else { How::Range(ids) }
                        } else if mods.command || mods.ctrl {
                            self.state.anchor = Some(node.id);
                            How::Toggle
                        } else {
                            self.state.anchor = Some(node.id);
                            How::Only
                        };
                        events.push(Event::Select { id: node.id, how });
                    }
                    Selectable::No => {
                        events.push(Event::Click(node.id));
                    }
                }
            }
        }
        if matches!(node.select, Selectable::Yes(_)) {
            self.state.curr_selectable.push(node.id);
        }
        let _ = slow_rename;

        let drag_started = resp.resp.as_ref().is_some_and(egui::Response::drag_started);
        if drag_started && node.drag.is_some() && self.state.active_drag.is_none() {
            self.state.active_drag = Some(Dragged { source: self.source, ids: vec![node.id], data: None });
        }
        if let (Some(drag), Some(rect)) = (&self.state.active_drag, resp.rect)
            && (node.drops.before || node.drops.into)
            && let Some(ptr) = self.ui.input(|i| i.pointer.interact_pos())
            && rect.contains(ptr)
            && self.ui.input(|i| i.pointer.primary_released())
        {
            let zone = if node.drops.before && (!node.drops.into || ptr.y < rect.top() + rect.height() * 0.25) {
                Zone::Before
            } else {
                Zone::Into
            };
            if node.accept.as_ref().is_none_or(|f| f(drag, zone)) {
                events.push(Event::Move {
                    id: node.id,
                    zone,
                    source: drag.source,
                    data: drag.data.clone(),
                });
            }
        }

        if !self.events_per_row {
            self.output.events.extend(events.iter().cloned());
        }
        let label_rect = resp
            .rect
            .zip(resp.label_left)
            .map(|(r, left)| Rect::from_min_max(egui::pos2(left.min(r.right()), r.top()), r.right_bottom()));
        let widget = resp.resp.as_ref().map(|r| r.id);
        RowOut {
            rect: resp.rect,
            label_rect,
            widget,
            hovered: resp.hovered,
            response: resp.resp,
            on_control: resp.on_control,
            drag_started,
            fold_at: resp.fold_at,
            eye_at: resp.eye_at,
            eye_rect: resp.eye_rect,
            radio_at: resp.radio_at,
            clicked: resp.clicked,
            double: resp.double,
            secondary: resp.secondary,
            events,
        }
    }
}
