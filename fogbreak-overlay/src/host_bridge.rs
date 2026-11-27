//! Direct `<canvas data-host="page">` host bridge and `window.fogbreakSolidHost` RPC handle.
#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{Sender, TryRecvError, channel};

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use serde_json::{Value, json};
use solidcraft_engine::io::vfs;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

pub const ENGINE_ID: &str = "fogbreak-solid";
pub const HOST_KEY: &str = "fogbreakSolidHost";
pub const CANVAS_ID: &str = "fogbreak_solid_canvas";
pub const LOADING_ID: &str = "fogbreak_solid_loading";

const EXPORT_FORMATS: [&str; 6] = ["step", "3mf", "stl", "obj", "glb", "iges"];

pub enum HostCommand {
    Request {
        method: String,
        params: Value,
        tx: Sender<Result<Value, String>>,
    },
    Screenshot {
        width: usize,
        height: usize,
        tx: Sender<Result<Vec<u8>, String>>,
    },
    Export {
        format: String,
        tx: Sender<Result<Vec<u8>, String>>,
    },
    Import {
        name: String,
        bytes: Vec<u8>,
        tx: Sender<Result<Value, String>>,
    },
    OpenBytes {
        name: String,
        bytes: Vec<u8>,
    },
    SetTheme {
        dark: bool,
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

pub fn listen_host_events(canvas: &web_sys::HtmlCanvasElement, queue: CommandQueue, ctx: egui::Context) {
    let q_open = queue.clone();
    let c_open = ctx.clone();
    let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
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
                q_open.push(HostCommand::OpenBytes { name, bytes });
                c_open.request_repaint();
            }
        }
    });
    let _ = canvas.add_event_listener_with_callback("open", on_open.as_ref().unchecked_ref());
    on_open.forget();

    let on_theme = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let Ok(custom) = ev.dyn_into::<web_sys::CustomEvent>() else { return };
        let detail = custom.detail();
        let dark = Reflect::get(&detail, &"dark".into())
            .ok()
            .and_then(|v| v.as_bool())
            .or_else(|| {
                Reflect::get(&detail, &"theme".into())
                    .ok()
                    .and_then(|v| v.as_string())
                    .map(|s| s.eq_ignore_ascii_case("dark"))
            });
        if let Some(dark) = dark {
            queue.push(HostCommand::SetTheme { dark });
            ctx.request_repaint();
        }
    });
    let _ = canvas.add_event_listener_with_callback("theme", on_theme.as_ref().unchecked_ref());
    on_theme.forget();
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

pub fn emit_plugin_run(plugin_id: &str, entry: &str, params: &Value) -> Result<(), String> {
    let payload = json!({
        "engineId": ENGINE_ID,
        "pluginId": plugin_id,
        "entry": entry,
        "params": params,
    });
    let js_detail = to_js(&payload);
    dispatch_canvas_event("plugin-run", &js_detail);
    if let Some(window) = web_sys::window() {
        let init = web_sys::CustomEventInit::new();
        init.set_detail(&js_detail);
        init.set_bubbles(false);
        init.set_cancelable(false);
        if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict("fogbreak:solid-plugin-run", &init) {
            let _ = window.dispatch_event(&ev);
        }
        if let Ok(host) = Reflect::get(&window, &JsValue::from_str(HOST_KEY))
            && let Ok(cb_val) = Reflect::get(&host, &"__onPluginRun".into())
            && let Ok(cb) = cb_val.dyn_into::<Function>()
        {
            let _ = cb.call1(&host, &js_detail);
        }
    }
    Ok(())
}

