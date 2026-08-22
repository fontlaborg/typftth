// this_file: typftth/tests/hinter_smoke.rs
use typftth::hinter::Hinter;
use typftth::loader::HintFont;

fn corpus(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var("TTH_FONTS").unwrap_or_else(|_| "/Users/adam/Developer/vcs3/github.fontlab/fontlab-test-fonts-priv/ttfv-tth".into());
    std::fs::read(format!("{dir}/{name}")).ok()
}

#[test]
fn cvt_is_scaled_to_pixels() {
    let Some(data) = corpus("Elstob-Regular.ttf") else { return };
    let f = HintFont::parse(&data, 0).unwrap();
    let h = Hinter::new(&f, 16, &[]).unwrap();
    let scale = h.machine().scale.units_per_em_scale.x;
    assert_eq!(scale.0, 0x10624, "16 ppem / 1000 upem → 1.024 in 16.16");
    // prep may round CVT entries; check a fresh machine without prep instead.
    let mut m = typftth::Machine::new(f.maxp, f.cvt.len());
    m.set_ppem(16, f.units_per_em as i16);
    let raw = f.cvt[0] as i64; // FUnits
    // FT_DivFix(1024, 1000) = 67109; FT_MulFix(raw, 67109)
    let px26_6 = ((raw * 67109 + 0x8000) >> 16) as i32;
    let scaled = typftth::hinter::scale_funit_i32(raw as i32, 16, f.units_per_em);
    assert_eq!(scaled, px26_6, "cvt[0] raw {raw}");
    let _ = m;
    // and through the hinter (post-prep) the value is in the pixel range, not FUnits<<6
    assert!(h.machine().cvt[0] < (raw << 6) as i32 / 8, "cvt[0] after prep = {} looks unscaled", h.machine().cvt[0]);
}
