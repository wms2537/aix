//! REFLEXIVITY property — certify must CERTIFY xlq's OWN transform, over the whole corpus × the
//! op/axis matrix. This single property collapses the entire over-refusal + coupling-inversion
//! family: it would have caught the pivot over-refusal (round 32) and the fullPrecision
//! over-refusal (round 33), which each refused a workbook xlq itself faithfully transformed.

mod common;
use common::*;

#[test]
fn certify_certifies_own_transform_over_corpus() {
    let mut checked = 0;
    for name in corpus_names() {
        for edit in faithful_edits("Sheet1") {
            let (wb, run) = transform(name, &edit);
            // A refused edit commits nothing to certify.
            if !committed(&run) {
                continue;
            }
            let cert = certify(&corpus_path(name), wb.path(), &edit);
            assert!(
                cert.certified(),
                "{name} [{} @{}x{}]: certify must CERTIFY xlq's OWN transform, got status={:?} \
                 reason={:?}\n{}",
                edit.op,
                edit.at,
                edit.count,
                cert.status(),
                cert.reason(),
                cert.stdout
            );
            checked += 1;
        }
    }
    assert!(
        checked >= corpus_names().len(),
        "reflexivity checked too few cases: {checked}"
    );
}

#[test]
fn certify_exit_code_matches_status() {
    // Contract: CERTIFIED -> exit 0, REFUSED -> exit 1.
    let name = "sum_band.xlsx";
    let edit = Edit::insert_rows("Sheet1", 1, 1);
    let (wb, run) = transform(name, &edit);
    assert!(committed(&run));
    let ok = certify(&corpus_path(name), wb.path(), &edit);
    assert!(
        ok.certified() && ok.code == 0,
        "certified must exit 0: {}",
        ok.stdout
    );

    // A value-mangled transform must REFUSE and exit 1.
    let mangled = mangle::change_input_value(&wb.bytes());
    let mwb = temp_from_bytes(&mangled, name);
    let bad = certify(&corpus_path(name), mwb.path(), &edit);
    assert!(
        bad.refused() && bad.code == 1,
        "refused must exit 1: {}",
        bad.stdout
    );
}
