//! this_file: typftth/src/gs.rs
//!
//! Graphics state, round state, unit vectors and the handful of vector
//! helpers the opcodes share. Port of `GraphicsState.swift`,
//! `RoundState.swift` and `VectorTypes.swift`.

#![allow(missing_docs)]

use crate::fixed::{
    mixed_mul_nearest_away, mixed_mul_nearest_up, mul_div_i64, round_f64, shift_right_i64, F16Dot16, F26Dot6,
    F28Dot36, F2Dot14, F2Dot30, Rounding,
};

/// Point zone selector (`zp0`/`zp1`/`zp2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZoneType {
    /// Zone 0 — the twilight zone.
    Twilight = 0,
    /// Zone 1 — the glyph zone.
    Glyph = 1,
}

impl ZoneType {
    /// Swift `ZoneType(rawValue:maxElements:)`.
    pub fn from_raw(raw: i32, max_elements: u16) -> Option<ZoneType> {
        if raw < 0 || raw >= i32::from(max_elements) {
            return None;
        }
        Some(if raw == 0 { ZoneType::Twilight } else { ZoneType::Glyph })
    }
}

/// Axis selector used by `alwaysTouchAxis` and IUP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// A 2.14 unit vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Vec2F2Dot14 {
    pub x: F2Dot14,
    pub y: F2Dot14,
}

impl Vec2F2Dot14 {
    pub const X_AXIS: Vec2F2Dot14 = Vec2F2Dot14 { x: F2Dot14::ONE, y: F2Dot14::ZERO };
    pub const Y_AXIS: Vec2F2Dot14 = Vec2F2Dot14 { x: F2Dot14::ZERO, y: F2Dot14::ONE };

    /// Dot product of two 2.14 vectors → 2.14 (wrapping, ties away, like
    /// Swift `&*`/`&+` on `F2Dot14`).
    pub fn dot14(self, o: Vec2F2Dot14) -> F2Dot14 {
        let a = fixed_mul_wrapping_i16(o.x.0, self.x.0);
        let b = fixed_mul_wrapping_i16(o.y.0, self.y.0);
        F2Dot14(a.wrapping_add(b))
    }

    /// Project a 26.6 coordinate onto this vector (Swift `FixedCartesian<F2Dot14>.dot`).
    #[inline]
    pub fn dot(self, p: Coord) -> F26Dot6 {
        F26Dot6(
            mixed_mul_nearest_away(p.x.0, self.x.0 as i32, 14)
                .wrapping_add(mixed_mul_nearest_away(p.y.0, self.y.0 as i32, 14)),
        )
    }
}

/// `a * b` for 2.14 operands, rounded to nearest-or-away, wrapping (Swift
/// `multipliedReportingOverflow(by:).wrappedValue` with the F2Dot14 default).
fn fixed_mul_wrapping_i16(a: i16, b: i16) -> i16 {
    let prod = (a as i64) * (b as i64);
    let half: i64 = 1 << 13;
    let sign: i64 = if prod < 0 { -1 } else { 0 };
    // toNearestOrAway: add half + sign (sign is all-ones when negative)
    let rounded = prod.wrapping_add(half.wrapping_add(sign)) >> 14;
    rounded as i16
}

/// A 26.6 coordinate or distance vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Coord {
    pub x: F26Dot6,
    pub y: F26Dot6,
}

