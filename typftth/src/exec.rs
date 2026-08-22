//! this_file: typftth/src/exec.rs
//!
//! Execution state: the three program byte slices, the call stack of
//! instruction streams, FDEF/IDEF tables, IF/ELSE seeking and push-operand
//! skipping. Port of `ExecutionState.swift` + `InstructionStream.swift`.

#![allow(missing_docs)]

use crate::error::InterpreterError;
use crate::opcodes as op;

/// Which program a byte range belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Program {
    Fpgm,
    Prep,
    Glyf,
}

impl Program {
    /// Code range id as used by the FreeType-compatible trace (1 fpgm, 2 prep, 3 glyf).
    pub fn range_id(self) -> u8 {
        match self {
            Program::Fpgm => 1,
            Program::Prep => 2,
            Program::Glyf => 3,
        }
    }
}

/// A function / instruction definition: program + byte range (end exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Definition {
    pub program: Program,
    pub start: usize,
    pub end: usize,
}

impl Definition {
    pub const UNDEFINED: Definition = Definition { program: Program::Fpgm, start: 0, end: 0 };
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.program == Program::Fpgm && self.start == 0 && self.end == 0
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// The three programs for one run.
#[derive(Clone, Copy, Debug)]
pub struct Code<'a> {
    pub fpgm: &'a [u8],
    pub prep: &'a [u8],
    pub glyf: &'a [u8],
}

impl<'a> Code<'a> {
    #[inline]
    pub fn of(&self, p: Program) -> &'a [u8] {
        match p {
            Program::Fpgm => self.fpgm,
            Program::Prep => self.prep,
            Program::Glyf => self.glyf,
        }
    }
}

/// What a stream is executing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    Program,
    Function(i32),
    Patch(u8),
}

/// One frame of the call stack.
#[derive(Clone, Copy, Debug)]
pub struct Stream {
    pub stream_type: StreamType,
    pub definition: Definition,
    pub index: usize,
    pub loop_count: i64,
}

impl Stream {
    pub fn new(stream_type: StreamType, definition: Definition, loop_count: i64) -> Stream {
        Stream { stream_type, definition, index: definition.start, loop_count }
    }
    #[inline]
    pub fn start(&self) -> usize {
        self.definition.start
    }
    #[inline]
    pub fn end(&self) -> usize {
        self.definition.end
    }
    #[inline]
    pub fn is_at_end(&self) -> bool {
        self.index >= self.end()
    }
    fn restart(&mut self) -> bool {
        self.loop_count = self.loop_count.wrapping_sub(1);
        if self.loop_count > 0 {
            self.index = self.start();
            return self.index < self.end();
        }
        false
    }
    /// Relative jump (Swift `jump(_:)`): `index - 1 + n` must stay inside the range.
    pub fn jump(&mut self, n: i32) -> Result<(), InterpreterError> {
        let new_index = (self.index as i64 - 1) + n as i64;
        if new_index < self.start() as i64 || new_index > self.end() as i64 {
            self.index = self.end();
            return Err(InterpreterError::RanOffEndOfInstructions);
        }
        self.index = new_index as usize;
        Ok(())
    }
    /// Next instruction position, or `None` at the end (optionally
    /// restarting for LOOPCALL).
    #[inline]
    pub fn next(&mut self, looping: bool) -> Option<usize> {
        if self.is_at_end() && !(looping && self.restart()) {
            return None;
        }
        let i = self.index;
        self.index += 1;
        Some(i)
    }
    pub fn skip(&mut self, count: usize) -> Result<(), InterpreterError> {
        let new_index = self.index.checked_add(count).ok_or(InterpreterError::RanOffEndOfInstructions)?;
        if new_index > self.end() {
            self.index = self.end();
            return Err(InterpreterError::RanOffEndOfInstructions);
        }
        self.index = new_index;
        Ok(())
    }
    /// Consume `count` bytes, returning their range.
    pub fn next_range(&mut self, count: usize) -> Result<(usize, usize), InterpreterError> {
        let start = self.index;
        self.skip(count)?;
        Ok((start, self.index))
    }
}

/// Max call depth (Swift `kMaxAllowedRecursivity`).
pub const MAX_CALL_DEPTH: usize = 64;
/// Swift `kMaxFunctionDefsSafeMargin`.
pub const FDEF_SAFE_MARGIN: i64 = 6;
/// Swift `kMaxInstructionDefsSafeMargin`.
pub const IDEF_SAFE_MARGIN: usize = 4;

/// FDEF table (capacity = maxp.maxFunctionDefs).
#[derive(Clone, Debug)]
pub struct FDefs {
    storage: Vec<Definition>,
}

impl FDefs {
    pub fn new(capacity: usize) -> FDefs {
        FDefs { storage: vec![Definition::UNDEFINED; capacity] }
    }
    #[inline]
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }
    pub fn lookup(&self, id: i32) -> Definition {
        if id < 0 {
            return Definition::UNDEFINED;
        }
        self.storage.get(id as usize).copied().unwrap_or(Definition::UNDEFINED)
    }
    /// Bincompat: ids over the limit by less than the safe margin are ignored.
    pub fn define(&mut self, id: i32, def: Definition) -> Result<(), InterpreterError> {
        let limit = self.storage.len() as i64 + FDEF_SAFE_MARGIN;
        if id < 0 || (id as i64) >= limit {
            return Err(InterpreterError::MaxpLimitExceeded);
        }
        if let Some(slot) = self.storage.get_mut(id as usize) {
            *slot = def;
        }
        Ok(())
    }
}

/// IDEF table (always 256 slots; count checked against maxp + margin).
#[derive(Clone, Debug)]
pub struct IDefs {
    storage: Box<[Definition; 256]>,
    count: usize,
    capacity: usize,
}

