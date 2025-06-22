#![warn(clippy::pedantic)]
mod renderer;

use csgrs::csg::CSG;
use eframe::egui;
use futures::channel::oneshot;
use geo::{Geometry, LineString};
use js_sys::Uint8Array;
use log::Level;
use nalgebra::{Matrix4, Perspective3, Point3, Translation3, UnitQuaternion, Vector3};
use once_cell::sync::OnceCell;
use std::{
    cell::RefCell,
    f32::consts::{FRAC_PI_2, PI},
    future::Future,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, HtmlInputElement, window};
use glow::{Context, HasContext as _};

const INVALID_SCALE: Vector3<f32> = Vector3::new(-1.0, -1.0, -1.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Control,
    Diagnostics,
    Design,
}

pub struct AluminaApp {
    rotation: UnitQuaternion<f32>,
    translation: egui::Vec2,
    zoom: f32,
    /// Un‑scaled geometry as loaded from disk (or the default icosahedron).
    base_model: CSG<()>,
    /// Geometry that is actually rendered (scaled version of `base_model`).
    model: CSG<()>,
    /// Desired scale factors set by the user (per‑axis, 1 = no change).
    model_scale: Vector3<f32>,
    /// Last scale that was applied to `model` – lets us avoid needless rebuilds.
    applied_scale: Vector3<f32>,
    /// User-controlled translation (mm)
    model_offset: Vector3<f32>,
    /// Last translation that was applied to `model`
    applied_offset: Vector3<f32>,
    workpiece_data: Arc<Mutex<Option<Vec<u8>>>>,
    model_data: Arc<Mutex<Option<Vec<u8>>>>,
    wireframe: bool,
    edges: bool,
    faces: bool,
    normals: bool,
    vertices: bool,
    workarea: bool,
    /// CNC working area dimensions (mm)
    work_size: Vector3<f32>, // x, y, z
    layer_height: f32,
    /// Index of the layer currently being inspected (0-based)
    current_layer: i32,
    /// `true` while the “tool-path” view is active
    show_slice: bool,
    /// The last slice that was generated for `current_layer`
    sliced_layer: Option<CSG<()>>,
    gpu: Option<Arc<Mutex<renderer::GpuLines>>>,
    gpu_faces:Option<Arc<Mutex<renderer::GpuLines>>>,
    vertex_storage: Vec<f32>,
    debug_id: u32,
    selected_tab: Tab,
    diag_poll: bool,
    diag_led: bool,
}

impl AluminaApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let base_model = CSG::<()>::icosahedron(100.0, None).float();
        let model_scale = Vector3::new(1.0, 1.0, 1.0);

        // ------------------------------------------------------------------
        // Default camera
        //   • orient like the “Front” toolbar button
        //   • zoom-in so the work-area almost fills the 60° frustum
        //
        //     With the eye placed at 3·radius, the model *exactly* fits when
        //       zoom = 3 · tan(fov / 2)  ≈ 1.732 …
        //     Using a touch more distance (1.75) leaves a 2–3 % safety margin.
        // ------------------------------------------------------------------
        let front_rot = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -FRAC_PI_2); // “Front”
        let initial_zoom = 1.75_f32;

        // ----- assign a unique id ------------------------------------------------
        static INSTANCE_SEQ: AtomicU32 = AtomicU32::new(0);
        let id = INSTANCE_SEQ.fetch_add(1, Ordering::SeqCst);

        Self {
            rotation: front_rot,
            translation: egui::Vec2::new(0.0, -250.0),
            zoom: initial_zoom,
            base_model: base_model.clone(),
            model: base_model,
            model_scale,
            applied_scale: model_scale,
            model_offset: Vector3::zeros(),
            applied_offset: Vector3::zeros(),
            workpiece_data: Arc::new(Mutex::new(None)),
            model_data: Arc::new(Mutex::new(None)),
            wireframe: true,
			edges: true,
			faces: true,
			normals: true,
			vertices: true,
			workarea: true,
            work_size: Vector3::new(200.0, 200.0, 200.0),
            layer_height: 0.20,
            current_layer: 0,
            show_slice: false,
            sliced_layer: None,
            gpu: None,
            gpu_faces:None,
            vertex_storage: Vec::new(),
            debug_id: id,
            selected_tab: Tab::Control,
            diag_poll: false,
            diag_led: false,
        }
    }

    /// Re‑creates the renderable `model` if the requested scale or translation has changed.
    fn refresh_model(&mut self) {
        if self.model_scale != self.applied_scale || self.model_offset != self.applied_offset {
            self.model = self
                .base_model
                .clone()
                .scale(
                    self.model_scale.x.into(),
                    self.model_scale.y.into(),
                    self.model_scale.z.into(),
                )
                .translate(
                    self.model_offset.x.into(),
                    self.model_offset.y.into(),
                    self.model_offset.z.into(),
                );
            self.applied_scale = self.model_scale;
            self.applied_offset = self.model_offset;
        }
    }

    /// Re-builds `sliced_layer` for the current Z level.
    fn refresh_slice(&mut self) {
        if !self.show_slice {
            return;
        }

        let z = self.current_layer as f32 * self.layer_height;
        let plane = csgrs::plane::Plane::from_normal(Vector3::z(), z.into());
        self.sliced_layer = Some(self.model.slice(plane));
    }

    /// Marks `model` as dirty so that next frame will rebuild
    fn invalidate_model(&mut self) {
        self.applied_scale = INVALID_SCALE;
        self.applied_offset = Vector3::repeat(f32::NAN);
    }

    /// Hard-reset base_model *and* force a rebuild/slice
    fn set_base_model(&mut self, csg: CSG<()>) {
        self.base_model = csg;
        self.invalidate_model();
        self.refresh_model();
        self.refresh_slice();
    }
}

