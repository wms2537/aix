//! MUTATION-ADVERSARY property — certify must REFUSE any VALUE/SECURITY divergence from xlq's
//! transform, and must NOT refuse a benign, value-preserving reserialization. This is the
//! false-certify guard: the mangled variant differs from xlq's own transform in exactly one
//! disqualifying way, so a certify that certified it would be laundering a wrong answer.

mod common;
use common::*;

fn baseline_transform(name: &str, edit: &Edit) -> TempWb {
    let (wb, run) = transform(name, edit);
    assert!(
        committed(&run),
        "{name}: transform must commit for the mangle baseline"
    );
    let base = certify(&corpus_path(name), wb.path(), edit);
    assert!(
        base.certified(),
        "{name}: the unmangled transform must certify (else the mangle proves nothing): {}",
        base.stdout
    );
    wb
}

fn assert_refused(name: &str, edit: &Edit, mangler: fn(&[u8]) -> Vec<u8>, label: &str) {
    let wb = baseline_transform(name, edit);
    let mangled = mangler(&wb.bytes());
    assert_ne!(
        mangled,
        wb.bytes(),
        "{name} [{label}]: mangle was a no-op — test is vacuous"
    );
    let mwb = temp_from_bytes(&mangled, name);
    let cert = certify(&corpus_path(name), mwb.path(), edit);
    assert!(
        cert.refused(),
        "{name} [{label}]: a value/security divergence from xlq's transform MUST be REFUSED, got \
         status={:?} reason={:?}\n{}",
        cert.status(),
        cert.reason(),
        cert.stdout
    );
}

fn assert_certified(name: &str, edit: &Edit, f: fn(&[u8]) -> Vec<u8>, label: &str) {
    let wb = baseline_transform(name, edit);
    let variant = f(&wb.bytes());
    let vwb = temp_from_bytes(&variant, name);
    let cert = certify(&corpus_path(name), vwb.path(), edit);
    assert!(
        cert.certified(),
        "{name} [{label}]: a benign value-preserving change must still CERTIFY, got status={:?} \
         reason={:?}\n{}",
        cert.status(),
        cert.reason(),
        cert.stdout
    );
}

#[test]
fn value_divergences_are_refused() {
    let edit = Edit::insert_rows("Sheet1", 1, 1);
    assert_refused(
        "sum_band.xlsx",
        &edit,
        mangle::change_input_value,
        "change_input_value",
    );
    assert_refused(
        "sum_band.xlsx",
        &edit,
        mangle::rewrite_first_formula,
        "rewrite_first_formula",
    );
    assert_refused(
        "sum_band.xlsx",
        &edit,
        mangle::remove_first_literal_cell,
        "remove_first_literal_cell",
    );
}

#[test]
fn value_mangles_refused_across_corpus() {
    // Every populated-cache fixture, a value mangle on the committed transform -> REFUSED.
    let edit = Edit::insert_rows("Sheet1", 1, 1);
    for name in corpus_names() {
        // settings.xlsx has no literal data cell to bump; use the formula mangle there.
        let mangler: fn(&[u8]) -> Vec<u8> = if *name == "settings.xlsx" {
            mangle::rewrite_first_formula
        } else {
            mangle::change_input_value
        };
        assert_refused(name, &edit, mangler, "value-mangle");
    }
}

#[test]
fn benign_reserialization_still_certifies() {
    let edit = Edit::insert_rows("Sheet1", 1, 1);
    assert_certified(
        "sum_band.xlsx",
        &edit,
        benign::reserialize_whitespace,
        "reserialize_whitespace",
    );
    // Stripping caches that xlq already blanked is a no-op certify, so assert the semantic point
    // via reserialize only; the cache-repopulation benign path is covered by the in-crate oracle.
}
