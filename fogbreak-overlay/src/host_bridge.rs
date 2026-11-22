//! Direct `<canvas data-host="page">` host bridge and `window.fogbreakSolidHost` RPC handle.
#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use serde_json::{Value, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

pub const ENGINE_ID: &str = "fogbreak-solid";
pub const HOST_KEY: &str = "fogbreakSolidHost";
pub const CANVAS_ID: &str = "fogbreak_solid_canvas";
pub const LOADING_ID: &str = "fogbreak_solid_loading";

pub enum HostCommand {
    Request {
        method: String,
        params: Value,
        tx: std::sync::mpsc::Sender<Result<Value, String>>,
    },
    Screenshot {
        width: usize,
        height: usize,
        tx: std::sync::mpsc::Sender<Result<Vec<u8>, String>>,
    },
    OpenBytes {
        name: String,
        bytes: Vec<u8>,
    },
}

#[derive(Clone, Default)]
pub struct CommandQueue {
    items: Rc<RefCell<Vec<HostCommand>>>,
}

impl CommandQueue {
    pub fn push(&self, cmd: HostCommand) {
        self.items.borrow_mut().push(cmd);
    }

    pub fn drain(&self) -> Vec<HostCommand> {
        self.items.borrow_mut().drain(..).collect()
    }
}

thread_local! {
    static CANVAS_REF: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static LAST_DIRTY: RefCell<Option<bool>> = const { RefCell::new(None) };
    static REVISION: RefCell<u64> = const { RefCell::new(0) };
}

pub fn find_page_canvas() -> Result<web_sys::HtmlCanvasElement, String> {
    let window = web_sys::window().ok_or_else(|| "missing browser window".to_string())?;
    if let Ok(top) = window.top()
        && let Some(top_win) = top
        && !js_sys::Object::is(&window, &top_win)
    {
        return Err("Refusing to mount inside an <iframe>; use <canvas data-host=\"page\">.".into());
    }
    let document = window.document().ok_or_else(|| "missing browser document".to_string())?;
    let el = document
        .query_selector("canvas[data-host=\"page\"][data-engine=\"fogbreak-solid\"]")
        .ok()
        .flatten()
        .or_else(|| document.get_element_by_id(CANVAS_ID))
        .ok_or_else(|| format!("missing <canvas id=\"{CANVAS_ID}\" data-host=\"page\">"))?;
    if el.tag_name().eq_ignore_ascii_case("IFRAME") {
        return Err("Refusing to mount on an <iframe>; use <canvas data-host=\"page\">.".into());
    }
    let canvas = el
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "target element is not a <canvas>".to_string())?;
    let _ = canvas.set_attribute("data-host", "page");
    let _ = canvas.set_attribute("data-engine", ENGINE_ID);
    let _ = canvas.set_attribute("data-rpc-key", HOST_KEY);
    let _ = canvas.set_attribute("data-iframe", "false");
    CANVAS_REF.with(|slot| *slot.borrow_mut() = Some(canvas.clone()));
    Ok(canvas)
}

pub fn listen_open(canvas: &web_sys::HtmlCanvasElement, queue: CommandQueue, ctx: egui::Context) {
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Ok(custom) = ev.dyn_into::<web_sys::CustomEvent>() else { return };
        let detail = custom.detail();
        let name = Reflect::get(&detail, &"name".into())
            .ok()
            .and_then(|v| v.as_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "part.sc".to_string());
        if let Ok(bytes_val) = Reflect::get(&detail, &"bytes".into()) {
            let bytes = Uint8Array::new(&bytes_val).to_vec();
            if !bytes.is_empty() {
                queue.push(HostCommand::OpenBytes { name, bytes });
                ctx.request_repaint();
            }
        }
    });
    let _ = canvas.add_event_listener_with_callback("open", cb.as_ref().unchecked_ref());
    cb.forget();
}

pub fn emit_ready(capabilities: &[String]) {
    let detail = Object::new();
    let caps = Array::new();
    for cap in capabilities {
        caps.push(&JsValue::from_str(cap));
    }
    let _ = Reflect::set(&detail, &"engineId".into(), &JsValue::from_str(ENGINE_ID));
    let _ = Reflect::set(&detail, &"hostMode".into(), &JsValue::from_str("page"));
    let _ = Reflect::set(&detail, &"capabilities".into(), &caps);
    dispatch_canvas_event("ready", &detail.into());
}

pub fn emit_dirty(dirty: bool) {
    let changed = LAST_DIRTY.with(|slot| {
        let prev = *slot.borrow();
        if prev == Some(dirty) {
            false
        } else {
            *slot.borrow_mut() = Some(dirty);
            true
        }
    });
    if !changed {
        return;
    }
    let rev = REVISION.with(|r| {
        let mut v = r.borrow_mut();
        *v += 1;
        *v
    });
    let detail = Object::new();
    let _ = Reflect::set(&detail, &"dirty".into(), &JsValue::from_bool(dirty));
    let _ = Reflect::set(&detail, &"revision".into(), &JsValue::from_f64(rev as f64));
    dispatch_canvas_event("dirty", &detail.into());
}

pub fn emit_save(name: &str, format: &str, bytes: &[u8]) {
    let detail = Object::new();
    let arr = Uint8Array::from(bytes);
    let _ = Reflect::set(&detail, &"name".into(), &JsValue::from_str(name));
    let _ = Reflect::set(&detail, &"format".into(), &JsValue::from_str(format));
    let _ = Reflect::set(&detail, &"bytes".into(), &arr.into());
    dispatch_canvas_event("save", &detail.into());
}

