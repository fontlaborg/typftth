//! this_file: typftth/src/fixed.rs
//!
//! Fixed-point numbers with the exact rounding rules the reference
//! interpreter relies on. This is a deliberately small hand-rolled subset of
//! the generic `FixedPoint` library vendored by the Swift source: only the
//! formats and operations the interpreter uses, but each one bit-exact.
//!
//! Conventions (matching Swift):
//! - `/` on integers truncates toward zero; `>>` floors. The distinction is
//!   load-bearing in several places (see `docs/bincompat.md`).
//! - "mixed" multiplications `F26Dot6 × F16Dot16 → F26Dot6` compute the full
//!   64-bit product and round it back to the left operand's format.

use core::fmt;

/// Rounding rules used by the interpreter. Names follow Swift Numerics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// Toward −∞.
    Down,
    /// Toward +∞.
    Up,
    /// Toward zero (truncation).
    TowardZero,
    /// Away from zero.
    AwayFromZero,
    /// Nearest, ties toward −∞.
    ToNearestOrDown,
    /// Nearest, ties toward +∞.
    ToNearestOrUp,
    /// Nearest, ties toward zero.
    ToNearestOrZero,
    /// Nearest, ties away from zero.
    ToNearestOrAway,
    /// Nearest, ties to even.
    ToNearestOrEven,
}

impl Rounding {
    /// The rule to apply to a magnitude when the signed result is negative.
    fn negated(self) -> Rounding {
        match self {
            Rounding::Down => Rounding::Up,
            Rounding::Up => Rounding::Down,
            Rounding::ToNearestOrDown => Rounding::ToNearestOrUp,
            Rounding::ToNearestOrUp => Rounding::ToNearestOrDown,
            other => other,
        }
    }
}

/// `dividend / divisor` on magnitudes with a rounding rule. `None` when the
/// divisor is zero or the quotient does not fit in `u64`.
fn div_mag(dividend: u128, divisor: u128, rule: Rounding) -> Option<u64> {
    if divisor == 0 {
        return None;
    }
    let q = match rule {
        Rounding::ToNearestOrEven => {
            let q = dividend / divisor;
            let r = dividend % divisor;
            let half = (divisor - (q & 1)) >> 1;
            if r > half {
                q + 1
            } else {
                q
            }
        }
        _ => {
            let addend = match rule {
                Rounding::Down | Rounding::TowardZero => 0,
                Rounding::Up | Rounding::AwayFromZero => divisor - 1,
                Rounding::ToNearestOrDown | Rounding::ToNearestOrZero => (divisor - 1) >> 1,
                Rounding::ToNearestOrUp | Rounding::ToNearestOrAway => divisor >> 1,
                Rounding::ToNearestOrEven => 0,
            };
            let adjusted = dividend.checked_add(addend)?;
            adjusted / divisor
        }
    };
    u64::try_from(q).ok()
}

/// Signed `x * a / b` with full-width intermediate and a rounding rule.
/// `None` if `b == 0` or the result does not fit in `i64`.
pub fn mul_div_i64(x: i64, a: i64, b: i64, rule: Rounding) -> Option<i64> {
    let negative = (x < 0) != ((a < 0) != (b < 0));
    let rule = if negative { rule.negated() } else { rule };
    let p = (x.unsigned_abs() as u128) * (a.unsigned_abs() as u128);
    let q = div_mag(p, b.unsigned_abs() as u128, rule)?;
    from_mag_i64(q, negative)
}

fn from_mag_i64(mag: u64, negative: bool) -> Option<i64> {
    if negative {
        if mag > i64::MIN.unsigned_abs() {
            None
        } else {
            Some(0i64.wrapping_sub(mag as i64))
        }
    } else {
        i64::try_from(mag).ok()
    }
}

fn from_mag_i32(mag: u64, negative: bool) -> Option<i32> {
    if negative {
        if mag > i32::MIN.unsigned_abs() as u64 {
            None
        } else {
            Some(0i32.wrapping_sub(mag as i32))
        }
    } else {
        i32::try_from(mag).ok()
    }
}

/// Signed `x * a / b` for 32-bit operands. `None` on overflow / zero divisor.
pub fn mul_div_i32(x: i32, a: i32, b: i32, rule: Rounding) -> Option<i32> {
    let negative = (x < 0) != ((a < 0) != (b < 0));
    let rule = if negative { rule.negated() } else { rule };
    let p = (x.unsigned_abs() as u128) * (a.unsigned_abs() as u128);
    let q = div_mag(p, b.unsigned_abs() as u128, rule)?;
    from_mag_i32(q, negative)
}

