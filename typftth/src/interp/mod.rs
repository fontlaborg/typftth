//! this_file: typftth/src/interp/mod.rs
//!
//! The interpreter: persistent machine state (`Machine`) plus one `Run` per
//! program execution. Port of `Interpreter.swift`. Every opcode lives in
//! `ops.rs`; geometry helpers in `geometry.rs`; DELTA/IUP in `delta_iup.rs`.

#![allow(missing_docs)]

mod delta_iup;
mod geometry;
mod ops;

use crate::error::InterpreterError;
use crate::exec::{Code, Exec, FDefs, IDefs, Program};
use crate::fixed::{F16Dot16, F26Dot6, F2Dot14};
use crate::gs::{GraphicsState, Parameters, ScaleFactors, ZoneType, DEFAULT_CVT_CUT_IN};
use crate::trace::{Flow, StepObserver, StepView};
use crate::zone::Zone;

/// The `maxp` values the interpreter needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Maxp {
    pub num_glyphs: u16,
    pub max_points: u16,
    pub max_contours: u16,
    pub max_composite_points: u16,
    pub max_composite_contours: u16,
    pub max_elements: u16,
    pub max_twilight_points: u16,
    pub max_storage: u16,
    pub max_function_defs: u16,
    pub max_instruction_defs: u16,
    pub max_stack_elements: u16,
    pub max_size_of_instructions: u16,
    pub max_component_elements: u16,
    pub max_component_depth: u16,
}

impl Maxp {
    /// Reasonable defaults for synthetic tests (Swift `sfntMaxProfileTable.init` defaults).
    pub fn for_tests() -> Maxp {
        Maxp {
            num_glyphs: 0,
            max_points: 0,
            max_contours: 1,
            max_composite_points: 0,
            max_composite_contours: 0,
            max_elements: 2,
            max_twilight_points: 0,
            max_storage: 1024,
            max_function_defs: 1,
            max_instruction_defs: 0,
            max_stack_elements: 1024,
            max_size_of_instructions: 0,
            max_component_elements: 0,
            max_component_depth: 0,
        }
    }
}

/// Hard stack cap (Swift: `UInt16.max`; maxp's limit is deliberately not enforced).
pub const MAX_STACK: usize = u16::MAX as usize;
/// Swift `kFairDiceRoll`.
pub const FAIR_DICE_ROLL: u32 = 17;

/// Interpreter state that survives between program runs: CVT, storage,
/// function tables, graphics-state parameters, twilight zone, scale.
#[derive(Clone, Debug)]
pub struct Machine {
    pub maxp: Maxp,
    pub gs: GraphicsState,
    /// CVT entries as 26.6 bit patterns (scaled).
    pub cvt: Vec<i32>,
    pub cvt_cut_in: F26Dot6,
    pub single_width_cut_in: F26Dot6,
    pub single_width_value: F26Dot6,
    pub storage: Vec<u32>,
    pub stack: Vec<i32>,
    pub fdefs: FDefs,
    pub idefs: IDefs,
    pub scale: ScaleFactors,
    pub twilight: Zone,
    /// Normalized variation coordinates (2.14). Empty = not a variation instance.
    pub coords: Vec<F2Dot14>,
    /// prep → glyf parameters.
    pub params: Parameters,
}

impl Machine {
    /// New machine for a font. `cvt_count` sizes the CVT (values set via [`Machine::set_cvt`]).
    pub fn new(maxp: Maxp, cvt_count: usize) -> Machine {
        Machine {
            maxp,
            gs: GraphicsState::default(),
            cvt: vec![0; cvt_count],
            cvt_cut_in: if cvt_count > 0 { DEFAULT_CVT_CUT_IN } else { F26Dot6::ZERO },
            single_width_cut_in: F26Dot6::ZERO,
            single_width_value: F26Dot6::ZERO,
            storage: vec![0; maxp.max_storage as usize],
            stack: Vec::with_capacity(1024),
            fdefs: FDefs::new(maxp.max_function_defs as usize),
            idefs: IDefs::new(maxp.max_instruction_defs as usize),
            scale: ScaleFactors::default(),
            twilight: Zone::with_capacity(ZoneType::Twilight, (maxp.max_twilight_points as usize).max(1), 1),
            coords: Vec::new(),
            params: Parameters::default(),
        }
    }

