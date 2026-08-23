#![allow(clippy::unwrap_used)]
//! 3D (multi-sheet) span references: `Sheet1:Sheet3!A5` aggregates a cell/range across the
//! inclusive workbook tab range. Excel semantics: iterate every tab from the left endpoint through
//! the right endpoint (by position, inclusive) and apply the trailing range on each.

use crate::test::util::new_empty_model;

/// 3-sheet model: A5 = 10 / 20 / 30 and A1:A3 = 1..3 / 4..6 / 7..9 on Sheet1/2/3.
fn three_sheet_model<'a>() -> crate::model::Model<'a> {
    let mut model = new_empty_model();
    model.new_sheet(); // Sheet2
    model.new_sheet(); // Sheet3
    model._set("Sheet1!A5", "10");
    model._set("Sheet2!A5", "20");
    model._set("Sheet3!A5", "30");
    for (s, base) in [("Sheet1", 1), ("Sheet2", 4), ("Sheet3", 7)] {
        model._set(&format!("{s}!A1"), &format!("{}", base));
        model._set(&format!("{s}!A2"), &format!("{}", base + 1));
        model._set(&format!("{s}!A3"), &format!("{}", base + 2));
    }
    model
}

fn eval(model: &mut crate::model::Model, formula: &str) -> String {
    model._set("Sheet1!Z1", formula);
    model.evaluate();
    model._get_text("Sheet1!Z1")
}

#[test]
fn canonical_3d_span_aggregates() {
    let mut m = three_sheet_model();
    // single cell A5 across three tabs: 10 + 20 + 30
    assert_eq!(eval(&mut m, "=SUM(Sheet1:Sheet3!A5)"), "60");
    assert_eq!(eval(&mut m, "=AVERAGE(Sheet1:Sheet3!A5)"), "20");
    assert_eq!(eval(&mut m, "=COUNT(Sheet1:Sheet3!A5)"), "3");
    assert_eq!(eval(&mut m, "=COUNTA(Sheet1:Sheet3!A5)"), "3");
    assert_eq!(eval(&mut m, "=MIN(Sheet1:Sheet3!A5)"), "10");
    assert_eq!(eval(&mut m, "=MAX(Sheet1:Sheet3!A5)"), "30");
    assert_eq!(eval(&mut m, "=PRODUCT(Sheet1:Sheet3!A5)"), "6000");
}

#[test]
fn canonical_3d_span_range_form() {
    let mut m = three_sheet_model();
    // A1:A3 on each of the three tabs: (1+2+3)+(4+5+6)+(7+8+9) = 6 + 15 + 24
    assert_eq!(eval(&mut m, "=SUM(Sheet1:Sheet3!A1:A3)"), "45");
    assert_eq!(eval(&mut m, "=COUNT(Sheet1:Sheet3!A1:A3)"), "9");
    assert_eq!(eval(&mut m, "=MAX(Sheet1:Sheet3!A1:A3)"), "9");
    assert_eq!(eval(&mut m, "=MIN(Sheet1:Sheet3!A1:A3)"), "1");
}

#[test]
fn two_sheet_and_reversed_and_self_span() {
    let mut m = three_sheet_model();
    // two-tab span
    assert_eq!(eval(&mut m, "=SUM(Sheet1:Sheet2!A5)"), "30");
    // reversed endpoints denote the same inclusive set
    assert_eq!(eval(&mut m, "=SUM(Sheet3:Sheet1!A5)"), "60");
    // a self-span is an ordinary single-sheet reference
    assert_eq!(eval(&mut m, "=SUM(Sheet2:Sheet2!A5)"), "20");
}

#[test]
fn span_ignores_blanks_and_text_like_excel() {
    let mut m = three_sheet_model();
    // Replace Sheet2!A5 with text and Sheet3!A5 with a blank.
    m._set("Sheet2!A5", "hello");
    m._set("Sheet3!A5", "");
    // SUM: only Sheet1!A5 = 10 is numeric.
    assert_eq!(eval(&mut m, "=SUM(Sheet1:Sheet3!A5)"), "10");
    // COUNT counts only numbers (1); COUNTA counts non-empty (text + number = 2).
    assert_eq!(eval(&mut m, "=COUNT(Sheet1:Sheet3!A5)"), "1");
    assert_eq!(eval(&mut m, "=COUNTA(Sheet1:Sheet3!A5)"), "2");
    // AVERAGE divides by the NUMERIC count only (10 / 1).
    assert_eq!(eval(&mut m, "=AVERAGE(Sheet1:Sheet3!A5)"), "10");
}

#[test]
fn single_sheet_ranges_unaffected() {
    // Regression: an ordinary single-sheet range/reference is unchanged by the sheet-loop.
    let mut m = three_sheet_model();
    assert_eq!(eval(&mut m, "=SUM(Sheet1!A1:A3)"), "6");
    assert_eq!(eval(&mut m, "=SUM(A1:A3)"), "6"); // Z1 is on Sheet1
    assert_eq!(eval(&mut m, "=MAX(Sheet3!A1:A3)"), "9");
}

#[test]
fn span_round_trips_to_canonical_form() {
    // get_formula must return the canonical `Sheet1:Sheet3!A5` so the certify detector sees it.
    let mut m = three_sheet_model();
    m._set("Sheet1!Z1", "=SUM(Sheet1:Sheet3!A5)");
    m.evaluate();
    assert_eq!(m._get_formula("Sheet1!Z1"), "=SUM(Sheet1:Sheet3!A5)");
    // range form + a quoted sheet name round-trips too.
    m.rename_sheet("Sheet2", "My Sheet").unwrap();
    m._set("Sheet1!Z2", "=SUM(Sheet1:Sheet3!A1:A3)");
    m.evaluate();
    assert_eq!(m._get_formula("Sheet1!Z2"), "=SUM(Sheet1:Sheet3!A1:A3)");
}

#[test]
fn ordinary_range_operator_untouched_by_3d_recognition() {
    // The lexer's 3D recognition must NOT hijack `A1:B2` (cells) or a defined-name range operator.
    let mut m = three_sheet_model();
    m._set("Sheet1!B1", "100");
    m._set("Sheet1!B2", "200");
    m._set("Sheet1!B3", "300");
    assert_eq!(eval(&mut m, "=SUM(B1:B3)"), "600");
}
