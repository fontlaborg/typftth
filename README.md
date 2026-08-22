<!-- this_file: README.md -->
# typftth

TrueType hinting bytecode interpreter in pure Rust — a port of Apple's
[TrueType hinting interpreter](https://github.com/apple/truetype-hinting-interpreter-example)
(QuickDraw GX lineage, published in Swift, MIT) — with a **step-observer
hook** for debuggers, a **font loader** on [`read-fonts`](https://crates.io/crates/read-fonts),
and a **WASM binding** used by the [FontLab TTH Debugger](https://studio.fontlab.com/tth-debugger/).

```text
fpgm / prep / glyf bytecode ──▶ typftth::Machine ──▶ hinted 26.6 points + touch flags
                                      │
                                      └── StepObserver: every instruction, full state
```

- No `unsafe`, no panics on malformed fonts: every failure is a typed error.
- Bit-exact with the Swift reference where it matters: SROUND/S45ROUND
  match Apple's reference-interpreter table for all 256 parameters; the
  ~30 documented quirks of the original C++ engine are reproduced
  ([`docs/bincompat.md`](docs/bincompat.md)).
- `no_std` + `alloc` core (`default-features = false`).

## Crates

| Crate | What |
|---|---|
| `typftth` | the interpreter, loader (`read-fonts`/`skrifa`), hinter, trace recorder |
| `typftth-cli` | `typftth info / hint / sweep` |
| `typftth-wasm` | `wasm-bindgen` surface (`TthFont::record` → debugger snapshot blob) |

## Use

```rust
use typftth::{hinter::Hinter, loader::HintFont, NoTrace};

let data = std::fs::read("font.ttf")?;
let font = HintFont::parse(&data, 0)?;
let coords = font.location(&[(*b"wght", 700.0)]);     // normalized 2.14, avar-aware
let mut hinter = Hinter::new(font, 16, &coords)?;      // runs fpgm + prep once
let glyph = hinter.hint_glyph(42, &mut NoTrace)?;      // runs the glyph program
for (x, y) in glyph.points() { /* 26.6 pixels */ }
```

Trace a run for a debugger:

```rust
use typftth::trace::Recorder;
let mut rec = Recorder::new(hinter.font.units_per_em as u32, 16, 42);
let glyph = hinter.hint_glyph(42, &mut rec)?;
rec.finish(&glyph.zone, glyph.error);
let blob = rec.to_blob();   // FontLab TTH Debugger snapshot v1
```

Or implement `StepObserver` yourself — it sees the machine, the execution
state (program, ip, call depth), both zones and the opcode before every
instruction, and can stop the run.

## CLI

```bash
cargo install typftth-cli
typftth info  Font.ttf
typftth hint  Font.ttf --gid 42 --ppem 16 --var wght=700 [--trace out.bin]
typftth sweep Font.ttf --ppems 9,12,16,24,48      # corpus health check
```

## What it is not

- Not FreeType: this is the GX interpreter (`GETINFO` version 7). It has no
  v35/v40 "backward compatibility" modes, so VTT-hinted fonts that branch on
  the rasterizer version take their "classic" path. FreeType stays the
  oracle in the debugger; typftth is the second opinion.
- Not a rasterizer: it produces hinted outlines. typf's `opixa` backend
  rasterizes them.
- Composite glyphs are flattened before hinting (component programs are
  not run individually yet).

## Scaling

Points and CVT entries are scaled like FreeType (`FT_DivFix`/`FT_MulFix`,
round to nearest) so unhinted outlines are identical to FreeType's and
engine comparisons only show interpreter differences.

## Licence

Apache-2.0. The interpreter is derived from Apple's MIT-licensed Swift
source (see `NOTICE`).
