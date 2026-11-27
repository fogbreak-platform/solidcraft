//! Mouse buttons and stylus/touch reconciliation between browser events and egui.

use egui::{Event, Modifiers, NUM_POINTER_BUTTONS, PointerButton, Pos2};

const BUTTONS: [PointerButton; NUM_POINTER_BUTTONS] =
    [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle, PointerButton::Extra1, PointerButton::Extra2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    pub buttons: u16,
    pub on_canvas: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happened {
    Pointer { buttons: u16, down_on_canvas: bool },
    Lost,
    Blur { of_window: bool },
}

#[derive(Debug, Default)]
pub struct Tracker {
    on_canvas: bool,
    pending: Option<Observed>,
    pen_contact: Option<i32>,
    pen_release: Option<Pos2>,
    pen_press: Option<Pos2>,
    finger_down: Option<Pos2>,
    stylus: Vec<egui::TouchId>,
}

const MAX_STYLUS: usize = 32;

impl Tracker {
    fn note_pen(&mut self, id: i32, pressed: bool, on_canvas: bool, pos: Pos2) {
        if pressed && on_canvas {
            self.pen_contact = Some(id);
            self.pen_press = Some(pos);
        } else if !pressed && self.pen_contact == Some(id) {
            self.pen_contact = None;
            self.pen_release = Some(pos);
        }
    }

    fn note_finger(&mut self, on_canvas: bool, pos: Pos2) {
        if on_canvas {
            self.finger_down = Some(pos);
        }
    }

    fn note_stylus(&mut self, id: egui::TouchId) {
        if self.stylus.len() < MAX_STYLUS && !self.stylus.contains(&id) {
            self.stylus.push(id);
        }
    }

    fn take_finger_down(&mut self) -> Option<Pos2> {
        self.finger_down.take()
    }

    fn cancel_pen(&mut self, id: Option<i32>) {
        if id.is_none() || self.pen_contact == id {
            self.pen_contact = None;
            self.pen_release = None;
            self.pen_press = None;
        }
    }

    fn take_pen_release(&mut self, events: &[Event]) -> Option<Pos2> {
        let at = self.pen_release.take()?;
        events
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } => Some(*pos),
                _ => None,
            })
            .filter(|pos| pos.distance(at) <= solidcraft_ui_egui::pencil::PEN_RELEASE_SLOP)
    }

    pub fn note(&mut self, happened: Happened) -> bool {
        let before = self.pending.map(|o| o.buttons);
        let buttons = match happened {
            Happened::Pointer { buttons, down_on_canvas } => {
                self.on_canvas |= down_on_canvas;
                buttons
            }
            Happened::Lost | Happened::Blur { of_window: true } => 0,
            Happened::Blur { of_window: false } => return false,
        };
        let observed = Observed { buttons, on_canvas: self.on_canvas };
        if buttons == 0 {
            self.on_canvas = false;
        }
        self.pending = Some(observed);
        before != Some(buttons)
    }

    pub fn take(&mut self) -> Option<Observed> {
        self.pending.take()
    }
}

