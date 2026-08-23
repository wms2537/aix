//! VALUE-DIFFERENTIAL property — R-value-faithful.
//!
//! A faithful structural transform preserves every computed value, relocating the edited sheet's
//! cells under σ. This is the check cache-soundness cannot make (the output's caches are all
//! blanked): it EVALUATES the input and the output and asserts the values line up under σ, so a
//! genuine σ mis-shift — a straddling range that changes a SUM, a name whose cell-shaped tail was
//! wrongly shifted, an off-grid materialization — is caught even though nothing about the stored
//! caches would reveal it.

use crate::testkit;

#[test]
fn values_relocate_under_sigma_over_corpus() {
    for case in testkit::corpus() {
        if !case.ironcalc_faithful {
            continue;
        }
        for edit in &case.faithful_edits {
            let (output, report) = testkit::transform(&case.bytes, edit).unwrap();
            if !report.residuals.is_empty() {
                continue; // refused: nothing committed
            }
            if let Err(d) = testkit::value_faithful(&case.bytes, edit, &output) {
                panic!(
                    "{}: edit {:?} is not value-faithful: {}",
                    case.name, edit, d.0
                );
            }
        }
    }
}

#[test]
fn value_differential_detects_a_mis_shift() {
    // Prove the differential is not vacuous: hand a DELIBERATELY corrupted "output" (the input,
    // NOT transformed) against an insert edit. The input's values are at their pre-edit positions,
    // so they do NOT line up under σ — the differential must flag it.
    let corpus = testkit::corpus();
    let case = corpus
        .iter()
        .find(|c| c.name == "sum_band.xlsx")
        .expect("sum_band");
    let edit = &case.faithful_edits[0]; // insert-rows @1x1
                                        // "output" = the untransformed input: every value is one row too high vs where σ maps it.
    let result = testkit::value_faithful(&case.bytes, edit, &case.bytes);
    assert!(
        result.is_err(),
        "the value-differential MUST detect an un-relocated (mis-shifted) output"
    );
}