impl Coord {
    pub const ZERO: Coord = Coord { x: F26Dot6::ZERO, y: F26Dot6::ZERO };
    #[inline]
    pub fn new(x: i32, y: i32) -> Coord {
        Coord { x: F26Dot6(x), y: F26Dot6(y) }
    }
    /// Swift `OutlineCoord(bitPattern: UnscaledCoord)` — FUnits reinterpreted
    /// as 26.6 bit patterns (i.e. divided by 64). Load-bearing quirk.
    #[inline]
    pub fn from_unscaled_bits(x: i16, y: i16) -> Coord {
        Coord::new(x as i32, y as i32)
    }
    #[inline]
    pub fn wrapping_sub(self, o: Coord) -> Coord {
        Coord { x: self.x.wrapping_sub(o.x), y: self.y.wrapping_sub(o.y) }
    }
    #[inline]
    pub fn wrapping_add(self, o: Coord) -> Coord {
        Coord { x: self.x.wrapping_add(o.x), y: self.y.wrapping_add(o.y) }
    }
    /// Returns `None` on overflow in either axis.
    #[inline]
    pub fn checked_add(self, o: Coord) -> Option<Coord> {
        Some(Coord { x: self.x.checked_add(o.x)?, y: self.y.checked_add(o.y)? })
    }
    /// Swift `OutlineCoord(scaling: value, by: vector)`.
    #[inline]
    pub fn scaling(value: F26Dot6, v: Vec2F2Dot14) -> Coord {
        Coord { x: value.mul_f2_away(v.x), y: value.mul_f2_away(v.y) }
    }
    /// Per-axis mixed multiply by a 16.16 vector, ties up.
    #[inline]
    pub fn mul_f16_up(self, s: Vec2F16Dot16) -> Coord {
        Coord { x: self.x.mul_f16_up(s.x), y: self.y.mul_f16_up(s.y) }
    }
    /// Per-axis division by a 16.16 vector (Swift `OutlineCoord.div`).
    #[inline]
    pub fn div_f16(self, s: Vec2F16Dot16, rule: Rounding) -> Coord {
        Coord { x: self.x.div_f16(s.x, rule), y: self.y.div_f16(s.y, rule) }
    }
}

/// A 16.16 vector (scale factors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Vec2F16Dot16 {
    pub x: F16Dot16,
    pub y: F16Dot16,
}

impl Vec2F16Dot16 {
    pub const IDENTITY: Vec2F16Dot16 = Vec2F16Dot16 { x: F16Dot16::ONE, y: F16Dot16::ONE };
    pub const ZERO: Vec2F16Dot16 = Vec2F16Dot16 { x: F16Dot16::ZERO, y: F16Dot16::ZERO };

    pub fn div_saturating(self, d: F16Dot16, rule: Rounding) -> Vec2F16Dot16 {
        Vec2F16Dot16 { x: self.x.div_saturating(d, rule), y: self.y.div_saturating(d, rule) }
    }
    pub fn mul_div(self, a: F16Dot16, b: F16Dot16, rule: Rounding) -> Vec2F16Dot16 {
        Vec2F16Dot16 { x: self.x.mul_div(a.0, b.0, rule), y: self.y.mul_div(a.0, b.0, rule) }
    }
    /// Swift `FixedVector<F16Dot16>.mixedMulRoundingToNearestOrAway(F2Dot14 vector)`.
    pub fn mul_f2_away(self, v: Vec2F2Dot14) -> Vec2F16Dot16 {
        Vec2F16Dot16 {
            x: F16Dot16(mixed_mul_nearest_away(self.x.0, v.x.0 as i32, 14)),
            y: F16Dot16(mixed_mul_nearest_away(self.y.0, v.y.0 as i32, 14)),
        }
    }
    /// Swift `magnitude(rounding: .towardZero)`: hypot in double, truncated,
    /// saturating to max.
    pub fn magnitude(self) -> F16Dot16 {
        let d = (self.x.to_f64()).hypot(self.y.to_f64());
        F16Dot16::from_f64_if_representable(d, Rounding::TowardZero).unwrap_or(F16Dot16::MAX)
    }
}

/// Port of `normalizeLikeFnt_NormalizeUsedTo` (the only unit-vector
/// normalisation the interpreter uses; every quirk is load-bearing).
pub fn normalize_like_fnt(x: F26Dot6, y: F26Dot6) -> Vec2F2Dot14 {
    let largest = x.abs_saturating().0.max(y.abs_saturating().0);
    let shift = largest.leading_zeros() as i32 - 2;
    if shift < 0 {
        return Vec2F2Dot14::X_AXIS;
    }
    let x1 = x.0.wrapping_shl(shift as u32);
    let y1 = y.0.wrapping_shl(shift as u32);
    let dx1 = x1 as f64;
    let dy1 = y1 as f64;
    let magnitude = (dx1 * dx1 + dy1 * dy1).sqrt().floor() as i32;
    if magnitude == 0 {
        return Vec2F2Dot14::X_AXIS;
    }
    let fract_one: f64 = (1i64 << 30) as f64;
    let divisor = (magnitude as f64) / fract_one;
    let qx = ((x1 as f64) / fract_one) / divisor;
    let qy = ((y1 as f64) / fract_one) / divisor;
    let div_x = (qx * fract_one) as i32;
    let div_y = (qy * fract_one) as i32;
    let tx = shift_right_i64(div_x as i64, 16, Rounding::ToNearestOrUp) as i32;
    let ty = shift_right_i64(div_y as i64, 16, Rounding::ToNearestOrUp) as i32;
    Vec2F2Dot14 { x: F2Dot14(tx as i16), y: F2Dot14(ty as i16) }
}

