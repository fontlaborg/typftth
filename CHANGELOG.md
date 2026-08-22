<!-- this_file: CHANGELOG.md -->
# Changelog

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
