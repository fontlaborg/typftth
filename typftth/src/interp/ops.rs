//! this_file: typftth/src/interp/ops.rs
//!
//! Opcode dispatch and implementations (port of the `impl_*` functions in
//! `Interpreter.swift`). Order follows the Swift file; bincompat notes are
//! kept verbatim where behaviour is deliberately odd.

use super::geometry::{
    clamp_to_minimum_distance, f16_mul_wrapping, fract_divide, fract_multiply, move_point,
};
use super::{Run, FAIR_DICE_ROLL};
use crate::error::InterpreterError;
use crate::exec::{Definition, Program, StreamType, FDEF_SAFE_MARGIN};
use crate::fixed::{F16Dot16, F26Dot6, F2Dot14, Rounding};
use crate::gs::{compute_unit_vector, fixed_mul_wrapping_i32, Axis, Coord, RoundState, Vec2F2Dot14, ZoneType};
use crate::opcodes as op;

impl<'m, 'a, 'g> Run<'m, 'a, 'g> {
    /// Execute one instruction.
    pub(crate) fn dispatch(&mut self, o: u8) -> Result<(), InterpreterError> {
        match o {
            op::SVTCA_Y => self.svtca(Axis::Y),
            op::SVTCA_X => self.svtca(Axis::X),
            op::SPVTCA_Y => {
                self.m.gs.set_projection(Vec2F2Dot14::Y_AXIS);
                Ok(())
            }
            op::SPVTCA_X => {
                self.m.gs.set_projection(Vec2F2Dot14::X_AXIS);
                Ok(())
            }
            op::SFVTCA_Y => {
                self.m.gs.set_freedom(Vec2F2Dot14::Y_AXIS);
                Ok(())
            }
            op::SFVTCA_X => {
                self.m.gs.set_freedom(Vec2F2Dot14::X_AXIS);
                Ok(())
            }
            0x06 | 0x07 => {
                let rotated = o == 0x07;
                let v = self.unit_vector_from_line(rotated)?;
                self.m.gs.set_projection(v);
                self.m.gs.projection_is_normal = rotated;
                Ok(())
            }
            0x08 | 0x09 => {
                let v = self.unit_vector_from_line(o == 0x09)?;
                self.m.gs.set_freedom(v);
                Ok(())
            }
            op::SPVFS => {
                let y = F2Dot14(self.pop()? as i16);
                let x = F2Dot14(self.pop()? as i16);
                self.m.gs.set_projection(Vec2F2Dot14 { x, y });
                Ok(())
            }
            op::SFVFS => {
                let y = F2Dot14(self.pop()? as i16);
                let x = F2Dot14(self.pop()? as i16);
                self.m.gs.set_freedom(Vec2F2Dot14 { x, y });
                Ok(())
            }
            op::GPV => {
                let p = self.m.gs.projection();
                self.push(p.x.0 as i32)?;
                self.push(p.y.0 as i32)
            }
            op::GFV => {
                let f = self.m.gs.freedom();
                self.push(f.x.0 as i32)?;
                self.push(f.y.0 as i32)
            }
            op::SFVTPV => {
                self.m.gs.set_freedom_to_projection();
                Ok(())
            }
            op::ISECT => self.isect(),
            op::SRP0 => {
                self.m.gs.rp0 = self.pop()?;
                Ok(())
            }
            op::SRP1 => {
                self.m.gs.rp1 = self.pop()?;
                Ok(())
            }
            op::SRP2 => {
                self.m.gs.rp2 = self.pop()?;
                Ok(())
            }
            op::SZP0 => {
                self.m.gs.zp0 = self.pop_zone_type()?;
                Ok(())
            }
            op::SZP1 => {
                self.m.gs.zp1 = self.pop_zone_type()?;
                Ok(())
            }
            op::SZP2 => {
                self.m.gs.zp2 = self.pop_zone_type()?;
                Ok(())
            }
            op::SZPS => {
                let z = self.pop_zone_type()?;
                self.m.gs.zp0 = z;
                self.m.gs.zp1 = z;
                self.m.gs.zp2 = z;
                Ok(())
            }
            op::SLOOP => {
                self.m.gs.loop_count = self.pop()?;
                Ok(())
            }
            op::RTG => {
                self.m.gs.round_state = RoundState::RTG;
                Ok(())
            }
            op::RTHG => {
                self.m.gs.round_state = RoundState::RTHG;
                Ok(())
            }
            op::SMD => {
                self.m.gs.minimum_distance = self.pop_f26()?;
                Ok(())
            }
            op::ELSE => self.exec.seek_after_conditional(false),
            op::JMPR => {
                let n = self.pop()?;
                self.exec.top.jump(n)
            }
            op::SCVTCI => {
                self.m.cvt_cut_in = self.pop_f26()?;
                Ok(())
            }
            op::SSWCI => {
                self.m.single_width_cut_in = self.pop_f26()?;
                Ok(())
            }
            op::SSW => {
                // BUG (bincompat): operand is the fractional part of an F16.16
                // (sign-extended at 16 bits), not integral FUnits.
                let raw = self.pop()?;
                let sw = F16Dot16((raw.wrapping_shl(16)) >> 16);
                let scaled = f16_mul_wrapping(sw, self.m.scale.units_per_em_scale.x);
                self.m.single_width_value = F26Dot6(scaled.0);
                Ok(())
            }
            op::DUP => {
                let v = self.peek(1)?;
                self.push(v)
            }
            op::POP => self.pop().map(|_| ()),
            op::CLEAR => {
                self.m.stack.clear();
                Ok(())
            }
            op::SWAP => {
                let n = self.m.stack.len();
                if n < 2 {
                    return Err(InterpreterError::StackUnderflow);
                }
                self.m.stack.swap(n - 2, n - 1);
                Ok(())
            }
            op::DEPTH => {
                let n = self.m.stack.len() as i32;
                self.push(n)
            }
            op::CINDEX => {
                let index = self.pop()? as u32;
                if index == 0 {
                    // FreeType rewrites the top of the stack with zero.
                    return self.push(0);
                }
                let v = self.peek(index)?;
                self.push(v)
            }
            op::MINDEX => {
                let index = self.pop()? as u32;
                match self.remove_at(index) {
                    Ok(v) => self.push(v),
                    Err(InterpreterError::StackUnderflow) => {
                        // Cpp interpreter and FreeType both return the original stack.
                        self.push(index as i32)?;
                        if index != 0 {
                            return Err(InterpreterError::StackUnderflow);
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            op::ALIGNPTS => self.alignpts(),
            op::UTP => {
                let index = self.pop()?;
                let fv = self.m.gs.freedom();
                let (zone, m) = self.zp(self.m.gs.zp0)?;
                zone.check_against_maxp(&m.maxp)?;
                let i = zone.check_point(index)?;
                zone.clear_moved(i, fv.x.0 != 0, fv.y.0 != 0);
                Ok(())
            }
            op::LOOPCALL => {
                let f = self.pop()?;
                if f < 0 || (f as i64) >= self.m.fdefs.capacity() as i64 + FDEF_SAFE_MARGIN {
                    return Err(InterpreterError::MaxpLimitExceeded);
                }
                let count = self.pop()? as i64;
                let def = self.m.fdefs.lookup(f);
                self.exec.call(StreamType::Function(f), def, count)
            }
            op::CALL => {
                let f = self.pop()?;
                if f < 0 {
                    return Err(InterpreterError::InvalidOperand);
                }
                if (f as i64) >= self.m.fdefs.capacity() as i64 + FDEF_SAFE_MARGIN {
                    return Err(InterpreterError::MaxpLimitExceeded);
                }
                let def = self.m.fdefs.lookup(f);
                self.exec.call(StreamType::Function(f), def, 1)
            }
            op::FDEF => {
                if self.exec.program == Program::Glyf {
                    return Err(InterpreterError::DefinitionsNotAllowedInGlyf);
                }
                let f = self.pop()?;
                let (start, end) = self.exec.consume_until_endf()?;
                self.m.fdefs.define(f, Definition { program: self.exec.program, start, end })
            }
            op::ENDF => Err(InterpreterError::UnexpectedEndf),
            0x2E | 0x2F => self.mdap(o == 0x2F),
            op::IUP_Y => self.iup(Axis::Y),
            op::IUP_X => self.iup(Axis::X),
            0x32 | 0x33 => self.shp(o == 0x33),
            0x34 | 0x35 => self.shc(o == 0x35),
            0x36 | 0x37 => self.shz(o == 0x37),
            op::SHPIX => self.shpix(),
            op::IP => self.ip(),
            0x3A | 0x3B => self.msirp(o == 0x3B),
            op::ALIGNRP => self.alignrp(),
            op::RTDG => {
                self.m.gs.round_state = RoundState::RTDG;
                Ok(())
            }
            0x3E | 0x3F => self.miap(o == 0x3F),
            op::NPUSHB => {
                let n = self.exec.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.pushb(n as usize)
            }
            op::NPUSHW => {
                let n = self.exec.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.pushw(n as usize)
            }
            op::WS => {
                let value = self.pop()? as u32;
                let location = self.pop()? as u32;
                match self.m.storage.get_mut(location as usize) {
                    Some(slot) => {
                        *slot = value;
                        Ok(())
                    }
                    None => Err(InterpreterError::StorageLocationOutOfBounds),
                }
            }
            op::RS => {
                let location = self.pop()? as u32;
                let v = *self.m.storage.get(location as usize).ok_or(InterpreterError::StorageLocationOutOfBounds)?;
                self.push(v as i32)
            }
            op::WCVTP => {
                let value = self.pop_f26()?;
                let location = self.pop()?;
                let scale = self.m.effective_cvt_scale();
                let doubly = F26Dot6(crate::fixed::mixed_mul_nearest_up(value.0, scale.0, 16));
                if value.0 == 0 || doubly.0 == 0 || doubly == value || self.exec.program == Program::Fpgm {
                    self.m.cvt_write(location, value)
                } else {
                    let rescaled = value.mul_div(value.0, doubly.0, Rounding::TowardZero);
                    self.m.cvt_write(location, rescaled)
                }
            }
            op::RCVT => {
                let index = self.pop()?;
                if self.exec.program == Program::Fpgm {
                    return self.push(0);
                }
                match self.m.cvt_read_stretched(index) {
                    Ok(v) => self.push_f26(v),
                    Err(InterpreterError::CvtLocationOutOfBounds) => {
                        if index < 0 {
                            return Err(InterpreterError::CvtLocationOutOfBounds);
                        }
                        self.push(0)
                    }
                    Err(e) => Err(e),
                }
            }
            0x46 | 0x47 => {
                let original = o == 0x47;
                let index = self.pop()?;
                let zone = self.zone(self.m.gs.zp2)?;
                let i = zone.check_point(index)?;
                let value = if original {
                    self.m.gs.dual_projection().dot(zone.scaled(i))
                } else {
                    self.m.gs.projection().dot(zone.hinted(i))
                };
                self.push_f26(value)
            }
            op::SCFS => {
                let coord = self.pop_f26()?;
                let pidx = self.pop()?;
                let zp2 = self.m.gs.zp2;
                let (zone, m) = self.zp(zp2)?;
                zone.check_against_maxp(&m.maxp)?;
                let i = zone.check_point(pidx)?;
                let cur = m.gs.projection().dot(zone.hinted(i));
                move_point(&mut m.gs, zone, i, coord.wrapping_sub(cur), false)?;
                if zp2 == ZoneType::Twilight {
                    let h = zone.hinted(i);
                    zone.set_scaled(i, h);
                }
                Ok(())
            }
            0x49 | 0x4A => self.md(o == 0x4A),
            op::MPPEM => {
                let ppem = self.m.scale.projected_integer_ppem(&self.m.gs);
                // `withoutExtending`: zero-extended 16-bit value
                self.push(i32::from(ppem as u16))
            }
            op::MPS => {
                let ps = self.m.scale.point_size;
                self.push(ps)
            }
            op::FLIPON => {
                self.m.gs.auto_flip = true;
                Ok(())
            }
            op::FLIPOFF => {
                self.m.gs.auto_flip = false;
                Ok(())
            }
            op::DEBUG => self.pop().map(|_| ()),
            op::LT => self.binop_i32(|b, a| (b < a) as i32),
            op::LTEQ => self.binop_i32(|b, a| (b <= a) as i32),
            op::GT => self.binop_i32(|b, a| (b > a) as i32),
            op::GTEQ => self.binop_i32(|b, a| (b >= a) as i32),
            op::EQ => self.binop_i32(|b, a| (b == a) as i32),
            op::NEQ => self.binop_i32(|b, a| (b != a) as i32),
            op::ODD => {
                let n = self.pop_f26()?;
                let r = RoundState::RTG.round(n).0 >> 6;
                self.push(if r % 2 == 0 { 0 } else { 1 })
            }
            op::EVEN => {
                let n = self.pop_f26()?;
                let r = RoundState::RTG.round(n).0 >> 6;
                self.push(if r % 2 == 0 { 1 } else { 0 })
            }
            op::IF => {
                let cond = self.pop()? != 0;
                if !cond {
                    self.exec.seek_after_conditional(true)?;
                }
                Ok(())
            }
            op::EIF => Ok(()),
            op::AND => self.binop_i32(|b, a| (b != 0 && a != 0) as i32),
            op::OR => self.binop_i32(|b, a| (b != 0 || a != 0) as i32),
            op::NOT => {
                let v = self.pop()?;
                self.push(if v == 0 { 1 } else { 0 })
            }
            op::DELTAP1 => self.delta_move_point(0),
            op::DELTAP2 => self.delta_move_point(16),
            op::DELTAP3 => self.delta_move_point(32),
            op::SDB => {
                self.m.gs.delta_base = self.pop()? as i16;
                Ok(())
            }
            op::SDS => {
                self.m.gs.delta_shift = self.pop()? as i16;
                Ok(())
            }
            op::ADD => self.binop_f26(|b, a| b.wrapping_add(a)),
            op::SUB => self.binop_f26(|b, a| b.wrapping_sub(a)),
            op::DIV => self.binop_f26(|num, denom| {
                if denom.0 == 0 {
                    if num.0 < 0 {
                        F26Dot6::MIN
                    } else {
                        F26Dot6::MAX
                    }
                } else {
                    num.div_saturating(denom, Rounding::TowardZero)
                }
            }),
            op::MUL => self.binop_f26(|a, b| {
                const LIMIT: i32 = 46_340;
                let fast = (-LIMIT..=LIMIT).contains(&a.0) && (-LIMIT..=LIMIT).contains(&b.0);
                let rule = if fast { Rounding::ToNearestOrUp } else { Rounding::ToNearestOrAway };
                F26Dot6(fixed_mul_wrapping_i32(a.0, b.0, 6, rule))
            }),
            op::ABS => self.unop_f26(|v| if v == F26Dot6::MIN { v } else { F26Dot6(v.0.abs()) }),
            op::NEG => self.unop_f26(|v| if v == F26Dot6::MIN { v } else { F26Dot6(-v.0) }),
            op::FLOOR => self.unop_f26(|v| v.rounded(Rounding::Down)),
            op::CEILING => self.unop_f26(|v| v.rounded(Rounding::Up)),
            0x68..=0x6B => {
                let a = self.pop_f26()?;
                let r = self.m.gs.round_state.round(a);
                self.push_f26(r)
            }
            0x6C..=0x6F => {
                if self.m.stack.is_empty() {
                    return Err(InterpreterError::StackUnderflow);
                }
                Ok(())
            }
            op::WCVTF => {
                let value = self.pop_f26()?;
                let location = self.pop()?;
                let scaled = value.mul_f16_up(self.m.scale.units_per_em_scale.x);
                self.m.cvt_write(location, scaled)
            }
            op::DELTAC1 => self.delta_cvt(0),
            op::DELTAC2 => self.delta_cvt(16),
            op::DELTAC3 => self.delta_cvt(32),
            op::SROUND => {
                let p = self.pop()? as u8;
                self.m.gs.round_state = RoundState::super_round(p);
                Ok(())
            }
            op::S45ROUND => {
                let p = self.pop()? as u8;
                self.m.gs.round_state = RoundState::super45_round(p);
                Ok(())
            }
            op::JROT => {
                let val = self.pop()?;
                let n = self.pop()?;
                if val != 0 {
                    self.exec.top.jump(n)?;
                }
                Ok(())
            }
            op::JROF => {
                let val = self.pop()?;
                let n = self.pop()?;
                if val == 0 {
                    self.exec.top.jump(n)?;
                }
                Ok(())
            }
            op::ROFF => {
                self.m.gs.round_state = RoundState::ROFF;
                Ok(())
            }
            op::RESERVED_7B => Err(InterpreterError::IllegalInstruction),
            op::RUTG => {
                self.m.gs.round_state = RoundState::RUTG;
                Ok(())
            }
            op::RDTG => {
                self.m.gs.round_state = RoundState::RDTG;
                Ok(())
            }
            op::SANGW | op::AA => self.pop().map(|_| ()),
            op::FLIPPT => {
                let loops = self.m.gs.loop_count.max(0);
                let (zone, m) = self.zp(m_zp0(self))?;
                zone.check_against_maxp(&m.maxp)?;
                for _ in 0..loops {
                    let index = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
                    let i = zone.check_point(index)?;
                    zone.toggle_on_curve(i);
                }
                m.gs.loop_count = 1;
                Ok(())
            }
            op::FLIPRGON => self.fliprg(true),
            op::FLIPRGOFF => self.fliprg(false),
            op::SCANCTRL => {
                let v = self.pop()? as u32;
                // bincompat: the stack value is not masked off properly
                self.m.gs.scan_control.0 = (self.m.gs.scan_control.0 & !0xffff) | v;
                Ok(())
            }
            0x86 | 0x87 => self.sdpvtl(o == 0x87),
            op::GETINFO => self.getinfo(),
            op::IDEF => {
                let opcode = self.pop()? as u8;
                if self.exec.program == Program::Glyf {
                    return Err(InterpreterError::DefinitionsNotAllowedInGlyf);
                }
                let (start, end) = self.exec.consume_until_endf()?;
                self.m.idefs.define(opcode, Definition { program: self.exec.program, start, end })
            }
            op::ROLL => {
                let n = self.m.stack.len();
                if n < 3 {
                    return Err(InterpreterError::StackUnderflow);
                }
                self.m.stack.swap(n - 3, n - 2);
                self.m.stack.swap(n - 2, n - 1);
                Ok(())
            }
            op::MAX => self.binop_i32(|b, a| b.max(a)),
            op::MIN => self.binop_i32(|b, a| b.min(a)),
            op::SCANTYPE => {
                let v = self.pop()?;
                match v {
                    0 => self.m.gs.scan_control.set_kind(0),
                    1 => self.m.gs.scan_control.set_kind(1),
                    2 => self.m.gs.scan_control.set_kind(2),
                    _ => {}
                }
                Ok(())
            }
            op::INSTCTRL => {
                let selector = self.pop()?;
                let value = self.pop()? as u8;
                if self.exec.program != Program::Prep {
                    return Ok(());
                }
                let ic = &mut self.m.gs.instruct_control;
                match selector {
                    1 => ic.0 = (ic.0 & !1) | (value & 1),
                    2 => ic.0 = (ic.0 & !2) | (value & 2),
                    _ => {}
                }
                Ok(())
            }
            op::GETVARIATION => {
                if !self.m.coords.is_empty() {
                    let coords: Vec<F2Dot14> = self.m.coords.clone();
                    for c in coords {
                        self.push(i32::from(c.0))?;
                    }
                    Ok(())
                } else {
                    self.instruction_patch(o)
                }
            }
            op::GETDATA => {
                let mut success = true;
                match self.pop()? {
                    1 => {
                        let n = self.pop()? as u32;
                        if n == 0 {
                            return Err(InterpreterError::InvalidOperand);
                        }
                        self.push((FAIR_DICE_ROLL % n) as i32)?;
                    }
                    _ => success = false,
                }
                self.push_bool(success)
            }
            op::PUSHB => {
                let b = self.exec.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.push(i32::from(b))
            }
            0xB1..=0xB7 => self.pushb((o - op::PUSHB) as usize + 1),
            op::PUSHW => {
                let hi = self.exec.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                let lo = self.exec.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.push(i32::from(i16::from_be_bytes([hi, lo])))
            }
            0xB9..=0xBF => self.pushw((o - op::PUSHW) as usize + 1),
            0xC0..=0xDF => self.mdrp(o - op::MDRP),
            0xE0..=0xFF => self.mirp(o - op::MIRP),
            _ => self.instruction_patch(o),
        }
    }

    /* ---------------------------------------------------------- helpers */

    fn instruction_patch(&mut self, o: u8) -> Result<(), InterpreterError> {
        let def = self.m.idefs.lookup(o);
        if def.is_undefined() {
            return Err(InterpreterError::IllegalInstruction);
        }
        self.exec.call(StreamType::Patch(o), def, 1)
    }

    fn binop_i32(&mut self, f: impl FnOnce(i32, i32) -> i32) -> Result<(), InterpreterError> {
        let a = self.pop()?;
        let b = self.pop()?;
        self.push(f(b, a))
    }
    fn binop_f26(&mut self, f: impl FnOnce(F26Dot6, F26Dot6) -> F26Dot6) -> Result<(), InterpreterError> {
        let a = self.pop_f26()?;
        let b = self.pop_f26()?;
        self.push_f26(f(b, a))
    }
    fn unop_f26(&mut self, f: impl FnOnce(F26Dot6) -> F26Dot6) -> Result<(), InterpreterError> {
        let a = self.pop_f26()?;
        self.push_f26(f(a))
    }

    fn pushb(&mut self, n: usize) -> Result<(), InterpreterError> {
        let bytes = self.exec.next_bytes(n)?;
        if self.m.stack.len() + bytes.len() > super::MAX_STACK {
            return Err(InterpreterError::StackDepthExceedsLimit);
        }
        self.m.stack.extend(bytes.iter().map(|&b| i32::from(b)));
        Ok(())
    }
    fn pushw(&mut self, n: usize) -> Result<(), InterpreterError> {
        let byte_count = n.checked_mul(2).ok_or(InterpreterError::RanOffEndOfInstructions)?;
        let bytes = self.exec.next_bytes(byte_count)?;
        if self.m.stack.len() + n > super::MAX_STACK {
            return Err(InterpreterError::StackDepthExceedsLimit);
        }
        for pair in bytes.chunks_exact(2) {
            self.m.stack.push(i32::from(i16::from_be_bytes([pair[0], pair[1]])));
        }
        Ok(())
    }

    fn svtca(&mut self, axis: Axis) -> Result<(), InterpreterError> {
        let v = match axis {
            Axis::X => Vec2F2Dot14::X_AXIS,
            Axis::Y => Vec2F2Dot14::Y_AXIS,
        };
        self.m.gs.set_freedom(v);
        self.m.gs.set_projection(v);
        self.m.gs.always_touch_axis = Some(axis);
        Ok(())
    }

    /* ---------------------------------------------------------- geometry opcodes */

    fn alignpts(&mut self) -> Result<(), InterpreterError> {
        let p2_index = self.pop()?;
        let p1_index = self.pop()?;
        let p2 = {
            let z = self.zone(self.m.gs.zp0)?;
            z.hinted(z.check_point(p2_index)?)
        };
        let p1 = {
            let z = self.zone(self.m.gs.zp1)?;
            z.hinted(z.check_point(p1_index)?)
        };
        let dist = self.m.gs.projection().dot(p2.wrapping_sub(p1));
        let mv = dist.div_saturating(F26Dot6::from_int(2), Rounding::Down);
        {
            let zp1 = self.m.gs.zp1;
            let (zone, m) = self.zp(zp1)?;
            zone.check_against_maxp(&m.maxp)?;
            let i = zone.check_point(p1_index)?;
            move_point(&mut m.gs, zone, i, mv, false)?;
        }
        {
            let zp0 = self.m.gs.zp0;
            let (zone, m) = self.zp(zp0)?;
            zone.check_against_maxp(&m.maxp)?;
            let i = zone.check_point(p2_index)?;
            move_point(&mut m.gs, zone, i, mv.wrapping_sub(dist), false)?;
        }
        Ok(())
    }

    fn alignrp(&mut self) -> Result<(), InterpreterError> {
        let rp0 = self.m.gs.rp0;
        let p0 = {
            let z = self.zone(self.m.gs.zp0)?;
            z.hinted(z.check_point(rp0)?)
        };
        let zp1 = self.m.gs.zp1;
        let (zone, m) = self.zp(zp1)?;
        zone.check_against_maxp(&m.maxp)?;
        let proj_vec = m.gs.projection();
        while m.gs.loop_count > 0 {
            let index = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
            let i = zone.check_point(index)?;
            let proj = F26Dot6(0i32.wrapping_sub(proj_vec.dot(zone.hinted(i).wrapping_sub(p0)).0));
            move_point(&mut m.gs, zone, i, proj, false)?;
            m.gs.loop_count -= 1;
        }
        m.gs.loop_count = 1;
        Ok(())
    }

    fn mdap(&mut self, round: bool) -> Result<(), InterpreterError> {
        let idx = self.pop()?;
        let zp0 = self.m.gs.zp0;
        let (zone, m) = self.zp(zp0)?;
        zone.check_against_maxp(&m.maxp)?;
        let i = zone.check_point(idx)?;
        m.gs.rp0 = idx;
        m.gs.rp1 = idx;
        let distance = if round {
            let cur = m.gs.projection().dot(zone.hinted(i));
            m.gs.round_state.round(cur).wrapping_sub(cur)
        } else {
            F26Dot6::ZERO
        };
        // BUG (bincompat): MDAP relies on movePoint for the touch flags.
        move_point(&mut m.gs, zone, i, distance, false)
    }

    fn mdrp(&mut self, imm: u8) -> Result<(), InterpreterError> {
        let uses_twilight = self.m.gs.zp0 == ZoneType::Twilight || self.m.gs.zp1 == ZoneType::Twilight;
        if !uses_twilight && self.unscaled_outline_is_wrong {
            self.correct_unscaled_outline()?;
        }
        let pt0_index = self.m.gs.rp0;
        let pt1_index = self.pop()?;
        {
            let z = self.zone(self.m.gs.zp0)?;
            z.check_against_maxp(&self.m.maxp)?;
        }
        let (pt0_hinted, pt0_ref) = {
            let z = self.zone(self.m.gs.zp0)?;
            let i = z.check_point(pt0_index)?;
            let reference = if uses_twilight {
                z.scaled(i)
            } else {
                let (ox, oy) = z.original_at(i);
                Coord::from_unscaled_bits(ox, oy)
            };
            (z.hinted(i), reference)
        };
        let zp1 = self.m.gs.zp1;
        let (zone, m) = self.zp(zp1)?;
        zone.check_against_maxp(&m.maxp)?;
        let i = zone.check_point(pt1_index)?;
        let mut distance = if uses_twilight {
            m.gs.dual_projection().dot(zone.scaled(i).wrapping_sub(pt0_ref))
        } else {
            // Bincompat: unscaled points read as F16.16 (65536× too small).
            let (ox, oy) = zone.original_at(i);
            let stretched = Coord::from_unscaled_bits(ox, oy).wrapping_sub(pt0_ref);
            let s = m.scale.units_per_em_scale;
            if s.x != s.y {
                m.gs.dual_projection().dot(stretched.mul_f16_up(s))
            } else {
                m.gs.dual_projection().dot(stretched).mul_f16_up(s.x)
            }
        };
        distance = m.apply_single_width_cut_in(distance);
        let negative_before = distance.0 < 0;
        if imm & 0b100 != 0 {
            distance = m.gs.round_state.round(distance);
        }
        let min_dist = if imm & 0b1000 != 0 { m.gs.minimum_distance } else { F26Dot6::ZERO };
        distance = clamp_to_minimum_distance(min_dist, distance, negative_before);
        distance = distance.wrapping_sub(m.gs.projection().dot(zone.hinted(i).wrapping_sub(pt0_hinted)));
        move_point(&mut m.gs, zone, i, distance, false)?;
        m.gs.rp1 = pt0_index;
        m.gs.rp2 = pt1_index;
        if imm & 0b10000 != 0 {
            m.gs.rp0 = pt1_index;
        }
        Ok(())
    }

    fn miap(&mut self, round: bool) -> Result<(), InterpreterError> {
        let entry = self.pop()?;
        let point_index = self.pop()?;
        let mut new_proj = self.m.cvt_read_stretched(entry)?;
        let zp0 = self.m.gs.zp0;
        let (zone, m) = self.zp(zp0)?;
        zone.check_against_maxp(&m.maxp)?;
        let i = zone.check_point(point_index)?;
        let current = if zp0 == ZoneType::Twilight {
            let updated = Coord::scaling(new_proj, m.gs.projection());
            zone.set_hinted(i, updated);
            zone.set_scaled(i, updated);
            m.gs.projection().dot(updated)
        } else {
            m.gs.projection().dot(zone.hinted(i))
        };
        m.gs.rp0 = point_index;
        m.gs.rp1 = point_index;
        if round {
            new_proj = m.round_and_cut_in(new_proj, current);
        }
        move_point(&mut m.gs, zone, i, new_proj.wrapping_sub(current), false)
    }

    fn mirp(&mut self, imm: u8) -> Result<(), InterpreterError> {
        let entry = self.pop()?;
        let mut distance = self.m.cvt_read_stretched(entry)?;
        let point_index = self.pop()?;
        let rp0 = self.m.gs.rp0;
        let (rp_scaled, rp_hinted) = {
            let z = self.zone(self.m.gs.zp0)?;
            let i = z.check_point(rp0)?;
            (z.scaled(i), z.hinted(i))
        };
        distance = self.m.apply_single_width_cut_in(distance);
        let zp1 = self.m.gs.zp1;
        let (zone, m) = self.zp(zp1)?;
        zone.check_against_maxp(&m.maxp)?;
        let i = zone.check_point(point_index)?;
        let between = if zp1 == ZoneType::Twilight {
            let d = Coord::scaling(distance, m.gs.projection());
            zone.set_scaled(i, rp_scaled.wrapping_add(d));
            zone.set_hinted(i, rp_hinted);
            distance
        } else {
            m.gs.dual_projection().dot(zone.scaled(i).wrapping_sub(rp_scaled))
        };
        if m.gs.auto_flip && (distance.0 < 0) != (between.0 < 0) {
            distance = F26Dot6(0i32.wrapping_sub(distance.0));
        }
        if imm & 0b100 != 0 {
            distance = m.round_and_cut_in(distance, between);
        }
        let min_dist = if imm & 0b1000 != 0 { m.gs.minimum_distance } else { F26Dot6::ZERO };
        distance = clamp_to_minimum_distance(min_dist, distance, between.0 < 0);
        distance = distance.wrapping_sub(m.gs.projection().dot(zone.hinted(i).wrapping_sub(rp_hinted)));
        move_point(&mut m.gs, zone, i, distance, false)?;
        m.gs.rp1 = rp0;
        m.gs.rp2 = point_index;
        if imm & 0b10000 != 0 {
            m.gs.rp0 = point_index;
        }
        Ok(())
    }

    fn msirp(&mut self, set_rp0: bool) -> Result<(), InterpreterError> {
        let distance = self.pop_f26()?;
        let point_to_modify = self.pop()?;
        let rp0 = self.m.gs.rp0;
        let is_twilight = self.m.gs.zp1 == ZoneType::Twilight;
        let (ref_hinted, ref_scaled) = {
            let z = self.zone(self.m.gs.zp0)?;
            let i = z.check_point(rp0)?;
            (z.hinted(i), if is_twilight { z.scaled(i) } else { Coord::ZERO })
        };
        let zp1 = self.m.gs.zp1;
        let (zone, m) = self.zp(zp1)?;
        zone.check_against_maxp(&m.maxp)?;
        let i = zone.check_point(point_to_modify)?;
        if is_twilight {
            let d = Coord::scaling(distance, m.gs.projection());
            zone.set_scaled(i, ref_scaled.wrapping_add(d));
            zone.set_hinted(i, ref_hinted);
        }
        let between = m.gs.projection().dot(zone.hinted(i).wrapping_sub(ref_hinted));
        move_point(&mut m.gs, zone, i, distance.wrapping_sub(between), false)?;
        m.gs.rp1 = rp0;
        m.gs.rp2 = point_to_modify;
        if set_rp0 {
            m.gs.rp0 = point_to_modify;
        }
        Ok(())
    }

    fn md(&mut self, original: bool) -> Result<(), InterpreterError> {
        let p2_index = self.pop()?;
        let p1_index = self.pop()?;
        if !original {
            let p1 = {
                let z = self.zone(self.m.gs.zp0)?;
                z.hinted(z.check_point(p1_index)?)
            };
            let p2 = {
                let z = self.zone(self.m.gs.zp1)?;
                z.hinted(z.check_point(p2_index)?)
            };
            let d = self.m.gs.projection().dot(p1.wrapping_sub(p2));
            self.push_f26(d)
        } else {
            if self.exec.program == Program::Glyf && self.unscaled_outline_is_wrong {
                self.correct_unscaled_outline()?;
            }
            let (o1x, o1y) = {
                let z = self.zone(self.m.gs.zp0)?;
                z.original_at(z.check_point(p1_index)?)
            };
            let (o2x, o2y) = {
                let z = self.zone(self.m.gs.zp1)?;
                z.original_at(z.check_point(p2_index)?)
            };
            // Bincompat: unscaled int16 points with F26.6 arithmetic.
            let distance = Coord::from_unscaled_bits(o1x.wrapping_sub(o2x), o1y.wrapping_sub(o2y))
                .mul_f16_up(self.m.scale.units_per_em_scale);
            let projected = self.m.gs.dual_projection().dot(distance);
            self.push_f26(projected)
        }
    }

    fn ip(&mut self) -> Result<(), InterpreterError> {
        if self.m.gs.loop_count <= 0 {
            self.m.gs.loop_count = 1;
            return Ok(());
        }
        let gs = &self.m.gs;
        let uses_twilight = gs.zp0 == ZoneType::Twilight || gs.zp1 == ZoneType::Twilight || gs.zp2 == ZoneType::Twilight;
        if !uses_twilight && self.unscaled_outline_is_wrong {
            self.correct_unscaled_outline()?;
        }
        let rp1 = self.m.gs.rp1;
        let rp2 = self.m.gs.rp2;
        let (r1h, r1s, r1o) = {
            let z = self.zone(self.m.gs.zp0)?;
            let i = z.check_point(rp1)?;
            (z.hinted(i), z.scaled(i), z.original_at(i))
        };
        let (r2h, r2s, r2o) = {
            let z = self.zone(self.m.gs.zp1)?;
            let i = z.check_point(rp2)?;
            (z.hinted(i), z.scaled(i), z.original_at(i))
        };
        let current_range = self.m.gs.projection().dot(r2h.wrapping_sub(r1h));
        let (old_range, dual_ref) = if uses_twilight {
            (self.m.gs.projection().dot(r2s.wrapping_sub(r1s)), r1s)
        } else {
            let u1 = Coord::from_unscaled_bits(r1o.0, r1o.1);
            let u2 = Coord::from_unscaled_bits(r2o.0, r2o.1);
            (self.m.gs.dual_projection().dot(u2.wrapping_sub(u1)), u1)
        };
        let zp2 = self.m.gs.zp2;
        let (zone, m) = self.zp(zp2)?;
        zone.check_against_maxp(&m.maxp)?;
        let proj_vec = m.gs.projection();
        let dual_vec = m.gs.dual_projection();
        let loops = m.gs.loop_count;
        for _ in 0..loops {
            let idx = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
            let i = zone.check_point(idx)?;
            let mut desired = if uses_twilight {
                dual_vec.dot(zone.scaled(i).wrapping_sub(dual_ref))
            } else {
                let (ox, oy) = zone.original_at(i);
                dual_vec.dot(Coord::from_unscaled_bits(ox, oy).wrapping_sub(dual_ref))
            };
            if old_range.0 != 0 {
                // bincompat: double-cast emulation via truncating mulDiv
                desired = desired.mul_div(current_range.0, old_range.0, Rounding::TowardZero);
            }
            let current = proj_vec.dot(zone.hinted(i).wrapping_sub(r1h));
            move_point(&mut m.gs, zone, i, desired.wrapping_sub(current), false)?;
        }
        m.gs.loop_count = 1;
        Ok(())
    }

    fn isect(&mut self) -> Result<(), InterpreterError> {
        let a0i = self.pop()?;
        let a1i = self.pop()?;
        let b0i = self.pop()?;
        let b1i = self.pop()?;
        let (a0, a1) = {
            let z = self.zone(self.m.gs.zp0)?;
            (z.hinted(z.check_point(a0i)?), z.hinted(z.check_point(a1i)?))
        };
        let (b0, b1) = {
            let z = self.zone(self.m.gs.zp1)?;
            (z.hinted(z.check_point(b0i)?), z.hinted(z.check_point(b1i)?))
        };
        let zp2 = self.m.gs.zp2;
        let (zone, m) = self.zp(zp2)?;
        let idx = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
        let i = zone.check_point(idx)?;
        zone.mark_moved(i, true, true);
        let d_ax = a0.x.wrapping_sub(a1.x);
        let d_ay = a0.y.wrapping_sub(a1.y);
        let d_bx = b0.x.wrapping_sub(b1.x);
        let d_by = b0.y.wrapping_sub(b1.y);
        let (n, d): (F26Dot6, F26Dot6);
        if d_ay.0 == 0 {
            if d_bx.0 == 0 {
                zone.set_hinted(i, Coord { x: b1.x, y: a1.y });
                return Ok(());
            }
            n = b1.y.wrapping_sub(a1.y);
            d = F26Dot6(0i32.wrapping_sub(d_by.0));
        } else if d_ax.0 == 0 {
            if d_by.0 == 0 {
                zone.set_hinted(i, Coord { x: a1.x, y: b1.y });
                return Ok(());
            }
            n = b1.x.wrapping_sub(a1.x);
            d = F26Dot6(0i32.wrapping_sub(d_bx.0));
        } else if d_ax.0.unsigned_abs() > d_ay.0.unsigned_abs() {
            let t = fract_divide(d_ay, d_ax);
            n = b1.y.wrapping_sub(a1.y).wrapping_sub(fract_multiply(t, b1.x.wrapping_sub(a1.x)));
            d = fract_multiply(d_bx, t).wrapping_sub(d_by);
        } else {
            let t = fract_divide(d_ax, d_ay);
            n = fract_multiply(b1.y.wrapping_sub(a1.y), t).wrapping_sub(b1.x.wrapping_sub(a1.x));
            d = d_bx.wrapping_sub(fract_multiply(d_by, t));
        }
        if d.0 != 0 {
            if n.0.unsigned_abs() < d.0.unsigned_abs() {
                let t = fract_divide(n, d);
                zone.set_hinted(
                    i,
                    Coord { x: b1.x.wrapping_add(fract_multiply(d_bx, t)), y: b1.y.wrapping_add(fract_multiply(d_by, t)) },
                );
            } else {
                let t = fract_divide(d, n);
                zone.set_hinted(
                    i,
                    Coord { x: b1.x.wrapping_add(fract_divide(d_bx, t)), y: b1.y.wrapping_add(fract_divide(d_by, t)) },
                );
            }
        } else {
            zone.set_hinted(
                i,
                Coord {
                    x: F26Dot6((b1.x.0.wrapping_add(d_bx.0 / 2).wrapping_add(a1.x.0).wrapping_add(d_ax.0 / 2)) / 2),
                    y: F26Dot6((b1.y.0.wrapping_add(d_by.0 / 2).wrapping_add(a1.y.0).wrapping_add(d_ay.0 / 2)) / 2),
                },
            );
        }
        Ok(())
    }

    fn shp(&mut self, use_rp1: bool) -> Result<(), InterpreterError> {
        let (ref_point, ref_zone) = if use_rp1 { (self.m.gs.rp1, self.m.gs.zp0) } else { (self.m.gs.rp2, self.m.gs.zp1) };
        let proj = {
            let z = self.zone(ref_zone)?;
            let i = z.check_point(ref_point)?;
            self.m.gs.projection().dot(z.hinted(i).wrapping_sub(z.scaled(i)))
        };
        let zp2 = self.m.gs.zp2;
        let (zone, m) = self.zp(zp2)?;
        let loops = m.gs.loop_count.max(0);
        for _ in 0..loops {
            let index = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
            let i = zone.check_point(index)?;
            move_point(&mut m.gs, zone, i, proj, true)?;
        }
        m.gs.loop_count = 1;
        Ok(())
    }

    fn shc(&mut self, use_rp1: bool) -> Result<(), InterpreterError> {
        let (ref_point, ref_zone) = if use_rp1 { (self.m.gs.rp1, self.m.gs.zp0) } else { (self.m.gs.rp2, self.m.gs.zp1) };
        let proj = {
            let z = self.zone(ref_zone)?;
            let i = z.check_point(ref_point)?;
            self.m.gs.projection().dot(z.hinted(i).wrapping_sub(z.scaled(i)))
        };
        let contour_index = self.pop()?;
        let zp2 = self.m.gs.zp2;
        let (zone, m) = self.zp(zp2)?;
        zone.check_against_maxp(&m.maxp)?;
        let (start, end) = zone.read_contour(contour_index)?;
        if end >= zone.max_point_count() {
            return Err(InterpreterError::InvalidOperand);
        }
        for index in start..=end {
            if index as i32 == ref_point && zone.zone_type == ref_zone {
                continue;
            }
            move_point(&mut m.gs, zone, index, proj, true)?;
        }
        Ok(())
    }

    fn shpix(&mut self) -> Result<(), InterpreterError> {
        let proj = self.pop_f26()?;
        let fv = self.m.gs.freedom();
        let delta = Coord::scaling(proj, fv);
        let x_moved = fv.x.0 != 0;
        let y_moved = fv.y.0 != 0;
        let zp2 = self.m.gs.zp2;
        let (zone, m) = self.zp(zp2)?;
        let loops = m.gs.loop_count.max(0);
        for _ in 0..loops {
            let index = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
            let i = zone.check_point(index)?;
            let h = zone.hinted(i).wrapping_add(delta);
            zone.set_hinted(i, h);
            zone.mark_moved(i, x_moved, y_moved);
        }
        m.gs.loop_count = 1;
        Ok(())
    }

    fn shz(&mut self, use_rp1: bool) -> Result<(), InterpreterError> {
        let (ref_point, ref_zone) = if use_rp1 { (self.m.gs.rp1, self.m.gs.zp0) } else { (self.m.gs.rp2, self.m.gs.zp1) };
        let delta = {
            let z = self.zone(ref_zone)?;
            let i = z.check_point(ref_point)?;
            self.m.gs.projection().dot(z.hinted(i).wrapping_sub(z.scaled(i)))
        };
        let target = self.pop_zone_type()?;
        let (zone, m) = self.zp(target)?;
        // UNDOCUMENTED: points below the first contour's start are not touched.
        let (mut start_index, _) = zone.read_contour(0)?;
        if start_index >= zone.phantom_start() {
            return Err(InterpreterError::InvalidOperand);
        }
        // BUG (bincompat): first point skipped when the reference point index <= start.
        if ref_point <= start_index as i32 {
            start_index += 1;
        }
        let d = m.gs.vector_for(delta);
        let fv = m.gs.freedom();
        let flag_x = fv.x.0 != 0 && fv.y.0 != 0;
        let flag_y0 = fv.y.0 != 0;
        for index in start_index..zone.phantom_start() {
            let mut flag_y = flag_y0;
            if zone.zone_type == ref_zone {
                if index as i32 == ref_point {
                    continue;
                }
                flag_y = flag_y && (index as i32) > ref_point;
            }
            let h = zone.hinted(index).wrapping_add(d);
            zone.set_hinted(index, h);
            zone.mark_moved(index, flag_x, flag_y);
        }
        Ok(())
    }

    fn sdpvtl(&mut self, rotated: bool) -> Result<(), InterpreterError> {
        let p2_index = self.pop()?;
        let p1_index = self.pop()?;
        let (p2h, p2s) = {
            let z = self.zone(self.m.gs.zp2)?;
            z.check_against_maxp(&self.m.maxp)?;
            let i = z.check_point(p2_index)?;
            (z.hinted(i), z.scaled(i))
        };
        // Bincompat: zone1 is not checked by SDPVTL
        let (p1h, p1s) = {
            let z = self.zone(self.m.gs.zp1)?;
            let i = z.check_point(p1_index)?;
            (z.hinted(i), z.scaled(i))
        };
        let pv = compute_unit_vector(p1h, p2h, rotated);
        let dv = compute_unit_vector(p1s, p2s, rotated);
        self.m.gs.set_projection(pv);
        self.m.gs.set_dual_projection(dv);
        self.m.gs.projection_is_normal = rotated;
        Ok(())
    }

    fn fliprg(&mut self, on: bool) -> Result<(), InterpreterError> {
        let zp0 = self.m.gs.zp0;
        let (zone, m) = self.zp(zp0)?;
        zone.check_against_maxp(&m.maxp)?;
        let max = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
        let min = m.stack.pop().ok_or(InterpreterError::StackUnderflow)?;
        let cap = zone.max_point_count() as i32;
        if min < 0 || min >= cap || max < 0 || max >= cap {
            return Err(InterpreterError::InvalidOperand);
        }
        if max >= min {
            for i in (min as usize)..=(max as usize) {
                if on {
                    zone.on_curve[i] |= crate::zone::ONCURVE;
                } else {
                    zone.on_curve[i] &= !crate::zone::ONCURVE;
                }
            }
        }
        Ok(())
    }

    fn getinfo(&mut self) -> Result<(), InterpreterError> {
        const VERSION: u32 = 1 << 0;
        const ROTATED: u32 = 1 << 1;
        const STRETCHED: u32 = 1 << 2;
        const VARIATION: u32 = 1 << 3;
        const VERTICAL: u32 = 1 << 4;
        const QUICKDRAW_GX_VERSION: u32 = 7;
        let selector = self.pop()? as u32;
        let mut result: u32 = 0;
        if selector & VERSION != 0 {
            result = QUICKDRAW_GX_VERSION;
        }
        if selector & ROTATED != 0 && self.m.scale.is_rotated {
            result |= 1 << 8;
        }
        if selector & STRETCHED != 0 && self.m.scale.is_stretched {
            result |= 1 << 9;
        }
        if selector & VARIATION != 0 {
            result |= 1 << 10;
        }
        if selector & VERTICAL != 0 {
            result |= 1 << 11;
        }
        self.push(result as i32)
    }
}

#[inline]
fn m_zp0(run: &Run<'_, '_, '_>) -> ZoneType {
    run.m.gs.zp0
}