/// Unit vector along `p1 - p2`, optionally rotated 90° counter-clockwise.
pub fn compute_unit_vector(p1: Coord, p2: Coord, rotated: bool) -> Vec2F2Dot14 {
    let d = p1.wrapping_sub(p2);
    let n = normalize_like_fnt(d.x, d.y);
    if rotated {
        Vec2F2Dot14 { x: F2Dot14(0i16.wrapping_sub(n.y.0)), y: n.x }
    } else {
        n
    }
}

/* ------------------------------------------------------------------ round state */

/// Rounding method of a `RoundState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundMethod {
    Mask,
    Divide,
}

/// TrueType round state (RTG, RTHG, SROUND…). Port of `RoundState.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundState {
    pub method: RoundMethod,
    pub phase: F26Dot6,
    pub threshold: F26Dot6,
    pub period: F26Dot6,
    pub period45: F28Dot36,
}

const ONE_HALF: F26Dot6 = F26Dot6(32);
const ONE_QUARTER: F26Dot6 = F26Dot6(16);
/// Period for the reserved period bits (Swift `garbagePeriod`).
pub const GARBAGE_PERIOD: i32 = 999;

impl RoundState {
    const fn mask(period: F26Dot6, phase: F26Dot6, threshold: F26Dot6) -> RoundState {
        RoundState { method: RoundMethod::Mask, phase, threshold, period, period45: F28Dot36::ZERO }
    }
    /// ROFF
    pub const ROFF: RoundState = RoundState::mask(F26Dot6(1), F26Dot6::ZERO, F26Dot6::ZERO);
    /// RDTG
    pub const RDTG: RoundState = RoundState::mask(F26Dot6::ONE, F26Dot6::ZERO, F26Dot6::ZERO);
    /// RUTG
    pub const RUTG: RoundState = RoundState::mask(F26Dot6::ONE, F26Dot6::ZERO, F26Dot6(63));
    /// RTG
    pub const RTG: RoundState = RoundState::mask(F26Dot6::ONE, F26Dot6::ZERO, ONE_HALF);
    /// RTHG
    pub const RTHG: RoundState = RoundState::mask(F26Dot6::ONE, ONE_HALF, ONE_HALF);
    /// RTDG
    pub const RTDG: RoundState = RoundState::mask(ONE_HALF, F26Dot6::ZERO, ONE_QUARTER);

    /// SROUND
    pub fn super_round(param: u8) -> RoundState {
        let period_bits = param >> 6;
        let period = match period_bits {
            0..=2 => F26Dot6(1i32 << (6 + i32::from(period_bits) - 1)),
            _ => F26Dot6(GARBAGE_PERIOD),
        };
        let (phase, threshold) = phase_and_threshold(param, period);
        RoundState::mask(period, phase, threshold)
    }

    // sqrt(2)/4, sqrt(2)/2, sqrt(2) as F2Dot30 (Swift hex float literals).
    const S45_PERIOD: [i32; 3] = [
        (core::f64::consts::FRAC_1_SQRT_2 / 2.0 * (1u64 << 30) as f64) as i32,
        (core::f64::consts::FRAC_1_SQRT_2 * (1u64 << 30) as f64) as i32,
        (core::f64::consts::SQRT_2 * (1u64 << 30) as f64) as i32,
    ];

    /// S45ROUND
    pub fn super45_round(param: u8) -> RoundState {
        let period45 = match param >> 6 {
            0 => F2Dot30(Self::S45_PERIOD[0]),
            1 => F2Dot30(Self::S45_PERIOD[1]),
            2 => F2Dot30(Self::S45_PERIOD[2]),
            _ => F2Dot30(GARBAGE_PERIOD),
        };
        // F26Dot6(period45): shift right 24 with toNearestOrUp
        let period = F26Dot6(shift_right_i64(period45.0 as i64, 24, Rounding::ToNearestOrUp) as i32);
        let (phase, threshold) = phase_and_threshold(param, period);
        RoundState {
            method: RoundMethod::Divide,
            phase,
            threshold,
            period,
            // F28Dot36(period45): shift left 6
            period45: F28Dot36((period45.0 as i64) << 6),
        }
    }