    /// Set CVT values (26.6 bit patterns, already scaled like Apple's
    /// `HinterContext`: FUnits `<< 6`).
    pub fn set_cvt(&mut self, values: &[i32]) {
        self.cvt.clear();
        self.cvt.extend_from_slice(values);
    }

    /// Set the normalized variation coordinates.
    pub fn set_coords(&mut self, coords: &[F2Dot14]) {
        self.coords.clear();
        self.coords.extend_from_slice(coords);
    }

    /// Shortcut for [`ScaleFactors::for_ppem`].
    pub fn set_ppem(&mut self, ppem: i32, units_per_em: i16) {
        self.scale = ScaleFactors::for_ppem(ppem, units_per_em);
    }

    /// Run one program. `glyph` is required for `Glyf`; for `Fpgm` the
    /// twilight zone is considered absent (as in the reference harness).
    ///
    /// A glyph-program error rolls the glyph zone back to the scaled outline
    /// before returning it. After a successful `Prep`, the carried-over
    /// parameters are captured unless `INSTCTRL` set the default bit.
    pub fn run(
        &mut self,
        program: Program,
        code: Code<'_>,
        glyph: Option<&mut Zone>,
        unscaled_outline_is_wrong: bool,
        observer: &mut dyn StepObserver,
    ) -> Result<(), InterpreterError> {
        self.gs.reset(&self.params);
        self.cvt_cut_in = self.params.control_value_cut_in;
        self.single_width_cut_in = self.params.single_width_cut_in;
        self.single_width_value = self.params.single_width_value;
        self.stack.clear();

        let twilight_present = program != Program::Fpgm;
        // Take the twilight zone out so `Run` can borrow zones and the rest
        // of the machine as disjoint fields.
        let twilight = core::mem::replace(&mut self.twilight, Zone::with_capacity(ZoneType::Twilight, 0, 0));
        let mut run = Run {
            m: self,
            exec: Exec::new(program, code),
            glyph,
            twilight,
            twilight_present,
            unscaled_outline_is_wrong,
        };
        let result = run.main_loop(observer);
        if result.is_err() && program == Program::Glyf {
            if let Some(g) = run.glyph.as_deref_mut() {
                g.rollback();
            }
        }
        let Run { twilight, .. } = run;
        self.twilight = twilight;
        if program == Program::Prep && result.is_ok() && !self.gs.instruct_control.contains(crate::gs::InstructControl::DEFAULT) {
            self.params = Parameters {
                control_value_cut_in: self.cvt_cut_in,
                single_width_cut_in: self.single_width_cut_in,
                single_width_value: self.single_width_value,
                auto_flip: self.gs.auto_flip,
                delta_base: self.gs.delta_base,
                delta_shift: self.gs.delta_shift,
                instruct_control: self.gs.instruct_control,
                minimum_distance: self.gs.minimum_distance,
                round_state: self.gs.round_state,
                scan_control: self.gs.scan_control,
            };
        }
        result
    }

    /// Effective CVT scale for the current projection vector.
    #[inline]
    pub fn effective_cvt_scale(&mut self) -> F16Dot16 {
        let s = self.scale.cvt_stretch;
        self.gs.effective_scale(s)
    }
}

/// One program execution.
pub(crate) struct Run<'m, 'a, 'g> {
    pub m: &'m mut Machine,
    pub exec: Exec<'a>,
    pub glyph: Option<&'g mut Zone>,
    pub twilight: Zone,
    pub twilight_present: bool,
    pub unscaled_outline_is_wrong: bool,
}

