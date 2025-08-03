//! Minimal Google Fonts integration for wasm32.
//!
//! - Fetch Google Fonts index (names, variants, download URLs) via the public REST API
//! - Download font bytes on demand
//! - Persist bytes to `localStorage` (base64) and list what’s already persisted
//!
//! Keep deps small: `gloo-net`, `serde`, `serde_json`, `base64`.
//!
//! Notes
//! -----
//! * The API requires an API key. You can safely ship a key that only reads public font metadata.
//! * localStorage is ~5–10 MB per origin; store only what you need.
//! * File URLs in the API may be TTF, WOFF2, etc. We just fetch the bytes as-is.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use gloo_net::http::Request;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;
use base64::Engine;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FontItem {
    pub family: String,
    #[serde(default)]
    pub variants: Vec<String>,
    /// Map from variant tag ("regular", "700", "italic", …) to a downloadable file URL.
    #[serde(default)]
    pub files: HashMap<String, String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub subsets: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct WebFontsResponse {
    #[serde(default)]
    items: Vec<FontItem>,
}

/// A compact view of what we persist.
#[derive(Debug, Clone)]
pub struct PersistedFont {
    pub family: String,
    pub variant: String,
}

const LS_PREFIX: &str = "ttf:";

#[cfg(target_arch = "wasm32")]
fn window() -> web_sys::Window {
    web_sys::window().expect("no `window` in this context")
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Result<web_sys::Storage, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .local_storage()?
        .ok_or_else(|| JsValue::from_str("no localStorage"))
}

#[cfg(target_arch = "wasm32")]
fn storage_key(family: &str, variant: &str) -> String {
    format!("alumina.ttf:{family}:{variant}")
}

/// Fetch the Google Fonts index (family names, variants, file URLs).
///
/// `api_key` — your Google Fonts Developer API key.
/// Returns a `Vec<FontItem>`.
#[cfg(target_arch = "wasm32")]
pub async fn gf_fetch_index(api_key: &str) -> Result<Vec<FontItem>, JsValue> {
    // Trim payload with `fields` so we only pull what we need.
    // Sort doesn't change content, only order (useful for UI).
    let url = format!(
        "https://www.googleapis.com/webfonts/v1/webfonts?key={key}&sort=popularity&fields=items(family,variants,files,category,subsets)",
        key = api_key
    );

    let text = Request::get(&url).send().await.map_err(to_js_err)?.text().await.map_err(to_js_err)?;
    let resp: WebFontsResponse = serde_json::from_str(&text).map_err(to_js_err)?;
    Ok(resp.items)
}

/// Case-insensitive substring search over family names.
pub fn gf_search<'a>(index: &'a [FontItem], query: &str) -> Vec<&'a FontItem> {
    let q = query.to_ascii_lowercase();
    index
        .iter()
        .filter(|f| f.family.to_ascii_lowercase().contains(&q))
        .collect()
}

/// Return a direct file URL for a `(family, variant)` if present.
///
/// `variant` examples: "regular", "italic", "700", "700italic", …
pub fn gf_find_file_url<'a>(index: &'a [FontItem], family: &str, variant: &str) -> Option<&'a str> {
    let fam = index.iter().find(|f| f.family.eq_ignore_ascii_case(family))?;
    fam.files.get(variant).map(String::as_str)
}

/// Download raw font bytes from a Google Fonts file URL (TTF/WOFF2/etc.).
#[cfg(target_arch = "wasm32")]
pub async fn gf_download_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let resp = Request::get(url).send().await.map_err(to_js_err)?;
    let bytes = resp.binary().await.map_err(to_js_err)?;
    Ok(bytes)
}

/// Persist bytes in `localStorage` as base64 with key `ttf:{family}:{variant}`.
#[cfg(target_arch = "wasm32")]
pub fn persist_ttf_localstorage(family: &str, variant: &str, bytes: &[u8]) -> Result<(), JsValue> {
    let key = storage_key(family, variant);
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    storage()?.set_item(&key, &b64).map_err(|e| e.into())
}

/// Load bytes back from `localStorage` for `(family, variant)`.
#[cfg(target_arch = "wasm32")]
pub fn load_persisted_ttf(family: &str, variant: &str) -> Result<Option<Vec<u8>>, JsValue> {
    let key = storage_key(family, variant);
    let store = storage()?;
    // get_item already returns Result<_, JsValue>
    let Some(b64) = store.get_item(&key)? else {
        return Ok(None);
    };
    // base64::decode returns a Rust error — convert that to JsValue explicitly
    let bytes = base64::decode(b64)
        .map_err(|e| JsValue::from_str(&format!("base64 decode error: {e}")))?;
    Ok(Some(bytes))
}

/// Enumerate all persisted fonts (`ttf:{family}:{variant}`) with decoded sizes.
#[cfg(target_arch = "wasm32")]
pub fn list_persisted_ttf() -> Result<Vec<PersistedFont>, JsValue> {
    let store = storage()?;
    let len = store.length()?; // no map_err here
    let mut out = Vec::new();
    for i in 0..len {
        if let Some(key) = store.key(i)? {
            if let Some(rest) = key.strip_prefix("alumina.ttf:") {
                let mut parts = rest.splitn(2, ':');
                let family = parts.next().unwrap_or_default().to_string();
                let variant = parts.next().unwrap_or_default().to_string();
                out.push(PersistedFont { family, variant });
            }
        }
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
fn to_js_err<E: core::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

// --------------------------- non-wasm stubs ---------------------------

#[cfg(not(target_arch = "wasm32"))]
pub async fn gf_fetch_index(_api_key: &str) -> Result<Vec<FontItem>, ()> {
    Ok(Vec::new())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn gf_search<'a>(index: &'a [FontItem], _query: &str) -> Vec<&'a FontItem> {
    index.iter().collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn gf_find_file_url<'a>(_index: &'a [FontItem], _family: &str, _variant: &str) -> Option<&'a str> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn gf_download_bytes(_url: &str) -> Result<Vec<u8>, ()> {
    Ok(Vec::new())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn persist_ttf_localstorage(_family: &str, _variant: &str, _bytes: &[u8]) -> Result<(), ()> {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_persisted_ttf(_family: &str, _variant: &str) -> Result<Option<Vec<u8>>, ()> {
    Ok(None)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn list_persisted_ttf() -> Result<Vec<PersistedFont>, ()> {
    Ok(Vec::new())
}