impl AluminaApp {
    /// (Re-)builds the VBO if the model, grid or scale changed.
    unsafe fn sync_buffers(&mut self, gl: &glow::Context) {
        self.vertex_storage.clear();
        let mut faces: Vec<f32> = Vec::new();

        // ── 1) grid (10 mm spacing, ±work_size/2) ───────────────────────
        if self.workarea {
            let minor = [0.55, 0.55, 0.55];
            let major = [1.0, 1.0, 1.0];

            let hx = self.work_size.x * 0.5;
            let hy = self.work_size.y * 0.5;
            let hz = self.work_size.z;

            // vertical (X) lines
            for i in 0..=(self.work_size.x / 10.0) as i32 {
                let x = -hx + i as f32 * 10.0;
                let col = if i % 10 == 0 { major } else { minor };
                self.vertex_storage.extend_from_slice(&[
                    x, -hy, 0.0, col[0], col[1], col[2], x, hy, 0.0, col[0], col[1], col[2],
                ]);
            }

            // horizontal (Y) lines
            for i in 0..=(self.work_size.y / 10.0) as i32 {
                let y = -hy + i as f32 * 10.0;
                let col = if i % 10 == 0 { major } else { minor };
                self.vertex_storage.extend_from_slice(&[
                    -hx, y, 0.0, col[0], col[1], col[2], hx, y, 0.0, col[0], col[1], col[2],
                ]);
            }

            // outline the remaining cuboid edges
            let edge = major;

            //  four vertical edges
            for (sx, sy) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
                let x = sx * hx;
                let y = sy * hy;
                self.vertex_storage.extend_from_slice(&[
                    x, y, 0.0, edge[0], edge[1], edge[2], // bottom
                    x, y, hz, edge[0], edge[1], edge[2], // top
                ]);
            }

            //  top rectangle (Z = hz)
            self.vertex_storage.extend_from_slice(&[
                -hx, -hy, hz, edge[0], edge[1], edge[2], hx, -hy, hz, edge[0], edge[1], edge[2],
                hx, -hy, hz, edge[0], edge[1], edge[2], hx, hy, hz, edge[0], edge[1], edge[2], hx,
                hy, hz, edge[0], edge[1], edge[2], -hx, hy, hz, edge[0], edge[1], edge[2], -hx, hy,
                hz, edge[0], edge[1], edge[2], -hx, -hy, hz, edge[0], edge[1], edge[2],
            ]);
        }

        // ── 2) model / slice ──────────────────────────────────────────────
        fn add_line_string(ls: &LineString<f64>, z: f32, col: [f32; 3], out: &mut Vec<f32>) {
            for w in ls.0.windows(2) {
                let a = w[0];
                let b = w[1];
                out.extend_from_slice(&[
                    a.x as f32, a.y as f32, z, col[0], col[1], col[2], b.x as f32, b.y as f32, z,
                    col[0], col[1], col[2],
                ]);
            }
        }

