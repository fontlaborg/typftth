//! this_file: typftth-cli/src/main.rs
//!
//! `typftth` — hint TrueType glyphs from the command line.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use typftth::hinter::Hinter;
use typftth::loader::HintFont;
use typftth::trace::{Recorder, StepCounter};
use typftth::{F2Dot14, GetInfoProfile};

#[derive(Parser)]
#[command(name = "typftth", version, about = "TrueType hinting interpreter (Apple GX lineage)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print hinting-related facts about a font.
    Info {
        font: PathBuf,
        #[arg(long, default_value_t = 0)]
        index: u32,
    },
    /// Hint one glyph and print the hinted points as JSON.
    Hint {
        font: PathBuf,
        #[arg(long, default_value_t = 0)]
        index: u32,
        /// Glyph id.
        #[arg(long)]
        gid: u32,
        #[arg(long, default_value_t = 16)]
        ppem: i32,
        /// Axis setting, e.g. `wght=700` (repeatable).
        #[arg(long = "var")]
        vars: Vec<String>,
        /// Write the debugger snapshot blob (v1) to this file.
        #[arg(long)]
        trace: Option<PathBuf>,
        /// Which program the trace records: glyf (default), prep or fpgm.
        #[arg(long, default_value = "glyf")]
        program: String,
        /// What GETINFO reports: `gx` (Apple, version 7), `35` or `40` (FreeType).
        #[arg(long, default_value = "gx")]
        getinfo: String,
        /// Render target used for the FreeType-style GETINFO flags: mono, gray, lcd, lcd-v.
        #[arg(long, default_value = "mono")]
        render: String,
    },
    /// Hint every glyph at several sizes and report errors (corpus check).
    Sweep {
        font: PathBuf,
        #[arg(long, default_value = "9,12,16,24,48")]
        ppems: String,
        #[arg(long = "var")]
        vars: Vec<String>,
        /// Limit to the first N glyphs.
        #[arg(long)]
        limit: Option<u32>,
    },
}

