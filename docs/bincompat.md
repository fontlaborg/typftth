<!-- this_file: docs/bincompat.md -->
# Binary-compatibility register

Every deliberate oddity of the reference interpreter that typftth reproduces,
with the Swift source location it was ported from
(`_private/truetype-hinting-interpreter-example/Sources/SwiftTrueTypeInterpreter/`)
and where it lives in this crate. "Load-bearing" means a clean-room
implementation would produce different pixels.

| # | Behaviour | Swift | Rust |
|---|---|---|---|
| 1 | SROUND threshold uses `>> 3` (floor), not `/ 8` (toward zero) — differs for negative periods | `RoundState.swift:215-220` | `gs::phase_and_threshold` |
| 2 | Reserved SROUND/S45ROUND period bits `3` → period 999 | `RoundState.swift:121` | `gs::GARBAGE_PERIOD` |
| 3 | S45ROUND division path: non-representable → return `-1` (the C++ double round-trip is UB) | `RoundState.swift:165-169` | `RoundState::round_positive` |
| 4 | `OutlineCoord(bitPattern: UnscaledCoord)` reinterprets FUnits as 26.6 bits (÷64) in IP / MD / MDRP | `VectorTypes.swift:175-185` | `Coord::from_unscaled_bits` |
| 5 | Unit-vector normalisation via leading-zero shift, `Double` hypot, `Fract` divide round-trip, `toNearestOrUp` >>16; negative shift → (1, 0) | `VectorTypes.swift:249-314` | `gs::normalize_like_fnt` |
| 6 | Vector magnitude via `Double hypot`, truncated, saturating | `VectorTypes.swift:156` | `Vec2F16Dot16::magnitude` |
| 7 | SFVTPV forces the cached P·F to exactly 1 | `GraphicsState.swift:126-133` | `GraphicsState::set_freedom_to_projection` |
| 8 | Near-orthogonal P·F (< 1/16) is treated as ±1 | `GraphicsState.swift:77-101` | `GraphicsState::calculate_pdotf` |
| 9 | `effectiveScale` non-normal branch promotes to double and truncates | `GraphicsState.swift:201-203` | `GraphicsState::effective_scale` |
| 10 | `movePoint` touches the projection axis unconditionally after SVTCA (`alwaysTouchAxis`) | `Interpreter.swift:2881-2890` | `geometry::move_point` |
| 11 | MDAP does not set touch flags itself; relies on `movePoint` | `Interpreter.swift:1348-1351` | `Run::mdap` |
| 12 | MDRP: unscaled points read as F16.16 (65536× too small) | `Interpreter.swift:1388-1396` | `Run::mdrp` |
| 13 | MD[1]: unscaled int16 points used with 26.6 arithmetic | `Interpreter.swift:1320-1323` | `Run::md` |
| 14 | IP: `double` cast side-effect emulated with truncating mulDiv | `Interpreter.swift:1086-1090` | `Run::ip` |
| 15 | ISECT treats 26.6 bit patterns as 2.30 throughout | `Interpreter.swift:1105-1108` | `geometry::fract_divide/fract_multiply` |
| 16 | MINDEX 0 pushes the index back and does not throw; other bad indices throw | `Interpreter.swift:1478-1484` | `dispatch(MINDEX)` |
| 17 | CINDEX 0 pushes 0 | `Interpreter.swift:555-566` | `dispatch(CINDEX)` |
| 18 | RCVT out of range pushes 0 (negative index throws); RCVT in `fpgm` pushes 0 | `Interpreter.swift:1640-1660` | `dispatch(RCVT)` |
| 19 | SCANCTRL does not mask the stack value | `Interpreter.swift:1854-1857` | `dispatch(SCANCTRL)` |
| 20 | SPVTL / SFVTL / SDPVTL do not `maxp`-check zone 1 | `Interpreter.swift:1928, 2863` | `Run::unit_vector_from_line`, `Run::sdpvtl` |
| 21 | SHZ ignores points below the first contour's start | `Interpreter.swift:2094-2099` | `Run::shz` |
| 22 | SHZ skips the first point when `refpoint <= start` | `Interpreter.swift:2101-2105` | `Run::shz` |
| 23 | SHZ: XMOVED only if FV non-zero on both axes; YMOVED only for points after the reference point in the same zone | `Interpreter.swift:2111-2116` | `Run::shz` |
| 24 | SHPIX moves along FV ignoring PV | `Interpreter.swift:2056-2057` | `Run::shpix` |
| 25 | SSW operand is the fractional part of an F16.16, not integral FUnits | `Interpreter.swift:2223-2231` | `dispatch(SSW)` |
| 26 | DELTAP with an invalid zp0 fails only when actually modifying a point | `Interpreter.swift:2445-2458` | `Run::delta_move_point` |
| 27 | DELTA binary search replicates the C++ `rangeSize >>= 1; &= ~1` walk plus the ppem re-check | `Interpreter.swift:2546-2568` | `delta_iup::walk_deltas` |
| 28 | IUP validates `ep` against `Int16.max`; int path vs F16.16 ratio path | `Interpreter.swift:2633-2717` | `delta_iup::iup_zone` |
| 29 | `maxp` stack limit not enforced (hard cap 65535) | `Stack.swift:15-16` | `interp::MAX_STACK` |
| 30 | FDEF/CALL/LOOPCALL indices over `maxFunctionDefs` by < 6 are silently ignored; IDEF count margin 4 | `ExecutionState.swift:278-287` | `exec::FDEF_SAFE_MARGIN`, `IDEF_SAFE_MARGIN` |
| 31 | Multiple ELSEs inside a skipped ELSE block are tolerated | `ExecutionState.swift:398-400` | `Exec::seek_after_conditional` |
| 32 | EIF is a no-op; ENDF outside a definition is an error | `Interpreter.swift` (EIF/ENDF) | `dispatch` |
| 33 | DIV by zero returns ±MAX instead of failing | `Interpreter.swift` (DIV) | `dispatch(DIV)` |
| 34 | MUL rounds ties-up inside ±46340, ties-away outside | `Interpreter.swift` (MUL) | `dispatch(MUL)` |
| 35 | GETINFO: version 7 (GX); `variation` and `verticalMetrics` result bits set whenever selected — the default `GetInfoProfile`; hosts may switch to FreeType's v35/v40 reporting | `Interpreter.swift:887-928` | `Run::getinfo` |
| 36 | GETDATA(1) returns 17 % n ("fair dice roll") | `Interpreter.swift:867-885` | `dispatch(GETDATA)` |
| 37 | WCVTP in `fpgm`, or when the rescale would be a no-op, writes the value unscaled | `Interpreter.swift` (WCVTP) | `dispatch(WCVTP)` |
| 38 | Glyph-program errors roll the glyph zone (outline + 4 public phantoms) back to the scaled outline | `Interpreter.swift:439,490-493`, `Zone.swift:173-178` | `Machine::run`, `Zone::rollback` |
| 39 | `prep` parameters are carried into glyph runs only if INSTCTRL did not set the default bit | `Interpreter.swift:445-449` | `Machine::run` |

