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
- `Hinter::hint_glyph` honours `INSTCTRL` no-grid-fit from `prep` (skips
  the glyph program like FreeType's `FT_LOAD_NO_HINTING`); Junicode at
  12 ppem now matches.
- The step observer now sees `ENDF` (once per LOOPCALL iteration) so traces
  align 1:1 with FreeType's.

### Added
- `GetInfoProfile` (`Machine::getinfo`, `Hinter::with_options`): what `GETINFO`
  reports. Default stays Apple's GX (version 7); `freetype_v35`/`freetype_v40`
  reproduce FreeType's `Ins_GETINFO` (grayscale / ClearType bits) so fonts that
  gate hinting on the rasterizer version take the same branches.
- `typftth hint --trace FILE --program prep|fpgm` records the setup programs;
  `--getinfo gx|35|40 --render mono|gray|lcd|lcd-v` select the profile.
- `HinterOptions { getinfo, lenient_cvt }` / `Machine::lenient_cvt`: FreeType's
  non-pedantic tolerance of out-of-range CVT indices (MIAP/MIRP skip the
  move but set reference points, WCVTP/WCVTF/DELTAC no-op, RCVT reads 0).
  `HinterOptions::freetype(profile)` turns both on; the CLI does so for
  `--getinfo 35|40`.
- wasm: `TthFont.recordWith(gid, ppem, coords, version, render)` (FreeType
  profile when `version` is 35 or 40).

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