pub fn trigger_download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no browser document")?;
    let parts = Array::new();
    parts.push(&Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(|e| format!("{e:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| format!("{e:?}"))?;
    let a: web_sys::HtmlAnchorElement = doc
        .create_element("a")
        .map_err(|e| format!("{e:?}"))?
        .dyn_into()
        .map_err(|_| "no anchor element".to_string())?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
    }
    Ok(())
}

pub fn open_bytes_in_tab(name: &str, media_type: &str, bytes: &[u8]) -> Result<(), String> {
    let window = web_sys::window().ok_or("no browser window")?;
    let parts = Array::new();
    parts.push(&Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(media_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(|e| format!("{e:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| format!("{e:?}"))?;
    let opened = window.open_with_url_and_target(&url, "_blank").map_err(|e| format!("{e:?}"))?;
    if opened.is_none() {
        let _ = web_sys::Url::revoke_object_url(&url);
        return Err(format!("Browser blocked opening {name}; allow pop-ups or download instead."));
    }
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000);
    Ok(())
}

pub fn pick_upload(exts: &[&str]) -> Pin<Box<dyn std::future::Future<Output = Option<String>>>> {
    let accept = if exts.is_empty() {
        ".sc,.step,.stp,.iges,.igs,.stl,.obj,.3mf,.glb,.svg,.dxf,.json".to_string()
    } else {
        exts.iter().map(|e| format!(".{}", e.trim_start_matches('.'))).collect::<Vec<_>>().join(",")
    };
    Box::pin(async move {
        let doc = web_sys::window().and_then(|w| w.document())?;
        let input = doc.create_element("input").ok()?.dyn_into::<web_sys::HtmlInputElement>().ok()?;
        input.set_type("file");
        input.set_accept(&accept);
        let _ = input.set_attribute("style", "display:none");
        doc.body()?.append_child(&input).ok()?;
        let promise = js_sys::Promise::new(&mut |resolve: Function, _reject: Function| {
            let done = Closure::once_into_js(move |event: web_sys::Event| {
                let file = event.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()).and_then(|el| {
                    el.set_onchange(None);
                    el.set_oncancel(None);
                    el.remove();
                    el.files().and_then(|list| list.get(0))
                });
                let _ = resolve.call1(&JsValue::UNDEFINED, &file.map_or(JsValue::NULL, JsValue::from));
            });
            input.set_onchange(Some(done.unchecked_ref()));
            input.set_oncancel(Some(done.unchecked_ref()));
        });
        input.click();
        let val = JsFuture::from(promise).await.ok()?;
        let file = val.dyn_into::<web_sys::File>().ok()?;
        let name = file.name();
        let buf = JsFuture::from(file.array_buffer()).await.ok()?;
        let bytes = Uint8Array::new(&buf).to_vec();
        let path = format!("/upload/{name}");
        vfs::insert(&path, bytes);
        Some(path)
    })
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

    for alias in ["commands", "listCommands"] {
        let f = Closure::<dyn Fn() -> Promise>::new(|| {
            future_to_promise(async {
                let list: Vec<Value> = solidcraft_engine::cmd::command_specs()
                    .into_iter()
                    .map(|c| Value::String(c.id.to_string()))
                    .collect();
                Ok(to_js(&Value::Array(list)))
            })
        });
        let _ = Reflect::set(&handle, &JsValue::from_str(alias), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let f = Closure::<dyn Fn(String) -> Promise>::new(|command: String| {
            future_to_promise(async move {
                let val = describe_command_json(&command);
                Ok(to_js(&val))
            })
        });
        let _ = Reflect::set(&handle, &"describeCommand".into(), f.as_ref().unchecked_ref());
        f.forget();
    }

    {
        let q = queue.clone();
        let c = ctx.clone();
        let f = Closure::<dyn Fn(JsValue) -> Promise>::new(move |steps: JsValue| {
            dispatch_req(q.clone(), c.clone(), "__execute_batch".to_string(), parse_params(&steps))
        });
        let _ = Reflect::set(&handle, &"executeBatch".into(), f.as_ref().unchecked_ref());
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
        let f = Closure::<dyn Fn(JsValue) -> Promise>::new(move |val: JsValue| {
            let dark = val.as_bool().unwrap_or_else(|| {
                val.as_string().is_some_and(|s| s.eq_ignore_ascii_case("dark"))
            });
            q.push(HostCommand::SetTheme { dark });
            c.request_repaint();
            future_to_promise(async move { Ok(JsValue::from_bool(dark)) })
        });
        let _ = Reflect::set(&handle, &"setTheme".into(), f.as_ref().unchecked_ref());
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

    install_native_io_object(&handle, &queue, &ctx);

    let _ = Reflect::set(&window, &JsValue::from_str(HOST_KEY), &handle);
}

fn describe_command_json(command: &str) -> Value {
    let specs = solidcraft_engine::cmd::command_specs();
    let spec = specs
        .iter()
        .copied()
        .find(|c| c.id == command)
        .or_else(|| specs.iter().copied().find(|c| c.id.eq_ignore_ascii_case(command)));
    let Some(spec) = spec else {
        return Value::Null;
    };
    let mut obj = json!({
        "id": spec.id,
        "label": spec.label,
        "summary": spec.label,
        "tab": spec.tab,
        "panel": spec.panel,
        "shortcut": spec.shortcut,
        "params": spec.params,
        "mutates": spec.undoable,
        "undoable": spec.undoable,
        "readsModel": spec.reads_model,
    });
    if let Some(map) = obj.as_object_mut() {
        if let Some(schema) = spec.contract.and_then(|ct| serde_json::to_value(ct.input_schema()).ok()) {
            map.insert("inputSchema".to_string(), schema);
        }
        if let Some(schema) = spec.output.and_then(|out_fn| serde_json::to_value(out_fn()).ok()) {
            map.insert("outputSchema".to_string(), schema);
        }
    }
    obj
}

fn install_native_io_object(handle: &Object, queue: &CommandQueue, ctx: &egui::Context) {
    let io_obj = Object::new();

    let formats_fn = Closure::<dyn Fn() -> Promise>::new(|| {
        future_to_promise(async {
            let arr: Vec<Value> = EXPORT_FORMATS.iter().map(|f| Value::String((*f).to_string())).collect();
            Ok(to_js(&Value::Array(arr)))
        })
    });
    let _ = Reflect::set(&io_obj, &"formats".into(), formats_fn.as_ref().unchecked_ref());
    formats_fn.forget();

    let exp_q = queue.clone();
    let exp_c = ctx.clone();
    let export_fn = Closure::<dyn Fn(String, JsValue) -> Promise>::new(move |format: String, _params: JsValue| {
        let q = exp_q.clone();
        let c = exp_c.clone();
        future_to_promise(async move {
            let (tx, rx) = channel();
            q.push(HostCommand::Export { format, tx });
            c.request_repaint();
            loop {
                match rx.try_recv() {
                    Ok(Ok(bytes)) => return Ok(Uint8Array::from(bytes.as_slice()).into()),
                    Ok(Err(e)) => return Err(JsValue::from_str(&e)),
                    Err(TryRecvError::Empty) => sleep_ms(16).await,
                    Err(TryRecvError::Disconnected) => return Err(JsValue::from_str("export request dropped")),
                }
            }
        })
    });
    let _ = Reflect::set(&io_obj, &"export".into(), export_fn.as_ref().unchecked_ref());
    export_fn.forget();

    let imp_q = queue.clone();
    let imp_c = ctx.clone();
    let import_fn = Closure::<dyn Fn(JsValue) -> Promise>::new(move |detail: JsValue| {
        let q = imp_q.clone();
        let c = imp_c.clone();
        let parsed = extract_import_detail(&detail);
        future_to_promise(async move {
            let (name, bytes) = parsed.map_err(|e| JsValue::from_str(&e))?;
            let (tx, rx) = channel();
            q.push(HostCommand::Import { name, bytes, tx });
            c.request_repaint();
            loop {
                match rx.try_recv() {
                    Ok(Ok(val)) => return Ok(to_js(&val)),
                    Ok(Err(e)) => return Err(JsValue::from_str(&e)),
                    Err(TryRecvError::Empty) => sleep_ms(16).await,
                    Err(TryRecvError::Disconnected) => return Err(JsValue::from_str("import request dropped")),
                }
            }
        })
    });
    let _ = Reflect::set(&io_obj, &"import".into(), import_fn.as_ref().unchecked_ref());
    import_fn.forget();

    let _ = Reflect::set(handle, &"io".into(), &io_obj);
}

fn infer_solid_ext(mime: Option<&str>) -> &'static str {
    match mime.unwrap_or("").to_ascii_lowercase().as_str() {
        "model/step" | "application/step" | "application/x-step" => "step",
        "model/iges" | "application/iges" => "iges",
        "model/3mf" | "application/vnd.ms-package.3dmanufacturing-3dmodel+xml" => "3mf",
        "model/stl" | "application/sla" => "stl",
        "model/obj" => "obj",
        "model/gltf-binary" => "glb",
        _ => "sc",
    }
}

fn extract_import_detail(detail: &JsValue) -> Result<(String, Vec<u8>), String> {
    let mime = Reflect::get(detail, &"mimeType".into())
        .ok()
        .and_then(|v| v.as_string());
    let raw_name = Reflect::get(detail, &"name".into())
        .ok()
        .and_then(|v| v.as_string())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let name = match raw_name {
        Some(n) if n.contains('.') => n,
        Some(n) => format!("{n}.{}", infer_solid_ext(mime.as_deref())),
        None => format!("part.{}", infer_solid_ext(mime.as_deref())),
    };
    let bytes_val = Reflect::get(detail, &"bytes".into())
        .map_err(|_| "import requires a `bytes` Uint8Array".to_string())?;
    let bytes = Uint8Array::new(&bytes_val).to_vec();
    if bytes.is_empty() {
        return Err("import requires non-empty bytes".to_string());
    }
    Ok((name, bytes))
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