    /// Round a 26.6 value.
    pub fn round(&self, value: F26Dot6) -> F26Dot6 {
        let rounded = if value.0 < 0 {
            F26Dot6(0i32.wrapping_sub(self.round_positive(F26Dot6(0i32.wrapping_sub(value.0))).0))
        } else {
            self.round_positive(value)
        };
        if (value.0 < 0) == (rounded.0 < 0) {
            return rounded;
        }
        if value.0 > 0 {
            self.phase
        } else {
            F26Dot6(0i32.wrapping_sub(self.phase.0))
        }
    }

    fn round_positive(&self, value: F26Dot6) -> F26Dot6 {
        let mut result = value.0.wrapping_add(self.threshold.0.wrapping_sub(self.phase.0));
        match self.method {
            RoundMethod::Mask => {
                result &= self.period.0.wrapping_neg();
            }
            RoundMethod::Divide => {
                // Promote to F28Dot36 (shift left 30), divide by period45 with
                // the F28Dot36 default rounding (nearest-or-away).
                let promoted: i64 = (result as i64) << 30;
                let Some(divided) = mul_div_i64(promoted, 1i64 << 36, self.period45.0, Rounding::ToNearestOrAway)
                else {
                    return F26Dot6(-1);
                };
                // reducePrecisionAndDropTopBits(divided, .down): F58Dot6 = >> 30 rounding down, truncate to i32
                let mut r = shift_right_i64(divided, 30, Rounding::Down) as i32;
                r &= !63;
                // multiply back: F28Dot36(r) * period45 → F28Dot36, nearest-or-away, overflow → -1
                let Some(multiplied) = fixed_mul_i64((r as i64) << 30, self.period45.0, 36, Rounding::ToNearestOrAway)
                else {
                    return F26Dot6(-1);
                };
                result = shift_right_i64(multiplied, 30, Rounding::ToNearestOrAway) as i32;
            }
        }
        F26Dot6(result.wrapping_add(self.phase.0))
    }
}

/// Full-width fixed multiply `(a*b) >> frac` with rounding; `None` on overflow
/// of the i64 result (Swift `multipliedReportingOverflow` with `overflow`).
pub fn fixed_mul_i64(a: i64, b: i64, frac: u32, rule: Rounding) -> Option<i64> {
    let prod: i128 = (a as i128) * (b as i128);
    let unit: i128 = 1i128 << frac;
    let fmask: i128 = unit - 1;
    let half: i128 = unit >> 1;
    let sign: i128 = if prod < 0 { -1 } else { 0 };
    let addend: i128 = match rule {
        Rounding::Down => 0,
        Rounding::Up => fmask,
        Rounding::TowardZero => fmask & sign,
        Rounding::AwayFromZero => fmask & !sign,
        Rounding::ToNearestOrDown => half - 1,
        Rounding::ToNearestOrUp => half,
        Rounding::ToNearestOrZero => half - 1 - sign,
        Rounding::ToNearestOrAway => half + sign,
        Rounding::ToNearestOrEven => half - 1 + ((prod >> frac) & 1),
    };
    let r = (prod + addend) >> frac;
    i64::try_from(r).ok()
}

/// Same as [`fixed_mul_i64`] but wrapping to i32 (Swift `.wrappedValue`).
pub fn fixed_mul_wrapping_i32(a: i32, b: i32, frac: u32, rule: Rounding) -> i32 {
    let prod: i128 = (a as i128) * (b as i128);
    let unit: i128 = 1i128 << frac;
    let fmask: i128 = unit - 1;
    let half: i128 = unit >> 1;
    let sign: i128 = if prod < 0 { -1 } else { 0 };
    let addend: i128 = match rule {
        Rounding::Down => 0,
        Rounding::Up => fmask,
        Rounding::TowardZero => fmask & sign,
        Rounding::AwayFromZero => fmask & !sign,
        Rounding::ToNearestOrDown => half - 1,
        Rounding::ToNearestOrUp => half,
        Rounding::ToNearestOrZero => half - 1 - sign,
        Rounding::ToNearestOrAway => half + sign,
        Rounding::ToNearestOrEven => half - 1 + ((prod >> frac) & 1),
    };
    ((prod + addend) >> frac) as i32
}

