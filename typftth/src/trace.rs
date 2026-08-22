//! this_file: typftth/src/trace.rs
//!
//! Step observation. The interpreter calls `StepObserver::before_instruction`
//! once before every instruction with a read-only view of the whole machine;
//! `Recorder` turns that into the FontLab TTH Debugger snapshot blob (v1).

#![allow(missing_docs)]

use crate::exec::Exec;
use crate::interp::Machine;
use crate::zone::Zone;

/// Observer decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Abort the run with `InterpreterError::Stopped`.
    Stop,
}

/// Read-only view of the interpreter right before an instruction executes.
pub struct StepView<'a> {
    pub machine: &'a Machine,
    pub exec: &'a Exec<'a>,
    pub glyph: Option<&'a Zone>,
    pub twilight: &'a Zone,
    /// Byte offset of the instruction in its program.
    pub ip: usize,
    pub opcode: u8,
}

/// Called before every instruction.
pub trait StepObserver {
    fn before_instruction(&mut self, view: &StepView<'_>) -> Flow;
}

/// Observer that does nothing.
pub struct NoTrace;

impl StepObserver for NoTrace {
    #[inline]
    fn before_instruction(&mut self, _view: &StepView<'_>) -> Flow {
        Flow::Continue
    }
}

/// Counts instructions (cheap "how much work" metric).
#[derive(Default)]
pub struct StepCounter {
    pub steps: u64,
}

impl StepObserver for StepCounter {
    #[inline]
    fn before_instruction(&mut self, _view: &StepView<'_>) -> Flow {
        self.steps += 1;
        Flow::Continue
    }
}