        if self.show_slice {
            const PURPLE: [f32; 3] = [0.6, 0.1, 0.8];
            if let Some(slice) = &self.sliced_layer {
                let z = self.current_layer as f32 * self.layer_height;

                for geom in &slice.geometry.0 {
                    match geom {
                        Geometry::LineString(ls) => {
                            add_line_string(ls, z, PURPLE, &mut self.vertex_storage)
                        }
                        Geometry::Polygon(poly) => {
                            add_line_string(&poly.exterior(), z, PURPLE, &mut self.vertex_storage);
                            for inner in poly.interiors() {
                                add_line_string(inner, z, PURPLE, &mut self.vertex_storage);
                            }
                        }
                        _ => {} // ignore points etc.
                    }
                }
            }
        } else {
			/* ---------- model wire-frame (edges) ----------------------------- */
			if self.edges {
				const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
				for p in &self.model.polygons {
					for (a, b) in p.edges() {
						self.vertex_storage.extend_from_slice(&[
							a.pos.x as f32, a.pos.y as f32, a.pos.z as f32,
							WHITE[0], WHITE[1], WHITE[2],
							b.pos.x as f32, b.pos.y as f32, b.pos.z as f32,
							WHITE[0], WHITE[1], WHITE[2],
						]);
					}
				}
			}

			/* ---------- model faces (solid) ---------------------------------- */
			if self.faces {
				for p in &self.model.polygons {
					let verts = &p.vertices;
					if verts.len() >= 3 {
						for i in 1..verts.len() - 1 {
							for v in [&verts[0].pos, &verts[i].pos, &verts[i + 1].pos] {
								faces.extend_from_slice(&[
									v.x as f32,
									v.y as f32,
									v.z as f32,
									renderer::EGUI_BLUE[0],
									renderer::EGUI_BLUE[1],
									renderer::EGUI_BLUE[2],
								]);
							}
						}
					}
				}
			}
		}

		// ---------- upload / (re-)create VBOs -----------------------------------
		if let Some(lines_gpu) = &self.gpu {
			if let Ok(mut g) = lines_gpu.lock() {
				g.upload_vertices(gl, &self.vertex_storage);
			}
		}

		// Faces VBO is present only while “faces” is checked
		if self.faces && !faces.is_empty() {
			let faces_gpu = self
				.gpu_faces
				.get_or_insert_with(|| Arc::new(Mutex::new(unsafe { renderer::GpuLines::new(gl) })));
			if let Ok(mut g) = faces_gpu.lock() {
				g.upload_vertices(gl, &faces);
			}
		} else {
			self.gpu_faces = None; // turn off solid drawing
		}
    }
}