/// Saturating fixed multiply for i32 formats (Swift `multipliedWithSaturation`).
pub fn fixed_mul_saturating_i32(a: i32, b: i32, frac: u32, rule: Rounding) -> i32 {
    match fixed_mul_i64(a as i64, b as i64, frac, rule) {
        Some(v) => v.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        None => {
            if ((a as i64) * (b as i64)) < 0 {
                i32::MIN
            } else {
                i32::MAX
            }
        }
    }
}

fn phase_and_threshold(bits: u8, period: F26Dot6) -> (F26Dot6, F26Dot6) {
    let p = period.0;
    let phase_bits = (bits >> 4) & 3;
    let threshold_bits = bits & 0xf;
    let phase = (2i32.wrapping_add(p.wrapping_mul(i32::from(phase_bits)))) / 4;
    let threshold = if threshold_bits == 0 {
        p.wrapping_sub(1)
    } else {
        // LOAD-BEARING RIGHT SHIFT (see docs/bincompat.md)
        (4i32.wrapping_add((i32::from(threshold_bits as i8) - 4).wrapping_mul(p))) >> 3
    };
    (F26Dot6(phase), F26Dot6(threshold))
}

/* ------------------------------------------------------------------ graphics state */

/// SCANCTRL/SCANTYPE state (raw u32; high 16 = scan type "kind").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ScanControl(pub u32);

impl ScanControl {
    pub fn kind(self) -> u16 {
        (self.0 >> 16) as u16
    }
    pub fn set_kind(&mut self, kind: u16) {
        self.0 = (self.0 & 0xffff) | (u32::from(kind) << 16);
    }
    pub fn low(self) -> u16 {
        (self.0 & 0xffff) as u16
    }
}

/// INSTCTRL flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InstructControl(pub u8);

impl InstructControl {
    pub const NO_GRID_FIT: u8 = 1 << 0;
    pub const DEFAULT: u8 = 1 << 1;
    pub fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// prep → glyf carried-over parameters (Swift `Parameters`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameters {
    pub control_value_cut_in: F26Dot6,
    pub single_width_cut_in: F26Dot6,
    pub single_width_value: F26Dot6,
    pub auto_flip: bool,
    pub delta_base: i16,
    pub delta_shift: i16,
    pub instruct_control: InstructControl,
    pub minimum_distance: F26Dot6,
    pub round_state: RoundState,
    pub scan_control: ScanControl,
}

/// 17/16 px — spec default for the CVT cut-in.
pub const DEFAULT_CVT_CUT_IN: F26Dot6 = F26Dot6(17 * 64 / 16);

impl Default for Parameters {
    fn default() -> Self {
        Parameters {
            control_value_cut_in: DEFAULT_CVT_CUT_IN,
            single_width_cut_in: F26Dot6::ZERO,
            single_width_value: F26Dot6::ZERO,
            auto_flip: true,
            delta_base: 9,
            delta_shift: 3,
            instruct_control: InstructControl(0),
            minimum_distance: F26Dot6::ONE,
            round_state: RoundState::RTG,
            scan_control: ScanControl(0),
        }
    }
}

/// The TrueType graphics state. Port of `GraphicsState.swift`.
#[derive(Clone, Debug)]
pub struct GraphicsState {
    pub auto_flip: bool,
    pub always_touch_axis: Option<Axis>,
    pub delta_base: i16,
    pub delta_shift: i16,
    pub loop_count: i32,
    pub minimum_distance: F26Dot6,
    pub round_state: RoundState,
    pub rp0: i32,
    pub rp1: i32,
    pub rp2: i32,
    pub instruct_control: InstructControl,
    pub scan_control: ScanControl,
    pub zp0: ZoneType,
    pub zp1: ZoneType,
    pub zp2: ZoneType,
    freedom: Vec2F2Dot14,
    projection: Vec2F2Dot14,
    dual: Option<Vec2F2Dot14>,
    pub projection_is_normal: bool,
    cached_pdotf: Option<F2Dot14>,
    memo_scale: Option<(Vec2F2Dot14, Vec2F16Dot16, F16Dot16)>,
}