/// Shift right with rounding (Swift `shifted(rightBy:rounding:)`, happy path
/// `0 < count < 64`). Negative counts shift left.
pub fn shift_right_i64(v: i64, count: u32, rule: Rounding) -> i64 {
    if count == 0 {
        return v;
    }
    if count >= 64 {
        // Everything but the sign is lost; round |v| / 2^count ∈ (0, 1).
        let floor: i64 = if v < 0 { -1 } else { 0 };
        if v == 0 {
            return 0;
        }
        let ceiling = floor + 1;
        return match rule {
            Rounding::Down => floor,
            Rounding::Up => ceiling,
            Rounding::TowardZero => 0,
            Rounding::AwayFromZero => {
                if v < 0 {
                    -1
                } else {
                    1
                }
            }
            _ => 0,
        };
    }
    let mask: u128 = (1u128 << count) - 1;
    let lost: u128 = (v as u64 as u128) & mask;
    let floor = v >> count;
    let half: u128 = 1u128 << (count - 1);
    round_floor_lost(floor, lost, mask, half, v, rule)
}

fn round_floor_lost(floor: i64, lost: u128, mask: u128, half: u128, v: i64, rule: Rounding) -> i64 {
    let ceiling = floor.wrapping_add(if lost == 0 { 0 } else { 1 });
    let count = mask.count_ones();
    let bump = |round: u128| -> i64 { floor.wrapping_add(((round + lost) >> count) as i64) };
    match rule {
        Rounding::Down => floor,
        Rounding::Up => ceiling,
        Rounding::TowardZero => {
            if v > 0 {
                floor
            } else {
                ceiling
            }
        }
        Rounding::AwayFromZero => {
            if v < 0 {
                floor
            } else {
                ceiling
            }
        }
        Rounding::ToNearestOrDown => bump(half - 1),
        Rounding::ToNearestOrUp => bump(half),
        Rounding::ToNearestOrZero => bump(half - if v < 0 { 0 } else { 1 }),
        Rounding::ToNearestOrAway => bump(half - if v > 0 { 0 } else { 1 }),
        Rounding::ToNearestOrEven => bump((mask >> 1) + ((floor & 1) as u128)),
    }
}

