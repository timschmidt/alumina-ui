//! Enumerate fonts available to the WASM app (best-effort).
//!
//! Strategy:
//! 1) Try the Local Font Access API (`window.queryLocalFonts`) when present
//!    (Chromium; requires a user gesture/permission).
//! 2) Fallback: probe a curated list of common families by measuring
//!    canvas text width vs generic fallbacks.
//!
//! Returns a de-duplicated, sorted list of family names.
//
//! This module is `wasm32`-first. On non-wasm targets we return an empty list.

#![allow(clippy::missing_errors_doc)]

use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct FontsInventory {
    /// Distinct font family names known/likely available to CSS on this page.
    pub families: Vec<String>,
}

impl FontsInventory {
    #[must_use]
    pub fn contains(&self, family: &str) -> bool {
        self.families.iter().any(|f| f.eq_ignore_ascii_case(family))
    }
}

/// Reasonable cross-platform candidates (system & popular web families).
/// You can add/remove as desired; detection is cheap.
pub const DEFAULT_CANDIDATES: &[&str] = &[
    // Web/popular families
    "Inter", "Roboto", "Open Sans", "Lato", "Montserrat",
    "Source Sans Pro", "Source Serif Pro", "Source Code Pro",
    "Noto Sans", "Noto Serif", "Noto Sans Mono",
    "Fira Sans", "Fira Mono", "Fira Code",
    "Nunito", "IBM Plex Sans", "IBM Plex Mono",
    "PT Sans", "PT Serif", "Work Sans", "Merriweather",
    "JetBrains Mono", "Overpass", "Manrope",
    // Windows
    "Segoe UI", "Segoe UI Variable", "Calibri", "Cambria",
    "Consolas", "Courier New", "Times New Roman",
    "Georgia", "Verdana", "Tahoma", "Trebuchet MS", "Impact",
    // macOS
    "San Francisco", "SF Pro", "SF Pro Text", "SF Mono",
    "Helvetica", "Helvetica Neue", "Menlo",
    // Linux/Free fonts
    "Ubuntu", "Cantarell",
    "DejaVu Sans", "DejaVu Serif", "DejaVu Sans Mono",
    "Liberation Sans", "Liberation Serif", "Liberation Mono",
    // Misc
    "Arial", "Palatino", "Garamond", "Comic Sans MS",
];

/// Collect all available fonts (best-effort).
///
/// - First tries Local Font Access (Chromium).
/// - Falls back to canvas probing over `DEFAULT_CANDIDATES`.
#[cfg(target_arch = "wasm32")]
pub async fn collect_available_fonts() -> FontsInventory {
    // 1) Try Local Font Access API (graceful failure if unsupported/denied)
    match try_query_local_fonts().await {
        Ok(inv) if !inv.families.is_empty() => return inv,
        _ => { /* fall through to probe */ }
    }

    // 2) Fallback: canvas probe against DEFAULT_CANDIDATES
    collect_by_canvas_probe(DEFAULT_CANDIDATES).unwrap_or_default()
}

/// Same as [`collect_available_fonts`] but with your own candidate list.
#[cfg(target_arch = "wasm32")]
pub async fn collect_available_fonts_with_candidates(candidates: &[&str]) -> FontsInventory {
    match try_query_local_fonts().await {
        Ok(inv) if !inv.families.is_empty() => inv,
        _ => collect_by_canvas_probe(candidates).unwrap_or_default(),
    }
}

/// Non-wasm stub: returns an empty list (so this crate stays buildable natively).
#[cfg(not(target_arch = "wasm32"))]
pub async fn collect_available_fonts() -> FontsInventory {
    FontsInventory::default()
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn collect_available_fonts_with_candidates(_: &[&str]) -> FontsInventory {
    FontsInventory::default()
}

#[cfg(target_arch = "wasm32")]
async fn try_query_local_fonts() -> Result<FontsInventory, wasm_bindgen::JsValue> {
    use js_sys::{Array, Function, Promise, Reflect};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let Some(win) = web_sys::window() else {
        return Ok(FontsInventory::default());
    };

    let qlf_val = Reflect::get(&win, &"queryLocalFonts".into())?;
    let Some(qlf_fn) = qlf_val.dyn_ref::<Function>() else {
        // API missing (non-Chromium, or not exposed)
        return Ok(FontsInventory::default());
    };

    // Note: Browsers may require a user gesture; if the promise rejects,
    // we simply return empty and fallback to probing.
    let promise_val = qlf_fn.call0(&win)?;
    let promise: Promise = promise_val.dyn_into()?;
    let result = JsFuture::from(promise).await?;

    // result is an array of FontMetadata objects:
    // { postscriptName, fullName, family, style }
    let arr = Array::from(&result);
    let mut set = BTreeSet::<String>::new();

    for item in arr.iter() {
        // family is most useful for selection; other props are optional.
        if let Ok(fam_val) = Reflect::get(&item, &"family".into()) {
            if let Some(fam) = fam_val.as_string() {
                if !fam.trim().is_empty() {
                    set.insert(fam);
                }
            }
        }
    }

    Ok(FontsInventory {
        families: set.into_iter().collect(),
    })
}

#[cfg(target_arch = "wasm32")]
fn collect_by_canvas_probe(candidates: &[&str]) -> Result<FontsInventory, wasm_bindgen::JsValue> {
    use std::collections::HashMap;
    use wasm_bindgen::JsCast;

    let Some(win) = web_sys::window() else {
        return Ok(FontsInventory::default());
    };
    let Some(doc) = win.document() else {
        return Ok(FontsInventory::default());
    };

    let canvas: web_sys::HtmlCanvasElement = doc
        .create_element("canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let ctx = canvas
        .get_context("2d")?
        .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
        .ok_or_else(|| js_sys::Error::new("2D context not available"))?;

    // Consistent sample text (mix of widths, ascenders/descenders)
    // Using longer string increases chances of width differences.
    let sample = "AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz \
                  0123456789 !@#$%^&*()_+-=[]{};':\",./<>?";

    // Measure with base generics
    let px = 72; // bigger size → larger metric differences
    let bases = ["serif", "sans-serif", "monospace"];

    let mut base_widths = HashMap::new();
    for base in &bases {
        let base_css = format!("{px}px {base}");
        ctx.set_font(&base_css);
        if let Ok(m) = ctx.measure_text(sample) {
            base_widths.insert(*base, m.width());
        }
    }

    let mut detected = BTreeSet::<String>::new();

    // Always include the generic families for convenience.
    for g in &bases {
        detected.insert((*g).to_string());
    }

    // Try each candidate by comparing width to each generic fallback.
    for name in candidates {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Quote names with spaces/keywords.
        let quoted = if trimmed.chars().any(char::is_whitespace) {
            format!("\"{trimmed}\"")
        } else {
            trimmed.to_string()
        };

        let mut looks_present = false;
        for base in &bases {
            let test_css = format!("{px}px {quoted}, {base}");
            ctx.set_font(&test_css);

            if let Ok(m) = ctx.measure_text(sample) {
                if let Some(&w_base) = base_widths.get(base) {
                    let w = m.width();
                    // If different from the base fallback width, we assume the requested
                    // family applied and thus is available.
                    if (w - w_base).abs() > f64::EPSILON {
                        looks_present = true;
                        break;
                    }
                }
            }
        }

        if looks_present {
            detected.insert(trimmed.to_string());
        }
    }

    Ok(FontsInventory {
        families: detected.into_iter().collect(),
    })
}