const MIN_PDOTF: i16 = 0x0400; // 1/16 in 2.14
const SAFE_PDOTF: i16 = 0x4000; // 1.0 in 2.14

impl Default for GraphicsState {
    fn default() -> Self {
        GraphicsState {
            auto_flip: true,
            always_touch_axis: None,
            delta_base: 9,
            delta_shift: 3,
            loop_count: 1,
            minimum_distance: F26Dot6::ONE,
            round_state: RoundState::RTG,
            rp0: 0,
            rp1: 0,
            rp2: 0,
            instruct_control: InstructControl(0),
            scan_control: ScanControl(0),
            zp0: ZoneType::Glyph,
            zp1: ZoneType::Glyph,
            zp2: ZoneType::Glyph,
            freedom: Vec2F2Dot14::X_AXIS,
            projection: Vec2F2Dot14::X_AXIS,
            dual: None,
            projection_is_normal: false,
            cached_pdotf: Some(F2Dot14::ONE),
            memo_scale: None,
        }
    }
}

impl GraphicsState {
    /// Reset at the start of every program run (Swift `reset(_:)`).
    pub fn reset(&mut self, p: &Parameters) {
        self.rp0 = 0;
        self.rp1 = 0;
        self.rp2 = 0;
        self.zp0 = ZoneType::Glyph;
        self.zp1 = ZoneType::Glyph;
        self.zp2 = ZoneType::Glyph;
        self.set_freedom(Vec2F2Dot14::X_AXIS);
        self.set_projection(Vec2F2Dot14::X_AXIS);
        self.always_touch_axis = Some(Axis::X);
        self.loop_count = 1;
        self.memo_scale = None;
        self.auto_flip = p.auto_flip;
        self.delta_base = p.delta_base;
        self.delta_shift = p.delta_shift;
        self.instruct_control = p.instruct_control;
        self.minimum_distance = p.minimum_distance;
        self.round_state = p.round_state;
        self.scan_control = p.scan_control;
    }

    #[inline]
    pub fn freedom(&self) -> Vec2F2Dot14 {
        self.freedom
    }
    #[inline]
    pub fn projection(&self) -> Vec2F2Dot14 {
        self.projection
    }
    #[inline]
    pub fn dual_projection(&self) -> Vec2F2Dot14 {
        self.dual.unwrap_or(self.projection)
    }
    /// Swift `freedomVector = …` (didSet: clears PdotF cache and alwaysTouchAxis).
    pub fn set_freedom(&mut self, v: Vec2F2Dot14) {
        self.freedom = v;
        self.cached_pdotf = None;
        self.always_touch_axis = None;
    }
    /// Swift `projectionVector = …` (didSet: clears normal flag, cache, dual, alwaysTouchAxis).
    pub fn set_projection(&mut self, v: Vec2F2Dot14) {
        self.projection = v;
        self.projection_is_normal = false;
        self.cached_pdotf = None;
        self.dual = None;
        self.always_touch_axis = None;
    }
    pub fn set_dual_projection(&mut self, v: Vec2F2Dot14) {
        self.dual = Some(v);
    }
    /// SFVTPV — forces the cached dot product to exactly 1.
    pub fn set_freedom_to_projection(&mut self) {
        let p = self.projection;
        self.set_freedom(p);
        self.cached_pdotf = Some(F2Dot14::ONE);
    }

    fn calculate_pdotf(&self) -> F2Dot14 {
        let (p, f) = (self.projection, self.freedom);
        let xa = Vec2F2Dot14::X_AXIS;
        let ya = Vec2F2Dot14::Y_AXIS;
        if (p == xa && f == xa) || (p == ya && f == ya) {
            return F2Dot14::ONE;
        }
        if (p == ya && f == xa) || (p == xa && f == ya) {
            return F2Dot14(SAFE_PDOTF);
        }
        let pdotf = p.dot14(f);
        if pdotf.0 > i16::MIN && pdotf.0.abs() < MIN_PDOTF {
            return F2Dot14(if pdotf.0 < 0 { -SAFE_PDOTF } else { SAFE_PDOTF });
        }
        pdotf
    }

