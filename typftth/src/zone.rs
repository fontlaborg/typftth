//! this_file: typftth/src/zone.rs
//!
//! Point zones. A zone owns parallel arrays for hinted, scaled and
//! original coordinates, on-curve flags, touch flags and contour bounds.
//! The glyph zone carries 8 private phantom points after the outline
//! (4 public: LSB, RSB, TSB, BSB). Port of `Zone.swift`.

#![allow(missing_docs)]

use crate::error::InterpreterError;
use crate::fixed::F26Dot6;
use crate::gs::{Coord, ZoneType};

/// Number of phantom points the glyph zone carries (4 public + 4 private).
pub const PRIVATE_PHANTOM_COUNT: usize = 8;
/// Number of public phantom points (LSB, RSB, TSB, BSB).
pub const PUBLIC_PHANTOM_COUNT: usize = 4;

/// On-curve bit in `on_curve`.
pub const ONCURVE: u8 = 0x01;
/// Touch flags in `f`.
pub const XMOVED: u8 = 0x01;
pub const YMOVED: u8 = 0x02;

/// A zone of points (twilight or glyph).
#[derive(Clone, Debug)]
pub struct Zone {
    pub zone_type: ZoneType,
    /// Hinted (current) positions, 26.6.
    pub x: Vec<i32>,
    pub y: Vec<i32>,
    /// Scaled, unhinted positions, 26.6.
    pub ox: Vec<i32>,
    pub oy: Vec<i32>,
    /// Unscaled FUnit positions.
    pub oox: Vec<i16>,
    pub ooy: Vec<i16>,
    pub on_curve: Vec<u8>,
    /// Touch flags (`XMOVED`/`YMOVED`).
    pub f: Vec<u8>,
    /// Contour start / end point indices.
    pub sp: Vec<u16>,
    pub ep: Vec<u16>,
    /// Outline point count (without phantoms) as set by the caller.
    pub outline_points: usize,
    pub contour_count: usize,
}

impl Zone {
    /// An empty zone with the given capacities (points include phantoms for
    /// the glyph zone).
    pub fn with_capacity(zone_type: ZoneType, max_points: usize, max_contours: usize) -> Zone {
        Zone {
            zone_type,
            x: vec![0; max_points],
            y: vec![0; max_points],
            ox: vec![0; max_points],
            oy: vec![0; max_points],
            oox: vec![0; max_points],
            ooy: vec![0; max_points],
            on_curve: vec![0; max_points],
            f: vec![0; max_points],
            sp: vec![0; max_contours.max(1)],
            ep: vec![0; max_contours.max(1)],
            outline_points: 0,
            contour_count: 0,
        }
    }

    /// Capacity in points (any index below this is addressable by opcodes).
    #[inline]
    pub fn max_point_count(&self) -> usize {
        self.x.len()
    }
    #[inline]
    pub fn max_contour_count(&self) -> usize {
        self.sp.len()
    }
    /// Outline points + phantoms (glyph zone) / outline points (twilight).
    #[inline]
    pub fn point_count(&self) -> usize {
        match self.zone_type {
            ZoneType::Glyph => self.outline_points + PRIVATE_PHANTOM_COUNT,
            ZoneType::Twilight => self.outline_points,
        }
    }
    #[inline]
    pub fn phantom_start(&self) -> usize {
        self.outline_points
    }

    /// Bounds check an opcode point index (against the allocation, like the
    /// reference, not against `point_count`).
    #[inline]
    pub fn check_point(&self, index: i32) -> Result<usize, InterpreterError> {
        if index < 0 || (index as usize) >= self.max_point_count() {
            return Err(InterpreterError::NoSuchPoint);
        }
        Ok(index as usize)
    }

    #[inline]
    pub fn hinted(&self, i: usize) -> Coord {
        Coord::new(self.x[i], self.y[i])
    }
    #[inline]
    pub fn scaled(&self, i: usize) -> Coord {
        Coord::new(self.ox[i], self.oy[i])
    }
    #[inline]
    pub fn original(&self) -> (&[i16], &[i16]) {
        (&self.oox, &self.ooy)
    }
    #[inline]
    pub fn original_at(&self, i: usize) -> (i16, i16) {
        (self.oox[i], self.ooy[i])
    }
    #[inline]
    pub fn set_hinted(&mut self, i: usize, c: Coord) {
        self.x[i] = c.x.0;
        self.y[i] = c.y.0;
    }
    #[inline]
    pub fn set_scaled(&mut self, i: usize, c: Coord) {
        self.ox[i] = c.x.0;
        self.oy[i] = c.y.0;
    }
    #[inline]
    pub fn set_original(&mut self, i: usize, x: i16, y: i16) {
        self.oox[i] = x;
        self.ooy[i] = y;
    }
    #[inline]
    pub fn mark_moved(&mut self, i: usize, x: bool, y: bool) {
        let bits = (if x { XMOVED } else { 0 }) | (if y { YMOVED } else { 0 });
        if bits != 0 {
            self.f[i] |= bits;
        }
    }
    #[inline]
    pub fn clear_moved(&mut self, i: usize, x: bool, y: bool) {
        let bits = (if x { XMOVED } else { 0 }) | (if y { YMOVED } else { 0 });
        if bits != 0 {
            self.f[i] &= !bits;
        }
    }
    #[inline]
    pub fn toggle_on_curve(&mut self, i: usize) {
        self.on_curve[i] ^= ONCURVE;
    }

