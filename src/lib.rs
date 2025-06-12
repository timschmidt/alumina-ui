mod renderer;

use eframe::egui;
use csgrs::csg::CSG;
use nalgebra::{ Vector3, Matrix4, UnitQuaternion, Translation3, Perspective3, Point3 };
use geo::LineString;
use std::f32::consts::{PI, FRAC_PI_2};
use rfd::AsyncFileDialog;
use std::{
    future::Future,
    sync::{Arc, Mutex},
};

pub struct AluminaApp {
    rotation: UnitQuaternion<f32>,
    translation: egui::Vec2,
    zoom: f32,
    /// Un‑scaled geometry as loaded from disk (or the default icosahedron).
    base_model: CSG<()>,
    /// Geometry that is actually rendered (scaled version of `base_model`).
    model: CSG<()>,
    /// Desired scale factors set by the user (per‑axis, 1 = no change).
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
    grid: bool,
    /// CNC working area dimensions (mm)
    work_size: Vector3<f32>, // x, y, z
    layer_height: f32,
	/// Index of the layer currently being inspected (0-based)
    current_layer: i32,
    /// `true` while the “tool-path” view is active
    show_slice: bool,
    /// The last slice that was generated for `current_layer`
    sliced_layer: Option<CSG<()>>,
    gpu: Option<Arc<renderer::GpuLines>>,
    vertex_storage: Vec<f32>,
}

impl AluminaApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let base_model = CSG::<()>::icosahedron(100.0, None).float();
        let model_scale = Vector3::new(1.0, 1.0, 1.0);

        Self {
            rotation: UnitQuaternion::identity(),
            translation: egui::Vec2::ZERO,
            zoom: 1.0,
            base_model: base_model.clone(),
            model: base_model,
            model_scale,
            applied_scale: model_scale,
            model_offset: Vector3::zeros(),
            applied_offset: Vector3::zeros(),
            workpiece_data: Arc::new(Mutex::new(None)),
            model_data: Arc::new(Mutex::new(None)),
            wireframe: true,
            grid: true,
            work_size: Vector3::new(200.0, 200.0, 200.0),
            layer_height: 0.20,
            current_layer: 0,
			show_slice:    false,
			sliced_layer:  None,
            gpu: None,
			vertex_storage: Vec::new(),
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
            self.applied_scale  = self.model_scale;
            self.applied_offset = self.model_offset;
        }
	}
	
	/// Re-builds `sliced_layer` for the current Z level.
    fn refresh_slice(&mut self) {
        if !self.show_slice { return; }

        let z = self.current_layer as f32 * self.layer_height;
        let plane = csgrs::plane::Plane::from_normal(Vector3::z(), z.into());
        self.sliced_layer = Some(self.model.slice(plane));
    }
}

impl AluminaApp {
    /// (Re-)builds the VBO if the model, grid or scale changed.
    unsafe fn sync_buffers(&mut self, gl: &glow::Context) {
        self.vertex_storage.clear();

        // ── 1) grid (10 mm spacing, ±work_size/2) ───────────────────────
        if self.grid {
			let minor = [0.55, 0.55, 0.55];
			let major = [1.0 , 1.0 , 1.0];

			let hx = self.work_size.x * 0.5;
			let hy = self.work_size.y * 0.5;
			let hz = self.work_size.z;

			// vertical (X) lines
			for i in 0..=(self.work_size.x / 10.0) as i32 {
				let x    = -hx + i as f32 * 10.0;
				let col  = if i % 10 == 0 { major } else { minor };
				self.vertex_storage.extend_from_slice(&[
					x, -hy, 0.0, col[0], col[1], col[2],
					x,  hy, 0.0, col[0], col[1], col[2],
				]);
			}

			// horizontal (Y) lines
			for i in 0..=(self.work_size.y / 10.0) as i32 {
				let y    = -hy + i as f32 * 10.0;
				let col  = if i % 10 == 0 { major } else { minor };
				self.vertex_storage.extend_from_slice(&[
					-hx, y, 0.0, col[0], col[1], col[2],
					 hx, y, 0.0, col[0], col[1], col[2],
				]);
			}
			
			// outline the remaining cuboid edges
			let edge = major;

			//  four vertical edges
			for (sx, sy) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
				let x = sx * hx;
				let y = sy * hy;
				self.vertex_storage.extend_from_slice(&[
					x, y, 0.0, edge[0], edge[1], edge[2],   // bottom
					x, y, hz , edge[0], edge[1], edge[2],   // top
				]);
			}
			
			//  top rectangle (Z = hz)
			self.vertex_storage.extend_from_slice(&[
				-hx, -hy, hz, edge[0], edge[1], edge[2],    hx, -hy, hz, edge[0], edge[1], edge[2],
				 hx, -hy, hz, edge[0], edge[1], edge[2],    hx,  hy, hz, edge[0], edge[1], edge[2],
				 hx,  hy, hz, edge[0], edge[1], edge[2],   -hx,  hy, hz, edge[0], edge[1], edge[2],
				-hx,  hy, hz, edge[0], edge[1], edge[2],   -hx, -hy, hz, edge[0], edge[1], edge[2],
			]);
		}