    #[inline]
    pub fn proj_dot_free(&mut self) -> F2Dot14 {
        if let Some(c) = self.cached_pdotf {
            return c;
        }
        let v = self.calculate_pdotf();
        self.cached_pdotf = Some(v);
        v
    }

    /// Distance along the projection vector → movement vector along the
    /// freedom vector (Swift `vector(for:)`).
    pub fn vector_for(&mut self, distance: F26Dot6) -> Coord {
        let pdotf = self.proj_dot_free();
        if pdotf == F2Dot14::ONE {
            return Coord::scaling(distance, self.freedom);
        }
        Coord {
            x: distance.mul_div(self.freedom.x.0 as i32, pdotf.0 as i32, Rounding::TowardZero),
            y: distance.mul_div(self.freedom.y.0 as i32, pdotf.0 as i32, Rounding::TowardZero),
        }
    }

    /// Swift `effectiveScale(for:)`.
    pub fn effective_scale(&mut self, stretch: Vec2F16Dot16) -> F16Dot16 {
        if self.projection.y.0 == 0 {
            return stretch.x;
        }
        if self.projection.x.0 == 0 {
            return stretch.y;
        }
        if stretch.x == F16Dot16::ONE && stretch.y == F16Dot16::ONE {
            return F16Dot16::ONE;
        }
        if let Some((p, s, r)) = self.memo_scale {
            if p == self.projection && s == stretch {
                return r;
            }
        }
        let result = if self.projection_is_normal {
            stretch.mul_f2_away(self.projection).magnitude()
        } else {
            let mag = Vec2F16Dot16 { x: stretch.y, y: stretch.x }.mul_f2_away(self.projection);
            stretch.x.mul_div(stretch.y.0, mag.magnitude().0, Rounding::TowardZero)
        };
        self.memo_scale = Some((self.projection, stretch, result));
        result
    }
}

/* ------------------------------------------------------------------ scale factors */

/// Scale factors derived from ppem / upem (Swift `ScaleFactors`).
#[derive(Clone, Copy, Debug)]
pub struct ScaleFactors {
    pub stretch: Vec2F16Dot16,
    pub units_per_em: i16,
    pub point_size: i32,
    pub cvt_scale: F16Dot16,
    pub is_rotated: bool,
    pub is_stretched: bool,
    pub cvt_stretch: Vec2F16Dot16,
    /// Integer pixels-per-em per axis.
    pub integer_ppem: (i16, i16),
    /// FUnit → 26.6 conversion factor per axis.
    pub units_per_em_scale: Vec2F16Dot16,
}

impl Default for ScaleFactors {
    fn default() -> Self {
        ScaleFactors {
            stretch: Vec2F16Dot16::IDENTITY,
            units_per_em: 0,
            point_size: 0,
            cvt_scale: F16Dot16::ZERO,
            is_rotated: false,
            is_stretched: false,
            cvt_stretch: Vec2F16Dot16::IDENTITY,
            integer_ppem: (0, 0),
            units_per_em_scale: Vec2F16Dot16::ZERO,
        }
    }
}

impl ScaleFactors {
    pub fn new(
        stretch: Vec2F16Dot16,
        units_per_em: i16,
        point_size: i32,
        cvt_scale: F16Dot16,
        is_rotated: bool,
        is_stretched: bool,
    ) -> ScaleFactors {
        let fixed_round = |v: F16Dot16| -> i16 { (v.rounded(Rounding::ToNearestOrUp).0 >> 16) as i16 };
        ScaleFactors {
            stretch,
            units_per_em,
            point_size,
            cvt_scale,
            is_rotated,
            is_stretched,
            cvt_stretch: stretch.div_saturating(cvt_scale, Rounding::TowardZero),
            integer_ppem: (fixed_round(stretch.x), fixed_round(stretch.y)),
            // Host choice: round like FreeType's `FT_DivFix(ppem·64, upem)`
            // (the Swift harness truncated). WCVTF/SSW/MIRP-unscaled paths
            // then agree with the CVT/outline scaling in `hinter.rs`, and
            // fonts that derive CVT indices from a scaled constant (e.g.
            // `cvt[1] = 2048·2048 FUnits`) read the same entries as FreeType.
            units_per_em_scale: stretch.mul_div(
                F16Dot16::from_int(64),
                F16Dot16::from_int(i32::from(units_per_em)),
                Rounding::ToNearestOrAway,
            ),
        }
    }

