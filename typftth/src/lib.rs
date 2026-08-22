//! this_file: typftth/src/lib.rs
//!
//! # typftth — TrueType hinting interpreter
//!
//! A Rust port of Apple's TrueType bytecode interpreter (QuickDraw GX
//! lineage, published in Swift under the MIT licence), extended with:
//!
//! - a **step observer** hook before every instruction ([`trace::StepObserver`]),
//! - a **recorder** that emits the FontLab TTH Debugger snapshot blob
//!   ([`trace::Recorder`], feature `std`),
//! - a **font loader** on `read-fonts` that turns a `glyf` glyph (simple or
//!   composite, at a variation location) into zone points and runs
//!   `fpgm`/`prep`/glyph programs ([`hinter::Hinter`], feature `loader`).
//!
//! The interpreter core is `no_std`-friendly (it uses `alloc`), has no
//! `unsafe` code, and never panics on malformed bytecode: every failure is an
//! [`InterpreterError`].
//!
//! Bit-exactness with the Swift reference is the design goal; see
//! `docs/bincompat.md` for the register of reproduced quirks.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod error;
pub mod exec;
pub mod fixed;
pub mod gs;
pub mod interp;
pub mod opcodes;
pub mod trace;
pub mod zone;

#[cfg(feature = "loader")]
pub mod hinter;
#[cfg(feature = "loader")]
pub mod loader;

pub use error::{InterpreterError, LoadError};
pub use exec::{Code, Program};
pub use fixed::{F16Dot16, F26Dot6, F2Dot14};
pub use gs::{GraphicsState, RoundState, ScaleFactors, ZoneType};
pub use interp::{Machine, Maxp};
pub use trace::{Flow, NoTrace, StepObserver, StepView};
pub use zone::Zone;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
