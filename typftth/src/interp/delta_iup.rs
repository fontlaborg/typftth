//! this_file: typftth/src/interp/delta_iup.rs
//!
//! DELTAP/DELTAC (with the reference's peculiar binary search) and IUP.

use super::geometry::move_point;
use super::Run;
use crate::error::InterpreterError;
use crate::fixed::{F16Dot16, F26Dot6, Rounding};
use crate::gs::{fixed_mul_saturating_i32, Axis, ZoneType};
use crate::exec::Program;
use crate::zone::Zone;

const DELTA_PHASE_STRIDE: i32 = 16;

/// Apply a delta exception list. `pairs` are (ppem|nibble, target) stack
/// words in push order; `apply(target, distance)` performs the move.
fn walk_deltas(
    pairs: &[i32],
    fake_ppem: i32,
    delta_shift: i16,
    mut apply: impl FnMut(i32, F26Dot6) -> Result<(), InterpreterError>,
) -> Result<(), InterpreterError> {
    if !(0..DELTA_PHASE_STRIDE).contains(&fake_ppem) {
        return Ok(()); // not within exception range
    }
    // The C++ binary search, reproduced (it is not a correct lower bound).
    let mut range_size = pairs.len();
    let mut start_at = 0usize;
    range_size >>= 1;
    range_size &= !1;
    while range_size > 2 {
        let ppem = pairs[start_at + range_size] >> 4;
        if ppem < fake_ppem {
            start_at += range_size;
        }
        range_size >>= 1;
        range_size &= !1;
    }
    let mut ppem_index = start_at;
    while ppem_index < pairs.len() {
        let ppem_shift = pairs[ppem_index];
        let ppem = ppem_shift >> 4;
        if ppem == fake_ppem {
            let mut modifier = ppem_shift & 0xf;
            modifier -= if modifier >= 8 { 7 } else { 8 };
            let distance = F26Dot6((modifier << 6).wrapping_shr((delta_shift as u32) & 31));
            let target = pairs.get(ppem_index + 1).copied().unwrap_or(0);
            apply(target, distance)?;
        } else if ppem > fake_ppem {
            break;
        }
        ppem_index += 2;
    }
    Ok(())
}

impl<'m, 'a, 'g> Run<'m, 'a, 'g> {
    fn pop_delta_pairs(&mut self) -> Result<Option<Vec<i32>>, InterpreterError> {
        let pair_count = self.pop()?;
        let elements = (pair_count as i64).wrapping_mul(2);
        if pair_count <= 0 || elements <= 0 {
            return Ok(None);
        }
        let elements = elements as usize;
        if self.m.stack.len() < elements {
            return Err(InterpreterError::StackUnderflow);
        }
        let at = self.m.stack.len() - elements;
        Ok(Some(self.m.stack.split_off(at)))
    }

    pub(crate) fn delta_cvt(&mut self, phase: i32) -> Result<(), InterpreterError> {
        let Some(pairs) = self.pop_delta_pairs()? else { return Ok(()) };
        let ppem = i32::from(self.m.scale.projected_integer_ppem(&self.m.gs));
        let fake = ppem - i32::from(self.m.gs.delta_base) - phase;
        let shift = self.m.gs.delta_shift;
        let m = &mut *self.m;
        walk_deltas(&pairs, fake, shift, |index, delta| {
            let v = match m.cvt_read(index) {
                Ok(v) => v,
                // FreeType non-pedantic DELTAC: out-of-range entries are skipped.
                Err(InterpreterError::CvtLocationOutOfBounds) if m.lenient_cvt => return Ok(()),
                Err(e) => return Err(e),
            };
            m.cvt_write(index, v.wrapping_add(delta))
        })
    }

    pub(crate) fn delta_move_point(&mut self, phase: i32) -> Result<(), InterpreterError> {
        let Some(pairs) = self.pop_delta_pairs()? else { return Ok(()) };
        let ppem = i32::from(self.m.scale.projected_integer_ppem(&self.m.gs));
        let fake = ppem - i32::from(self.m.gs.delta_base) - phase;
        let shift = self.m.gs.delta_shift;
        let zp0 = self.m.gs.zp0;
        // BINCOMPAT: with an invalid zone, fail only when actually modifying.
        let invalid = (zp0 == ZoneType::Glyph && self.exec.program != Program::Glyf)
            || (zp0 == ZoneType::Twilight && self.exec.program == Program::Fpgm);
        if invalid {
            return walk_deltas(&pairs, fake, shift, |_, _| {
                Err(if zp0 == ZoneType::Twilight {
                    InterpreterError::InvalidAccessToTwilightZone
                } else {
                    InterpreterError::InvalidAccessToGlyphZone
                })
            });
        }
        let (zone, m) = self.zp(zp0)?;
        let maxp = m.maxp;
        walk_deltas(&pairs, fake, shift, |index, delta| {
            zone.check_against_maxp(&maxp)?;
            let i = zone.check_point(index)?;
            move_point(&mut m.gs, zone, i, delta, false)
        })
    }