    /// Plain "ppem on both axes" setup, as the benchmark's `HinterContext` does.
    pub fn for_ppem(ppem: i32, units_per_em: i16) -> ScaleFactors {
        let s = F16Dot16(ppem.wrapping_shl(16));
        ScaleFactors::new(Vec2F16Dot16 { x: s, y: s }, units_per_em, ppem, s, false, false)
    }

    /// Swift `projectedIntegerPPEM`.
    pub fn projected_integer_ppem(&self, gs: &GraphicsState) -> i16 {
        let (sx, sy) = self.integer_ppem;
        if sx == sy {
            return sx;
        }
        let p = gs.projection();
        if p.y.0 == 0 {
            return sx;
        }
        if p.x.0 == 0 {
            return sy;
        }
        let h = ((sx as f64) * p.x.to_f64()).hypot((sy as f64) * p.y.to_f64());
        // F18Dot14(clamping: h) rounds to nearest-even at 14 bits, then
        // roundingWithSaturation (nearest-or-away) and take the integral part.
        let scaled = round_f64(h * 16384.0, Rounding::ToNearestOrEven);
        let bits: i32 = if scaled >= i32::MAX as f64 {
            i32::MAX
        } else if scaled <= i32::MIN as f64 {
            i32::MIN
        } else {
            scaled as i32
        };
        let rounded = bits.saturating_add(0x2000 + if bits < 0 { -1 } else { 0 }) & !0x3fff;
        (rounded >> 14) as i16
    }
}

/// Swift `ControlValue.stretched(by:)`.
#[inline]
pub fn cvt_stretched(value: F26Dot6, scale: F16Dot16) -> F26Dot6 {
    if scale == F16Dot16::ONE {
        value
    } else {
        F26Dot6(mixed_mul_nearest_up(value.0, scale.0, 16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_rtg() {
        let r = RoundState::RTG;
        assert_eq!(r.round(F26Dot6(32)).0, 64);
        assert_eq!(r.round(F26Dot6(31)).0, 0);
        assert_eq!(r.round(F26Dot6(-32)).0, -64);
        assert_eq!(r.round(F26Dot6(-31)).0, 0);
    }

    #[test]
    fn round_rthg_rtdg_rutg_rdtg_roff() {
        assert_eq!(RoundState::RTHG.round(F26Dot6(0)).0, 32);
        assert_eq!(RoundState::RTHG.round(F26Dot6(64)).0, 96);
        assert_eq!(RoundState::RTDG.round(F26Dot6(20)).0, 32);
        assert_eq!(RoundState::RUTG.round(F26Dot6(1)).0, 64);
        assert_eq!(RoundState::RDTG.round(F26Dot6(63)).0, 0);
        assert_eq!(RoundState::ROFF.round(F26Dot6(37)).0, 37);
    }

    #[test]
    fn sround_default_params_equal_rtg() {
        // period 1 (bits 01), phase 0, threshold 8 (= half period) → identical to RTG
        let s = RoundState::super_round(0b0100_1000);
        assert_eq!(s.period, RoundState::RTG.period);
        assert_eq!(s.phase, RoundState::RTG.phase);
        assert_eq!(s.threshold, RoundState::RTG.threshold);
    }

    #[test]
    fn normalize_axes() {
        assert_eq!(normalize_like_fnt(F26Dot6(640), F26Dot6(0)), Vec2F2Dot14::X_AXIS);
        let v = normalize_like_fnt(F26Dot6(0), F26Dot6(-640));
        assert_eq!(v.x.0, 0);
        assert_eq!(v.y.0, -16384);
        let d = normalize_like_fnt(F26Dot6(64), F26Dot6(64));
        assert!((d.x.0 - 11585).abs() <= 1 && (d.y.0 - 11585).abs() <= 1, "{d:?}");
    }

    #[test]
    fn effective_scale_axes() {
        let mut gs = GraphicsState::default();
        let s = Vec2F16Dot16 { x: F16Dot16(12 << 16), y: F16Dot16(24 << 16) };
        assert_eq!(gs.effective_scale(s), F16Dot16(12 << 16));
        gs.set_projection(Vec2F2Dot14::Y_AXIS);
        assert_eq!(gs.effective_scale(s), F16Dot16(24 << 16));
    }
}