impl eframe::App for AluminaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, Tab::Diagnostics, "Diagnostics");
                ui.selectable_value(&mut self.selected_tab, Tab::Design, "Design");
                ui.selectable_value(&mut self.selected_tab, Tab::Control, "Control");
            });
        });

        match self.selected_tab {
            Tab::Control => {
                // ------------------------------------------------------------------
                // Sidebar
                // ------------------------------------------------------------------
                egui::SidePanel::left("side_panel")
                    .resizable(false)
                    .min_width(140.0)
                    .show(ctx, |ui| {
                        ui.heading("Control");

                        ui.separator();
                        ui.label("Snap view");
                        ui.horizontal_wrapped(|ui| {
                            let pitch =
                                UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -FRAC_PI_2); //  -90° about X  (Z-up ➜ Y-up)
                            if ui.button("Front").clicked() {
                                self.rotation = pitch;
                            } //  -90° about X
                            if ui.button("Back").clicked() {
                                self.rotation =
                                    pitch * UnitQuaternion::from_axis_angle(&Vector3::z_axis(), PI);
                            } // 180° roll
                            if ui.button("Left").clicked() {
                                self.rotation =
                                    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), FRAC_PI_2)
                                        * pitch;
                            } // +90° yaw
                            if ui.button("Right").clicked() {
                                self.rotation =
                                    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -FRAC_PI_2)
                                        * pitch;
                            } // –90° yaw
                            if ui.button("Top").clicked() {
                                self.rotation = UnitQuaternion::identity();
                            } // no change
                            if ui.button("Bottom").clicked() {
                                self.rotation =
                                    UnitQuaternion::from_axis_angle(&Vector3::x_axis(), PI);
                            } // look from below
                        });

                        ui.separator();
                        ui.checkbox(&mut self.edges, "edges");
                        ui.checkbox(&mut self.faces, "faces");
                        ui.checkbox(&mut self.normals, "normals");
                        ui.checkbox(&mut self.vertices, "vertices");
                        ui.checkbox(&mut self.workarea, "Work area");

                        // ────────────── Scale Controls ──────────────
                        ui.separator();
                        ui.collapsing("Model scale", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("X:");
                                ui.add(
                                    egui::DragValue::new(&mut self.model_scale.x)
                                        .speed(0.01)
                                        .clamp_range(0.01..=100.0),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y:");
                                ui.add(
                                    egui::DragValue::new(&mut self.model_scale.y)
                                        .speed(0.01)
                                        .clamp_range(0.01..=100.0),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("Z:");
                                ui.add(
                                    egui::DragValue::new(&mut self.model_scale.z)
                                        .speed(0.01)
                                        .clamp_range(0.01..=100.0),
                                );
                            });

                            if ui.button("Reset scale").clicked() {
                                self.model_scale = Vector3::new(1.0, 1.0, 1.0);
                            }
                        });

                        // ────────────── Position Controls ──────────────
                        ui.separator();
                        ui.collapsing("Model position", |ui| {
                            if ui.button("Float (Z = 0)").clicked() {
                                self.model_offset = Vector3::zeros();
                                self.set_base_model(self.base_model.clone().float());
                            }

                            if ui.button("Center").clicked() {
                                self.model_offset = Vector3::zeros();
                                self.set_base_model(self.base_model.clone().center());
                            }

                            ui.horizontal(|ui| {
                                ui.label("X:");
                                ui.add(egui::DragValue::new(&mut self.model_offset.x).speed(1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y:");
                                ui.add(egui::DragValue::new(&mut self.model_offset.y).speed(1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Z:");
                                ui.add(egui::DragValue::new(&mut self.model_offset.z).speed(1.0));
                            });

                            if ui.button("Reset position").clicked() {
                                self.model_offset = Vector3::zeros();
                            }
                        });

                        ui.separator();
                        ui.collapsing("Work area (mm)", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("X:");
                                ui.add(egui::DragValue::new(&mut self.work_size.x).speed(1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y:");
                                ui.add(egui::DragValue::new(&mut self.work_size.y).speed(1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Z:");
                                ui.add(egui::DragValue::new(&mut self.work_size.z).speed(1.0));
                            });
                        });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Layer height (mm):");
                            ui.add(
                                egui::DragValue::new(&mut self.layer_height)
                                    .speed(0.01)
                                    .clamp_range(0.01..=10.0),
                            );
                        });

                        ui.horizontal(|ui| {
                            let max_layers = (self.work_size.z / self.layer_height).floor() as i32;
                            let prev = self.current_layer;
                            ui.label("Current layer:");
                            ui.add(
                                egui::DragValue::new(&mut self.current_layer)
                                    .clamp_range(0..=max_layers)
                                    .speed(1),
                            );
                            if self.current_layer != prev {
                                self.refresh_slice();
                            }
                        });
                        if ui.checkbox(&mut self.show_slice, "slice").changed() {
                            self.refresh_slice();
                        }

                        ui.separator();
                        if ui.button("load workpiece").clicked() {
                            spawn_file_picker(
                                Arc::clone(&self.workpiece_data),
                                "Workpiece mesh (stl,dxf)",
                                &["stl", "dxf"],
                            );
                        }

                        if ui.button("load model").clicked() {
                            spawn_file_picker(
                                Arc::clone(&self.model_data),
                                "Model mesh (stl,dxf)",
                                &["stl", "dxf"],
                            );
                        }
                        if ui.button("send").clicked() {
                            // TODO: implement send action
                            log::info!("'send' button pressed");
                        }
                        if ui.button("toggle").clicked() {
                            // Example: toggle wireframe state when this button is pressed
                            self.wireframe = !self.wireframe;
                        }
                    });

                // ── workpiece ────────────────────────────────────────────────
                let workpiece_bytes_opt = {
                    let mut guard = self.workpiece_data.lock().unwrap();
                    guard.take()
                };
                if let Some(bytes) = workpiece_bytes_opt {
                    match load_csg_from_bytes(&bytes) {
                        Some(csg) => {
                            self.set_base_model(csg.float());
                            log::info!(
                                "[alumina] app#{}   workpiece loaded → {} bytes, poly={}",
                                self.debug_id,
                                bytes.len(),
                                self.base_model.polygons.len()
                            );
                        }
                        None => {
                            log::error!("Could not parse workpiece file – unsupported or corrupt")
                        }
                    }
                }

                // ── model ────────────────────────────────────────────────────
                let model_bytes_opt = {
                    let mut guard = self.model_data.lock().unwrap();
                    guard.take()
                };
                if let Some(bytes) = model_bytes_opt {
                    match load_csg_from_bytes(&bytes) {
                        Some(csg) => {
                            self.set_base_model(csg.float());
                            log::info!(
                                "[alumina] app#{}   model loaded → {} bytes, poly={}",
                                self.debug_id,
                                bytes.len(),
                                self.base_model.polygons.len()
                            );
                        }
                        None => log::error!("Could not parse model file – unsupported or corrupt"),
                    }
                }

                // Apply scaling if the user changed any of the factors -------------
                self.refresh_model();
                self.refresh_slice();

                // ------------------------------------------------------------------
                // Main viewport
                // ------------------------------------------------------------------
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_min_size(ui.available_size());
                    let (rect, response) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

                    // ───── Interaction ─────
                    if response.dragged() {
                        let delta = response.drag_delta();
                        let input = ui.input(|i| i.clone());
                        if input.pointer.primary_down() {
                            // left‑drag → rotate
                            let yaw = delta.x * 0.01;
                            let pitch = delta.y * 0.01;
                            self.rotation =
                                UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw)
                                    * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch)
                                    * self.rotation;
                        } else if input.pointer.secondary_down() {
                            // right‑drag → pan
                            self.translation += -delta;
                        }
                    }

                    // scroll → zoom
                    let scroll = ui.input(|i| i.raw_scroll_delta.y);
                    if scroll.abs() > 0.0 {
                        self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.0, 500.0);
                    }

                    // ------------------------------------------------------------------
                    // Ask egui for the GL context once per frame
                    // ------------------------------------------------------------------
                    if let Some(gl) = _frame.gl() {
                        // ── 1) create once ─────────────────────────────────────────────
                        if self.gpu.is_none() {
                            self.gpu =
                                Some(Arc::new(Mutex::new(unsafe { renderer::GpuLines::new(gl) })));
                        }

                        // ── 2) keep vertex buffer in sync ─────────────────────────────
                        unsafe { self.sync_buffers(gl) };

                        // ── 3) schedule GL paint right after egui’s own meshes ────────
                        if let Some(lines_gpu)=&self.gpu{
							let lines_gpu = lines_gpu.clone();
							let faces_gpu = self.gpu_faces.clone();
                            let mvp = mvp(self, rect); // copy for the closure

                            let callback = egui_glow::CallbackFn::new(move |_info, painter| {
                                let gl = painter.gl();
								unsafe{
									gl.enable(glow::DEPTH_TEST);
									gl.depth_func(glow::LEQUAL);
									gl.clear(glow::DEPTH_BUFFER_BIT);

									// draw filled faces first (slight offset keeps outlines crisp)
									if let Some(faces_gpu)=&faces_gpu{
										if let Ok(f)=faces_gpu.lock(){
											gl.enable(glow::POLYGON_OFFSET_FILL);
											gl.polygon_offset(1.0,1.0);
											f.paint_tris(gl,mvp);
											gl.disable(glow::POLYGON_OFFSET_FILL);
										}
									}
									// then draw outlines
									if let Ok(l)=lines_gpu.lock(){ l.paint(gl,mvp); }
                                }
                            });

                            ui.painter().add(egui::PaintCallback {
                                rect,
                                callback: Arc::new(callback),
                            });
                        }
                    }
                });
            }

            Tab::Diagnostics => {
                egui::SidePanel::left("diag_side")
                    .resizable(false)
                    .min_width(140.0)
                    .show(ctx, |ui| {
                        ui.heading("Diagnostics");
                        ui.separator();
                        ui.checkbox(&mut self.diag_poll, "Poll");
                        ui.checkbox(&mut self.diag_led, "LED");
                    });

                egui::CentralPanel::default().show(ctx, |_| {
                    // (optional) placeholder – nothing rendered for now
                });
            }

            Tab::Design => {
                egui::SidePanel::left("design_side")
                    .resizable(false)
                    .min_width(140.0)
                    .show(ctx, |ui| {
                        ui.heading("Design");
                        ui.separator();

                        if ui.button("Load").clicked() { /* TODO: hook-up */ }
                        if ui.button("Save").clicked() { /* TODO: hook-up */ }
                        if ui.button("Render").clicked() { /* TODO: hook-up */ }
                    });

                egui::CentralPanel::default().show(ctx, |_| {
                    // (optional) placeholder
                });
            }
        }
    }
}

