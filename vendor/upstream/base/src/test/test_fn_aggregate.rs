#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn fn_aggregate_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=AGGREGATE()");
    model._set("A2", "=AGGREGATE(9)");
    model._set("A3", "=AGGREGATE(9, 4)");
    // Functions 14-19 need exactly one array and a k argument
    model._set("A4", "=AGGREGATE(14, 4, B1:B3)");
    model._set("A5", "=AGGREGATE(14, 4, B1:B3, 1, 2)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
    assert_eq!(model._get_text("A4"), *"#ERROR!");
    assert_eq!(model._get_text("A5"), *"#ERROR!");
}

#[test]
fn fn_aggregate_invalid_function_num_and_options() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "2");

    model._set("A1", "=AGGREGATE(0, 4, B1:B2)");
    model._set("A2", "=AGGREGATE(20, 4, B1:B2)");
    model._set("A3", "=AGGREGATE(9, 8, B1:B2)");
    model._set("A4", "=AGGREGATE(9, -1, B1:B2)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
}

#[test]
fn fn_aggregate_scalar_functions() {
    let mut model = new_empty_model();

    // B1:B6 = 2, 4, 4, 6, 8, 12 and a string that must be ignored
    model._set("B1", "2");
    model._set("B2", "4");
    model._set("B3", "4");
    model._set("B4", "6");
    model._set("B5", "8");
    model._set("B6", "12");
    model._set("B7", "some text");

    // C1:C8 = 2, 4, 4, 4, 5, 5, 7, 9 (population variance 4)
    model._set("C1", "2");
    model._set("C2", "4");
    model._set("C3", "4");
    model._set("C4", "4");
    model._set("C5", "5");
    model._set("C6", "5");
    model._set("C7", "7");
    model._set("C8", "9");

    // D1:D3 = 1, 3, 5 (sample variance 4)
    model._set("D1", "1");
    model._set("D2", "3");
    model._set("D3", "5");

    model._set("A1", "=AGGREGATE(1, 4, B1:B7)");
    model._set("A2", "=AGGREGATE(2, 4, B1:B7)");
    model._set("A3", "=AGGREGATE(3, 4, B1:B7)");
    model._set("A4", "=AGGREGATE(4, 4, B1:B7)");
    model._set("A5", "=AGGREGATE(5, 4, B1:B7)");
    model._set("A6", "=AGGREGATE(6, 4, B1:B7)");
    model._set("A7", "=AGGREGATE(7, 4, D1:D3)");
    model._set("A8", "=AGGREGATE(8, 4, C1:C8)");
    model._set("A9", "=AGGREGATE(9, 4, B1:B7)");
    model._set("A10", "=AGGREGATE(10, 4, D1:D3)");
    model._set("A11", "=AGGREGATE(11, 4, C1:C8)");
    model._set("A12", "=AGGREGATE(12, 4, B1:B7)");
    model._set("A13", "=AGGREGATE(13, 4, B1:B7)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"6"); // AVERAGE
    assert_eq!(model._get_text("A2"), *"6"); // COUNT
    assert_eq!(model._get_text("A3"), *"7"); // COUNTA
    assert_eq!(model._get_text("A4"), *"12"); // MAX
    assert_eq!(model._get_text("A5"), *"2"); // MIN
    assert_eq!(model._get_text("A6"), *"18432"); // PRODUCT
    assert_eq!(model._get_text("A7"), *"2"); // STDEV.S
    assert_eq!(model._get_text("A8"), *"2"); // STDEV.P
    assert_eq!(model._get_text("A9"), *"36"); // SUM
    assert_eq!(model._get_text("A10"), *"4"); // VAR.S
    assert_eq!(model._get_text("A11"), *"4"); // VAR.P
    assert_eq!(model._get_text("A12"), *"5"); // MEDIAN
    assert_eq!(model._get_text("A13"), *"4"); // MODE.SNGL
}

#[test]
fn fn_aggregate_k_functions() {
    let mut model = new_empty_model();

    // B1:B6 = 2, 4, 4, 6, 8, 12
    model._set("B1", "2");
    model._set("B2", "4");
    model._set("B3", "4");
    model._set("B4", "6");
    model._set("B5", "8");
    model._set("B6", "12");

    model._set("A1", "=AGGREGATE(14, 4, B1:B6, 2)");
    model._set("A2", "=AGGREGATE(15, 4, B1:B6, 2)");
    model._set("A3", "=AGGREGATE(16, 4, B1:B6, 0.5)");
    model._set("A4", "=AGGREGATE(17, 4, B1:B6, 1)");
    model._set("A5", "=AGGREGATE(18, 4, B1:B6, 0.5)");
    model._set("A6", "=AGGREGATE(19, 4, B1:B6, 1)");
    // Out of range k
    model._set("A7", "=AGGREGATE(14, 4, B1:B6, 7)");
    model._set("A8", "=AGGREGATE(16, 4, B1:B6, 1.5)");
    model._set("A9", "=AGGREGATE(19, 4, B1:B6, 4)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"8"); // LARGE
    assert_eq!(model._get_text("A2"), *"4"); // SMALL
    assert_eq!(model._get_text("A3"), *"5"); // PERCENTILE.INC
    assert_eq!(model._get_text("A4"), *"4"); // QUARTILE.INC
    assert_eq!(model._get_text("A5"), *"5"); // PERCENTILE.EXC
    assert_eq!(model._get_text("A6"), *"3.5"); // QUARTILE.EXC
    assert_eq!(model._get_text("A7"), *"#NUM!");
    assert_eq!(model._get_text("A8"), *"#NUM!");
    assert_eq!(model._get_text("A9"), *"#NUM!");
}

#[test]
fn fn_aggregate_ignore_errors() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "=1/0");
    model._set("B3", "3");

    model._set("A1", "=AGGREGATE(9, 4, B1:B3)");
    model._set("A2", "=AGGREGATE(9, 6, B1:B3)");
    model._set("A3", "=AGGREGATE(4, 6, B1:B3)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#DIV/0!");
    assert_eq!(model._get_text("A2"), *"4");
    assert_eq!(model._get_text("A3"), *"3");
}