macro_rules! fixed_type {
    ($(#[$doc:meta])* $name:ident, $int:ty, $frac:expr, $default:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(pub $int);

        #[allow(missing_docs)]
        impl $name {
            /// Number of fraction bits.
            pub const FRAC_BITS: u32 = $frac;
            /// Default rounding used by conversions and `rounded()`.
            pub const DEFAULT_ROUNDING: Rounding = $default;
            /// 1.0
            pub const ONE: $name = $name((1 as $int) << $frac);
            /// Largest representable value.
            pub const MAX: $name = $name(<$int>::MAX);
            /// Smallest representable value.
            pub const MIN: $name = $name(<$int>::MIN);
            /// Zero.
            pub const ZERO: $name = $name(0);

            #[inline]
            pub const fn from_bits(bits: $int) -> Self {
                $name(bits)
            }
            #[inline]
            pub const fn bits(self) -> $int {
                self.0
            }
            /// Integer value scaled into this format (wrapping on overflow, as
            /// Swift's integer-literal init does for in-range literals).
            #[inline]
            pub const fn from_int(v: $int) -> Self {
                $name(v.wrapping_shl($frac))
            }
            #[inline]
            pub fn wrapping_add(self, o: Self) -> Self {
                $name(self.0.wrapping_add(o.0))
            }
            #[inline]
            pub fn wrapping_sub(self, o: Self) -> Self {
                $name(self.0.wrapping_sub(o.0))
            }
            #[inline]
            pub fn checked_add(self, o: Self) -> Option<Self> {
                self.0.checked_add(o.0).map($name)
            }
            #[inline]
            pub fn checked_sub(self, o: Self) -> Option<Self> {
                self.0.checked_sub(o.0).map($name)
            }
            #[inline]
            pub fn saturating_add(self, o: Self) -> Self {
                $name(self.0.saturating_add(o.0))
            }
            #[inline]
            pub fn saturating_sub(self, o: Self) -> Self {
                $name(self.0.saturating_sub(o.0))
            }
            #[inline]
            pub fn wrapping_neg(self) -> Self {
                $name(self.0.wrapping_neg())
            }
            /// Swift `absWithSaturation`.
            #[inline]
            pub fn abs_saturating(self) -> Self {
                if self.0 < 0 {
                    $name((0 as $int).saturating_sub(self.0))
                } else {
                    self
                }
            }
            /// Round to an integral value with saturation
            /// (Swift `roundingWithSaturation`).
            pub fn rounded(self, rule: Rounding) -> Self {
                let bits = self.0;
                let int_mask: $int = !(((1 as $int) << $frac) - 1);
                let frac: $int = ((1 as $int) << $frac) - 1;
                let unit: $int = (1 as $int) << $frac;
                let half: $int = unit >> 1;
                let sign: $int = if bits < 0 { -1 } else { 0 };
                let sat = |add: $int| bits.saturating_add(add) & int_mask;
                match rule {
                    Rounding::Down => $name(bits & int_mask),
                    Rounding::TowardZero => $name(bits.wrapping_add(frac & sign) & int_mask),
                    Rounding::Up => $name(sat(frac)),
                    Rounding::AwayFromZero => $name(sat(frac & !sign)),
                    Rounding::ToNearestOrDown => $name(sat(half.wrapping_sub(1))),
                    Rounding::ToNearestOrUp => $name(sat(half)),
                    Rounding::ToNearestOrZero => $name(sat(half.wrapping_sub(1).wrapping_sub(sign))),
                    Rounding::ToNearestOrAway => $name(sat(half.wrapping_add(sign))),
                    Rounding::ToNearestOrEven => {
                        let p = (bits >> $frac) & 1;
                        $name(sat(half.wrapping_sub(1).wrapping_add(p)))
                    }
                }
            }
            /// Integral part after rounding with the default rule.
            #[inline]
            pub fn to_int(self, rule: Rounding) -> $int {
                self.rounded(rule).0 >> $frac
            }
            /// `self * num / den` with full-width intermediate; `None` on
            /// overflow or zero divisor (Swift `scaledIfRepresentable`).
            #[inline]
            pub fn scaled_if_representable(self, num: $int, den: $int, rule: Rounding) -> Option<Self> {
                mul_div_i64(self.0 as i64, num as i64, den as i64, rule)
                    .and_then(|v| <$int>::try_from(v).ok())
                    .map($name)
            }
            /// Swift `mulDiv`: never fails — falls back to a double
            /// computation with clamping, or saturates on division by zero.
            pub fn mul_div(self, num: $int, den: $int, rule: Rounding) -> Self {
                if let Some(v) = self.scaled_if_representable(num, den, rule) {
                    return v;
                }
                if den != 0 {
                    let d = (self.0 as f64) * (num as f64) / (den as f64);
                    return Self::from_f64_clamping(d, rule);
                }
                if (self.0 >= 0) != (num >= 0) {
                    Self::MIN
                } else {
                    Self::MAX
                }
            }
            /// `self / other` with saturation (Swift `dividedWithSaturation`).
            pub fn div_saturating(self, other: Self, rule: Rounding) -> Self {
                let num: i64 = 1i64 << $frac;
                match mul_div_i64(self.0 as i64, num, other.0 as i64, rule)
                    .and_then(|v| <$int>::try_from(v).ok())
                {
                    Some(v) => $name(v),
                    None => {
                        if (self.0 ^ other.0) >= 0 {
                            Self::MAX
                        } else {
                            Self::MIN
                        }
                    }
                }
            }
            /// Swift `init(clamping: Double, rounding:)` — the double is
            /// scaled by 2^frac, rounded with `rule`, then clamped.
            pub fn from_f64_clamping(value: f64, rule: Rounding) -> Self {
                let scaled = value * ((1u64 << $frac) as f64);
                let r = round_f64(scaled, rule);
                if r.is_nan() {
                    return Self::ZERO;
                }
                if r >= (<$int>::MAX as f64) {
                    Self::MAX
                } else if r <= (<$int>::MIN as f64) {
                    Self::MIN
                } else {
                    $name(r as $int)
                }
            }
            /// Swift `init(ifRepresentable: Double, rounding:)`.
            pub fn from_f64_if_representable(value: f64, rule: Rounding) -> Option<Self> {
                let scaled = value * ((1u64 << $frac) as f64);
                let r = round_f64(scaled, rule);
                if !r.is_finite() || r > (<$int>::MAX as f64) || r < (<$int>::MIN as f64) {
                    return None;
                }
                Some($name(r as $int))
            }
            /// Value as f64.
            #[inline]
            pub fn to_f64(self) -> f64 {
                (self.0 as f64) / ((1u64 << $frac) as f64)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_f64())
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.to_f64())
            }
        }
    };
}