        // ── 2) model / slice ──────────────────────────────────────────────
        fn add_line_string(ls: &LineString<f64>, z: f32, col: [f32;3], out: &mut Vec<f32>) {
			for w in ls.0.windows(2) {
				let a = w[0]; let b = w[1];
				out.extend_from_slice(&[
					a.x as f32, a.y as f32, z,  col[0],col[1],col[2],
					b.x as f32, b.y as f32, z,  col[0],col[1],col[2],
				]);
			}
		}
        
		if self.show_slice {
			const YELLOW: [f32;3] = [1.0, 1.0, 0.0];
			if let Some(slice) = &self.sliced_layer {
				let z = self.current_layer as f32 * self.layer_height;

				for geom in &slice.geometry.0 {
					use geo::{LineString, Polygon, Geometry};

					match geom {
						Geometry::LineString(ls) => add_line_string(ls, z, YELLOW, &mut self.vertex_storage),
						Geometry::Polygon(poly) =>  {
							add_line_string(&poly.exterior(), z, YELLOW, &mut self.vertex_storage);
							for inner in poly.interiors() {
								add_line_string(inner, z, YELLOW, &mut self.vertex_storage);
							}
						}
						_ => {} // ignore points etc.
					}
				}
			}
		} else {
			for p in &self.model.polygons {
				for (a, b) in p.edges() {
					const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

					self.vertex_storage.extend_from_slice(&[
						a.pos.x as f32, a.pos.y as f32, a.pos.z as f32,  WHITE[0], WHITE[1], WHITE[2],
						b.pos.x as f32, b.pos.y as f32, b.pos.z as f32,  WHITE[0], WHITE[1], WHITE[2],
					]);
				}
			}
		}

        // Upload (only when we still own the *single* strong ref)
        if let Some(gpu_arc) = &mut self.gpu {
            if let Some(gpu) = Arc::get_mut(gpu_arc) {
                gpu.upload_vertices(gl, &self.vertex_storage);
            }
        }
    }
}

