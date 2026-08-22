//! this_file: typftth/src/interp/geometry.rs
//!
//! Shared geometry helpers: `move_point`, cut-ins, minimum distance,
//! unit vectors from lines, the ISECT "Fract" arithmetic and the lazy
//! composite outline correction.

use super::{Machine, Run};
use crate::error::InterpreterError;
use crate::fixed::{F16Dot16, F26Dot6, F2Dot30, Rounding};
use crate::gs::{compute_unit_vector, cvt_stretched, fixed_mul_wrapping_i32, Axis, GraphicsState, Vec2F2Dot14, ZoneType};
use crate::zone::Zone;

/// Swift `GraphicsState.movePoint`.
pub(crate) fn move_point(
    gs: &mut GraphicsState,
    zone: &mut Zone,
    index: usize,
    delta: F26Dot6,
    ignoring_overflow: bool,
) -> Result<(), InterpreterError> {
    let fv = gs.freedom();
    let mut move_x = fv.x.0 != 0;
    let mut move_y = fv.y.0 != 0;
    match gs.always_touch_axis {
        Some(Axis::X) => move_x = true,
        Some(Axis::Y) => move_y = true,
        None => {}
    }
    let projected = gs.vector_for(delta);
    let current = zone.hinted(index);
    let new_pos = if ignoring_overflow {
        current.wrapping_add(projected)
    } else {
        current.checked_add(projected).ok_or(InterpreterError::ArithmeticError)?
    };
    zone.set_hinted(index, new_pos);
    zone.mark_moved(index, move_x, move_y);
    Ok(())
}

/// Swift `clampToMinimumDistance`.
pub(crate) fn clamp_to_minimum_distance(minimum: F26Dot6, distance: F26Dot6, bias_negative: bool) -> F26Dot6 {
    let (lo, hi) = if minimum.0 >= 0 {
        (F26Dot6(0i32.wrapping_sub(minimum.0)), minimum)
    } else {
        (minimum, F26Dot6(0i32.wrapping_sub(minimum.0)))
    };
    if distance.0 >= lo.0 && distance.0 <= hi.0 {
        return if bias_negative { lo } else { hi };
    }
    distance
}

impl Machine {
    /// Swift `Substate.applySingleWidthCutIn`.
    pub(crate) fn apply_single_width_cut_in(&mut self, value: F26Dot6) -> F26Dot6 {
        if value.0 < 0 {
            F26Dot6(0i32.wrapping_sub(self.pos_at_least_single_width(F26Dot6(0i32.wrapping_sub(value.0))).0))
        } else {
            self.pos_at_least_single_width(value)
        }
    }

    fn pos_at_least_single_width(&mut self, value: F26Dot6) -> F26Dot6 {
        if self.single_width_cut_in.0 == 0 {
            return value;
        }
        let scale = self.effective_cvt_scale();
        let single_width = cvt_stretched(self.single_width_value, scale);
        let mut delta = value.wrapping_sub(single_width);
        if delta.0 < 0 {
            delta = F26Dot6(0i32.wrapping_sub(delta.0));
        }
        if delta.0 < self.single_width_cut_in.0 {
            single_width
        } else {
            value
        }
    }

    /// Swift `Substate.roundAndCutIn`.
    pub(crate) fn round_and_cut_in(&self, distance_to_move: F26Dot6, distance_between_points: F26Dot6) -> F26Dot6 {
        let mut d = distance_to_move;
        let mut cut = d.wrapping_sub(distance_between_points);
        if cut.0 < 0 {
            cut = F26Dot6(0i32.wrapping_sub(cut.0));
        }
        if cut.0 > self.cvt_cut_in.0 {
            d = distance_between_points;
        }
        self.gs.round_state.round(d)
    }

    /// Read a CVT entry scaled by the effective CVT scale.
    pub(crate) fn cvt_read_stretched(&mut self, index: i32) -> Result<F26Dot6, InterpreterError> {
        let raw = self.cvt_read(index)?;
        let scale = self.effective_cvt_scale();
        Ok(cvt_stretched(raw, scale))
    }

    pub(crate) fn cvt_read(&self, index: i32) -> Result<F26Dot6, InterpreterError> {
        if index < 0 {
            return Err(InterpreterError::CvtLocationOutOfBounds);
        }
        self.cvt.get(index as usize).map(|&v| F26Dot6(v)).ok_or(InterpreterError::CvtLocationOutOfBounds)
    }

    pub(crate) fn cvt_write(&mut self, index: i32, value: F26Dot6) -> Result<(), InterpreterError> {
        if index < 0 {
            return Err(InterpreterError::CvtLocationOutOfBounds);
        }
        match self.cvt.get_mut(index as usize) {
            Some(slot) => {
                *slot = value.0;
                Ok(())
            }
            None => Err(InterpreterError::CvtLocationOutOfBounds),
        }
    }
}

impl<'m, 'a, 'g> Run<'m, 'a, 'g> {
    /// SPVTL / SFVTL: unit vector from the line zp1[p1] → zp2[p2].
    pub(crate) fn unit_vector_from_line(&mut self, rotated: bool) -> Result<Vec2F2Dot14, InterpreterError> {
        let p2_index = self.pop()?;
        let p1_index = self.pop()?;
        let p2 = {
            let z = self.zone(self.m.gs.zp2)?;
            z.check_against_maxp(&self.m.maxp)?;
            z.hinted(z.check_point(p2_index)?)
        };
        // Bincompat: zone1 is not checked by SPVTL or SFVTL
        let p1 = {
            let z = self.zone(self.m.gs.zp1)?;
            z.hinted(z.check_point(p1_index)?)
        };
        Ok(compute_unit_vector(p1, p2, rotated))
    }

    /// Invert hinted → unscaled once for composites under variations
    /// (Swift `correctUnscaledOutline`).
    pub(crate) fn correct_unscaled_outline(&mut self) -> Result<(), InterpreterError> {
        self.unscaled_outline_is_wrong = false;
        let scale = self.m.scale.units_per_em_scale;
        let (zone, _) = self.zp(ZoneType::Glyph)?;
        for index in 0..zone.phantom_start() {
            let rescaled = zone.hinted(index).div_f16(scale, Rounding::ToNearestOrUp);
            zone.set_original(index, rescaled.x.0 as i16, rescaled.y.0 as i16);
        }
        Ok(())
    }
}

/// Swift `fractDivide`: F26.6 bit patterns treated as 2.30, saturating, toward zero.
pub(crate) fn fract_divide(dividend: F26Dot6, divisor: F26Dot6) -> F26Dot6 {
    let a = F2Dot30(dividend.0);
    let b = F2Dot30(divisor.0);
    F26Dot6(a.div_saturating(b, Rounding::TowardZero).0)
}

/// Swift `fractMultiply`: 2.30 multiply, nearest-or-up, wrapping.
pub(crate) fn fract_multiply(a: F26Dot6, b: F26Dot6) -> F26Dot6 {
    F26Dot6(fixed_mul_wrapping_i32(a.0, b.0, 30, Rounding::ToNearestOrUp))
}

/// Swift `F16Dot16 &* F16Dot16` (SSW): 16.16 multiply, nearest-or-up, wrapping.
pub(crate) fn f16_mul_wrapping(a: F16Dot16, b: F16Dot16) -> F16Dot16 {
    F16Dot16(fixed_mul_wrapping_i32(a.0, b.0, 16, Rounding::ToNearestOrUp))
}