/// Build an MVP matrix that always keeps the entire model in front of the camera.
///
/// * `zoom` is interpreted as a dolly factor: 1 = default distance, 2 = half the distance, etc.
/// * `bounds` is the half-extent of the work area or of the model, whichever is larger.
fn mvp(app: &AluminaApp, rect: egui::Rect) -> Matrix4<f32> {
    // ─ 1. camera distance ─
    let radius = app.work_size.norm() * 0.5;
    let base_eye = radius * 3.0;
    let eye = Point3::new(0.0, 0.0, base_eye / app.zoom);

    // ─ 2. matrices ─
    let aspect = rect.width() / rect.height();
    let proj = Perspective3::new(aspect, 60_f32.to_radians(), 0.1, 10_000.0).to_homogeneous();
    let view = nalgebra::Isometry3::look_at_rh(
        &eye,
        &Point3::origin(),            // target
        &Vector3::new(0.0, 1.0, 0.0), // up
    )
    .to_homogeneous();

    // screen-pixel panning (same maths as before)
    let pixels_per_world = rect.height() / (radius * 2.0);
    let pan = Vector3::new(
        -app.translation.x / pixels_per_world,
        app.translation.y / pixels_per_world,
        0.0,
    );
    let model = Translation3::from(pan).to_homogeneous() * app.rotation.to_homogeneous();

    proj * view * model
}

