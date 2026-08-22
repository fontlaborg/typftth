//! this_file: typftth/src/hinter.rs
//!
//! End-to-end hinting: set up a [`Machine`] for a font at a ppem and
//! variation location (run `fpgm` + `prep` once), then hint glyphs. Each
//! glyph starts from a copy of the post-`prep` machine so glyph programs
//! cannot leak CVT/storage/twilight changes into the next glyph.

#![allow(missing_docs)]

use alloc::vec::Vec;

use crate::error::LoadError;
use crate::exec::{Code, Program};
use crate::fixed::{F26Dot6, F2Dot14};
use crate::gs::ZoneType;
use crate::interp::{GetInfoProfile, Machine};
use crate::loader::{GlyphOutline, HintFont};
use crate::trace::{NoTrace, StepObserver};
use crate::zone::{Zone, PRIVATE_PHANTOM_COUNT};
use crate::InterpreterError;

/// A hinted glyph.
#[derive(Clone, Debug)]
pub struct HintedGlyph {
    /// The glyph zone after the glyph program (hinted + scaled + original
    /// points, flags, contours, 8 phantoms).
    pub zone: Zone,
    /// Error the glyph program stopped with, if any (zone rolled back).
    pub error: Option<InterpreterError>,
    /// The outline as loaded.
    pub outline: GlyphOutline,
}

impl HintedGlyph {
    /// Hinted outline points (26.6), without phantoms.
    pub fn points(&self) -> Vec<(F26Dot6, F26Dot6)> {
        self.zone.hinted_points().collect()
    }
    /// Hinted advance width in 26.6 (pp2.x − pp1.x).
    pub fn advance(&self) -> F26Dot6 {
        let n = self.zone.phantom_start();
        F26Dot6(self.zone.x[n + 1].wrapping_sub(self.zone.x[n]))
    }
}

/// Host choices for a [`Hinter`] (all default to the reference behaviour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HinterOptions {
    /// What `GETINFO` reports.
    pub getinfo: GetInfoProfile,
    /// Tolerate out-of-range CVT indices like FreeType (non-pedantic).
    pub lenient_cvt: bool,
}

impl HinterOptions {
    /// Behave like FreeType as far as the host can: v35/v40 `GETINFO`
    /// reporting for the given target and lenient CVT access.
    pub fn freetype(getinfo: GetInfoProfile) -> HinterOptions {
        HinterOptions { getinfo, lenient_cvt: true }
    }
}

/// Hinting context for one (font, ppem, location).
pub struct Hinter<'a> {
    pub font: HintFont<'a>,
    pub ppem: i32,
    pub coords: Vec<F2Dot14>,
    /// Machine state after `fpgm` + `prep`.
    base: Machine,
    /// Error from `prep` (we still hint, like FreeType after a failed prep? — no: we report).
    pub prep_error: Option<InterpreterError>,
    zone: Zone,
}

impl<'a> Hinter<'a> {
    /// Run `fpgm` and `prep` for `ppem` at `coords` (normalized 2.14).
    pub fn new(font: HintFont<'a>, ppem: i32, coords: &[F2Dot14]) -> Result<Hinter<'a>, LoadError> {
        Self::with_observer(font, ppem, coords, &mut NoTrace)
    }

    /// Like [`Hinter::new`] but tracing the `fpgm`/`prep` runs.
    pub fn with_observer(
        font: HintFont<'a>,
        ppem: i32,
        coords: &[F2Dot14],
        observer: &mut dyn StepObserver,
    ) -> Result<Hinter<'a>, LoadError> {
        Self::with_options(font, ppem, coords, HinterOptions::default(), observer)
    }

