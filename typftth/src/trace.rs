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

/* ------------------------------------------------------------------ recorder */

/// Snapshot blob magic (`"THD1"` big-endian as read by the debugger).
pub const BLOB_MAGIC: u32 = 0x5448_4431;
/// Blob format version this recorder writes.
pub const BLOB_VERSION: u32 = 1;
/// Maximum recorded steps (matches the FreeType wrapper's `MAX_STEPS`).
pub const MAX_STEPS: usize = 12_000;

/// FreeType tag bits used by the debugger: on-curve, touch x, touch y.
const FT_TAG_ON: u8 = 0x01;
const FT_TAG_TOUCH_X: u8 = 0x08;
const FT_TAG_TOUCH_Y: u8 = 0x10;

/// Map our round state to FreeType's `round_state` enum for the UI
/// (`half=0 grid=1 double=2 down=3 up=4 off=5 super=6 super45=7`).
fn round_state_code(r: &crate::gs::RoundState) -> i32 {
    use crate::gs::{RoundMethod, RoundState};
    if *r == RoundState::RTHG {
        0
    } else if *r == RoundState::RTG {
        1
    } else if *r == RoundState::RTDG {
        2
    } else if *r == RoundState::RDTG {
        3
    } else if *r == RoundState::RUTG {
        4
    } else if *r == RoundState::ROFF {
        5
    } else if r.method == RoundMethod::Divide {
        7
    } else {
        6
    }
}

#[derive(Clone)]
struct Snap {
    ip: i32,
    call_top: i32,
    range: u8,
    opcode: u8,
    stack: alloc::vec::Vec<i32>,
    gs: [i32; 24],
    cur_x: alloc::vec::Vec<i32>,
    cur_y: alloc::vec::Vec<i32>,
    tags: alloc::vec::Vec<u8>,
    tw_x: alloc::vec::Vec<i32>,
    tw_y: alloc::vec::Vec<i32>,
    cvt: alloc::vec::Vec<i32>,
    storage: alloc::vec::Vec<i32>,
}

/// Records every instruction of a glyph run into the TTH Debugger
/// snapshot format (identical to what the FreeType wrapper emits).
pub struct Recorder {
    upem: u32,
    ppem: u32,
    glyph_index: u32,
    steps: alloc::vec::Vec<Snap>,
    truncated: bool,
    n_points: usize,
    n_contours: usize,
    n_twilight: usize,
    contours: alloc::vec::Vec<u16>,
    org_x: alloc::vec::Vec<i32>,
    org_y: alloc::vec::Vec<i32>,
    org_tags: alloc::vec::Vec<u8>,
    glyph_ins: alloc::vec::Vec<u8>,
    ins_error: i32,
    /// Only record instructions of this program (the glyph program by
    /// default); `None` records everything (fpgm/prep too).
    pub only: Option<crate::exec::Program>,
}

impl Recorder {
    /// New recorder for glyph `glyph_index` at `ppem`.
    pub fn new(upem: u32, ppem: u32, glyph_index: u32) -> Recorder {
        Recorder {
            upem,
            ppem,
            glyph_index,
            steps: alloc::vec::Vec::new(),
            truncated: false,
            n_points: 0,
            n_contours: 0,
            n_twilight: 0,
            contours: alloc::vec::Vec::new(),
            org_x: alloc::vec::Vec::new(),
            org_y: alloc::vec::Vec::new(),
            org_tags: alloc::vec::Vec::new(),
            glyph_ins: alloc::vec::Vec::new(),
            ins_error: 0,
            only: Some(crate::exec::Program::Glyf),
        }
    }