fixed_type!(
    /// 58.6 fixed point (i64).
    F58Dot6, i64, 6, Rounding::ToNearestOrAway
);
fixed_type!(
    /// 28.36 fixed point (i64), used by the divide rounding path.
    F28Dot36, i64, 36, Rounding::ToNearestOrAway
);
fixed_type!(
    /// 2.30 fixed point (i32).
    F2Dot30, i32, 30, Rounding::ToNearestOrAway
);
fixed_type!(
    /// 26.6 fixed point (i32) — the interpreter's pixel unit.
    F26Dot6, i32, 6, Rounding::ToNearestOrUp
);
fixed_type!(
    /// 18.14 fixed point (i32).
    F18Dot14, i32, 14, Rounding::ToNearestOrAway
);
fixed_type!(
    /// 16.16 fixed point (i32) — scale factors.
    F16Dot16, i32, 16, Rounding::ToNearestOrUp
);
fixed_type!(
    /// 2.14 fixed point (i16) — unit vectors and variation coordinates.
    F2Dot14, i16, 14, Rounding::ToNearestOrAway
);

/// Round an f64 to an integer-valued f64 with a rounding rule.
pub fn round_f64(v: f64, rule: Rounding) -> f64 {
    match rule {
        Rounding::Down => v.floor(),
        Rounding::Up => v.ceil(),
        Rounding::TowardZero => v.trunc(),
        Rounding::AwayFromZero => {
            if v < 0.0 {
                v.floor()
            } else {
                v.ceil()
            }
        }
        Rounding::ToNearestOrDown => {
            let f = v.floor();
            if v - f > 0.5 {
                f + 1.0
            } else {
                f
            }
        }
        Rounding::ToNearestOrUp => {
            let f = v.floor();
            if v - f >= 0.5 {
                f + 1.0
            } else {
                f
            }
        }
        Rounding::ToNearestOrZero => {
            let t = v.trunc();
            if (v - t).abs() > 0.5 {
                t + v.signum()
            } else {
                t
            }
        }
        Rounding::ToNearestOrAway => v.round(),
        Rounding::ToNearestOrEven => {
            let r = v.round();
            if (v - v.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
                r - v.signum()
            } else {
                r
            }
        }
    }
}

