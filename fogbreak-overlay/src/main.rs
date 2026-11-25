//! Fogbreak Solid Studio browser runner (clean-room MIT OR Apache-2.0 over `solidcraft-engine` + `solidcraft-viewport-egui`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod host_bridge;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};
    use solidcraft_doc::Document;
    use solidcraft_engine::Session;
    use solidcraft_theme::Tokens;
    use solidcraft_viewport::{Camera, MAX_SECTION_PLANES, Scene, StandardView, render_png};
    use solidcraft_viewport_egui::{ViewportCallback, install, paint_background};
    use solidcraft_viewport_gpu::capture::scene_geometry;
    use solidcraft_viewport_gpu::view::{AO_SAMPLES, Frame, GpuTarget, SceneSlot};

    use crate::host_bridge::{self, CommandQueue, HostCommand, LOADING_ID};

    fn fit_camera(session: &Session, camera: &mut Camera) {
        if let Ok(b) = solidcraft_engine::view::bounds(session)
            && !b.is_empty()
        {
            camera.fit(&solidcraft_engine::render::view_bounds(&b));
        }
    }

    struct StudioApp {
        session: Session,
        camera: Camera,
        queue: CommandQueue,
        scene_slot: SceneSlot,
        empty_slot: SceneSlot,
        scene_key: u64,
        cached_scene: Option<Scene>,
        has_gpu: bool,
        dark: bool,
        status: String,
    }

    impl StudioApp {
        fn new(queue: CommandQueue, dark: bool) -> Self {
            let mut session = Session::new(Document::new("Untitled"));
            if let Some(first) = solidcraft_engine::sample::samples().into_iter().next() {
                let _ = session.run_script(&(first.script)());
            }
            let mut camera = Camera::default();
            let cached_scene = solidcraft_engine::view::scene(&session, &camera).ok();
            fit_camera(&session, &mut camera);
            let gpu_scene = cached_scene.as_ref().map(scene_geometry);
            Self {
                session,
                camera,
                queue,
                scene_slot: Arc::new(Mutex::new(gpu_scene)),
                empty_slot: Arc::new(Mutex::new(None)),
                scene_key: 1,
                cached_scene,
                has_gpu: false,
                dark,
                status: "Ready".to_string(),
            }
        }

        fn rebuild_scene(&mut self) {
            if let Ok(sc) = solidcraft_engine::view::scene(&self.session, &self.camera) {
                let gpu_sc = scene_geometry(&sc);
                if let Ok(mut slot) = self.scene_slot.lock() {
                    *slot = Some(gpu_sc);
                }
                self.cached_scene = Some(sc);
                self.scene_key = self.scene_key.wrapping_add(1);
            }
        }

        fn process_commands(&mut self) {
            for cmd in self.queue.drain() {
                match cmd {
                    HostCommand::OpenBytes { name, bytes } => match solidcraft_io::read_design(&bytes) {
                        Ok(doc) => {
                            self.session = Session::new(doc);
                            self.rebuild_scene();
                            fit_camera(&self.session, &mut self.camera);
                            self.status = format!("Opened {name}");
                        }
                        Err(err) => {
                            self.status = format!("Open error: {err}");
                        }
                    },
                    HostCommand::Screenshot { width, height, tx } => {
                        let res = self
                            .cached_scene
                            .as_ref()
                            .and_then(|sc| render_png(sc, &self.camera, width, height))
                            .ok_or_else(|| "Unable to render PNG screenshot".to_string());
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
                    HostCommand::Request { method, params, tx } => {
                        let out = self.handle_request(&method, &params);
                        let _ = tx.send(out);
                    }
                }
            }
        }

        fn export_bytes(&self, format: &str) -> Result<Vec<u8>, String> {
            let fmt = format.trim().trim_start_matches('.').to_ascii_lowercase();
            let name = self.session.save_as_name();
            if fmt == "sc" || fmt == "design" {
                return solidcraft_io::write_design(&self.session.doc).map_err(|e| e.to_string());
            }
            let world = self.session.world_state().map_err(|e| e.to_string())?;
            if fmt == "glb" {
                return solidcraft_io::export_glb(
                    &self.session.doc,
                    &self.session.model.state(),
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
            if io_fmt == solidcraft_io::Format::Step && !self.session.doc.occurrences.is_empty() {
                return solidcraft_io::step_assembly(
                    &self.session.doc,
                    &self.session.doc.painted(&self.session.model.state()),
                    &name,
                )
                .map_err(|e| e.to_string());
            }
            let painted = self.session.doc.painted(&world);
            solidcraft_io::export_with_metadata(&painted, &[], io_fmt, &name, &self.session.doc.metadata)
                .map_err(|e| e.to_string())
        }

        fn import_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Value, String> {
            let byte_len = bytes.len();
            if solidcraft_io::is_design_path(name) {
                let doc = solidcraft_io::read_design(&bytes).map_err(|e| e.to_string())?;
                self.session = Session::new(doc);
                self.rebuild_scene();
                fit_camera(&self.session, &mut self.camera);
                self.status = format!("Opened {name}");
                return Ok(json!({ "ok": true, "name": name, "byteLength": byte_len }));
            }
            if solidcraft_io::is_step_path(name) || solidcraft_io::is_mesh_path(name) {
                let cmd = if solidcraft_io::is_step_path(name) { "file.insert_step" } else { "file.insert_mesh" };
                let vpath = format!("/import/{name}");
                solidcraft_io::vfs::insert(&vpath, bytes);
                let res = self.session.execute(cmd, &json!({ "path": vpath })).map_err(|e| e.to_string());
                let _ = solidcraft_io::vfs::remove(&vpath);
                let val = res?;
                self.rebuild_scene();
                fit_camera(&self.session, &mut self.camera);
                self.status = format!("Imported {name}");
                return Ok(json!({ "ok": true, "name": name, "byteLength": byte_len, "result": val }));
            }
            let doc = solidcraft_io::read_design(&bytes).map_err(|e| e.to_string())?;
            self.session = Session::new(doc);
            self.rebuild_scene();
            fit_camera(&self.session, &mut self.camera);
            self.status = format!("Opened {name}");
            Ok(json!({ "ok": true, "name": name, "byteLength": byte_len }))
        }

        fn handle_request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
            match method {
                "__ui_inspect" => Ok(json!({
                    "engineId": host_bridge::ENGINE_ID,
                    "hostMode": "page",
                    "theme": if self.dark { "dark" } else { "light" },
                    "gpu": self.has_gpu,
                    "status": self.status,
                })),
                "__execute_batch" => {
                    let results = self.session.run_script(params).map_err(|e| e.to_string())?;
                    self.rebuild_scene();
                    Ok(Value::Array(results))
                }
                "file.saveDesign" => {
                    let bytes = solidcraft_io::write_design(&self.session.doc).map_err(|e| e.to_string())?;
                    let name = format!("{}.sc", self.session.save_as_name());
                    host_bridge::emit_save(&name, "sc", &bytes);
                    Ok(json!({ "name": name, "byteLength": bytes.len() }))
                }
                "view.fit" => {
                    fit_camera(&self.session, &mut self.camera);
                    Ok(json!({ "ok": true }))
                }
                _ => {
                    let res = self.session.execute(method, params).map_err(|e| e.to_string())?;
                    self.rebuild_scene();
                    Ok(res)
                }
            }
        }
    }

    impl eframe::App for StudioApp {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            solidcraft_theme::set_dark(self.dark);
            solidcraft_theme::apply(ctx);
            self.process_commands();
            host_bridge::emit_dirty(self.session.is_dirty());
        }

        fn persist_egui_memory(&self) -> bool {
            false
        }

        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let t = Tokens::get();
            egui::Panel::top("fogbreak_solid_bar").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Fogbreak Solid Studio");
                    ui.separator();
                    if ui.button("Fit").clicked() {
                        fit_camera(&self.session, &mut self.camera);
                    }
                    if ui.button("Iso").clicked() {
                        let (yaw, pitch) = StandardView::Iso.angles();
                        self.camera.yaw = yaw;
                        self.camera.pitch = pitch;
                    }
                    if ui.button("Save .sc").clicked()
                        && let Ok(bytes) = solidcraft_io::write_design(&self.session.doc)
                    {
                        let name = format!("{}.sc", self.session.save_as_name());
                        host_bridge::emit_save(&name, "sc", &bytes);
                    }
                    if ui.button(if self.dark { "Light" } else { "Dark" }).clicked() {
                        self.dark = !self.dark;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&self.status).color(t.text_dim));
                    });
                });
            });

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                    if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Secondary) {
                        let d = resp.drag_delta();
                        self.camera.orbit(f64::from(d.x) * 0.01, f64::from(d.y) * 0.01);
                    } else if resp.dragged_by(egui::PointerButton::Middle) {
                        let d = resp.drag_delta();
                        self.camera.pan(f64::from(d.x), f64::from(d.y), f64::from(rect.height().max(1.0)));
                    }
                    if resp.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll.abs() > 0.01 {
                            let factor = (1.0 - f64::from(scroll) * 0.002).clamp(0.5, 2.0);
                            self.camera.zoom_at(factor, None);
                        }
                    }

                    paint_background(ui.painter(), rect, t.viewport_top, t.viewport_bottom);
                    if self.has_gpu && rect.width() > 2.0 && rect.height() > 2.0 {
                        let ppp = ui.ctx().pixels_per_point();
                        let aspect = f64::from(rect.width()) / f64::from(rect.height().max(1.0));
                        let radius = self.cached_scene.as_ref().map_or(100.0, |s| s.radius.max(1.0));
                        let vp = self.camera.view_proj(aspect, radius);
                        let eye = self.camera.eye().to_f32();
                        let frame = Frame {
                            product_lights: None,
                            product: false,
                            upload_error: None,
                            key: self.scene_key,
                            slot: self.scene_slot.clone(),
                            sketch_key: 0,
                            sketch_slot: self.empty_slot.clone(),
                            hl_key: 0,
                            hl_slot: self.empty_slot.clone(),
                            pv_key: 0,
                            pv_slot: self.empty_slot.clone(),
                            pv_hide: false,
                            preview_bounds: None,
                            view_proj: vp.to_f32(),
                            turned_depth: Some(vp.turned_depth_f32()),
                            back: self.camera.back().to_f32(),
                            size_px: [rect.width() * ppp, rect.height() * ppp],
                            clip: [[0.0; 4]; MAX_SECTION_PLANES],
                            clip_hatch: [0; MAX_SECTION_PLANES],
                            cap: [0.03, 0.50, 0.61, 0.0],
                            images: Vec::new(),
                            clouds: Vec::new(),
                            analysis: [0.0; 8],
                            analysis_colors: [0.0; 36],
                            access: None,
                            outline: true,
                            eye: [eye[0], eye[1], eye[2], if self.camera.fov > 0.0 { 1.0 } else { 0.0 }],
                            ao_radius: 0.0,
                            ao_samples: AO_SAMPLES,
                            ssaa: 1.0,
                            hover_tint: [0.0; 3],
                            selection_tint: [0.03, 0.50, 0.61],
                            hatch: Default::default(),
                            sky: [0.0, 0.0, 1.0, 1.0],
                            moving: None,
                            motions: Default::default(),
                            ground: None,
                        };
                        ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, ViewportCallback(frame)));
                    }
                });
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

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
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
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let mut options = eframe::WebOptions::default();
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }

            let queue_for_cb = queue.clone();
            let result = eframe::WebRunner::new()
                .start(
                    canvas.clone(),
                    options,
                    Box::new(move |cc| {
                        solidcraft_theme::set_dark(dark);
                        solidcraft_theme::apply(&cc.egui_ctx);
                        host_bridge::install_rpc(queue_for_cb.clone(), cc.egui_ctx.clone());
                        host_bridge::listen_open(&canvas, queue_for_cb.clone(), cc.egui_ctx.clone());

                        let mut app = StudioApp::new(queue_for_cb, dark);
                        if let Some(rs) = &cc.wgpu_render_state {
                            let scene_target = GpuTarget {
                                format: rs.target_format,
                                depth: egui_wgpu::depth_format_from_bits(24, 0),
                                samples: 1,
                                tess: false,
                                oit: false,
                                edges: false,
                            };
                            let window_target = GpuTarget {
                                format: rs.target_format,
                                depth: None,
                                samples: 1,
                                tess: false,
                                oit: false,
                                edges: false,
                            };
                            let _ = install(rs, scene_target, window_target, false, false, |_| {});
                            app.has_gpu = true;
                        }

                        let caps: Vec<String> = solidcraft_engine::cmd::command_specs()
                            .into_iter()
                            .map(|c| c.id.to_string())
                            .collect();
                        host_bridge::emit_ready(&caps);
                        Ok(Box::new(app))
                    }),
                )
                .await;

            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => el.set_inner_html(&format!("<p>Fogbreak Solid Studio failed to start: {e:?}</p>")),
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("fogbreak-solid runs in the browser via trunk build --release");
}
