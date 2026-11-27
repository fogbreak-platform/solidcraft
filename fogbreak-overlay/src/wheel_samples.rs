//! Browser wheel-event capture for smooth trackpad/wheel zoom.

pub use web::install;

mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use solidcraft_ui_egui::wheel_hint::WheelSample;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    const MAX_PENDING: usize = 64;

    pub fn install(window: &web_sys::Window) -> Rc<RefCell<Vec<WheelSample>>> {
        let pending = Rc::new(RefCell::new(Vec::new()));
        let sink = pending.clone();
        let on = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(e) = e.dyn_ref::<web_sys::WheelEvent>() else { return };
            let legacy = js_sys::Reflect::get(e.as_ref(), &"wheelDeltaY".into()).ok().and_then(|v| v.as_f64());
            let mut sink = sink.borrow_mut();
            if sink.len() < MAX_PENDING {
                sink.push(WheelSample {
                    line: e.delta_mode() != web_sys::WheelEvent::DOM_DELTA_PIXEL,
                    delta_x: e.delta_x(),
                    delta_y: e.delta_y(),
                    wheel_delta_y: legacy,
                    bare: !(e.ctrl_key() || e.shift_key() || e.alt_key() || e.meta_key()),
                    time: e.time_stamp() / 1000.0,
                });
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_capture(true);
        options.set_passive(true);
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options("wheel", on.as_ref().unchecked_ref(), &options);
        on.forget();
        pending
    }
}