    /// Full constructor: choose the host options (what `GETINFO` reports,
    /// CVT leniency) before `fpgm`/`prep` run, and trace them.
    pub fn with_options(
        font: HintFont<'a>,
        ppem: i32,
        coords: &[F2Dot14],
        options: HinterOptions,
        observer: &mut dyn StepObserver,
    ) -> Result<Hinter<'a>, LoadError> {
        let cvt_fdot6 = font.cvt_at(coords);
        let mut m = Machine::new(font.maxp, cvt_fdot6.len());
        m.getinfo = options.getinfo;
        m.lenient_cvt = options.lenient_cvt;
        m.set_ppem(ppem, font.units_per_em as i16);
        m.set_coords(coords);
        let cvt: Vec<i32> = cvt_fdot6.iter().map(|&v| scale_cvt(v, ppem, font.units_per_em)).collect();
        m.set_cvt(&cvt);
        let code = Code { fpgm: font.fpgm, prep: font.prep, glyf: &[] };
        let mut prep_error = None;
        if !font.fpgm.is_empty() {
            m.run(Program::Fpgm, code, None, false, observer)?;
        }
        if !font.prep.is_empty() {
            if let Err(e) = m.run(Program::Prep, code, None, false, observer) {
                prep_error = Some(e);
            }
        }
        let maxp = font.maxp;
        let max_points = (maxp.max_points.max(maxp.max_composite_points) as usize) + PRIVATE_PHANTOM_COUNT;
        let max_contours = maxp.max_contours.max(maxp.max_composite_contours) as usize;
        Ok(Hinter {
            font,
            ppem,
            coords: coords.to_vec(),
            base: m,
            prep_error,
            zone: Zone::with_capacity(ZoneType::Glyph, max_points.max(PRIVATE_PHANTOM_COUNT), max_contours.max(1)),
        })
    }

    /// The post-`prep` machine (CVT, storage, twilight, function tables).
    pub fn machine(&self) -> &Machine {
        &self.base
    }

    /// FUnit → 26.6 at this ppem. Rounds to nearest (ties away from zero)
    /// like FreeType's `FT_MulFix`, so unhinted outlines match FreeType's
    /// exactly and engine comparisons only show interpreter differences.
    /// (Apple's benchmark harness truncated; that is a host choice, not an
    /// interpreter rule.)
    #[inline]
    pub fn scale_funits(&self, v: i16) -> i32 {
        scale_funit(v, self.ppem, self.font.units_per_em)
    }

    /// Load a glyph into a fresh zone, run its program, return the result.
    pub fn hint_glyph(&mut self, gid: u32, observer: &mut dyn StepObserver) -> Result<HintedGlyph, LoadError> {
        let outline = self.font.glyph(gid, &self.coords)?;
        let mut zone = self.zone.clone();
        let ppem = self.ppem;
        let upem = self.font.units_per_em;
        zone.load_outline(&outline.xs, &outline.ys, &outline.on_curve, &outline.end_pts, outline.phantoms, |v| {
            scale_funit(v, ppem, upem)
        });
        let mut m = self.base.clone();
        let code = Code { fpgm: self.font.fpgm, prep: self.font.prep, glyf: &outline.instructions };
        // INSTCTRL selector 1 set by `prep` disables grid-fitting for every
        // glyph at this size (FreeType: `FT_LOAD_NO_HINTING`). The reference
        // interpreter only records the flag; the host has to honour it.
        let no_grid_fit = self.base.gs.instruct_control.contains(crate::gs::InstructControl::NO_GRID_FIT);
        let error = if outline.instructions.is_empty() || no_grid_fit {
            None
        } else {
            let unscaled_wrong = outline.is_composite && !self.coords.is_empty();
            m.run(Program::Glyf, code, Some(&mut zone), unscaled_wrong, observer).err()
        };
        Ok(HintedGlyph { zone, error, outline })
    }
}

/// CVT entry (26.6 FUnits, see [`HintFont::cvt_at`]) → 26.6 pixels exactly
/// as FreeType's `tt_size_run_prep` does it: the 16.16 scale is first
/// **shifted right by 6** (dropping precision), then applied with
/// `FT_MulFix`. This is why FreeType's scaled CVT can differ by one unit
/// from `scale_funit` of the same value; fonts that branch on CVT values
/// or derive CVT indices from them depend on it.
#[inline]
pub fn scale_cvt(v_fdot6: i32, ppem: i32, upem: u16) -> i32 {
    let s = ft_scale(ppem, upem) >> 6;
    ft_mul_fix(i64::from(v_fdot6), s)
}

/// `FT_MulFix(a, b)`: `(a·b + 0x8000 − (a·b < 0)) >> 16` (arithmetic shift).
#[inline]
pub fn ft_mul_fix(a: i64, b: i64) -> i32 {
    let ab = a * b;
    ((ab + 0x8000 - i64::from(ab < 0)) >> 16) as i32
}

/// FUnit → 26.6 pixels exactly as FreeType does it: the scale is first
/// quantized to 16.16 with `FT_DivFix(ppem·64, upem)`, then applied with
/// `FT_MulFix` (round to nearest, symmetric around zero).
#[inline]
pub fn scale_funit(v: i16, ppem: i32, upem: u16) -> i32 {
    scale_funit_i32(i32::from(v), ppem, upem)
}

/// 16.16 scale factor `ppem·64 / upem` rounded like `FT_DivFix`.
#[inline]
pub fn ft_scale(ppem: i32, upem: u16) -> i64 {
    let a = i64::from(ppem) * 64;
    let b = i64::from(upem).max(1);
    ((a << 16) + b / 2) / b
}

#[inline]
pub fn scale_funit_i32(v: i32, ppem: i32, upem: u16) -> i32 {
    let s = ft_scale(ppem, upem);
    let prod = i64::from(v).abs() * s;
    let c = (prod + 0x8000) >> 16;
    (if v < 0 { -c } else { c }) as i32
}