    pub(crate) fn iup(&mut self, axis: Axis) -> Result<(), InterpreterError> {
        let zp2 = self.m.gs.zp2;
        let (zone, _) = self.zp(zp2)?;
        iup_zone(zone, axis)
    }
}

#[inline]
fn incremented_wrapping(i: usize, start: usize, end: usize) -> usize {
    if i == end {
        start
    } else {
        i + 1
    }
}

/// Port of `Zone.iup(_:axis:)`.
fn iup_zone(zone: &mut Zone, axis: Axis) -> Result<(), InterpreterError> {
    let max_points = zone.max_point_count();
    let contour_count = zone.contour_count;
    for c in 0..contour_count {
        let (cs, ce) = zone.read_contour(c as i32)?;
        // BINCOMPAT: ep validated against Int16.max (reference used a local SInt16).
        if ce >= max_points || ce > i16::MAX as usize {
            return Err(InterpreterError::InvalidOperand);
        }
        let moved_bit = match axis {
            Axis::X => crate::zone::XMOVED,
            Axis::Y => crate::zone::YMOVED,
        };
        let is_moved = |z: &Zone, i: usize| z.f[i] & moved_bit != 0;
        let Some(first) = (cs..=ce).find(|&i| is_moved(zone, i)) else { continue };
        let finish = first;
        let mut start = first;
        loop {
            let mut end = start;
            loop {
                end = incremented_wrapping(end, cs, ce);
                if is_moved(zone, end) || start == end {
                    break;
                }
            }
            // UntouchedRangeIterator: nil when start+1 == end (nothing between)
            if incremented_wrapping(start, cs, ce) != end {
                let (hinted, scaled, original): (&mut Vec<i32>, &Vec<i32>, &Vec<i16>) = match axis {
                    Axis::X => (&mut zone.x, &zone.ox, &zone.oox),
                    Axis::Y => (&mut zone.y, &zone.oy, &zone.ooy),
                };
                let (low_pt, high_pt) = if original[start] < original[end] { (start, end) } else { (end, start) };
                let dx = hinted[low_pt];
                let dx1 = scaled[low_pt];
                let dx2 = i32::from(original[low_pt]);
                let high = scaled[high_pt];
                let dhigh = hinted[high_pt].wrapping_sub(high);
                let tmp32 = hinted[high_pt].wrapping_sub(dx);
                let tmp32b = i32::from(original[high_pt]).wrapping_sub(dx2);
                let low = dx1;
                let dlow = dx.wrapping_sub(dx1);
                let mut iter = start;
                if tmp32b != 0 {
                    if tmp32b <= i32::from(i16::MAX) && tmp32 <= i32::from(i16::MAX) {
                        loop {
                            iter = incremented_wrapping(iter, cs, ce);
                            if iter == end {
                                break;
                            }
                            let mut v = scaled[iter];
                            if v <= low {
                                v = v.wrapping_add(dlow);
                            } else if v >= high {
                                v = v.wrapping_add(dhigh);
                            } else {
                                v = i32::from(original[iter]).wrapping_sub(dx2);
                                v = v.wrapping_mul(tmp32);
                                v = v.wrapping_add(tmp32b >> 1);
                                v /= tmp32b;
                                v = v.wrapping_add(dx);
                            }
                            hinted[iter] = v;
                        }
                    } else {
                        loop {
                            iter = incremented_wrapping(iter, cs, ce);
                            if iter == end {
                                break;
                            }
                            let mut v = scaled[iter];
                            if v <= low {
                                v = v.wrapping_add(dlow);
                            } else if v >= high {
                                v = v.wrapping_add(dhigh);
                            } else {
                                // Bincompat: tmp32/tmp32B reinterpreted as F16.16 for the ratio.
                                let ratio = F16Dot16(tmp32).div_saturating(F16Dot16(tmp32b), Rounding::TowardZero);
                                let shifted = i32::from(original[iter]).wrapping_sub(dx2);
                                let applied = fixed_mul_saturating_i32(shifted, ratio.0, 16, Rounding::ToNearestOrUp);
                                v = applied.wrapping_add(dx);
                            }
                            hinted[iter] = v;
                        }
                    }
                } else if dlow != 0 {
                    loop {
                        iter = incremented_wrapping(iter, cs, ce);
                        if iter == end {
                            break;
                        }
                        hinted[iter] = hinted[iter].wrapping_add(dlow);
                    }
                }
            }
            start = end;
            if start == finish {
                break;
            }
        }
    }
    Ok(())
}