impl eframe::App for AluminaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ------------------------------------------------------------------
        // Sidebar
        // ------------------------------------------------------------------
        egui::SidePanel::left("side_panel")
            .resizable(false)
            .min_width(140.0)
            .show(ctx, |ui| {
                ui.heading("Controls");

				ui.separator();
				ui.label("Snap view");
				ui.horizontal_wrapped(|ui| {
					let pitch = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -FRAC_PI_2); //  -90° about X  (Z-up ➜ Y-up)
					if ui.button("Front").clicked() { self.rotation = pitch; } //  -90° about X
					if ui.button("Back").clicked() { self.rotation = pitch * UnitQuaternion::from_axis_angle(&Vector3::z_axis(), PI); } // 180° roll
					if ui.button("Left").clicked() { self.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), FRAC_PI_2)  * pitch; } // +90° yaw
					if ui.button("Right").clicked() { self.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -FRAC_PI_2) * pitch; } // –90° yaw
					if ui.button("Top").clicked() { self.rotation = UnitQuaternion::identity(); } // no change
					if ui.button("Bottom").clicked() { self.rotation = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), PI); } // look from below
				});

                ui.separator();
                ui.checkbox(&mut self.wireframe, "wireframe");
                ui.checkbox(&mut self.grid, "grid");
                
                // ────────────── Scale Controls ──────────────
                ui.separator();
                ui.collapsing("Model scale", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("X:");
                        ui.add(egui::DragValue::new(&mut self.model_scale.x)
                            .speed(0.01)
                            .clamp_range(0.01..=100.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Y:");
                        ui.add(egui::DragValue::new(&mut self.model_scale.y)
                            .speed(0.01)
                            .clamp_range(0.01..=100.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Z:");
                        ui.add(egui::DragValue::new(&mut self.model_scale.z)
                            .speed(0.01)
                            .clamp_range(0.01..=100.0));
                    });

                    if ui.button("Reset scale").clicked() {
                        self.model_scale = Vector3::new(1.0, 1.0, 1.0);
                    }
                });
                
                // ────────────── Position Controls ──────────────
                ui.separator();
                ui.collapsing("Model position", |ui| {
                    if ui.button("Float (Z = 0)").clicked() {
                        self.base_model = self.base_model.clone().float();
                        // force rebuild
                        self.applied_scale  = Vector3::new(-1.0, -1.0, -1.0);
                        self.applied_offset = Vector3::repeat(f32::NAN);
                        self.model_offset   = Vector3::zeros();
                        self.refresh_model();
                        self.refresh_slice();
                    }

                    if ui.button("Center").clicked() {
                        self.base_model = self.base_model.clone().center();
                        self.applied_scale  = Vector3::new(-1.0, -1.0, -1.0);
                        self.applied_offset = Vector3::repeat(f32::NAN);
                        self.model_offset   = Vector3::zeros();
                        self.refresh_model();
                        self.refresh_slice();
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
                
                ui.horizontal(|ui|{
					let max_layers = (self.work_size.z / self.layer_height).floor() as i32;
					let prev = self.current_layer;
					ui.label("Current layer:");
					ui.add(egui::DragValue::new(&mut self.current_layer)
						   .clamp_range(0..=max_layers)
						   .speed(1));
					if self.current_layer != prev { self.refresh_slice(); }
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
                    self.base_model = csg;
                    // ── force a rebuild ─────────────────────────────────────────────
					self.applied_scale = Vector3::new(-1.0, -1.0, -1.0);          // anything ≠ model_scale works
					self.applied_offset = Vector3::repeat(f32::NAN);
					self.refresh_model();
					self.refresh_slice();
                    log::info!("Workpiece geometry loaded ({} bytes)", bytes.len());
                }
                None => log::error!("Could not parse workpiece file – unsupported or corrupt"),
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
                    self.base_model = csg;
                    // ── force a rebuild ─────────────────────────────────────────────
					self.applied_scale = Vector3::new(-1.0, -1.0, -1.0);          // anything ≠ model_scale works
					self.applied_offset = Vector3::repeat(f32::NAN);
					self.refresh_model();
					self.refresh_slice();
                    log::info!("Model geometry loaded ({} bytes)", bytes.len());
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
                    self.gpu = Some(Arc::new(unsafe { renderer::GpuLines::new(gl) }));
                }

                // ── 2) keep vertex buffer in sync ─────────────────────────────
                unsafe { self.sync_buffers(gl) };

                // ── 3) schedule GL paint right after egui’s own meshes ────────
                if let Some(gpu_arc) = &self.gpu {
                    let gpu_for_cb = gpu_arc.clone(); // Arc<T> is Send + Sync
                    let mvp = mvp(self, rect);        // copy for the closure

                    let callback = egui_glow::CallbackFn::new(move |_info, painter| {
						unsafe {
							gpu_for_cb.paint(painter.gl(), mvp);
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
}

/// Build an MVP matrix that always keeps the entire model in front of the camera.
///
/// * `zoom` is interpreted as a dolly factor: 1 = default distance, 2 = half the distance, etc.
/// * `bounds` is the half-extent of the work area or of the model, whichever is larger.
fn mvp(app: &AluminaApp, rect: egui::Rect) -> Matrix4<f32> {
    // ─ 1. camera distance ─
    let radius   = app.work_size.norm() * 0.5;
    let base_eye = radius * 3.0;
    let eye      = Point3::new(0.0, 0.0, base_eye / app.zoom);

    // ─ 2. matrices ─
    let aspect = rect.width() / rect.height();
    let proj   = Perspective3::new(aspect, 60_f32.to_radians(), 0.1, 10_000.0).to_homogeneous();
    let view   = nalgebra::Isometry3::look_at_rh(
                    &eye,
                    &Point3::origin(),                   // target
                    &Vector3::new(0.0, 1.0, 0.0),        // up
                 ).to_homogeneous();

    // screen-pixel panning (same maths as before)
    let pixels_per_world = rect.height() / (radius * 2.0);
    let pan = Vector3::new(
        -app.translation.x / pixels_per_world,
         app.translation.y / pixels_per_world,
         0.0,
    );
    let model = Translation3::from(pan).to_homogeneous()
               * app.rotation.to_homogeneous();

    proj * view * model
}

fn spawn_file_picker(
    target: Arc<Mutex<Option<Vec<u8>>>>,
    filter_name: &'static str,
    exts: &'static [&'static str],   // ← now the slice is 'static too
) {
    execute(async move {
        if let Some(handle) = AsyncFileDialog::new()
            .add_filter(filter_name, exts)
            .pick_file()
            .await
        {
            let bytes = handle.read().await;
            *target.lock().unwrap() = Some(bytes);
        }
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

// ── Web entry‑point ──
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    // Redirect `log` macros & panic messages to the browser console
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    console_error_panic_hook::set_once();

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

// ── Native entry‑point ──
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Alumina",
        options,
        Box::new(|cc| Box::new(AluminaApp::new(cc))),
    )
}

// Executes an async future without blocking the egui thread
#[cfg(not(target_arch = "wasm32"))]
fn execute<F: Future<Output = ()> + Send + 'static>(f: F) {
    std::thread::spawn(move || futures::executor::block_on(f));
}
#[cfg(target_arch = "wasm32")]
fn execute<F: Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}
