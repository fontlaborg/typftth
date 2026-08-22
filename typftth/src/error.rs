//! this_file: typftth/src/error.rs
//!
//! Interpreter and loader errors. The variants mirror the Swift
//! `InterpreterError` enum one-to-one; `code()` gives each a stable small
//! integer for the trace blob (`docs/errors.md`).

use core::fmt;

/// Why a program run stopped. Any error aborts the whole run; glyph runs
/// roll the glyph zone back to the scaled outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum InterpreterError {
    ArithmeticError,
    CallStackTooDeep,
    CalledFunctionNotDefined,
    CvtLocationOutOfBounds,
    DefinitionsCannotBeNested,
    DefinitionsNotAllowedInGlyf,
    IllegalInstruction,
    InternalError,
    InvalidAccessToGlyphZone,
    InvalidAccessToTwilightZone,
    InvalidOperand,
    JumpOutOfBounds,
    MaxpLimitExceeded,
    NoInstructionsProvided,
    NoSuchPoint,
    NoSuchContour,
    RanOffEndOfInstructions,
    StackDepthExceedsLimit,
    StackUnderflow,
    StorageLocationOutOfBounds,
    UnbalancedIfElseEif,
    UnexpectedEndf,
    /// The trace observer asked to stop (not an error of the program).
    Stopped,
}

impl InterpreterError {
    /// Stable numeric code (0 = no error) used in trace blobs.
    pub fn code(self) -> i32 {
        match self {
            InterpreterError::ArithmeticError => 1,
            InterpreterError::CallStackTooDeep => 2,
            InterpreterError::CalledFunctionNotDefined => 3,
            InterpreterError::CvtLocationOutOfBounds => 4,
            InterpreterError::DefinitionsCannotBeNested => 5,
            InterpreterError::DefinitionsNotAllowedInGlyf => 6,
            InterpreterError::IllegalInstruction => 7,
            InterpreterError::InternalError => 8,
            InterpreterError::InvalidAccessToGlyphZone => 9,
            InterpreterError::InvalidAccessToTwilightZone => 10,
            InterpreterError::InvalidOperand => 11,
            InterpreterError::JumpOutOfBounds => 12,
            InterpreterError::MaxpLimitExceeded => 13,
            InterpreterError::NoInstructionsProvided => 14,
            InterpreterError::NoSuchPoint => 15,
            InterpreterError::NoSuchContour => 16,
            InterpreterError::RanOffEndOfInstructions => 17,
            InterpreterError::StackDepthExceedsLimit => 18,
            InterpreterError::StackUnderflow => 19,
            InterpreterError::StorageLocationOutOfBounds => 20,
            InterpreterError::UnbalancedIfElseEif => 21,
            InterpreterError::UnexpectedEndf => 22,
            InterpreterError::Stopped => 100,
        }
    }

    /// Short stable name.
    pub fn name(self) -> &'static str {
        match self {
            InterpreterError::ArithmeticError => "arithmeticError",
            InterpreterError::CallStackTooDeep => "callStackTooDeep",
            InterpreterError::CalledFunctionNotDefined => "calledFunctionNotDefined",
            InterpreterError::CvtLocationOutOfBounds => "cvtLocationOutOfBounds",
            InterpreterError::DefinitionsCannotBeNested => "definitionsCannotBeNested",
            InterpreterError::DefinitionsNotAllowedInGlyf => "definitionsNotAllowedInGlyf",
            InterpreterError::IllegalInstruction => "illegalInstruction",
            InterpreterError::InternalError => "internalError",
            InterpreterError::InvalidAccessToGlyphZone => "invalidAccessToGlyphZone",
            InterpreterError::InvalidAccessToTwilightZone => "invalidAccessToTwilightZone",
            InterpreterError::InvalidOperand => "invalidOperand",
            InterpreterError::JumpOutOfBounds => "jumpOutOfBounds",
            InterpreterError::MaxpLimitExceeded => "maxpLimitExceeded",
            InterpreterError::NoInstructionsProvided => "noInstructionsProvided",
            InterpreterError::NoSuchPoint => "noSuchPoint",
            InterpreterError::NoSuchContour => "noSuchContour",
            InterpreterError::RanOffEndOfInstructions => "ranOffEndOfInstructions",
            InterpreterError::StackDepthExceedsLimit => "stackDepthExceedsLimit",
            InterpreterError::StackUnderflow => "stackUnderflow",
            InterpreterError::StorageLocationOutOfBounds => "storageLocationOutOfBounds",
            InterpreterError::UnbalancedIfElseEif => "unbalancedIF_ELSE_EIF",
            InterpreterError::UnexpectedEndf => "unexpectedENDF",
            InterpreterError::Stopped => "stopped",
        }
    }
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for InterpreterError {}

/// Font loading failures (feature `loader`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadError {
    /// Not an sfnt / face index out of range.
    BadFont,
    /// No `glyf`/`loca` (CFF fonts are not TrueType-hinted).
    NotTrueType,
    /// Required table missing or malformed.
    Table(&'static str),
    /// Glyph id out of range.
    NoSuchGlyph(u32),
    /// Composite nesting too deep or cyclic.
    CompositeDepth,
    /// Interpreter failure while running `fpgm`/`prep`.
    Interpreter(InterpreterError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::BadFont => f.write_str("not a usable sfnt font"),
            LoadError::NotTrueType => f.write_str("font has no glyf/loca (CFF is not supported)"),
            LoadError::Table(t) => write!(f, "table {t} missing or malformed"),
            LoadError::NoSuchGlyph(g) => write!(f, "glyph {g} out of range"),
            LoadError::CompositeDepth => f.write_str("composite glyph nests too deep"),
            LoadError::Interpreter(e) => write!(f, "interpreter: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LoadError {}

impl From<InterpreterError> for LoadError {
    fn from(e: InterpreterError) -> Self {
        LoadError::Interpreter(e)
    }
}