## Host choices (not in the reference)

| What | Choice | Why |
|---|---|---|
| FUnit → 26.6 scaling of points | `FT_DivFix`/`FT_MulFix` emulation (round to nearest) | identical unhinted outlines to FreeType, so engine diffs show only interpreter semantics (`hinter::scale_funit`) |
| CVT scaling | FreeType `tt_size_run_prep`: CVT kept in 26.6 FUnits, scale = `FT_DivFix(ppem·64, upem) >> 6`, then `FT_MulFix` (`hinter::scale_cvt`) | the dropped six bits make e.g. 729 FUnits @ 9 ppem/1000 upem scale to 419 (not 420); fonts that branch on CVT values or derive CVT indices from scaled constants (Muli) depend on it |
| `units_per_em_scale` (WCVTF, SSW, unscaled MIRP/MDRP) | `FT_DivFix(ppem·64, upem)` rounded to nearest (the Swift harness truncated) | same factor as point scaling |
| ENDF | reported to the step observer (ip = ENDF position) once per LOOPCALL iteration | FreeType executes ENDF as an instruction; traces line up 1:1 |
| Phantom points | FreeType convention: pp1 = (xMin − lsb, 0), pp2 = pp1 + advance, pp3/pp4 = (0, 0); `gvar` phantom deltas applied | matches the debugger's FreeType sessions |
| CVT at a variation location | `cvar` deltas (16.16) added at 1/64 FUnit precision (`FT_fixedToFdot6`) | FreeType `tt_face_vary_cvt` |
| Composites | flattened in FUnits (component transform, offsets, `gvar` component deltas); only the composite program runs, with the "unscaled outline is wrong" correction when varied | Apple's harness contract; component stepping is a planned extension |
| Twilight zone in `fpgm` | absent (access → error) | reference harness |
