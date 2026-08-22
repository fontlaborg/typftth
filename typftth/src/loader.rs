//! this_file: typftth/src/loader.rs
//!
//! Font loading on `read-fonts`: the tables the interpreter needs, glyph
//! outlines (simple and composite, flattened to one point list) with `gvar`
//! deltas applied, and the CVT with `cvar` applied. FUnits in, FUnits out;
//! scaling is the hinter's job.

#![allow(missing_docs)]

use alloc::vec;
use alloc::vec::Vec;

use read_fonts::tables::glyf::{Anchor, CompositeGlyphFlags, Glyf, Glyph, PointFlags, PointMarker};
use read_fonts::tables::loca::Loca;
use read_fonts::types::{F2Dot14 as RfF2Dot14, Fixed, GlyphId, Point, Tag};
use read_fonts::{FontRef, TableProvider};

use crate::error::LoadError;
use crate::fixed::F2Dot14;
use crate::interp::Maxp;

/// Composite nesting limit (mirrors skrifa's).
pub const COMPOSITE_RECURSION_LIMIT: usize = 32;

/// A variation axis.
#[derive(Clone, Debug)]
pub struct AxisInfo {
    pub tag: [u8; 4],
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// One glyph ready for the interpreter: FUnit points, flags, contours,
/// phantom points and the program to run.
#[derive(Clone, Debug, Default)]
pub struct GlyphOutline {
    pub xs: Vec<i16>,
    pub ys: Vec<i16>,
    pub on_curve: Vec<bool>,
    pub end_pts: Vec<u16>,
    /// LSB, RSB, TSB, BSB phantom points in FUnits.
    pub phantoms: [(i16, i16); 4],
    pub instructions: Vec<u8>,
    pub is_composite: bool,
    /// Number of components (composites only).
    pub component_count: usize,
    pub advance_width: u16,
    pub lsb: i16,
}

/// The hinting-relevant view of a TrueType font. Cheap to clone (borrowed
/// table slices plus the CVT and axis list).
#[derive(Clone)]
pub struct HintFont<'a> {
    font: FontRef<'a>,
    glyf: Glyf<'a>,
    loca: Loca<'a>,
    pub units_per_em: u16,
    pub maxp: Maxp,
    pub fpgm: &'a [u8],
    pub prep: &'a [u8],
    /// CVT in FUnits.
    pub cvt: Vec<i16>,
    pub axes: Vec<AxisInfo>,
    pub glyph_count: u32,
}