#[test]
fn fn_aggregate_ignore_hidden_rows() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "2");
    model._set("B3", "3");
    model._set("B4", "4");

    model._set("A1", "=AGGREGATE(9, 4, B1:B4)");
    model._set("A2", "=AGGREGATE(9, 5, B1:B4)");
    model._set("A3", "=AGGREGATE(2, 5, B1:B4)");

    model.set_row_hidden(0, 2, true).unwrap();

    model.evaluate();

    // Option 4 does not ignore hidden rows, option 5 does
    assert_eq!(model._get_text("A1"), *"10");
    assert_eq!(model._get_text("A2"), *"8");
    assert_eq!(model._get_text("A3"), *"3");
}

#[test]
fn fn_aggregate_ignore_nested() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "2");
    model._set("B3", "=AGGREGATE(9, 4, B1:B2)");
    model._set("B4", "=SUBTOTAL(9, B1:B2)");

    model._set("A1", "=AGGREGATE(9, 0, B1:B4)");
    model._set("A2", "=AGGREGATE(9, 4, B1:B4)");

    model.evaluate();

    // Option 0 ignores nested AGGREGATE and SUBTOTAL results, option 4 does not
    assert_eq!(model._get_text("A1"), *"3");
    assert_eq!(model._get_text("A2"), *"9");
}

#[test]
fn fn_aggregate_direct_nested_call_is_value_error() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "2");

    // A direct SUBTOTAL/AGGREGATE call as a ref argument is not a reference:
    // Excel rejects it with #VALUE! (it is not silently skipped).
    model._set("A1", "=AGGREGATE(3, 0, SUBTOTAL(9, B1:B2))");
    model._set("A2", "=AGGREGATE(9, 0, AGGREGATE(9, 4, B1:B2))");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
}

#[test]
fn fn_aggregate_whole_column_range() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "2");
    model._set("B3", "3");

    // Open ranges are clamped to the sheet dimension (this would otherwise
    // walk all 1,048,576 rows — and, with option 5, scan the row-style list
    // for each of them).
    model._set("A1", "=AGGREGATE(9, 4, B:B)");
    model._set("A2", "=AGGREGATE(9, 5, B:B)");

    model.set_row_hidden(0, 2, true).unwrap();

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"6");
    assert_eq!(model._get_text("A2"), *"4");
}

#[test]
fn fn_aggregate_quartile_inc_negative_quart() {
    let mut model = new_empty_model();

    model._set("B1", "1");
    model._set("B2", "3");
    model._set("B3", "5");
    model._set("B4", "7");

    // quart in (-1, 0) truncates to -0.0; Excel returns #NUM! for any
    // negative quart (QUARTILE.INC agrees).
    model._set("A1", "=AGGREGATE(17, 4, B1:B4, -0.5)");
    model._set("A2", "=QUARTILE.INC(B1:B4, -0.5)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#NUM!");
    assert_eq!(model._get_text("A2"), *"#NUM!");
}

#[test]
fn fn_aggregate_product_no_values() {
    let mut model = new_empty_model();

    model._set("C1", "some text");

    // Excel returns 0 for a product over no numeric values, not the
    // empty-product identity 1.
    model._set("A1", "=AGGREGATE(6, 4, D1:D3)");
    model._set("A2", "=AGGREGATE(6, 4, C1)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"0");
    assert_eq!(model._get_text("A2"), *"0");
}