impl<'m, 'a, 'g> Run<'m, 'a, 'g> {
    fn main_loop(&mut self, observer: &mut dyn StepObserver) -> Result<(), InterpreterError> {
        loop {
            while let Some(opcode) = self.exec.next_opcode(true) {
                let ip = self.exec.top.index - 1;
                let view = StepView {
                    machine: self.m,
                    exec: &self.exec,
                    glyph: self.glyph.as_deref(),
                    twilight: &self.twilight,
                    ip,
                    opcode,
                };
                if observer.before_instruction(&view) == Flow::Stop {
                    return Err(InterpreterError::Stopped);
                }
                self.dispatch(opcode)?;
            }
            if !self.exec.pop_frame() {
                return Ok(());
            }
        }
    }

    /* ---------------------------------------------------------- stack */

    #[inline]
    pub(crate) fn pop(&mut self) -> Result<i32, InterpreterError> {
        self.m.stack.pop().ok_or(InterpreterError::StackUnderflow)
    }
    #[inline]
    pub(crate) fn push(&mut self, v: i32) -> Result<(), InterpreterError> {
        if self.m.stack.len() >= MAX_STACK {
            return Err(InterpreterError::StackDepthExceedsLimit);
        }
        self.m.stack.push(v);
        Ok(())
    }
    #[inline]
    pub(crate) fn pop_f26(&mut self) -> Result<F26Dot6, InterpreterError> {
        Ok(F26Dot6(self.pop()?))
    }
    #[inline]
    pub(crate) fn push_f26(&mut self, v: F26Dot6) -> Result<(), InterpreterError> {
        self.push(v.0)
    }
    #[inline]
    pub(crate) fn push_bool(&mut self, b: bool) -> Result<(), InterpreterError> {
        self.push(if b { 1 } else { 0 })
    }
    /// 1-based index below the top → absolute index (Swift `_index(belowTop:)`).
    fn index_below_top(&self, k: u32) -> Result<usize, InterpreterError> {
        let n = self.m.stack.len();
        if k == 0 || (k as usize) > n {
            return Err(InterpreterError::StackUnderflow);
        }
        Ok(n - k as usize)
    }
    pub(crate) fn peek(&self, k: u32) -> Result<i32, InterpreterError> {
        Ok(self.m.stack[self.index_below_top(k)?])
    }
    pub(crate) fn remove_at(&mut self, k: u32) -> Result<i32, InterpreterError> {
        let i = self.index_below_top(k)?;
        Ok(self.m.stack.remove(i))
    }

    /* ---------------------------------------------------------- zones */

    pub(crate) fn zone(&self, t: ZoneType) -> Result<&Zone, InterpreterError> {
        match t {
            ZoneType::Glyph => self.glyph.as_deref().ok_or(InterpreterError::InvalidAccessToGlyphZone),
            ZoneType::Twilight => {
                if self.twilight_present {
                    Ok(&self.twilight)
                } else {
                    Err(InterpreterError::InvalidAccessToTwilightZone)
                }
            }
        }
    }

    /// Mutable zone plus the machine (disjoint fields of `Run`).
    pub(crate) fn zp(&mut self, t: ZoneType) -> Result<(&mut Zone, &mut Machine), InterpreterError> {
        match t {
            ZoneType::Glyph => {
                let g = self.glyph.as_deref_mut().ok_or(InterpreterError::InvalidAccessToGlyphZone)?;
                Ok((g, &mut *self.m))
            }
            ZoneType::Twilight => {
                if !self.twilight_present {
                    return Err(InterpreterError::InvalidAccessToTwilightZone);
                }
                Ok((&mut self.twilight, &mut *self.m))
            }
        }
    }

    pub(crate) fn pop_zone_type(&mut self) -> Result<ZoneType, InterpreterError> {
        let v = self.pop()?;
        ZoneType::from_raw(v, self.m.maxp.max_elements).ok_or(InterpreterError::InvalidOperand)
    }
}

