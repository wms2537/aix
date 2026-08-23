//! FLAGSHIP property — R-no-stale-cache (the class that hid for 31 rounds).
//!
//! A structural edit changes computed values, so any formula cache the committed output still
//! carries must equal the engine's recomputation of that output. xlq is engine-free and blanks
//! every cache, so this holds vacuously on correct output — but a regression that copies a stale
//! cache verbatim (the original HIGH bug) makes a surviving cache disagree with the recompute and
//! fails here. Runs over the POPULATED-cache corpus × a spread of affecting edits.

use crate::testkit;

#[test]
fn cache_soundness_holds_over_corpus_x_edits() {
    for case in testkit::corpus() {
        for edit in &case.faithful_edits {
            let (output, report) = match testkit::transform(&case.bytes, edit) {
                Ok(x) => x,
                Err(e) => panic!("{}: transform {:?} errored: {e}", case.name, edit),
            };
            // A refused edit commits nothing — no output to check.
            if !report.residuals.is_empty() {
                continue;
            }
            if let Err(m) = testkit::cache_soundness(&output) {
                panic!(
                    "{}: edit {:?} committed a STALE cache: {}",
                    case.name, edit, m.0
                );
            }
        }
    }
}

#[test]
fn output_formula_caches_are_blanked() {
    // The engine-free heir of the bug: the input carries populated caches; every committed
    // output must carry NONE (a cache-less formula is recomputed by any reader).
    for case in testkit::corpus() {
        assert!(
            testkit::populated_cache_count(&case.bytes) > 0,
            "{}: corpus fixture must ship populated input caches",
            case.name
        );
        for edit in &case.faithful_edits {
            let (output, report) = testkit::transform(&case.bytes, edit).unwrap();
            if !report.residuals.is_empty() {
                continue;
            }
            assert_eq!(
                testkit::populated_cache_count(&output),
                0,
                "{}: edit {:?} left a formula cache in the output",
                case.name,
                edit
            );
        }
    }
}

#[test]
fn poisoned_cache_is_detected() {
    // Prove the oracle is NOT vacuous: plant a wrong cache into a committed (blanked) output and
    // assert cache_soundness catches it. If this fails, the flagship property above is toothless.
    let corpus = testkit::corpus();
    let case = corpus
        .iter()
        .find(|c| c.name == "sum_band.xlsx")
        .expect("sum_band in corpus");
    let edit = &case.faithful_edits[0];
    let (output, report) = testkit::transform(&case.bytes, edit).unwrap();
    assert!(report.residuals.is_empty(), "sum_band insert should commit");
    let poisoned = testkit::plant_stale_cache(&output, "99999").expect("planted a stale cache");
    assert!(
        testkit::cache_soundness(&poisoned).is_err(),
        "the cache oracle MUST detect a planted stale cache (else the property is vacuous)"
    );
}
