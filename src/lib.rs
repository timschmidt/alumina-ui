use eframe::egui;
use glam::{Quat, Vec3, Vec4, Mat4};
use csgrs::csg::CSG;
use std::collections::HashSet;
use std::f32::consts::{PI, FRAC_PI_2};

#[derive(Default)]
pub struct AluminaApp {
    rotation: Quat,
    translation: egui::Vec2,
    zoom: f32,
    edges: Vec<(Vec3, Vec3)>,
    wireframe: bool,
    grid: bool,
    /// CNC working area dimensions (mm)
    work_size: Vec3, // x, y, z
}

impl AluminaApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // ── build a cube with csgrs and collect its unique edges ──────────────
        let mut uniq: HashSet<((i64, i64, i64), (i64, i64, i64))> = HashSet::new();
        //let cube = CSG::<()>::cube(2.0, 2.0, 2.0, None).center();
        let cube = CSG::<()>::icosahedron(20.0, None).center();

        for poly in &cube.polygons {
            for (a, b) in poly.edges() {
                // key ≤---> canonicalised (small-grid-snapped) pair
                let snap = |p: &csgrs::float_types::Real| (*p * 1e5).round() as i64;
                let key = {
                    let ka = (snap(&a.pos.x), snap(&a.pos.y), snap(&a.pos.z));
                    let kb = (snap(&b.pos.x), snap(&b.pos.y), snap(&b.pos.z));
                    if ka < kb { (ka, kb) } else { (kb, ka) }
                };
                uniq.insert(key);
            }
        }

        let edges = uniq
            .into_iter()
            .map(|(ka, kb)| {
                let v = |(x, y, z): (i64, i64, i64)| Vec3::new(x as f32, y as f32, z as f32) / 1e5;
                (v(ka), v(kb))
            })
            .collect();

        Self {
            rotation: Quat::IDENTITY,
            translation: egui::Vec2::ZERO,
            zoom: 1.0,
            edges,
            wireframe: false,
            grid: true,
            work_size: Vec3::new(200.0, 200.0, 200.0), // default 200 × 200 × 200 mm
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
					if ui.button("Front").clicked()  { self.rotation = Quat::from_rotation_x(-FRAC_PI_2); }
					if ui.button("Back").clicked()   { self.rotation = Quat::from_rotation_x(FRAC_PI_2); }
					if ui.button("Left").clicked()   { self.rotation = Quat::from_rotation_y(FRAC_PI_2); }
					if ui.button("Right").clicked()  { self.rotation = Quat::from_rotation_y(-FRAC_PI_2); }
					if ui.button("Top").clicked()    { self.rotation = Quat::IDENTITY; }
					if ui.button("Bottom").clicked() { self.rotation = Quat::from_rotation_y(PI); }
				});

                ui.separator();
                ui.checkbox(&mut self.wireframe, "wireframe");
                ui.checkbox(&mut self.grid, "grid");
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

                if ui.button("open").clicked() {
                    // TODO: implement open action
                    log::info!("'open' button pressed");
                }
                if ui.button("toolpath").clicked() {
                    // TODO: implement toolpath action
                    log::info!("'toolpath' button pressed");
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
                        Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * self.rotation;
                } else if input.pointer.secondary_down() {
                    // right‑drag → pan
                    self.translation += delta;
                }
            }

            // scroll → zoom
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.0, 500.0);
            }

            // ───── Paint ─────
            let painter = ui.painter_at(rect);
            draw_scene(&painter, rect, self);
        });
    }
}

/// Build an MVP matrix that always keeps the entire model in front of the camera.
///
/// * `zoom` is interpreted as a dolly factor: 1 = default distance, 2 = half the distance, etc.
/// * `bounds` is the half-extent of the work area or of the model, whichever is larger.
fn mvp(app: &AluminaApp, rect: egui::Rect) -> Mat4 {
    // ---------------------------------------------------------------
    // 1. camera placement
    // ---------------------------------------------------------------
    let radius = app.work_size.length() * 0.5;           // world units (mm)
    let base_eye = radius * 3.0;                         // “far enough” for a 60° FOV
    let eye = Vec3::new(0.0, 0.0, base_eye / app.zoom);  // dolly with the scroll wheel

    // ---------------------------------------------------------------
    // 2. matrices
    // ---------------------------------------------------------------
    let aspect = rect.width() / rect.height();
    let proj   = Mat4::perspective_rh_gl(60_f32.to_radians(), aspect, 0.1, 10_000.0);
    let view   = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let model  = Mat4::from_quat(app.rotation);

    proj * view * model // one 4 × 4 matrix to project a point all the way to NDC
}

/// Project a 3-D vertex with the supplied MVP matrix into egui pixel space.
/// Returns `None` only when the point is not renderable at all
/// (i.e. behind the camera or outside the near/far planes).
fn project(v: Vec3, mvp: Mat4, rect: egui::Rect, pan: egui::Vec2) -> Option<egui::Pos2> {
    let clip: Vec4 = mvp * v.extend(1.0);

    // trivial reject: behind the eye or outside near/far
    if clip.w <= 0.0 || clip.z < 0.0 || clip.z > clip.w {
        return None;
    }

    // perspective divide → Normalised Device Coordinates
    let ndc = clip.truncate() / clip.w; // (-∞ … +∞) after we removed the test

    // NDC → egui pixels
    let half = egui::vec2(rect.width(), rect.height()) * 0.5;
    Some(rect.center() + pan + egui::vec2(ndc.x * half.x, -ndc.y * half.y))
}