    /// Swift `readContour(index:)` — closed range of point indices.
    pub fn read_contour(&self, index: i32) -> Result<(usize, usize), InterpreterError> {
        if index < 0 || (index as usize) >= self.contour_count {
            return Err(InterpreterError::NoSuchContour);
        }
        let start = self.sp[index as usize] as usize;
        let end = self.ep[index as usize] as usize;
        if start > end {
            return Err(InterpreterError::InvalidOperand);
        }
        Ok((start, end))
    }

    /// Swift `check(against:)` — only some opcodes perform this check.
    pub fn check_against_maxp(&self, maxp: &crate::Maxp) -> Result<(), InterpreterError> {
        if self.zone_type != ZoneType::Glyph {
            return Ok(());
        }
        let maxp_contours = maxp.max_contours.max(maxp.max_composite_contours) as usize;
        let maxp_points = maxp.max_points.max(maxp.max_composite_points) as usize;
        if self.point_count() > maxp_points + PRIVATE_PHANTOM_COUNT || self.contour_count > maxp_contours {
            return Err(InterpreterError::MaxpLimitExceeded);
        }
        Ok(())
    }

    /// Reset hinted ← scaled for outline + public phantoms (Swift `rollback`).
    pub fn rollback(&mut self) {
        let n = (self.phantom_start() + PUBLIC_PHANTOM_COUNT).min(self.max_point_count());
        for i in 0..n {
            self.x[i] = self.ox[i];
            self.y[i] = self.oy[i];
        }
    }

    /// Load an outline: unscaled FUnits, on-curve flags, contour end points,
    /// the four public phantom FUnit positions; scales with `scale`.
    #[allow(clippy::too_many_arguments)]
    pub fn load_outline(
        &mut self,
        xs: &[i16],
        ys: &[i16],
        on_curve: &[bool],
        end_pts: &[u16],
        phantoms: [(i16, i16); PUBLIC_PHANTOM_COUNT],
        scale: impl Fn(i16) -> i32,
    ) {
        let n = xs.len();
        let total = n + PRIVATE_PHANTOM_COUNT;
        if total > self.max_point_count() {
            self.grow_points(total);
        }
        if end_pts.len() > self.max_contour_count() {
            self.sp.resize(end_pts.len(), 0);
            self.ep.resize(end_pts.len(), 0);
        }
        for i in 0..n {
            let sx = scale(xs[i]);
            let sy = scale(ys[i]);
            self.oox[i] = xs[i];
            self.ooy[i] = ys[i];
            self.ox[i] = sx;
            self.oy[i] = sy;
            self.x[i] = sx;
            self.y[i] = sy;
            self.on_curve[i] = if on_curve[i] { ONCURVE } else { 0 };
            self.f[i] = 0;
        }
        for (j, i) in (n..n + PRIVATE_PHANTOM_COUNT).enumerate() {
            let (px, py) = phantoms.get(j).copied().unwrap_or((0, 0));
            let sx = scale(px);
            let sy = scale(py);
            self.oox[i] = px;
            self.ooy[i] = py;
            self.ox[i] = sx;
            self.oy[i] = sy;
            self.x[i] = sx;
            self.y[i] = sy;
            self.on_curve[i] = 0;
            self.f[i] = 0;
        }
        let mut prev: i32 = -1;
        for (c, &last) in end_pts.iter().enumerate() {
            self.sp[c] = (prev + 1) as u16;
            self.ep[c] = last;
            prev = i32::from(last);
        }
        self.outline_points = n;
        self.contour_count = end_pts.len();
    }

    fn grow_points(&mut self, n: usize) {
        self.x.resize(n, 0);
        self.y.resize(n, 0);
        self.ox.resize(n, 0);
        self.oy.resize(n, 0);
        self.oox.resize(n, 0);
        self.ooy.resize(n, 0);
        self.on_curve.resize(n, 0);
        self.f.resize(n, 0);
    }

    /// Hinted outline as 26.6 values (outline points only, no phantoms).
    pub fn hinted_points(&self) -> impl Iterator<Item = (F26Dot6, F26Dot6)> + '_ {
        (0..self.outline_points).map(|i| (F26Dot6(self.x[i]), F26Dot6(self.y[i])))
    }
}
