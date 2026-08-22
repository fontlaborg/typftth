// this_file: typftth/tests/sround.rs
//! SROUND / S45ROUND rounding against Apple's reference-interpreter table.

mod sround_reference;

use sround_reference::*;
use typftth::fixed::F26Dot6;
use typftth::gs::RoundState;

#[test]
fn sround_matches_reference_for_all_256_parameters() {
    let mut failures = Vec::new();
    for param in 0..256usize {
        let rs = RoundState::super_round(param as u8);
        for (i, &input) in SROUND_INPUTS.iter().enumerate() {
            let got = rs.round(F26Dot6(input)).0;
            let want = SROUND_EXPECTED[param][i];
            if got != want {
                failures.push(format!("SROUND[{param:#04x}] round({input}) = {got}, want {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} mismatches, first 20:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn s45round_matches_reference_for_all_256_parameters() {
    let mut failures = Vec::new();
    for param in 0..256usize {
        let rs = RoundState::super45_round(param as u8);
        for (i, &input) in S45ROUND_INPUTS.iter().enumerate() {
            let got = rs.round(F26Dot6(input)).0;
            let want = S45ROUND_EXPECTED[param][i];
            if got != want {
                failures.push(format!("S45ROUND[{param:#04x}] round({input}) = {got}, want {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} mismatches, first 20:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn s45round_never_panics_on_overflow_inputs() {
    for param in 0..256usize {
        let rs = RoundState::super45_round(param as u8);
        for v in [i32::MIN, -1_518_500_250, -759_250_125, 759_250_125, 1_518_500_250, i32::MAX] {
            let _ = rs.round(F26Dot6(v));
        }
    }
}