/// Clip AB to the canonical volume -w≤x≤w, -w≤y≤w, 0≤z≤w.
/// Returns None if the segment is completely outside.
fn clip_segment(mut a: Vec4, mut b: Vec4) -> Option<(Vec4, Vec4)> {
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let d = b - a;

    let clip = |p: f32, q: f32, t0: &mut f32, t1: &mut f32| -> bool {
        if p == 0.0 { // parallel to plane
            q >= 0.0 // keep only if inside
        } else {
            let r = q / p;
            if p < 0.0 { *t0 = t0.max(r); *t0 <= *t1 }
            else        { *t1 = t1.min(r); *t0 <= *t1 }
        }
    };

    // six planes
    if !clip(-d.x - d.w,  a.x + a.w, &mut t0, &mut t1) { return None; } // x ≥ -w
    if !clip( d.x - d.w, -a.x + a.w, &mut t0, &mut t1) { return None; } // x ≤  w
    if !clip(-d.y - d.w,  a.y + a.w, &mut t0, &mut t1) { return None; } // y ≥ -w
    if !clip( d.y - d.w, -a.y + a.w, &mut t0, &mut t1) { return None; } // y ≤  w
    if !clip(-d.z,        a.z,        &mut t0, &mut t1) { return None; } // z ≥  0
    if !clip( d.z - d.w, -a.z + a.w, &mut t0, &mut t1) { return None; } // z ≤  w

    Some((a + d * t0, a + d * t1))
}

fn draw_scene(painter: &egui::Painter, rect: egui::Rect, app: &AluminaApp) {
    let mvp = mvp(app, rect);

    // one closure that turns a clip-space vertex into an egui point
    let to_screen = |p: Vec4| {
        let ndc  = p.truncate() / p.w;
        let half = egui::vec2(rect.width(), rect.height()) * 0.5;
        rect.center() + app.translation +
            egui::vec2(ndc.x * half.x, -ndc.y * half.y)
    };

    // one closure that does proper clipping before it paints
    let mut draw_line =
        |a: Vec3, b: Vec3, stroke: egui::Stroke| {
            let a_c = mvp * a.extend(1.0); // world → clip
            let b_c = mvp * b.extend(1.0);

            if let Some((ca, cb)) = clip_segment(a_c, b_c) {
                painter.line_segment([to_screen(ca), to_screen(cb)], stroke);
            }
        };

    // ───────────── GRID ─────────────
	if app.grid {
		let half_x = app.work_size.x * 0.5;
		let half_y = app.work_size.y * 0.5;

		// vertical grid lines ────────────────────────────────────────────────
		let mut x   = -half_x;
		let mut idx = 0usize; // count drawn lines
		while x <= half_x + 0.1 {
			let major = idx % 10 == 0; // every 10th line is white
			let col   = if major { egui::Color32::WHITE }
								 else { egui::Color32::from_gray(80) };
			draw_line(
				Vec3::new(x, -half_y, 0.0),
				Vec3::new(x,  half_y, 0.0),
				egui::Stroke::new(1.0, col),
			);
			x   += 10.0;
			idx += 1;
		}

		// horizontal grid lines ──────────────────────────────────────────────
		let mut y   = -half_y;
		let mut idx = 0usize; // reset counter for rows
		while y <= half_y + 0.1 {
			let major = idx % 10 == 0;
			let col   = if major { egui::Color32::WHITE }
								 else { egui::Color32::from_gray(80) };
			draw_line(
				Vec3::new(-half_x, y, 0.0),
				Vec3::new( half_x, y, 0.0),
				egui::Stroke::new(1.0, col),
			);
			y   += 10.0;
			idx += 1;
		}
		
		// ────────────────────── WORK-AREA CUBOID ──────────────────────
		let top_z  = app.work_size.z; // height of the box
		let stroke = egui::Stroke::new(1.5, egui::Color32::LIGHT_GRAY);

		// Four bottom corners (z = 0)
		let bottom = [
			Vec3::new(-half_x, -half_y, 0.0),
			Vec3::new( half_x, -half_y, 0.0),
			Vec3::new( half_x,  half_y, 0.0),
			Vec3::new(-half_x,  half_y, 0.0),
		];

		// Vertical edges
		for &p in &bottom {
			draw_line(p, p + Vec3::Z * top_z, stroke);
		}

		// Top rectangle (horizontal edges)
		for i in 0..4 {
			let a = bottom[i]             + Vec3::Z * top_z;
			let b = bottom[(i + 1) % 4]   + Vec3::Z * top_z;
			draw_line(a, b, stroke);
		}
	}

    // ───────────── MODEL ─────────────
    let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
    for &(a_w, b_w) in &app.edges {
        draw_line(a_w, b_w, stroke); // same helper, same rules
    }
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