fn spawn_file_picker(
    target: Arc<Mutex<Option<Vec<u8>>>>,
    _filter_name: &'static str,
    exts: &'static [&'static str],
) {
    // 100 % non-blocking: the async task lives in the browser’s micro-task queue
    execute(async move {
        // ---1) build an <input type="file"> on the fly --------------------
        let document = window()
            .expect("no window")
            .document()
            .expect("no document");
        let input: HtmlInputElement = document
            .create_element("input")
            .unwrap()
            .dyn_into()
            .unwrap();
        input.set_type("file");

        // Accept filter (".stl,.dxf", etc.)
        let accept = exts
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(",");
        input.set_accept(&accept);

        input.style().set_property("display", "none").unwrap(); // invisible
        document.body().unwrap().append_child(&input).unwrap();

        // ---2) turn the "change" event into a Future -----------------------
        let (tx, rx) = oneshot::channel::<()>();

        // Wrap the Sender so we can *move* it exactly once inside an FnMut closure
        let tx_cell = Rc::new(RefCell::new(Some(tx)));
        let tx_handle = Rc::clone(&tx_cell);

        let closure = Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_e| {
            if let Some(sender) = tx_handle.borrow_mut().take() {
                let _ = sender.send(()); // 2nd call → already None → no-op
            }
        }));
        input
            .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget(); // leak => stays alive for the element’s lifetime

        input.click(); // **opens** the browser dialog
        rx.await.ok(); // wait until the user picked a file

        // ---3) extract bytes with File::arrayBuffer -----------------------
        let files = input.files().unwrap();
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();

        let buf_promise = file.array_buffer();
        let js_buf = JsFuture::from(buf_promise).await.unwrap();
        let u8_array = Uint8Array::new(&js_buf);
        let mut bytes = vec![0u8; u8_array.length() as usize];
        u8_array.copy_to(&mut bytes);

        *target.lock().unwrap() = Some(bytes); // hand off to the egui thread
    });
}

fn load_csg_from_bytes(bytes: &[u8]) -> Option<CSG<()>> {
    if let Ok(csg) = CSG::<()>::from_stl(bytes, None) {
        return Some(csg);
    }

    if let Ok(csg) = CSG::<()>::from_dxf(bytes, None) {
        return Some(csg);
    }

    None
}

/// Ensures we only create **one** `AluminaApp` per page, even if the module is
/// re-loaded by Trunk’s hot-reload mechanism.
static STARTED: OnceCell<()> = OnceCell::new();

#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    // bail out if we’ve already started once
    if STARTED.set(()).is_err() {
        log::warn!("[alumina] second call to `start()` ignored");
        return Ok(());
    }
    // Redirect `log` macros & panic messages to the browser console
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(Level::Debug);

    let web_options = eframe::WebOptions::default();

    // The element id must match the <canvas> in your index.html
    eframe::WebRunner::new()
        .start(
            "alumina_canvas", // canvas id
            web_options,
            Box::new(|cc| Box::new(AluminaApp::new(cc))),
        )
        .await?;

    Ok(())
}

fn execute<F: Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}
