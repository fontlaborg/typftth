<!-- this_file: CHANGELOG.md -->
# Changelog

## Unreleased

### Fixed
- CVT scaling now reproduces FreeType's `tt_size_run_prep` exactly: entries are
  kept in 26.6 FUnits (with `cvar` deltas at 1/64 FUnit precision) and scaled
  with `FT_MulFix(v, FT_DivFix(ppem·64, upem) >> 6)`. Fonts that derive CVT
  indices or branch on CVT values (Muli/Mulish) now hint identically to
  FreeType at every tested size (`hinter::scale_cvt`, `HintFont::cvt_at`).
- `ScaleFactors::units_per_em_scale` rounds like `FT_DivFix` instead of
  truncating, so WCVTF/SSW agree with point scaling.
- The step observer now sees `ENDF` (once per LOOPCALL iteration) so traces
  align 1:1 with FreeType's.

### Added
- `typftth hint --trace FILE --program prep|fpgm` records the setup programs.

## 0.1.0 — 2026-08-22

First release.

- Interpreter core ported from Apple's Swift TrueType hinting interpreter:
  all opcodes, FDEF/IDEF, IF/ELSE, jumps, twilight zone, rounding states,
  DELTA exceptions with the reference's search quirk, IUP, composite
  outline correction; every reproduced quirk listed in `docs/bincompat.md`.
- `StepObserver` hook before each instruction; `Recorder` writes the FontLab
  TTH Debugger snapshot blob (v1, FreeType-compatible tags/round states).
- `loader`: `read-fonts`-based font view — `maxp`/`fpgm`/`prep`/`cvt`,
  simple and composite `glyf` outlines (flattened), `gvar` (dense + sparse
  with inferred deltas), `cvar`, `avar`-aware locations via `skrifa`.
- `hinter`: per-size `fpgm`+`prep` setup, per-glyph machine snapshot,
  FreeType-exact FUnit scaling.
- SROUND/S45ROUND reference-table tests (Apple's C++ data, 256 parameters).
- `typftth-cli` (`info`, `hint`, `sweep`) and `typftth-wasm` (`TthFont`).
