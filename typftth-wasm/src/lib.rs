//! this_file: typftth-wasm/src/lib.rs
//!
//! wasm-bindgen surface for the FontLab TTH Debugger. One `TthFont` per
//! opened font; `record()` returns the snapshot blob the debugger already
//! parses for FreeType sessions.

use wasm_bindgen::prelude::*;

use typftth::hinter::Hinter;
use typftth::loader::HintFont;
use typftth::trace::Recorder;
use typftth::F2Dot14;

/// An opened TrueType font.
#[wasm_bindgen]
pub struct TthFont {
    data: Vec<u8>,
    index: u32,
    glyph_count: u32,
    upem: u32,
    axes: Vec<String>,
    hinted: bool,
}

#[wasm_bindgen]
impl TthFont {
    /// Parse face `index` of `bytes`.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], index: u32) -> Result<TthFont, JsError> {
        let data = bytes.to_vec();
        let f = HintFont::parse(&data, index).map_err(|e| JsError::new(&e.to_string()))?;
        let axes = f.axes.iter().map(|a| String::from_utf8_lossy(&a.tag).into_owned()).collect();
        Ok(TthFont { glyph_count: f.glyph_count, upem: u32::from(f.units_per_em), axes, hinted: f.is_hinted(), data, index })
    }

    /// Number of glyphs.
    #[wasm_bindgen(getter, js_name = glyphCount)]
    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }
    /// Units per em.
    #[wasm_bindgen(getter, js_name = unitsPerEm)]
    pub fn units_per_em(&self) -> u32 {
        self.upem
    }
    /// Variation axis tags, in `fvar` order.
    #[wasm_bindgen(getter)]
    pub fn axes(&self) -> Vec<String> {
        self.axes.clone()
    }
    /// Whether the font has `fpgm`/`prep`.
    #[wasm_bindgen(getter)]
    pub fn hinted(&self) -> bool {
        self.hinted
    }

    /// Convert user-space axis values (one per axis, `fvar` order) to the
    /// normalized 2.14 coordinates `record`/`hint` expect (`avar` applied).
    pub fn normalize(&self, design: &[f32]) -> Result<Vec<i16>, JsError> {
        let f = HintFont::parse(&self.data, self.index).map_err(|e| JsError::new(&e.to_string()))?;
        let user: Vec<([u8; 4], f32)> = f.axes.iter().zip(design).map(|(a, v)| (a.tag, *v)).collect();
        Ok(f.location(&user).into_iter().map(|c| c.0).collect())
    }

    /// Record the glyph program of `gid` at `ppem`. `coords` are normalized
    /// 2.14 values (i16), one per axis — the same layout the FreeType
    /// wrapper takes (pass an empty array for static fonts or the default
    /// location). Returns the snapshot blob (v1).
    pub fn record(&self, gid: u32, ppem: u32, coords: &[i16]) -> Result<Vec<u8>, JsError> {
        let f = HintFont::parse(&self.data, self.index).map_err(|e| JsError::new(&e.to_string()))?;
        let coords: Vec<F2Dot14> = coords.iter().map(|&c| F2Dot14(c)).collect();
        let mut h = Hinter::new(f.clone(), ppem as i32, &coords).map_err(|e| JsError::new(&e.to_string()))?;
        let mut rec = Recorder::new(self.upem, ppem, gid);
        let g = h.hint_glyph(gid, &mut rec).map_err(|e| JsError::new(&e.to_string()))?;
        rec.finish(&g.zone, g.error);
        Ok(rec.to_blob())
    }

    /// Hint `gid` and return `[x0, y0, x1, y1, …]` 26.6 outline points
    /// (no phantoms), or throw.
    pub fn hint(&self, gid: u32, ppem: u32, coords: &[i16]) -> Result<Vec<i32>, JsError> {
        let f = HintFont::parse(&self.data, self.index).map_err(|e| JsError::new(&e.to_string()))?;
        let coords: Vec<F2Dot14> = coords.iter().map(|&c| F2Dot14(c)).collect();
        let mut h = Hinter::new(f.clone(), ppem as i32, &coords).map_err(|e| JsError::new(&e.to_string()))?;
        let g = h.hint_glyph(gid, &mut typftth::NoTrace).map_err(|e| JsError::new(&e.to_string()))?;
        let mut out = Vec::with_capacity(g.zone.outline_points * 2);
        for (x, y) in g.zone.hinted_points() {
            out.push(x.0);
            out.push(y.0);
        }
        Ok(out)
    }
}

/// Engine name + version, for session metadata.
#[wasm_bindgen]
pub fn version() -> String {
    format!("typftth {} (GX interpreter v7)", typftth::VERSION)
}
