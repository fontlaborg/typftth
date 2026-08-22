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
use crate::interp::Machine;
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

/// Hinting context for one (font, ppem, location).
pub struct Hinter<'f, 'a> {
    pub font: &'f HintFont<'a>,
    pub ppem: i32,
    pub coords: Vec<F2Dot14>,
    /// Machine state after `fpgm` + `prep`.
    base: Machine,
    /// Error from `prep` (we still hint, like FreeType after a failed prep? — no: we report).
    pub prep_error: Option<InterpreterError>,
    zone: Zone,
}

impl<'f, 'a> Hinter<'f, 'a> {
    /// Run `fpgm` and `prep` for `ppem` at `coords` (normalized 2.14).
    pub fn new(font: &'f HintFont<'a>, ppem: i32, coords: &[F2Dot14]) -> Result<Hinter<'f, 'a>, LoadError> {
        Self::with_observer(font, ppem, coords, &mut NoTrace)
    }

    /// Like [`Hinter::new`] but tracing the `fpgm`/`prep` runs.
    pub fn with_observer(
        font: &'f HintFont<'a>,
        ppem: i32,
        coords: &[F2Dot14],
        observer: &mut dyn StepObserver,
    ) -> Result<Hinter<'f, 'a>, LoadError> {
        let cvt_funits = font.cvt_at(coords);
        let mut m = Machine::new(font.maxp, cvt_funits.len());
        m.set_ppem(ppem, font.units_per_em as i16);
        m.set_coords(coords);
        // CVT: FUnits → 26.6 pixels with the same factor WCVTF uses.
        let scale = m.scale.units_per_em_scale.x;
        let cvt: Vec<i32> = cvt_funits.iter().map(|&v| F26Dot6::from_int(v).mul_f16_up(scale).0).collect();
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

    /// FUnit → 26.6 at this ppem (Apple's `HinterContext.scale`: truncating).
    #[inline]
    pub fn scale_funits(&self, v: i16) -> i32 {
        let prod = i64::from(v) * i64::from(self.ppem) * 64;
        (prod / i64::from(self.font.units_per_em)) as i32
    }

    /// Load a glyph into a fresh zone, run its program, return the result.
    pub fn hint_glyph(&mut self, gid: u32, observer: &mut dyn StepObserver) -> Result<HintedGlyph, LoadError> {
        let outline = self.font.glyph(gid, &self.coords)?;
        let mut zone = self.zone.clone();
        let ppem = self.ppem;
        let upem = i64::from(self.font.units_per_em);
        zone.load_outline(&outline.xs, &outline.ys, &outline.on_curve, &outline.end_pts, outline.phantoms, |v| {
            ((i64::from(v) * i64::from(ppem) * 64) / upem) as i32
        });
        let mut m = self.base.clone();
        let code = Code { fpgm: self.font.fpgm, prep: self.font.prep, glyf: &outline.instructions };
        let error = if outline.instructions.is_empty() {
            None
        } else {
            let unscaled_wrong = outline.is_composite && !self.coords.is_empty();
            m.run(Program::Glyf, code, Some(&mut zone), unscaled_wrong, observer).err()
        };
        Ok(HintedGlyph { zone, error, outline })
    }
}
