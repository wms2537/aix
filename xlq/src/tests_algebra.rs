//! σ ALGEBRA laws + OUTPUT WELL-FORMEDNESS.
//!
//! - Round-trip: shifting a formula through an insert and then the matching delete restores it
//!   exactly (σ⁻¹ ∘ σ = id, away from the grid edge). A tokenizer/boundary regression — a name
//!   whose cell-shaped tail is wrongly shifted, a straddle miscounted — breaks the round-trip.
//! - Well-formedness: every coordinate a transform emits is a valid in-grid A1 with no duplicate.

use crate::refshift::{shift_formula, Axis, Op, StructuralEdit};
use crate::testkit;

fn row_edit(op: Op, at: u32, count: u32) -> StructuralEdit {
    StructuralEdit {
        axis: Axis::Row,
        at,
        count,
        op,
        sheet: "Sheet1".to_string(),
        dest: 0,
    }
}

#[test]
fn insert_then_delete_restores_formula() {
    // Diverse shapes: relative/absolute/mixed refs, ranges, straddles, cross-sheet, a function
    // whose name looks cell-shaped, a period name, a non-ASCII name, a nested range.
    let formulas = [
        "A5",
        "$A$5",
        "A$5",
        "$A5",
        "SUM(A2:A10)",
        "SUM(A5:A100)",
        "Sheet2!C4",
        "IF(A5>0,B6,C7)",
        "A5+B5*C5",
        "SUM(A1:A10)+D5",
        "myName",
        "A1.tax",
        "売上A5",
        "BIN2DEC(A5)",
        "SUM(A2:CHOOSE(3,A3,A4))",
    ];
    for f in formulas {
        for at in [1u32, 3, 6, 12] {
            let (shifted, _) = shift_formula(f, "Sheet1", &row_edit(Op::Insert, at, 1));
            let (back, _) = shift_formula(&shifted, "Sheet1", &row_edit(Op::Delete, at, 1));
            assert_eq!(
                back, f,
                "insert@{at} then delete@{at} must restore {f:?} (got {shifted:?} -> {back:?})"
            );
        }
    }
}

#[test]
fn transform_output_is_wellformed_over_corpus() {
    for case in testkit::corpus() {
        for edit in &case.faithful_edits {
            let (output, report) = testkit::transform(&case.bytes, edit).unwrap();
            if !report.residuals.is_empty() {
                continue;
            }
            if let Err(e) = testkit::wellformed(&output) {
                panic!(
                    "{}: edit {:?} produced a malformed output: {e}",
                    case.name, edit
                );
            }
        }
    }
}

#[test]
fn wellformed_detects_off_grid_and_duplicate() {
    // Non-vacuity: hand-craft an off-grid and a duplicate-coordinate worksheet and assert the
    // checker flags them.
    let base = testkit::corpus()[0].bytes.clone();
    let sheets = crate::ooxml::all_sheets(&base).unwrap();
    let (_, part) = &sheets[0];
    let off = testkit::replace_part(
        &base,
        part,
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1048577"><v>1</v></c></row></sheetData></worksheet>"#,
    )
    .unwrap();
    assert!(
        testkit::wellformed(&off).is_err(),
        "must flag an off-grid coordinate"
    );
    let dup = testkit::replace_part(
        &base,
        part,
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="A1"><v>2</v></c></row></sheetData></worksheet>"#,
    )
    .unwrap();
    assert!(
        testkit::wellformed(&dup).is_err(),
        "must flag a duplicate coordinate"
    );
}
