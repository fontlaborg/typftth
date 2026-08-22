// this_file: typftth/tests/hinter_smoke.rs
#![allow(clippy::unwrap_used)]
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
    let h = Hinter::new(f.clone(), 16, &[]).unwrap();
    let scale = h.machine().scale.units_per_em_scale.x;
    assert_eq!(scale.0, 67109, "16 ppem / 1000 upem → 1.024 in 16.16, rounded like FT_DivFix");
    // prep may round CVT entries; check the scaling function directly.
    let raw = f.cvt[0] as i64; // FUnits
    // FreeType: FT_MulFix(raw << 6, FT_DivFix(1024, 1000) >> 6) = FT_MulFix(raw·64, 1048)
    let px26_6 = ((raw * 64 * 1048 + 0x8000) >> 16) as i32;
    let scaled = typftth::hinter::scale_cvt((raw as i32) << 6, 16, f.units_per_em);
    assert_eq!(scaled, px26_6, "cvt[0] raw {raw}");
    // and through the hinter (post-prep) the value is in the pixel range, not FUnits<<6
    assert!(h.machine().cvt[0] < (raw << 6) as i32 / 8, "cvt[0] after prep = {} looks unscaled", h.machine().cvt[0]);
}

/// The interpreter's FUnit→26.6 factor (used by WCVTF/SSW/GETINFO-free
/// paths) must be the same number FreeType uses (`FT_DivFix(ppem·64, upem)`),
/// otherwise fonts that derive CVT indices from scaled constants read
/// different entries than FreeType does.
#[test]
fn units_per_em_scale_matches_ft_divfix() {
    use typftth::gs::ScaleFactors;
    use typftth::hinter::ft_scale;
    for &upem in &[1000i16, 2048, 1024, 256, 4096, 2000, 16384] {
        for ppem in 1..=200 {
            let sf = ScaleFactors::for_ppem(ppem, upem);
            assert_eq!(
                i64::from(sf.units_per_em_scale.x.0),
                ft_scale(ppem, upem as u16),
                "ppem {ppem} upem {upem}: units_per_em_scale must equal FT_DivFix(ppem*64, upem)"
            );
        }
    }
}

/// Regression for the Elstob/Muli divergences: FreeType drops the low six
/// bits of the 16.16 scale before scaling the CVT, so 729 FUnits at 9 ppem /
/// 1000 upem is 419 (not 420), while 104 FUnits is 60 (not 59).
#[test]
fn cvt_scaling_drops_scale_precision_like_freetype() {
    use typftth::hinter::scale_cvt;
    assert_eq!(scale_cvt(729 << 6, 9, 1000), 419);
    assert_eq!(scale_cvt(104 << 6, 9, 1000), 60);
    assert_eq!(scale_cvt(-245 << 6, 9, 1000), -141);
    assert_eq!(scale_cvt(27 << 6, 9, 1000), 16);
    assert_eq!(scale_cvt(515 << 6, 12, 1000), 395, "Muli cvt[46] @12");
    assert_eq!(scale_cvt(729 << 6, 16, 1000), 746);
    assert_eq!(scale_cvt(104 << 6, 16, 1000), 106);
}

/// FreeType 2.14 scales with full precision: 729 FUnits @ 9 ppem → 420.
#[test]
fn cvt_scaling_214_rounds_with_full_precision() {
    use typftth::hinter::scale_cvt_214;
    assert_eq!(scale_cvt_214(729 << 6, 9, 1000), 420);
    assert_eq!(scale_cvt_214(515 << 6, 12, 1000), 396, "Muli cvt[46] @12 under 2.14");
    assert_eq!(scale_cvt_214((515 << 6) + 40, 12, 1000), 396, "cvar fraction truncated before scaling");
}
