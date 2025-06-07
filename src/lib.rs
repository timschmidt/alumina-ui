use eframe::egui;
use glam::{Quat, Vec3};
use csgrs::csg::CSG;
use std::collections::HashSet;

#[derive(Default)]
pub struct AluminaApp {
    rotation: Quat,
    translation: egui::Vec2,
    zoom: f32,
    edges: Vec<(Vec3, Vec3)>,
    wireframe: bool,
    grid: bool,
}

impl AluminaApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // ── build a cube with csgrs and collect its unique edges ──────────────
        let mut uniq: HashSet<((i64, i64, i64), (i64, i64, i64))> = HashSet::new();
        //let cube = CSG::<()>::cube(2.0, 2.0, 2.0, None).center();
        let cube = CSG::<()>::icosahedron(2.0, None).center();

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
            grid: false,
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
            .min_width(120.0)
            .show(ctx, |ui| {
                ui.heading("Controls");
                ui.separator();

                ui.checkbox(&mut self.wireframe, "wireframe");
                ui.checkbox(&mut self.grid, "grid");
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
                self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.2, 5.0);
            }

            // ───── Paint ─────
            let painter = ui.painter_at(rect);
            draw_csgrs_cube(&painter, rect, self);
        });
    }
}

fn draw_csgrs_cube(painter: &egui::Painter, rect: egui::Rect, app: &AluminaApp) {
    let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
    let size = rect.width().min(rect.height()) * 0.25 * app.zoom;

	// basic perspective projection
	let dist = 4.0;

	let project = |v: Vec3| {
		let rotated = app.rotation * v;
		let scale = dist / (dist - rotated.z);
		let p = egui::vec2(rotated.x * scale, rotated.y * scale);
		let offset = (egui::vec2(p.x, -p.y) * size) + app.translation;
		rect.center() + offset
	};

    for &(a, b) in &app.edges {
        painter.line_segment([project(a), project(b)], stroke);
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