impl<'a> HintFont<'a> {
    /// Parse face `index` of `data`.
    pub fn parse(data: &'a [u8], index: u32) -> Result<HintFont<'a>, LoadError> {
        let font = FontRef::from_index(data, index).map_err(|_| LoadError::BadFont)?;
        let head = font.head().map_err(|_| LoadError::Table("head"))?;
        let maxp_t = font.maxp().map_err(|_| LoadError::Table("maxp"))?;
        let loca = font.loca(head.index_to_loc_format() == 1).map_err(|_| LoadError::NotTrueType)?;
        let glyf = font.glyf().map_err(|_| LoadError::NotTrueType)?;
        let fpgm = font.table_data(Tag::new(b"fpgm")).map(|d| d.as_bytes()).unwrap_or(&[]);
        let prep = font.table_data(Tag::new(b"prep")).map(|d| d.as_bytes()).unwrap_or(&[]);
        let cvt: Vec<i16> = font
            .table_data(Tag::new(b"cvt "))
            .map(|d| d.as_bytes().chunks_exact(2).map(|c| i16::from_be_bytes([c[0], c[1]])).collect())
            .unwrap_or_default();
        let axes = font
            .fvar()
            .ok()
            .and_then(|f| f.axes().ok())
            .map(|axes| {
                axes.iter()
                    .map(|a| AxisInfo {
                        tag: a.axis_tag().to_be_bytes(),
                        min: a.min_value().to_f32(),
                        default: a.default_value().to_f32(),
                        max: a.max_value().to_f32(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let maxp = Maxp {
            num_glyphs: maxp_t.num_glyphs(),
            max_points: maxp_t.max_points().unwrap_or(0),
            max_contours: maxp_t.max_contours().unwrap_or(0),
            max_composite_points: maxp_t.max_composite_points().unwrap_or(0),
            max_composite_contours: maxp_t.max_composite_contours().unwrap_or(0),
            max_elements: maxp_t.max_zones().unwrap_or(2),
            max_twilight_points: maxp_t.max_twilight_points().unwrap_or(0),
            max_storage: maxp_t.max_storage().unwrap_or(0),
            max_function_defs: maxp_t.max_function_defs().unwrap_or(0),
            max_instruction_defs: maxp_t.max_instruction_defs().unwrap_or(0),
            max_stack_elements: maxp_t.max_stack_elements().unwrap_or(0),
            max_size_of_instructions: maxp_t.max_size_of_instructions().unwrap_or(0),
            max_component_elements: maxp_t.max_component_elements().unwrap_or(0),
            max_component_depth: maxp_t.max_component_depth().unwrap_or(0),
        };
        Ok(HintFont {
            font,
            glyf,
            loca,
            units_per_em: head.units_per_em(),
            maxp,
            fpgm,
            prep,
            cvt,
            axes,
            glyph_count: u32::from(maxp_t.num_glyphs()),
        })
    }

    /// Underlying `read-fonts` face.
    pub fn font_ref(&self) -> &FontRef<'a> {
        &self.font
    }

    /// Does the font carry TrueType hinting programs at all?
    pub fn is_hinted(&self) -> bool {
        !self.fpgm.is_empty() || !self.prep.is_empty()
    }

    /// Normalized 2.14 coordinates for user-space axis values (`avar`-aware
    /// via skrifa). Missing axes take their defaults.
    pub fn location(&self, user: &[([u8; 4], f32)]) -> Vec<F2Dot14> {
        use skrifa::MetadataProvider;
        let settings: Vec<(skrifa::Tag, f32)> = user.iter().map(|(t, v)| (skrifa::Tag::new(t), *v)).collect();
        let loc = self.font.axes().location(settings.iter().map(|(t, v)| (*t, *v)));
        loc.coords().iter().map(|c| F2Dot14(c.to_bits())).collect()
    }

    /// CVT values in FUnits at `coords` (`cvar` applied).
    /// CVT entries at `coords` in **26.6 FUnits** (`value << 6`), the way
    /// FreeType stores `face->cvt`: `cvar` deltas (16.16) are added with
    /// 1/64 FUnit precision (`FT_fixedToFdot6`), not rounded to whole
    /// FUnits. Scale with [`crate::hinter::scale_cvt`].
    pub fn cvt_at(&self, coords: &[F2Dot14]) -> Vec<i32> {
        let mut values: Vec<i32> = self.cvt.iter().map(|&v| i32::from(v) << 6).collect();
        if coords.is_empty() || coords.iter().all(|c| c.0 == 0) {
            return values;
        }
        if let Ok(cvar) = self.font.cvar() {
            let rf: Vec<RfF2Dot14> = coords.iter().map(|c| RfF2Dot14::from_bits(c.0)).collect();
            let mut deltas = vec![0i32; values.len()];
            if cvar.deltas(self.axes.len() as u16, &rf, &mut deltas).is_ok() {
                for (v, d) in values.iter_mut().zip(deltas) {
                    // FT_fixedToFdot6: (x + 0x200) >> 10 (arithmetic shift)
                    *v += (d.wrapping_add(0x200)) >> 10;
                }
            }
        }
        values
    }

    /// Load a glyph's outline (with `gvar` deltas for `coords`).
    pub fn glyph(&self, gid: u32, coords: &[F2Dot14]) -> Result<GlyphOutline, LoadError> {
        if gid >= self.glyph_count {
            return Err(LoadError::NoSuchGlyph(gid));
        }
        let rf_coords: Vec<RfF2Dot14> = coords.iter().map(|c| RfF2Dot14::from_bits(c.0)).collect();
        let mut out = GlyphOutline::default();
        let hmtx = self.font.hmtx().ok();
        let gid_t = GlyphId::new(gid);
        let adv = hmtx.as_ref().and_then(|h| h.advance(gid_t)).unwrap_or(0);
        let lsb = hmtx.as_ref().and_then(|h| h.side_bearing(gid_t)).unwrap_or(0);
        out.advance_width = adv;
        out.lsb = lsb;

        let mut pts: Vec<Point<i32>> = Vec::new();
        let mut flags: Vec<bool> = Vec::new();
        let mut ends: Vec<u16> = Vec::new();
        let mut phantom = [Point::new(0i32, 0i32); 4];
        let x_min = self.load_into(gid_t, &rf_coords, &mut pts, &mut flags, &mut ends, &mut phantom, &mut out, 0)?;
        // FreeType phantom convention: pp1 = (xMin - lsb, 0), pp2 = pp1 + advance
        if !out.is_composite || phantom == [Point::new(0, 0); 4] {
            // (simple glyph: phantoms computed here; composites: from the
            // USE_MY_METRICS logic in load_into, else computed here too)
        }
        let pp1x = i32::from(x_min) - i32::from(lsb);
        let base = [Point::new(pp1x, 0), Point::new(pp1x + i32::from(adv), 0), Point::new(0, 0), Point::new(0, 0)];
        let phantoms = [
            (clamp16(base[0].x + phantom[0].x), clamp16(base[0].y + phantom[0].y)),
            (clamp16(base[1].x + phantom[1].x), clamp16(base[1].y + phantom[1].y)),
            (clamp16(base[2].x + phantom[2].x), clamp16(base[2].y + phantom[2].y)),
            (clamp16(base[3].x + phantom[3].x), clamp16(base[3].y + phantom[3].y)),
        ];
        out.phantoms = phantoms;
        out.xs = pts.iter().map(|p| clamp16(p.x)).collect();
        out.ys = pts.iter().map(|p| clamp16(p.y)).collect();
        out.on_curve = flags;
        out.end_pts = ends;
        Ok(out)
    }

    /// Recursively load `gid` appending points; returns the glyph's xMin.
    #[allow(clippy::too_many_arguments)]
    fn load_into(
        &self,
        gid: GlyphId,
        coords: &[RfF2Dot14],
        pts: &mut Vec<Point<i32>>,
        flags: &mut Vec<bool>,
        ends: &mut Vec<u16>,
        phantom_deltas: &mut [Point<i32>; 4],
        out: &mut GlyphOutline,
        depth: usize,
    ) -> Result<i16, LoadError> {
        if depth > COMPOSITE_RECURSION_LIMIT {
            return Err(LoadError::CompositeDepth);
        }
        let glyph = self.loca.get_glyf(gid, &self.glyf).map_err(|_| LoadError::Table("glyf"))?;
        let Some(glyph) = glyph else {
            return Ok(0); // empty glyph
        };
        match glyph {
            Glyph::Simple(simple) => {
                let n = simple.num_points();
                let mut points = vec![Point::new(0i32, 0i32); n];
                let mut pflags = vec![PointFlags::default(); n];
                simple.read_points_fast(&mut points, &mut pflags).map_err(|_| LoadError::Table("glyf"))?;
                let contour_ends: Vec<u16> = simple.end_pts_of_contours().iter().map(|e| e.get()).collect();
                let mut deltas = vec![Point::new(0i32, 0i32); n + 4];
                if !coords.is_empty() {
                    self.apply_gvar(gid, coords, &points, &contour_ends, &mut deltas);
                }
                let base = pts.len() as u16;
                for (i, p) in points.iter().enumerate() {
                    pts.push(Point::new(p.x + deltas[i].x, p.y + deltas[i].y));
                    flags.push(pflags[i].is_on_curve());
                }
                for e in contour_ends {
                    ends.push(e.wrapping_add(base));
                }
                if depth == 0 {
                    out.instructions = simple.instructions().to_vec();
                    for (k, d) in phantom_deltas.iter_mut().enumerate() {
                        *d = deltas[n + k];
                    }
                }
                Ok(simple.x_min())
            }
            Glyph::Composite(comp) => {
                if depth == 0 {
                    out.is_composite = true;
                    out.instructions = comp.instructions().map(|i| i.to_vec()).unwrap_or_default();
                }
                let components: Vec<_> = comp.components().collect();
                let ncomp = components.len();
                if depth == 0 {
                    out.component_count = ncomp;
                }
                let mut deltas = vec![Point::new(0i32, 0i32); ncomp + 4];
                if !coords.is_empty() {
                    self.apply_gvar_composite(gid, coords, &mut deltas);
                }
                for (ci, c) in components.iter().enumerate() {
                    let start = pts.len();
                    let mut sub_phantom = [Point::new(0i32, 0i32); 4];
                    let mut sub_out = GlyphOutline::default();
                    self.load_into(
                        GlyphId::from(c.glyph),
                        coords,
                        pts,
                        flags,
                        ends,
                        &mut sub_phantom,
                        &mut sub_out,
                        depth + 1,
                    )?;
                    // transform
                    let t = c.transform;
                    let has_xform = t.xx != RfF2Dot14::from_bits(0x4000)
                        || t.yy != RfF2Dot14::from_bits(0x4000)
                        || t.xy != RfF2Dot14::from_bits(0)
                        || t.yx != RfF2Dot14::from_bits(0);
                    if has_xform {
                        for p in &mut pts[start..] {
                            let x = p.x as f64;
                            let y = p.y as f64;
                            let nx = x * t.xx.to_f32() as f64 + y * t.yx.to_f32() as f64;
                            let ny = x * t.xy.to_f32() as f64 + y * t.yy.to_f32() as f64;
                            p.x = nx.round() as i32;
                            p.y = ny.round() as i32;
                        }
                    }
                    let (mut dx, mut dy) = match c.anchor {
                        Anchor::Offset { x, y } => (i32::from(x), i32::from(y)),
                        Anchor::Point { base, component } => {
                            let b = pts.get(base as usize).copied().unwrap_or_default();
                            let cpt = pts.get(start + component as usize).copied().unwrap_or_default();
                            (b.x - cpt.x, b.y - cpt.y)
                        }
                    };
                    if matches!(c.anchor, Anchor::Offset { .. }) {
                        if c.flags.contains(CompositeGlyphFlags::SCALED_COMPONENT_OFFSET) && has_xform {
                            let fx = dx as f64 * t.xx.to_f32() as f64 + dy as f64 * t.yx.to_f32() as f64;
                            let fy = dx as f64 * t.xy.to_f32() as f64 + dy as f64 * t.yy.to_f32() as f64;
                            dx = fx.round() as i32;
                            dy = fy.round() as i32;
                        }
                        dx += deltas[ci].x;
                        dy += deltas[ci].y;
                    }
                    for p in &mut pts[start..] {
                        p.x += dx;
                        p.y += dy;
                    }
                    if c.flags.contains(CompositeGlyphFlags::USE_MY_METRICS) && depth == 0 {
                        *phantom_deltas = sub_phantom;
                    }
                }
                if depth == 0 && phantom_deltas.iter().all(|p| p.x == 0 && p.y == 0) {
                    for (k, d) in phantom_deltas.iter_mut().enumerate() {
                        *d = deltas[ncomp + k];
                    }
                }
                Ok(comp.x_min())
            }
        }
    }

    fn apply_gvar(&self, gid: GlyphId, coords: &[RfF2Dot14], points: &[Point<i32>], ends: &[u16], deltas: &mut [Point<i32>]) {
        let Ok(gvar) = self.font.gvar() else { return };
        let Ok(Some(var)) = gvar.glyph_variation_data(gid) else { return };
        let n = deltas.len();
        let mut acc = vec![Point::new(Fixed::ZERO, Fixed::ZERO); n];
        let mut tuple_deltas = vec![Point::new(Fixed::ZERO, Fixed::ZERO); n];
        let mut tflags = vec![PointFlags::default(); n];
        let ref_points: Vec<Point<Fixed>> =
            points.iter().map(|p| Point::new(Fixed::from_i32(p.x), Fixed::from_i32(p.y))).collect();
        for (tuple, scalar) in var.active_tuples_at(coords) {
            for d in tuple_deltas.iter_mut() {
                *d = Point::new(Fixed::ZERO, Fixed::ZERO);
            }
            if tuple.has_deltas_for_all_points() {
                if tuple.accumulate_dense_deltas(&mut tuple_deltas, scalar).is_err() {
                    continue;
                }
            } else {
                for f in tflags.iter_mut() {
                    *f = PointFlags::default();
                }
                if tuple.accumulate_sparse_deltas(&mut tuple_deltas, &mut tflags, scalar).is_err() {
                    continue;
                }
                infer_deltas(&ref_points, ends, &tflags, &mut tuple_deltas);
            }
            for (a, d) in acc.iter_mut().zip(&tuple_deltas) {
                a.x += d.x;
                a.y += d.y;
            }
        }
        for (d, a) in deltas.iter_mut().zip(acc) {
            d.x = a.x.round().to_i32();
            d.y = a.y.round().to_i32();
        }
    }

    fn apply_gvar_composite(&self, gid: GlyphId, coords: &[RfF2Dot14], deltas: &mut [Point<i32>]) {
        let Ok(gvar) = self.font.gvar() else { return };
        let Ok(Some(var)) = gvar.glyph_variation_data(gid) else { return };
        let n = deltas.len();
        let mut acc = vec![Point::new(Fixed::ZERO, Fixed::ZERO); n];
        for (tuple, scalar) in var.active_tuples_at(coords) {
            for d in tuple.deltas() {
                let ix = d.position as usize;
                if ix < n {
                    let p = d.apply_scalar::<Fixed>(scalar);
                    acc[ix].x += p.x;
                    acc[ix].y += p.y;
                }
            }
        }
        for (d, a) in deltas.iter_mut().zip(acc) {
            d.x = a.x.round().to_i32();
            d.y = a.y.round().to_i32();
        }
    }
}

fn clamp16(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Interpolate deltas for unreferenced points (the gvar "IUP" rule), per
/// contour, per axis. `flags` carry `HAS_DELTA` for referenced points.
fn infer_deltas(points: &[Point<Fixed>], ends: &[u16], flags: &[PointFlags], deltas: &mut [Point<Fixed>]) {
    let mut start = 0usize;
    for &e in ends {
        let end = e as usize;
        if end < start || end >= points.len() {
            break;
        }
        infer_contour(&points[start..=end], &flags[start..=end], &mut deltas[start..=end]);
        start = end + 1;
    }
}

fn infer_contour(points: &[Point<Fixed>], flags: &[PointFlags], deltas: &mut [Point<Fixed>]) {
    let n = points.len();
    if n == 0 {
        return;
    }
    let has = |i: usize| flags[i].has_marker(PointMarker::HAS_DELTA);
    let Some(first) = (0..n).find(|&i| has(i)) else { return };
    let mut ref1 = first;
    loop {
        // find next referenced point (wrapping)
        let mut ref2 = (ref1 + 1) % n;
        let mut steps = 0;
        while !has(ref2) && steps < n {
            ref2 = (ref2 + 1) % n;
            steps += 1;
        }
        // interpolate points strictly between ref1 and ref2
        let mut i = (ref1 + 1) % n;
        while i != ref2 {
            for axis in 0..2 {
                let (p1, p2, p) = if axis == 0 {
                    (points[ref1].x, points[ref2].x, points[i].x)
                } else {
                    (points[ref1].y, points[ref2].y, points[i].y)
                };
                let (d1, d2) = if axis == 0 {
                    (deltas[ref1].x, deltas[ref2].x)
                } else {
                    (deltas[ref1].y, deltas[ref2].y)
                };
                let (lo, hi, dlo, dhi) = if p1 <= p2 { (p1, p2, d1, d2) } else { (p2, p1, d2, d1) };
                let d = if lo == hi {
                    if d1 == d2 {
                        d1
                    } else {
                        Fixed::ZERO
                    }
                } else if p <= lo {
                    dlo
                } else if p >= hi {
                    dhi
                } else {
                    // linear interpolation
                    let t = (p - lo).to_f64() / (hi - lo).to_f64();
                    Fixed::from_f64(dlo.to_f64() + (dhi - dlo).to_f64() * t)
                };
                if axis == 0 {
                    deltas[i].x = d;
                } else {
                    deltas[i].y = d;
                }
            }
            i = (i + 1) % n;
        }
        ref1 = ref2;
        if ref1 == first {
            break;
        }
    }
}