pub fn corrections(down: [bool; NUM_POINTER_BUTTONS], events: &[Event], observed: Observed, pos: Pos2, modifiers: Modifiers) -> Vec<Event> {
    let mut down = down;
    let mut pos = pos;
    for event in events {
        match event {
            Event::PointerButton { button, pressed, pos: at, .. } => {
                down[*button as usize] = *pressed;
                pos = *at;
            }
            Event::PointerMoved(at) => pos = *at,
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (bit, button) in BUTTONS.into_iter().enumerate() {
        let held = observed.buttons & (1 << bit) != 0;
        let pressed = match (down[bit], held) {
            (true, false) => false,
            (false, true) if observed.on_canvas => true,
            _ => continue,
        };
        out.push(Event::PointerButton { pos, button, pressed, modifiers });
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub fn egui_down(ctx: &egui::Context) -> [bool; NUM_POINTER_BUTTONS] {
    ctx.input(|i| BUTTONS.map(|b| i.pointer.button_down(b)))
}

#[cfg(target_arch = "wasm32")]
pub fn reconcile(tracker: &std::cell::RefCell<Tracker>, ctx: &egui::Context, raw: &mut egui::RawInput) -> solidcraft_ui_egui::pencil::PlatformTaps {
    let mut tracker = tracker.borrow_mut();
    let taps = solidcraft_ui_egui::pencil::PlatformTaps {
        pen_press: tracker.pen_press.take(),
        pen_release: tracker.take_pen_release(&raw.events),
        finger_down: tracker.take_finger_down(),
        pen_down: tracker.pen_contact.is_some(),
        stylus: std::mem::take(&mut tracker.stylus),
    };
    let Some(observed) = tracker.take() else { return taps };
    let pos = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())).unwrap_or_default();
    let modifiers = ctx.input(|i| i.modifiers);
    let fixes = corrections(egui_down(ctx), &raw.events, observed, pos, modifiers);
    raw.events.extend(fixes);
    taps
}

#[cfg(target_arch = "wasm32")]
pub use web::install;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::{Happened, Tracker};

    pub fn install(window: &web_sys::Window, canvas: &web_sys::HtmlCanvasElement, ctx: &egui::Context) -> Rc<RefCell<Tracker>> {
        let tracker = Rc::new(RefCell::new(Tracker::default()));
        let listen = |name: &str, on: Box<dyn FnMut(web_sys::Event)>| {
            let on = Closure::<dyn FnMut(web_sys::Event)>::new(on);
            let _ = window.add_event_listener_with_callback_and_bool(name, on.as_ref().unchecked_ref(), true);
            on.forget();
        };
        for name in ["pointerdown", "pointermove", "pointerup"] {
            let (tracker, canvas, ctx) = (tracker.clone(), canvas.clone(), ctx.clone());
            listen(
                name,
                Box::new(move |e: web_sys::Event| {
                    let Some(e) = e.dyn_ref::<web_sys::PointerEvent>() else { return };
                    let at = || {
                        let rect = canvas.get_bounding_client_rect();
                        let zoom = ctx.zoom_factor();
                        egui::pos2((e.client_x() as f32 - rect.left() as f32) / zoom, (e.client_y() as f32 - rect.top() as f32) / zoom)
                    };
                    let source = e.pointer_type();
                    if source == "touch" {
                        if name == "pointerdown" {
                            let on_canvas = e.target().is_some_and(|t| canvas.is_same_node(t.dyn_ref::<web_sys::Node>()));
                            tracker.borrow_mut().note_finger(on_canvas, at());
                        }
                        return;
                    }
                    let mut down_on_canvas = false;
                    if name == "pointerdown" && e.target().is_some_and(|t| canvas.is_same_node(t.dyn_ref::<web_sys::Node>())) {
                        down_on_canvas = true;
                        let _ = canvas.set_pointer_capture(e.pointer_id());
                    }
                    let mut tracker = tracker.borrow_mut();
                    if source == "pen" && web_sys::MouseEvent::button(e) == 0 && name != "pointermove" {
                        tracker.note_pen(e.pointer_id(), name == "pointerdown", down_on_canvas, at());
                    }
                    if tracker.note(Happened::Pointer { buttons: e.buttons(), down_on_canvas }) {
                        ctx.request_repaint();
                    }
                }),
            );
        }
        {
            let tracker = tracker.clone();
            listen(
                "touchstart",
                Box::new(move |e: web_sys::Event| {
                    let Some(list) = e.dyn_ref::<web_sys::TouchEvent>().map(web_sys::TouchEvent::changed_touches) else { return };
                    for i in 0..list.length() {
                        let Some(touch) = list.get(i) else { continue };
                        let kind = js_sys::Reflect::get(&touch, &"touchType".into()).ok().and_then(|v| v.as_string());
                        if kind.as_deref() == Some("stylus") {
                            tracker.borrow_mut().note_stylus(egui::TouchId::from(touch.identifier()));
                        }
                    }
                }),
            );
        }
        let lost = {
            let (tracker, ctx) = (tracker.clone(), ctx.clone());
            move |id| {
                let mut tracker = tracker.borrow_mut();
                tracker.cancel_pen(id);
                if tracker.note(Happened::Lost) {
                    ctx.request_repaint();
                }
            }
        };
        for name in ["pointercancel", "lostpointercapture"] {
            let lost = lost.clone();
            listen(name, Box::new(move |e| lost(e.dyn_ref::<web_sys::PointerEvent>().map(web_sys::PointerEvent::pointer_id))));
        }
        {
            let (tracker, ctx) = (tracker.clone(), ctx.clone());
            listen(
                "blur",
                Box::new(move |e: web_sys::Event| {
                    let of_window = e.target().is_some_and(|t| t.dyn_ref::<web_sys::Window>().is_some());
                    let mut tracker = tracker.borrow_mut();
                    if of_window {
                        tracker.cancel_pen(None);
                    }
                    if tracker.note(Happened::Blur { of_window }) {
                        ctx.request_repaint();
                    }
                }),
            );
        }
        let document = window.document();
        listen(
            "visibilitychange",
            Box::new(move |_| {
                if document.as_ref().is_some_and(|d| d.visibility_state() == web_sys::VisibilityState::Hidden) {
                    lost(None);
                }
            }),
        );
        tracker
    }
}