pub fn emit_handoff(target_shore: &str, artifact_name: &str, format: &str, bytes: &[u8]) {
    let detail = Object::new();
    let arr = Uint8Array::from(bytes);
    let _ = Reflect::set(&detail, &"targetShore".into(), &JsValue::from_str(target_shore));
    let _ = Reflect::set(&detail, &"artifactName".into(), &JsValue::from_str(artifact_name));
    let _ = Reflect::set(&detail, &"format".into(), &JsValue::from_str(format));
    let _ = Reflect::set(&detail, &"bytes".into(), &arr.into());
    dispatch_canvas_event("handoff", &detail.into());
}

fn dispatch_canvas_event(name: &str, detail: &JsValue) {
    CANVAS_REF.with(|slot| {
        let Some(canvas) = slot.borrow().as_ref().cloned() else { return };
        let init = web_sys::CustomEventInit::new();
        init.set_detail(detail);
        init.set_bubbles(false);
        init.set_cancelable(false);
        if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
            let _ = canvas.dispatch_event(&ev);
        }
    });
}

pub fn install_rpc(queue: CommandQueue, ctx: egui::Context) {
    let Some(window) = web_sys::window() else { return };
    let handle = Object::new();
    let _ = Reflect::set(&handle, &"hostMode".into(), &JsValue::from_str("page"));
    let _ = Reflect::set(&handle, &"engineId".into(), &JsValue::from_str(ENGINE_ID));

    {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(String, JsValue) -> Promise>::new(move |method: String, params: JsValue| {
            dispatch_req(q.clone(), c.clone(), method, parse_params(&params))
        });
        let _ = Reflect::set(&handle, &"request".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    for alias in ["execute", "executeCommand"] {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(String, JsValue) -> Promise>::new(move |command: String, params: JsValue| {
            dispatch_req(q.clone(), c.clone(), command, parse_params(&params))
        });
        let _ = Reflect::set(&handle, &JsValue::from_str(alias), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let f = Closure::<dyn Fn() -> Promise>::new(|| {
            future_to_promise(async {
                let list: Vec<Value> = solidcraft_engine::cmd::command_specs()
                    .into_iter()
                    .map(|c| Value::String(c.id.to_string()))
                    .collect();
                Ok(to_js(&Value::Array(list)))
            })
        });
        let _ = Reflect::set(&handle, &"commands".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    for alias in ["docInspect", "inspect"] {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(JsValue) -> Promise>::new(move |params: JsValue| {
            dispatch_req(q.clone(), c.clone(), "document.inspect".to_string(), parse_params(&params))
        });
        let _ = Reflect::set(&handle, &JsValue::from_str(alias), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(JsValue) -> Promise>::new(move |params: JsValue| {
            dispatch_req(q.clone(), c.clone(), "__ui_inspect".to_string(), parse_params(&params))
        });
        let _ = Reflect::set(&handle, &"uiInspect".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(JsValue) -> Promise>::new(move |params: JsValue| {
            let p = parse_params(&params);
            let width = p.get("width").and_then(Value::as_u64).unwrap_or(800).clamp(64, 2048) as usize;
            let height = p.get("height").and_then(Value::as_u64).unwrap_or(600).clamp(64, 2048) as usize;
            let q2 = q.clone();
            let c2 = c.clone();
            future_to_promise(async move {
                let (tx, rx) = std::sync::mpsc::channel();
                q2.push(HostCommand::Screenshot { width, height, tx });
                c2.request_repaint();
                loop {
                    if let Ok(res) = rx.try_recv() {
                        return match res {
                            Ok(bytes) => Ok(Uint8Array::from(bytes.as_slice()).into()),
                            Err(e) => Err(JsValue::from_str(&e)),
                        };
                    }
                    sleep_ms(16).await;
                }
            })
        });
        let _ = Reflect::set(&handle, &"screenshot".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(String, JsValue) -> Promise>::new(move |name: String, bytes: JsValue| {
            let data = Uint8Array::new(&bytes).to_vec();
            q.push(HostCommand::OpenBytes { name, bytes: data });
            c.request_repaint();
            future_to_promise(async { Ok(JsValue::TRUE) })
        });
        let _ = Reflect::set(&handle, &"open".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let f = Closure::<dyn Fn(String, String, String, JsValue)>::new(
            |target_shore: String, artifact_name: String, format: String, bytes: JsValue| {
                let data = Uint8Array::new(&bytes).to_vec();
                emit_handoff(&target_shore, &artifact_name, &format, &data);
            },
        );
        let _ = Reflect::set(&handle, &"emitHandoff".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    let _ = Reflect::set(&window, &JsValue::from_str(HOST_KEY), &handle);
}

fn dispatch_req(queue: CommandQueue, ctx: egui::Context, method: String, params: Value) -> Promise {
    future_to_promise(async move {
        let (tx, rx) = std::sync::mpsc::channel();
        queue.push(HostCommand::Request { method, params, tx });
        ctx.request_repaint();
        loop {
            if let Ok(res) = rx.try_recv() {
                return match res {
                    Ok(v) => Ok(to_js(&v)),
                    Err(e) => Err(JsValue::from_str(&e)),
                };
            }
            sleep_ms(16).await;
        }
    })
}

fn parse_params(value: &JsValue) -> Value {
    if value.is_undefined() || value.is_null() {
        return json!({});
    }
    js_sys::JSON::stringify(value)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({}))
}

async fn sleep_ms(ms: i32) {
    let promise = Promise::new(&mut |resolve: Function, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

fn to_js(val: &Value) -> JsValue {
    let text = serde_json::to_string(val).unwrap_or_else(|_| "null".to_string());
    js_sys::JSON::parse(&text).unwrap_or(JsValue::NULL)
}