impl IDefs {
    pub fn new(capacity: usize) -> IDefs {
        IDefs { storage: Box::new([Definition::UNDEFINED; 256]), count: 0, capacity }
    }
    #[inline]
    pub fn lookup(&self, opcode: u8) -> Definition {
        self.storage[opcode as usize]
    }
    pub fn define(&mut self, opcode: u8, def: Definition) -> Result<(), InterpreterError> {
        let slot = &mut self.storage[opcode as usize];
        if slot.is_undefined() {
            if self.count >= self.capacity + IDEF_SAFE_MARGIN {
                return Err(InterpreterError::MaxpLimitExceeded);
            }
            self.count += 1;
        }
        *slot = def;
        Ok(())
    }
}

/// Per-run execution state.
pub struct Exec<'a> {
    pub program: Program,
    pub code: Code<'a>,
    pub call_stack: Vec<Stream>,
    pub top: Stream,
    /// Bytes of the program the top frame executes.
    pub opcodes: &'a [u8],
}

impl<'a> Exec<'a> {
    pub fn new(program: Program, code: Code<'a>) -> Exec<'a> {
        let opcodes = code.of(program);
        let def = Definition { program, start: 0, end: opcodes.len() };
        Exec {
            program,
            code,
            call_stack: Vec::with_capacity(8),
            top: Stream::new(StreamType::Program, def, 1),
            opcodes,
        }
    }

    /// Call depth (0 = main program).
    #[inline]
    pub fn call_depth(&self) -> usize {
        self.call_stack.len()
    }

    pub fn definition_of(&self, t: StreamType, fdefs: &FDefs, idefs: &IDefs) -> Definition {
        match t {
            StreamType::Program => Definition { program: self.program, start: 0, end: self.code.of(self.program).len() },
            StreamType::Function(f) => fdefs.lookup(f),
            StreamType::Patch(i) => idefs.lookup(i),
        }
    }

    #[inline]
    pub fn previous_opcode(&self) -> Option<u8> {
        if self.top.index > self.top.start() {
            self.opcodes.get(self.top.index - 1).copied()
        } else {
            None
        }
    }

    #[inline]
    pub fn next_opcode(&mut self, looping: bool) -> Option<u8> {
        let pos = self.top.next(looping)?;
        self.opcodes.get(pos).copied()
    }

    /// Consume `count` operand bytes.
    pub fn next_bytes(&mut self, count: usize) -> Result<&'a [u8], InterpreterError> {
        let (s, e) = self.top.next_range(count)?;
        self.opcodes.get(s..e).ok_or(InterpreterError::RanOffEndOfInstructions)
    }

    /// Return to the caller; `false` when the main program finished.
    pub fn pop_frame(&mut self) -> bool {
        match self.call_stack.pop() {
            Some(frame) => {
                self.top = frame;
                self.opcodes = self.code.of(self.top.definition.program);
                true
            }
            None => false,
        }
    }

    /// Call a function / patch. Silent no-op for undefined/empty definitions
    /// and non-positive loop counts (Swift `call(type:loop:)`).
    pub fn call(&mut self, t: StreamType, def: Definition, loop_count: i64) -> Result<(), InterpreterError> {
        if def.is_empty() || loop_count <= 0 {
            return Ok(());
        }
        if self.call_stack.len() >= MAX_CALL_DEPTH {
            return Err(InterpreterError::CallStackTooDeep);
        }
        self.opcodes = self.code.of(def.program);
        let new_top = Stream::new(t, def, loop_count);
        let old = core::mem::replace(&mut self.top, new_top);
        self.call_stack.push(old);
        Ok(())
    }

    /// Scan to the matching ENDF after FDEF/IDEF; returns the body range.
    pub fn consume_until_endf(&mut self) -> Result<(usize, usize), InterpreterError> {
        let start = self.top.index;
        while let Some(o) = self.next_opcode(false) {
            match o {
                op::ENDF => return Ok((start, self.top.index - 1)),
                op::FDEF | op::IDEF => return Err(InterpreterError::DefinitionsCannotBeNested),
                _ => self.skip_push_ops(o)?,
            }
        }
        Err(InterpreterError::RanOffEndOfInstructions)
    }

    /// Skip a not-taken IF/ELSE block.
    pub fn seek_after_conditional(&mut self, stop_on_else: bool) -> Result<(), InterpreterError> {
        let mut level = 1i32;
        while let Some(o) = self.next_opcode(false) {
            match o {
                op::EIF => {
                    level -= 1;
                    if level == 0 {
                        return Ok(());
                    }
                }
                op::IF => level += 1,
                op::ELSE => {
                    if level == 1 && stop_on_else {
                        return Ok(());
                    }
                }
                _ => self.skip_push_ops(o)?,
            }
        }
        Err(InterpreterError::RanOffEndOfInstructions)
    }

    /// Skip the inline operands of PUSHB/PUSHW/NPUSHB/NPUSHW.
    pub fn skip_push_ops(&mut self, o: u8) -> Result<(), InterpreterError> {
        match o {
            op::PUSHB..=0xB7 => self.top.skip((o - op::PUSHB) as usize + 1),
            op::PUSHW..=0xBF => self.top.skip(((o - op::PUSHW) as usize + 1) * 2),
            op::NPUSHB => {
                let n = self.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.top.skip(n as usize)
            }
            op::NPUSHW => {
                let n = self.next_opcode(false).ok_or(InterpreterError::RanOffEndOfInstructions)?;
                self.top.skip(n as usize * 2)
            }
            _ => Ok(()),
        }
    }
}
