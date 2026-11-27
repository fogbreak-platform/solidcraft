//! Fogbreak Solid Studio browser runner: full `solidcraft_ui_egui::SolveApp` mounted onto `<canvas data-host="page">`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod host_bridge;
#[cfg(target_arch = "wasm32")]
mod mouse_buttons;
#[cfg(target_arch = "wasm32")]
mod wheel_samples;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};

    use serde_json::{Value, json};
    use solidcraft_control::ControlRequest;
    use solidcraft_doc::Document;
    use solidcraft_engine::Session;
    use solidcraft_engine::io::vfs;
    use solidcraft_ui_egui::{Services, SolveApp, control, documents, gpu, prefs, toolbar, window_chrome};
    use wasm_bindgen::JsCast as _;

    use crate::host_bridge::{self, CommandQueue, HostCommand, LOADING_ID};

    const DEPTH_BITS: u8 = 32;

    struct PendingControl {
        reply: Receiver<Value>,
        tx: Sender<Result<Value, String>>,
    }

    struct Shell {
        app: SolveApp,
        queue: CommandQueue,
        control_tx: Sender<ControlRequest>,
        pending_controls: Vec<PendingControl>,
        wheel_gestures: solidcraft_ui_egui::WebWheelGestures,
        wheel_samples: Rc<RefCell<Vec<solidcraft_ui_egui::wheel_hint::WheelSample>>>,
        mouse_buttons: Rc<RefCell<crate::mouse_buttons::Tracker>>,
    }

    impl Shell {
        fn set_theme(&mut self, dark: bool) {
            let target = if dark { "dark" } else { "light" };
            self.app.preferences.theme = target.to_string();
            solidcraft_theme::set_dark(dark);
        }

        fn process_host_commands(&mut self) {
            for cmd in self.queue.drain() {
                match cmd {
                    HostCommand::OpenBytes { name, bytes } => {
                        let clean = name.trim().trim_start_matches('/').to_string();
                        let vpath = format!("/host/{clean}");
                        vfs::insert(&vpath, bytes);
                        self.app.home.open = false;
                        self.app.open_path_async(&vpath);
                    }
                    HostCommand::Screenshot { width, height, tx } => {
                        let res = control::view_png(&self.app, width as u32, height as u32);
                        let _ = tx.send(res);
                    }
                    HostCommand::Export { format, tx } => {
                        let res = self.export_bytes(&format);
                        let _ = tx.send(res);
                    }
                    HostCommand::Import { name, bytes, tx } => {
                        let res = self.import_bytes(&name, bytes);
                        let _ = tx.send(res);
                    }
                    HostCommand::SetTheme { dark } => {
                        self.set_theme(dark);
                    }
                    HostCommand::Request { method, params, tx } => {
                        self.handle_request(method, params, tx);
                    }
                }
            }
        }

        fn export_bytes(&self, format: &str) -> Result<Vec<u8>, String> {
            let fmt = format.trim().trim_start_matches('.').to_ascii_lowercase();
            let name = self.app.session.save_as_name();
            if fmt == "sc" || fmt == "design" {
                return solidcraft_io::write_design(&self.app.session.doc).map_err(|e| e.to_string());
            }
            let world = self.app.session.world_state().map_err(|e| e.to_string())?;
            if fmt == "glb" {
                return solidcraft_io::export_glb(
                    &self.app.session.doc,
                    &self.app.session.model.state(),
                    &world,
                    &[],
                    &name,
                    solidcraft_io::MeshQuality::Standard,
                )
                .map_err(|e| e.to_string());
            }
            let io_fmt = match fmt.as_str() {
                "step" | "stp" => solidcraft_io::Format::Step,
                "3mf" => solidcraft_io::Format::ThreeMf,
                "stl" => solidcraft_io::Format::StlBinary,
                "obj" => solidcraft_io::Format::Obj,
                "iges" | "igs" => solidcraft_io::Format::Iges,
                _ => return Err(format!("unsupported export format `{fmt}`")),
            };
            if io_fmt == solidcraft_io::Format::Step && !self.app.session.doc.occurrences.is_empty() {
                return solidcraft_io::step_assembly(
                    &self.app.session.doc,
                    &self.app.session.doc.painted(&self.app.session.model.state()),
                    &name,
                )
                .map_err(|e| e.to_string());
            }
            let painted = self.app.session.doc.painted(&world);
            solidcraft_io::export_with_metadata(&painted, &[], io_fmt, &name, &self.app.session.doc.metadata)
                .map_err(|e| e.to_string())
        }

        fn import_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Value, String> {
            let byte_len = bytes.len();
            let clean = name.trim().trim_start_matches('/').to_string();
            let vpath = format!("/import/{clean}");
            vfs::insert(&vpath, bytes);
            self.app.home.open = false;
            if solidcraft_io::is_design_path(&clean) {
                self.app.open_path_async(&vpath);
                return Ok(json!({ "ok": true, "name": clean, "byteLength": byte_len }));
            }
            self.app.insert_path_async(&vpath);
            Ok(json!({ "ok": true, "name": clean, "byteLength": byte_len }))
        }

        fn dispatch_control(&mut self, method: &str, params: Value, tx: Sender<Result<Value, String>>) {
            let (req, reply) = ControlRequest::new(method, params);
            if self.control_tx.send(req).is_err() {
                let _ = tx.send(Err("Fogbreak Solid control queue closed".to_string()));
                return;
            }
            self.pending_controls.push(PendingControl { reply, tx });
        }

        fn handle_request(&mut self, method: String, params: Value, tx: Sender<Result<Value, String>>) {
            match method.as_str() {
                "__ui_inspect" => {
                    let status = self.app.status.as_ref().map(|(s, _, _)| s.clone()).unwrap_or_default();
                    let _ = tx.send(Ok(json!({
                        "engineId": host_bridge::ENGINE_ID,
                        "hostMode": "page",
                        "theme": self.app.preferences.theme,
                        "gpu": self.app.viewport.gpu.is_some(),
                        "status": status,
                        "dirty": documents::any_dirty(&self.app),
                        "tab": self.app.ui.tab,
                        "designShown": toolbar::design_shown(&self.app),
                        "commandsCount": solidcraft_engine::cmd::command_specs().len(),
                    })));
                }
                "file.saveDesign" => match solidcraft_io::write_design(&self.app.session.doc).map_err(|e| e.to_string()) {
                    Ok(bytes) => {
                        let name = format!("{}.sc", self.app.session.save_as_name());
                        host_bridge::emit_save(&name, "sc", &bytes);
                        self.app.session.mark_saved();
                        let _ = tx.send(Ok(json!({ "name": name, "byteLength": bytes.len() })));
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err));
                    }
                },
                "view.fit" => {
                    self.dispatch_control("ui.view", json!({ "view": "fit" }), tx);
                }
                "__execute_batch" => {
                    let batch = if params.is_array() { json!({ "commands": params }) } else { params };
                    self.dispatch_control("engine.batch", batch, tx);
                }
                "engine.execute"
                | "command"
                | "engine.batch"
                | "engine.script"
                | "engine.commands"
                | "document.inspect"
                | "document.revision"
                | "document.export"
                | "document.import"
                | "document.saved"
                | "document.close" => {
                    self.dispatch_control(&method, params, tx);
                }
                _ if method.starts_with("ui.") || method.starts_with("control.") || method.starts_with("mcp.") => {
                    self.dispatch_control(&method, params, tx);
                }
                _ => {
                    self.dispatch_control("engine.execute", json!({ "command": method, "params": params }), tx);
                }
            }
        }

        fn poll_controls(&mut self, ctx: &egui::Context) {
            self.pending_controls.retain(|pending| match pending.reply.try_recv() {
                Ok(reply) => {
                    let res = if reply.get("ok").and_then(Value::as_bool) == Some(true) {
                        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(reply
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("Fogbreak Solid request failed")
                            .to_string())
                    };
                    let _ = pending.tx.send(res);
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => {
                    let _ = pending.tx.send(Err("Fogbreak Solid request dropped".to_string()));
                    false
                }
            });
            if !self.pending_controls.is_empty() {
                ctx.request_repaint();
            }
        }

        fn flush_vfs_saves(&mut self) {
            for (path, bytes) in vfs::take_writes() {
                let name = path.rsplit('/').next().unwrap_or("export.bin");
                let ext = name.rsplit('.').next().unwrap_or("bin").to_ascii_lowercase();
                host_bridge::emit_save(name, &ext, &bytes);
                if path.starts_with("/downloads/") {
                    let _ = host_bridge::trigger_download(name, &bytes);
                }
            }
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.process_host_commands();
            self.app.logic(ctx);
            self.poll_controls(ctx);
            self.flush_vfs_saves();
            host_bridge::emit_dirty(documents::any_dirty(&self.app));
        }

        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            let taps = crate::mouse_buttons::reconcile(&self.mouse_buttons, ctx, raw);
            window_chrome::release_stale_input(ctx, raw);
            let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
            if let Some(delay) = self.wheel_gestures.update(raw, now, self.app.preferences.browser_scroll_wheel) {
                ctx.request_repaint_after(delay);
            }
            let samples = std::mem::take(&mut *self.wheel_samples.borrow_mut());
            solidcraft_ui_egui::wheel_hint::feed(&mut self.app, ctx, &samples);
            for f in std::mem::take(&mut raw.dropped_files) {
                let name = f
                    .path()
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "dropped.sc".into());
                let ctx_c = ctx.clone();
                let queue_c = self.queue.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(bytes) = f.bytes_async().await {
                        queue_c.push(HostCommand::OpenBytes { name, bytes });
                        ctx_c.request_repaint();
                    }
                });
            }
            self.app.raw_input_hook_with_taps(ctx, raw, taps);
        }

        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.app.ui(ui);
            self.app.retain_view_pictures(frame.wgpu_render_state());
        }

        fn persist_egui_memory(&self) -> bool {
            false
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn prefers_dark() -> bool {
        let q = query();
        if q.contains("theme=light") {
            return false;
        }
        if q.contains("theme=dark") {
            return true;
        }
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .is_some_and(|m| m.matches())
    }

    fn install_viewport(app: &mut SolveApp, rs: &eframe::egui_wgpu::RenderState) {
        let backend = rs.adapter.get_info().backend;
        let depth = eframe::egui_wgpu::depth_format_from_bits(DEPTH_BITS, 0);
        let supported = eframe::egui_wgpu::supported_msaa_samples(&rs.adapter, &rs.device, rs.target_format, depth);
        let requested = gpu::scene_samples(backend, app.preferences.msaa, &supported);
        let drawn = app.set_wgpu_split(rs, DEPTH_BITS, 0, requested);
        app.msaa_supported = gpu::scene_sample_choices(backend, supported);
        log::info!("Fogbreak Solid graphics: {backend:?}, MSAA {drawn}x (saved {}x)", app.preferences.msaa);
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        solidcraft_ui_egui::i18n::install();
        solidcraft_look::install_tables();
        solidcraft_ui_egui::workspace::install_tabs();
        solidcraft_ui_egui::navigation_slots::install();

        let canvas = match host_bridge::find_page_canvas() {
            Ok(c) => c,
            Err(err) => {
                log::error!("Fogbreak Solid host error: {err}");
                return;
            }
        };
        let dark = prefers_dark();
        let queue = CommandQueue::default();

        wasm_bindgen_futures::spawn_local(async move {
            let Some(window) = web_sys::window() else { return };
            let Some(document) = window.document() else { return };

            let mut options = eframe::WebOptions { depth_buffer: 0, ..Default::default() };
            if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
                if query().contains("webgl") {
                    create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
                }
                let base = create.device_descriptor.clone();
                create.device_descriptor = std::sync::Arc::new(move |adapter| {
                    let mut d = base(adapter);
                    d.required_features |= adapter.features() & eframe::wgpu::Features::INDIRECT_FIRST_INSTANCE;
                    d
                });
            }

            let mouse_canvas = canvas.clone();
            let queue_for_cb = queue.clone();
            let result = eframe::WebRunner::new()
                .start(
                    canvas.clone(),
                    options,
                    Box::new(move |cc| {
                        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
                        gpu::install_cpu_picture_renderer();
                        host_bridge::install_rpc(queue_for_cb.clone(), cc.egui_ctx.clone());
                        host_bridge::listen_host_events(&canvas, queue_for_cb.clone(), cc.egui_ctx.clone());

                        let services = Services {
                            pick_open: Some(Box::new(|exts: &[&str]| host_bridge::pick_upload(exts))),
                            pick_save: Some(Box::new(|name: &str, _exts: &[&str]| {
                                Box::pin(std::future::ready(Some(format!("/downloads/{name}"))))
                            })),
                            download_design: Some(Box::new(|name: &str, bytes: &[u8]| {
                                host_bridge::emit_save(name, "sc", bytes);
                                host_bridge::trigger_download(name, bytes)
                            })),
                            open_bytes: Some(Box::new(|name: &str, media_type: &str, bytes: &[u8]| {
                                host_bridge::open_bytes_in_tab(name, media_type, bytes)
                            })),
                            run_cloud_plugin: Some(Box::new(|plugin_id: &str, entry: &str, params: &Value| {
                                host_bridge::emit_plugin_run(plugin_id, entry, params)
                            })),
                            ..Default::default()
                        };

                        let (control_tx, control_rx) = std::sync::mpsc::channel();
                        let mut session = Session::new(Document::new("Untitled"));
                        if query().contains("sample") {
                            let _ = session.run_script(&solidcraft_engine::sample::script());
                            session.doc_mut().name = "Sample Plate".into();
                            session.mark_saved();
                        }

                        let mut app = SolveApp::new(session, services).with_control(control_rx);
                        app.preferences.theme = if dark { "dark" } else { "light" }.to_string();
                        solidcraft_theme::set_dark(dark);
                        prefs::apply_new_design(&mut app);
                        app.home.open = false;

                        if let Some(rs) = &cc.wgpu_render_state {
                            install_viewport(&mut app, rs);
                        }

                        let mouse_buttons = crate::mouse_buttons::install(&window, &mouse_canvas, &cc.egui_ctx);
                        let wheel_samples = crate::wheel_samples::install(&window);

                        if let Some(el) = document.get_element_by_id(LOADING_ID) {
                            el.remove();
                        }
                        let mut caps: Vec<String> = solidcraft_engine::cmd::command_specs()
                            .into_iter()
                            .map(|c| c.id.to_string())
                            .collect();
                        caps.push("plugin.run".to_string());
                        host_bridge::emit_ready(&caps);

                        Ok(Box::new(Shell {
                            app,
                            queue: queue_for_cb,
                            control_tx,
                            pending_controls: Vec::new(),
                            wheel_gestures: solidcraft_ui_egui::WebWheelGestures::default(),
                            wheel_samples,
                            mouse_buttons,
                        }))
                    }),
                )
                .await;

            if let Err(err) = result {
                log::error!("Fogbreak Solid start failed: {err:?}");
                if let Some(el) = document.get_element_by_id(LOADING_ID) {
                    el.set_inner_html(&format!("<p>Fogbreak Solid failed to start: {err:?}</p>"));
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    solidcraft_engine::kernel::start();
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