fn parse_vars(vars: &[String]) -> Vec<([u8; 4], f32)> {
    vars.iter()
        .filter_map(|v| {
            let (tag, val) = v.split_once('=')?;
            let mut t = [b' '; 4];
            for (i, b) in tag.bytes().take(4).enumerate() {
                t[i] = b;
            }
            Some((t, val.parse().ok()?))
        })
        .collect()
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info { font, index } => {
            let data = std::fs::read(&font)?;
            let f = HintFont::parse(&data, index)?;
            println!("glyphs: {}", f.glyph_count);
            println!("unitsPerEm: {}", f.units_per_em);
            println!("fpgm: {} bytes, prep: {} bytes, cvt: {} entries", f.fpgm.len(), f.prep.len(), f.cvt.len());
            println!(
                "maxp: storage {} fdefs {} idefs {} stack {} twilight {} points {}/{}",
                f.maxp.max_storage,
                f.maxp.max_function_defs,
                f.maxp.max_instruction_defs,
                f.maxp.max_stack_elements,
                f.maxp.max_twilight_points,
                f.maxp.max_points,
                f.maxp.max_composite_points
            );
            for a in &f.axes {
                println!("axis {} {}..{}..{}", String::from_utf8_lossy(&a.tag), a.min, a.default, a.max);
            }
        }
        Cmd::Hint { font, index, gid, ppem, vars, trace, program, getinfo, render } => {
            let data = std::fs::read(&font)?;
            let f = HintFont::parse(&data, index)?;
            let coords: Vec<F2Dot14> = f.location(&parse_vars(&vars));
            let profile = getinfo_profile(&getinfo, &render, !f.axes.is_empty())?;
            let only = match program.as_str() {
                "glyf" => typftth::exec::Program::Glyf,
                "prep" => typftth::exec::Program::Prep,
                "fpgm" => typftth::exec::Program::Fpgm,
                other => return Err(format!("unknown --program {other} (glyf|prep|fpgm)").into()),
            };
            let mut rec = Recorder::new(f.units_per_em as u32, ppem as u32, gid);
            rec.only = Some(only);
            let trace_setup = trace.is_some() && only != typftth::exec::Program::Glyf;
            let mut h = if trace_setup {
                Hinter::with_options(f.clone(), ppem, &coords, profile, &mut rec)?
            } else {
                Hinter::with_options(f.clone(), ppem, &coords, profile, &mut typftth::NoTrace)?
            };
            if let Some(e) = h.prep_error {
                eprintln!("prep failed: {e}");
            }
            let g = if let Some(path) = trace {
                let g = if trace_setup {
                    let g = h.hint_glyph(gid, &mut typftth::NoTrace)?;
                    rec.finish(&g.zone, h.prep_error);
                    g
                } else {
                    let g = h.hint_glyph(gid, &mut rec)?;
                    rec.finish(&g.zone, g.error);
                    g
                };
                std::fs::write(&path, rec.to_blob())?;
                eprintln!("trace ({program}): {} steps → {}", rec.step_count(), path.display());
                g
            } else {
                let mut counter = StepCounter::default();
                let g = h.hint_glyph(gid, &mut counter)?;
                eprintln!("{} instructions", counter.steps);
                g
            };
            let pts: Vec<serde_json::Value> = (0..g.zone.outline_points)
                .map(|i| {
                    serde_json::json!({
                        "x": g.zone.x[i], "y": g.zone.y[i],
                        "ox": g.zone.ox[i], "oy": g.zone.oy[i],
                        "on": g.zone.on_curve[i] & 1 == 1,
                        "touched": g.zone.f[i],
                    })
                })
                .collect();
            let out = serde_json::json!({
                "gid": gid, "ppem": ppem, "composite": g.outline.is_composite,
                "error": g.error.map(|e| e.name()),
                "contours": g.outline.end_pts,
                "advance26_6": g.advance().0,
                "points": pts,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        Cmd::Sweep { font, ppems, vars, limit } => {
            let data = std::fs::read(&font)?;
            let f = HintFont::parse(&data, index_of(&font))?;
            let coords: Vec<F2Dot14> = f.location(&parse_vars(&vars));
            let sizes: Vec<i32> = ppems.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            let n = limit.unwrap_or(f.glyph_count).min(f.glyph_count);
            let mut total = 0u64;
            let mut errors = std::collections::BTreeMap::<String, usize>::new();
            let mut composites = 0usize;
            let mut steps = 0u64;
            for &ppem in &sizes {
                let mut h = Hinter::new(f.clone(), ppem, &coords)?;
                if let Some(e) = h.prep_error {
                    println!("prep@{ppem}: {e}");
                }
                for gid in 0..n {
                    let mut c = StepCounter::default();
                    let g = h.hint_glyph(gid, &mut c)?;
                    total += 1;
                    steps += c.steps;
                    if g.outline.is_composite {
                        composites += 1;
                    }
                    if let Some(e) = g.error {
                        *errors.entry(format!("{e}")).or_default() += 1;
                        if errors.values().sum::<usize>() <= 10 {
                            println!("gid {gid} @{ppem}: {e}{}", if g.outline.is_composite { " (composite)" } else { "" });
                        }
                    }
                }
            }
            println!(
                "{}: {} glyph-runs, {} composite, {} instructions, errors: {:?}",
                font.display(),
                total,
                composites,
                steps,
                errors
            );
        }
    }
    Ok(())
}

/// Parse `--getinfo` / `--render` into a [`GetInfoProfile`].
fn getinfo_profile(version: &str, render: &str, variation: bool) -> Result<GetInfoProfile, String> {
    let (mono, lcd, lcd_v) = match render {
        "mono" => (true, false, false),
        "gray" | "grey" => (false, false, false),
        "lcd" => (false, true, false),
        "lcd-v" | "lcdv" => (false, false, true),
        other => return Err(format!("unknown --render {other} (mono|gray|lcd|lcd-v)")),
    };
    Ok(match version {
        "gx" | "7" => GetInfoProfile::GX,
        "35" => GetInfoProfile::freetype_v35(!mono, variation),
        "40" => GetInfoProfile::freetype_v40(mono, lcd, lcd_v, variation),
        other => return Err(format!("unknown --getinfo {other} (gx|35|40)")),
    })
}

fn index_of(_p: &std::path::Path) -> u32 {
    0
}