/// Mixed multiply `self(26.6) × other(fracBits)` → 26.6, ties away from zero.
/// Swift `mixedMulRoundingToNearestOrAway`, for ≤32-bit operands.
#[inline]
pub fn mixed_mul_nearest_away(lhs: i32, rhs: i32, rhs_frac: u32) -> i32 {
    if rhs_frac == 0 && rhs == 1 {
        return lhs;
    }
    if rhs == (1i32 << rhs_frac) {
        return lhs;
    }
    let divisor: i64 = 1i64 << rhs_frac;
    let half: i64 = divisor >> 1;
    let product = (lhs as i64) * (rhs as i64);
    let truncated = product / divisor;
    let remainder = product.abs() % divisor;
    let mut rounded = truncated;
    if remainder >= half {
        rounded += if product >= 0 { 1 } else { -1 };
    }
    rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Mixed multiply with ties toward +∞. Swift `mixedMulRoundingToNearestOrUp`.
#[inline]
pub fn mixed_mul_nearest_up(lhs: i32, rhs: i32, rhs_frac: u32) -> i32 {
    if rhs == (1i32 << rhs_frac) {
        return lhs;
    }
    let divisor: i64 = 1i64 << rhs_frac;
    let half: i64 = divisor >> 1;
    let product = (lhs as i64) * (rhs as i64);
    let truncated = product / divisor;
    let remainder = product.abs() % divisor;
    let mut rounded = truncated;
    if remainder > half {
        rounded += if product >= 0 { 1 } else { -1 };
    } else if remainder == half && product >= 0 {
        rounded += 1;
    }
    rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

impl F26Dot6 {
    /// `self × F16Dot16` rounded to nearest-or-away.
    #[inline]
    pub fn mul_f16_away(self, s: F16Dot16) -> F26Dot6 {
        F26Dot6(mixed_mul_nearest_away(self.0, s.0, 16))
    }
    /// `self × F16Dot16` rounded to nearest-or-up.
    #[inline]
    pub fn mul_f16_up(self, s: F16Dot16) -> F26Dot6 {
        F26Dot6(mixed_mul_nearest_up(self.0, s.0, 16))
    }
    /// `self × F2Dot14` rounded to nearest-or-away.
    #[inline]
    pub fn mul_f2_away(self, s: F2Dot14) -> F26Dot6 {
        F26Dot6(mixed_mul_nearest_away(self.0, s.0 as i32, 14))
    }
    /// `self / F16Dot16` (Swift `F26Dot6.div(_:rounding:)`).
    #[inline]
    pub fn div_f16(self, s: F16Dot16, rule: Rounding) -> F26Dot6 {
        self.mul_div(1, s.0, rule)
    }
    /// Swift `F2Dot30(self)` style promotion: shift left by 24 (wrapping).
    #[inline]
    pub fn reinterpret_f2dot30(self) -> F2Dot30 {
        F2Dot30(self.0)
    }
}

impl F16Dot16 {
    /// Convert from 26.6 (shift left 10, wrapping as Swift's `init` would
    /// for in-range values).
    #[inline]
    pub fn from_f26(v: F26Dot6) -> F16Dot16 {
        F16Dot16(v.0.wrapping_shl(10))
    }
}

/// Convert between formats with rounding (Swift `init(_ other:, rounding:)`).
pub fn convert_bits(bits: i64, from_frac: u32, to_frac: u32, rule: Rounding) -> i64 {
    if from_frac >= to_frac {
        shift_right_i64(bits, from_frac - to_frac, rule)
    } else {
        bits.wrapping_shl(to_frac - from_frac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_div_rounds_like_swift() {
        assert_eq!(mul_div_i64(7, 1, 2, Rounding::TowardZero), Some(3));
        assert_eq!(mul_div_i64(-7, 1, 2, Rounding::TowardZero), Some(-3));
        assert_eq!(mul_div_i64(-7, 1, 2, Rounding::Down), Some(-4));
        assert_eq!(mul_div_i64(7, 1, 2, Rounding::ToNearestOrUp), Some(4));
        assert_eq!(mul_div_i64(-7, 1, 2, Rounding::ToNearestOrUp), Some(-3));
        assert_eq!(mul_div_i64(-7, 1, 2, Rounding::ToNearestOrAway), Some(-4));
        assert_eq!(mul_div_i64(1, 1, 0, Rounding::Down), None);
        assert_eq!(mul_div_i64(i64::MAX, 2, 1, Rounding::Down), None);
    }

    #[test]
    fn rounded_matches_rules() {
        let v = F26Dot6(-96); // -1.5
        assert_eq!(v.rounded(Rounding::ToNearestOrUp).0, -64);
        assert_eq!(v.rounded(Rounding::ToNearestOrAway).0, -128);
        assert_eq!(v.rounded(Rounding::TowardZero).0, -64);
        assert_eq!(v.rounded(Rounding::Down).0, -128);
        assert_eq!(F26Dot6(96).rounded(Rounding::ToNearestOrUp).0, 128);
        assert_eq!(F26Dot6::MAX.rounded(Rounding::Up).0, i32::MAX & !63);
    }

    #[test]
    fn mixed_mul() {
        // 1.5 * 0.5 (16.16) = 0.75
        assert_eq!(mixed_mul_nearest_away(96, 0x8000, 16), 48);
        // ties: 0.25 px * 0.5 = 0.125 px = 8 units exactly; use 1 unit * 0.5 = 0.5 → away: 1, up: 1, negative: away -1, up 0
        assert_eq!(mixed_mul_nearest_away(1, 0x8000, 16), 1);
        assert_eq!(mixed_mul_nearest_up(1, 0x8000, 16), 1);
        assert_eq!(mixed_mul_nearest_away(-1, 0x8000, 16), -1);
        assert_eq!(mixed_mul_nearest_up(-1, 0x8000, 16), 0);
    }

    #[test]
    fn shift_right_rounding() {
        assert_eq!(shift_right_i64(-3, 1, Rounding::Down), -2);
        assert_eq!(shift_right_i64(-3, 1, Rounding::TowardZero), -1);
        assert_eq!(shift_right_i64(-3, 1, Rounding::ToNearestOrUp), -1);
        assert_eq!(shift_right_i64(-3, 1, Rounding::ToNearestOrAway), -2);
        assert_eq!(shift_right_i64(5, 1, Rounding::ToNearestOrEven), 2);
        assert_eq!(shift_right_i64(7, 1, Rounding::ToNearestOrEven), 4);
    }
}