    /// Number of recorded steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    fn tag_of(zone: &Zone, i: usize) -> u8 {
        let mut t = 0u8;
        if zone.on_curve[i] & crate::zone::ONCURVE != 0 {
            t |= FT_TAG_ON;
        }
        if zone.f[i] & crate::zone::XMOVED != 0 {
            t |= FT_TAG_TOUCH_X;
        }
        if zone.f[i] & crate::zone::YMOVED != 0 {
            t |= FT_TAG_TOUCH_Y;
        }
        t
    }

    fn capture(&mut self, m: &Machine, exec: &Exec<'_>, glyph: Option<&Zone>, twilight: &Zone, ip: usize, opcode: u8) {
        // FreeType's "points" = outline + 4 public phantoms.
        let (n, cur_x, cur_y, tags) = match glyph {
            Some(z) => {
                let n = z.phantom_start() + crate::zone::PUBLIC_PHANTOM_COUNT;
                (n, z.x[..n].to_vec(), z.y[..n].to_vec(), (0..n).map(|i| Self::tag_of(z, i)).collect())
            }
            None => (0, alloc::vec::Vec::new(), alloc::vec::Vec::new(), alloc::vec::Vec::new()),
        };
        if self.steps.is_empty() {
            self.n_points = n;
            self.n_twilight = twilight.max_point_count();
            if let Some(z) = glyph {
                self.n_contours = z.contour_count;
                self.contours = z.ep[..z.contour_count].to_vec();
                self.org_x = z.ox[..n].to_vec();
                self.org_y = z.oy[..n].to_vec();
                self.org_tags = (0..n).map(|i| Self::tag_of(z, i) & FT_TAG_ON).collect();
            }
            self.glyph_ins = exec.code.glyf.to_vec();
        }
        let gs = &m.gs;
        let pv = gs.projection();
        let fv = gs.freedom();
        let dv = gs.dual_projection();
        let gs_arr: [i32; 24] = [
            pv.x.0 as i32,
            pv.y.0 as i32,
            fv.x.0 as i32,
            fv.y.0 as i32,
            dv.x.0 as i32,
            dv.y.0 as i32,
            gs.rp0,
            gs.rp1,
            gs.rp2,
            gs.zp0 as i32,
            gs.zp1 as i32,
            gs.zp2 as i32,
            gs.loop_count,
            gs.minimum_distance.0,
            round_state_code(&gs.round_state),
            gs.auto_flip as i32,
            m.cvt_cut_in.0,
            m.single_width_cut_in.0,
            m.single_width_value.0,
            i32::from(gs.delta_base),
            i32::from(gs.delta_shift),
            i32::from(gs.instruct_control.0),
            (gs.scan_control.low() != 0) as i32,
            i32::from(gs.scan_control.kind()),
        ];
        self.steps.push(Snap {
            ip: ip as i32,
            call_top: exec.call_depth() as i32,
            range: exec.top.definition.program.range_id(),
            opcode,
            stack: m.stack.clone(),
            gs: gs_arr,
            cur_x,
            cur_y,
            tags,
            tw_x: twilight.x.clone(),
            tw_y: twilight.y.clone(),
            cvt: m.cvt.clone(),
            storage: m.storage.iter().map(|&v| v as i32).collect(),
        });
    }

    /// Append the final "done" snapshot after the run (the debugger shows
    /// the end state with `ip` past the last glyph instruction).
    pub fn finish(&mut self, zone: &Zone, error: Option<crate::InterpreterError>) {
        self.ins_error = error.map(|e| e.code()).unwrap_or(0);
        if let Some(last) = self.steps.last().cloned() {
            let n = self.n_points;
            let done = Snap {
                ip: self.glyph_ins.len() as i32,
                call_top: 0,
                range: 3,
                opcode: 0,
                stack: alloc::vec::Vec::new(),
                gs: last.gs,
                cur_x: zone.x[..n].to_vec(),
                cur_y: zone.y[..n].to_vec(),
                tags: (0..n).map(|i| Self::tag_of(zone, i)).collect(),
                tw_x: last.tw_x.clone(),
                tw_y: last.tw_y.clone(),
                cvt: last.cvt.clone(),
                storage: last.storage.clone(),
            };
            self.steps.push(done);
        }
    }

    /// Serialize to the snapshot blob (little-endian, 4-byte aligned sections).
    pub fn to_blob(&self) -> alloc::vec::Vec<u8> {
        let mut b: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let u32w = |b: &mut alloc::vec::Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        let i32w = |b: &mut alloc::vec::Vec<u8>, v: i32| b.extend_from_slice(&v.to_le_bytes());
        let pad4 = |b: &mut alloc::vec::Vec<u8>| {
            while b.len() % 4 != 0 {
                b.push(0);
            }
        };
        let max_stack = self.steps.iter().map(|s| s.stack.len()).max().unwrap_or(0) as u32;
        u32w(&mut b, BLOB_MAGIC);
        u32w(&mut b, BLOB_VERSION);
        u32w(&mut b, self.steps.len() as u32);
        u32w(&mut b, self.n_points as u32);
        u32w(&mut b, self.n_contours as u32);
        u32w(&mut b, self.n_twilight as u32);
        u32w(&mut b, self.steps.first().map(|s| s.cvt.len()).unwrap_or(0) as u32);
        u32w(&mut b, self.steps.first().map(|s| s.storage.len()).unwrap_or(0) as u32);
        u32w(&mut b, self.upem);
        u32w(&mut b, self.ppem);
        u32w(&mut b, self.glyph_index);
        i32w(&mut b, self.ins_error);
        u32w(&mut b, self.truncated as u32);
        u32w(&mut b, self.glyph_ins.len() as u32);
        u32w(&mut b, max_stack);
        for &c in &self.contours {
            b.extend_from_slice(&c.to_le_bytes());
        }
        pad4(&mut b);
        for &v in &self.org_x {
            i32w(&mut b, v);
        }
        for &v in &self.org_y {
            i32w(&mut b, v);
        }
        b.extend_from_slice(&self.org_tags);
        pad4(&mut b);
        if !self.glyph_ins.is_empty() {
            b.extend_from_slice(&self.glyph_ins);
            pad4(&mut b);
        }
        for s in &self.steps {
            i32w(&mut b, s.ip);
            i32w(&mut b, s.call_top);
            b.extend_from_slice(&(s.stack.len() as u16).to_le_bytes());
            b.push(s.range);
            b.push(s.opcode);
            for v in &s.gs[..6] {
                b.extend_from_slice(&(*v as i16).to_le_bytes());
            }
            for v in &s.gs[6..12] {
                b.extend_from_slice(&(*v as u16).to_le_bytes());
            }
            for v in &s.gs[12..24] {
                i32w(&mut b, *v);
            }
            if self.n_points > 0 {
                for v in &s.cur_x {
                    i32w(&mut b, *v);
                }
                for v in &s.cur_y {
                    i32w(&mut b, *v);
                }
                b.extend_from_slice(&s.tags);
                pad4(&mut b);
            }
            for v in &s.tw_x {
                i32w(&mut b, *v);
            }
            for v in &s.tw_y {
                i32w(&mut b, *v);
            }
            for v in &s.stack {
                i32w(&mut b, *v);
            }
            for v in &s.cvt {
                i32w(&mut b, *v);
            }
            for v in &s.storage {
                i32w(&mut b, *v);
            }
        }
        b
    }
}

impl StepObserver for Recorder {
    fn before_instruction(&mut self, view: &StepView<'_>) -> Flow {
        if let Some(only) = self.only {
            if view.exec.program != only {
                return Flow::Continue;
            }
        }
        if self.steps.len() >= MAX_STEPS {
            self.truncated = true;
            return Flow::Continue;
        }
        self.capture(view.machine, view.exec, view.glyph, view.twilight, view.ip, view.opcode);
        Flow::Continue
    }
}
