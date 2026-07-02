#![allow(clippy::unwrap_used)]

use crate::model::Model;
use crate::test::util::new_empty_model;

// Shared fixture:
//   A: region, B: product, C: sales
//   North Apple  10
//   South Apple  20
//   North Banana 30
//   South Banana 40
//   North Apple  50
fn set_sales_data(model: &mut Model<'_>) {
    model._set("A1", "North");
    model._set("B1", "Apple");
    model._set("C1", "10");
    model._set("A2", "South");
    model._set("B2", "Apple");
    model._set("C2", "20");
    model._set("A3", "North");
    model._set("B3", "Banana");
    model._set("C3", "30");
    model._set("A4", "South");
    model._set("B4", "Banana");
    model._set("C4", "40");
    model._set("A5", "North");
    model._set("B5", "Apple");
    model._set("C5", "50");
}

// ── GROUPBY ───────────────────────────────────────────────────────────────────

#[test]
fn fn_groupby_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=GROUPBY()");
    model._set("A2", "=GROUPBY(B1:B2)");
    model._set("A3", "=GROUPBY(B1:B2, C1:C2)");
    model._set("A4", "=GROUPBY(B1:B2, C1:C2, SUM, 0, 1, 1, D1:D2, 0, 1)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
    assert_eq!(model._get_text("A4"), *"#ERROR!");
}

#[test]
fn fn_groupby_sum_single_field() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM)");
    model.evaluate();

    // Groups sorted ascending; grand total at the bottom (default depth 1).
    assert_eq!(model._get_text("E1"), *"North");
    assert_eq!(model._get_text("F1"), *"90");
    assert_eq!(model._get_text("E2"), *"South");
    assert_eq!(model._get_text("F2"), *"60");
    assert_eq!(model._get_text("E3"), *"Total");
    assert_eq!(model._get_text("F3"), *"150");
    assert_eq!(model._get_text("E4"), *"");
}

#[test]
fn fn_groupby_eta_functions() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // No totals to keep the grids small.
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, AVERAGE, 0, 0)");
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, COUNT, 0, 0)");
    model._set("K1", "=GROUPBY(A1:A5, C1:C5, COUNTA, 0, 0)");
    model._set("N1", "=GROUPBY(A1:A5, C1:C5, MAX, 0, 0)");
    model._set("Q1", "=GROUPBY(A1:A5, C1:C5, MIN, 0, 0)");
    model._set("T1", "=GROUPBY(A1:A5, C1:C5, MEDIAN, 0, 0)");
    model._set("W1", "=GROUPBY(A1:A5, C1:C5, PRODUCT, 0, 0)");
    model.evaluate();

    // North: 10, 30, 50; South: 20, 40
    assert_eq!(model._get_text("F1"), *"30"); // AVERAGE North
    assert_eq!(model._get_text("F2"), *"30"); // AVERAGE South
    assert_eq!(model._get_text("I1"), *"3"); // COUNT North
    assert_eq!(model._get_text("I2"), *"2"); // COUNT South
    assert_eq!(model._get_text("L1"), *"3"); // COUNTA North
    assert_eq!(model._get_text("O1"), *"50"); // MAX North
    assert_eq!(model._get_text("O2"), *"40"); // MAX South
    assert_eq!(model._get_text("R1"), *"10"); // MIN North
    assert_eq!(model._get_text("R2"), *"20"); // MIN South
    assert_eq!(model._get_text("U1"), *"30"); // MEDIAN North
    assert_eq!(model._get_text("U2"), *"30"); // MEDIAN South
    assert_eq!(model._get_text("X1"), *"15000"); // PRODUCT North
    assert_eq!(model._get_text("X2"), *"800"); // PRODUCT South
}

#[test]
fn fn_groupby_percentof() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, PERCENTOF)");
    model.evaluate();

    // North 90/150, South 60/150, Total 150/150.
    assert_eq!(model._get_text("F1"), *"0.6");
    assert_eq!(model._get_text("F2"), *"0.4");
    assert_eq!(model._get_text("E3"), *"Total");
    assert_eq!(model._get_text("F3"), *"1");
}

#[test]
fn fn_groupby_multi_field_subtotals() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:B5, C1:C5, SUM, 0, 2)");
    model.evaluate();

    // North Apple 60 / North Banana 30 / North Total 90
    // South Apple 20 / South Banana 40 / South Total 60 / Total 150
    assert_eq!(model._get_text("E1"), *"North");
    assert_eq!(model._get_text("F1"), *"Apple");
    assert_eq!(model._get_text("G1"), *"60");
    assert_eq!(model._get_text("E2"), *"North");
    assert_eq!(model._get_text("F2"), *"Banana");
    assert_eq!(model._get_text("G2"), *"30");
    assert_eq!(model._get_text("E3"), *"North Total");
    assert_eq!(model._get_text("F3"), *"");
    assert_eq!(model._get_text("G3"), *"90");
    assert_eq!(model._get_text("E4"), *"South");
    assert_eq!(model._get_text("F4"), *"Apple");
    assert_eq!(model._get_text("G4"), *"20");
    assert_eq!(model._get_text("E5"), *"South");
    assert_eq!(model._get_text("F5"), *"Banana");
    assert_eq!(model._get_text("G5"), *"40");
    assert_eq!(model._get_text("E6"), *"South Total");
    assert_eq!(model._get_text("G6"), *"60");
    assert_eq!(model._get_text("E7"), *"Total");
    assert_eq!(model._get_text("G7"), *"150");
}

#[test]
fn fn_groupby_totals_on_top() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:B5, C1:C5, SUM, 0, -2)");
    model.evaluate();

    // Grand total first, subtotals above their groups.
    assert_eq!(model._get_text("E1"), *"Total");
    assert_eq!(model._get_text("G1"), *"150");
    assert_eq!(model._get_text("E2"), *"North Total");
    assert_eq!(model._get_text("G2"), *"90");
    assert_eq!(model._get_text("E3"), *"North");
    assert_eq!(model._get_text("F3"), *"Apple");
    assert_eq!(model._get_text("G3"), *"60");
    assert_eq!(model._get_text("E5"), *"South Total");
    assert_eq!(model._get_text("E6"), *"South");
    assert_eq!(model._get_text("F6"), *"Apple");
}

#[test]
fn fn_groupby_no_totals() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"North");
    assert_eq!(model._get_text("E2"), *"South");
    assert_eq!(model._get_text("E3"), *"");
}

#[test]
fn fn_groupby_sort_order() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // Descending by the first (and only) row field.
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0, -1)");
    // Ascending by the aggregate column (index 2 = fields + 1).
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0, 2)");
    // Descending by the aggregate column.
    model._set("K1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0, -2)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"South");
    assert_eq!(model._get_text("E2"), *"North");

    // South 60 < North 90
    assert_eq!(model._get_text("H1"), *"South");
    assert_eq!(model._get_text("H2"), *"North");

    assert_eq!(model._get_text("K1"), *"North");
    assert_eq!(model._get_text("K2"), *"South");
}

#[test]
fn fn_groupby_sort_order_out_of_range() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0, 3)");
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 0, 0)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#VALUE!");
    assert_eq!(model._get_text("H1"), *"#VALUE!");
}

#[test]
fn fn_groupby_filter_array() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // Keep only rows with sales > 25 (rows 3, 4, 5).
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 1, 1, C1:C5>25)");
    model.evaluate();

    // North: 30 + 50 = 80; South: 40.
    assert_eq!(model._get_text("E1"), *"North");
    assert_eq!(model._get_text("F1"), *"80");
    assert_eq!(model._get_text("E2"), *"South");
    assert_eq!(model._get_text("F2"), *"40");
    assert_eq!(model._get_text("E3"), *"Total");
    assert_eq!(model._get_text("F3"), *"120");
}

#[test]
fn fn_groupby_filter_array_errors() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // Length mismatch.
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 1, 1, C1:C3>25)");
    // Nothing left after filtering.
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 1, 1, C1:C5>100)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#VALUE!");
    assert_eq!(model._get_text("H1"), *"#CALC!");
}

#[test]
fn fn_groupby_lambda() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, LAMBDA(x, SUM(x)*2), 0, 0)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"North");
    assert_eq!(model._get_text("F1"), *"180");
    assert_eq!(model._get_text("E2"), *"South");
    assert_eq!(model._get_text("F2"), *"120");
}

#[test]
fn fn_groupby_two_parameter_lambda() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // A hand-written PERCENTOF: the second parameter receives the grand set.
    model._set(
        "E1",
        "=GROUPBY(A1:A5, C1:C5, LAMBDA(x, y, SUM(x)/SUM(y)), 0, 0)",
    );
    model.evaluate();

    assert_eq!(model._get_text("F1"), *"0.6");
    assert_eq!(model._get_text("F2"), *"0.4");
}

#[test]
fn fn_groupby_headers_inferred_and_shown() {
    let mut model = new_empty_model();
    model._set("A1", "Region");
    model._set("B1", "Sales");
    model._set("A2", "North");
    model._set("B2", "10");
    model._set("A3", "South");
    model._set("B3", "20");
    model._set("A4", "North");
    model._set("B4", "30");

    // Headers inferred (text over numbers): stripped, not shown.
    model._set("D1", "=GROUPBY(A1:A4, B1:B4, SUM, , 0)");
    // Headers declared and shown.
    model._set("G1", "=GROUPBY(A1:A4, B1:B4, SUM, 3, 0)");
    // Headers declared but hidden.
    model._set("J1", "=GROUPBY(A1:A4, B1:B4, SUM, 1, 0)");
    model.evaluate();

    assert_eq!(model._get_text("D1"), *"North");
    assert_eq!(model._get_text("E1"), *"40");
    assert_eq!(model._get_text("D2"), *"South");
    assert_eq!(model._get_text("E2"), *"20");

    assert_eq!(model._get_text("G1"), *"Region");
    assert_eq!(model._get_text("H1"), *"Sales");
    assert_eq!(model._get_text("G2"), *"North");
    assert_eq!(model._get_text("H2"), *"40");

    assert_eq!(model._get_text("J1"), *"North");
    assert_eq!(model._get_text("K1"), *"40");
}

#[test]
fn fn_groupby_no_headers_all_text_data() {
    let mut model = new_empty_model();
    // All-text keys over numbers, but the first values cell is a number:
    // no headers inferred, all four rows are data.
    model._set("A1", "a");
    model._set("B1", "1");
    model._set("A2", "b");
    model._set("B2", "2");
    model._set("A3", "a");
    model._set("B3", "3");
    model._set("D1", "=GROUPBY(A1:A3, B1:B3, SUM, , 0)");
    model.evaluate();

    assert_eq!(model._get_text("D1"), *"a");
    assert_eq!(model._get_text("E1"), *"4");
    assert_eq!(model._get_text("D2"), *"b");
    assert_eq!(model._get_text("E2"), *"2");
}

#[test]
fn fn_groupby_multiple_value_columns() {
    let mut model = new_empty_model();
    model._set("A1", "x");
    model._set("B1", "1");
    model._set("C1", "10");
    model._set("A2", "y");
    model._set("B2", "2");
    model._set("C2", "20");
    model._set("A3", "x");
    model._set("B3", "3");
    model._set("C3", "30");
    model._set("E1", "=GROUPBY(A1:A3, B1:C3, SUM, 0, 1)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"x");
    assert_eq!(model._get_text("F1"), *"4");
    assert_eq!(model._get_text("G1"), *"40");
    assert_eq!(model._get_text("E2"), *"y");
    assert_eq!(model._get_text("F2"), *"2");
    assert_eq!(model._get_text("G2"), *"20");
    assert_eq!(model._get_text("E3"), *"Total");
    assert_eq!(model._get_text("F3"), *"6");
    assert_eq!(model._get_text("G3"), *"60");
}

#[test]
fn fn_groupby_gated_options() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // field_headers 2 (generate) is not implemented.
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, SUM, 2)");
    // field_relationship 1 (table) is not implemented.
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, SUM, 0, 1, 1, , 1)");
    // Eta-reduced CONCAT is not implemented.
    model._set("K1", "=GROUPBY(A1:A5, C1:C5, CONCAT)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#VALUE!");
    assert_eq!(model._get_text("H1"), *"#VALUE!");
    assert_eq!(model._get_text("K1"), *"#VALUE!");
}

#[test]
fn fn_groupby_invalid_function_argument() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // An unknown bare name is #NAME?; a scalar is #VALUE!.
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, NOTAFUNCTION)");
    model._set("H1", "=GROUPBY(A1:A5, C1:C5, 7)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#NAME?");
    assert_eq!(model._get_text("H1"), *"#VALUE!");
}

#[test]
fn fn_groupby_row_count_mismatch() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C4, SUM)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#VALUE!");
}

#[test]
fn fn_groupby_numeric_keys() {
    let mut model = new_empty_model();
    model._set("A1", "3");
    model._set("B1", "1");
    model._set("A2", "1");
    model._set("B2", "2");
    model._set("A3", "3");
    model._set("B3", "4");
    model._set("D1", "=GROUPBY(A1:A3, B1:B3, SUM, 0, 1)");
    model.evaluate();

    assert_eq!(model._get_text("D1"), *"1");
    assert_eq!(model._get_text("E1"), *"2");
    assert_eq!(model._get_text("D2"), *"3");
    assert_eq!(model._get_text("E2"), *"5");
    // Numeric key in the grand total label.
    assert_eq!(model._get_text("D3"), *"Total");
    assert_eq!(model._get_text("E3"), *"7");
}

#[test]
fn fn_groupby_case_insensitive_grouping() {
    let mut model = new_empty_model();
    model._set("A1", "north");
    model._set("B1", "1");
    model._set("A2", "NORTH");
    model._set("B2", "2");
    model._set("D1", "=GROUPBY(A1:A2, B1:B2, SUM, 0, 0)");
    model.evaluate();

    // One group, keeping the first spelling.
    assert_eq!(model._get_text("D1"), *"north");
    assert_eq!(model._get_text("E1"), *"3");
    assert_eq!(model._get_text("D2"), *"");
}

// ── PIVOTBY ───────────────────────────────────────────────────────────────────

#[test]
fn fn_pivotby_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=PIVOTBY()");
    model._set("A2", "=PIVOTBY(B1:B2, C1:C2, D1:D2)");
    model._set(
        "A3",
        "=PIVOTBY(B1:B2, C1:C2, D1:D2, SUM, 0, 1, 1, 1, 1, E1:E2, 0, 9)",
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
}

#[test]
fn fn_pivotby_sum() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM)");
    model.evaluate();

    //         Apple  Banana  Total
    // North   60     30      90
    // South   20     40      60
    // Total   80     70      150
    assert_eq!(model._get_text("E1"), *"");
    assert_eq!(model._get_text("F1"), *"Apple");
    assert_eq!(model._get_text("G1"), *"Banana");
    assert_eq!(model._get_text("H1"), *"Total");
    assert_eq!(model._get_text("E2"), *"North");
    assert_eq!(model._get_text("F2"), *"60");
    assert_eq!(model._get_text("G2"), *"30");
    assert_eq!(model._get_text("H2"), *"90");
    assert_eq!(model._get_text("E3"), *"South");
    assert_eq!(model._get_text("F3"), *"20");
    assert_eq!(model._get_text("G3"), *"40");
    assert_eq!(model._get_text("H3"), *"60");
    assert_eq!(model._get_text("E4"), *"Total");
    assert_eq!(model._get_text("F4"), *"80");
    assert_eq!(model._get_text("G4"), *"70");
    assert_eq!(model._get_text("H4"), *"150");
}

#[test]
fn fn_pivotby_empty_intersection() {
    let mut model = new_empty_model();
    // x/a and y/b only: the x/b and y/a intersections are empty.
    model._set("A1", "x");
    model._set("B1", "a");
    model._set("C1", "5");
    model._set("A2", "y");
    model._set("B2", "b");
    model._set("C2", "7");
    model._set("E1", "=PIVOTBY(A1:A2, B1:B2, C1:C2, SUM, 0, 0, , 0)");
    model._set("J1", "=PIVOTBY(A1:A2, B1:B2, C1:C2, AVERAGE, 0, 0, , 0)");
    model.evaluate();

    // SUM over an empty subset is 0.
    assert_eq!(model._get_text("F2"), *"5");
    assert_eq!(model._get_text("G2"), *"0");
    assert_eq!(model._get_text("F3"), *"0");
    assert_eq!(model._get_text("G3"), *"7");
    // AVERAGE over an empty subset is #DIV/0!.
    assert_eq!(model._get_text("L2"), *"#DIV/0!");
}

#[test]
fn fn_pivotby_no_totals_and_sort() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // No totals, rows descending, columns descending.
    model._set("E1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM, 0, 0, -1, 0, -1)");
    model.evaluate();

    //         Banana  Apple
    // South   40      20
    // North   30      60
    assert_eq!(model._get_text("F1"), *"Banana");
    assert_eq!(model._get_text("G1"), *"Apple");
    assert_eq!(model._get_text("E2"), *"South");
    assert_eq!(model._get_text("F2"), *"40");
    assert_eq!(model._get_text("G2"), *"20");
    assert_eq!(model._get_text("E3"), *"North");
    assert_eq!(model._get_text("F3"), *"30");
    assert_eq!(model._get_text("G3"), *"60");
    assert_eq!(model._get_text("E4"), *"");
}

#[test]
fn fn_pivotby_percentof_relative_to() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // Default relative_to 0: share of the column total.
    model._set("E1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, PERCENTOF, 0, 0, , 0)");
    // relative_to 1: share of the row total.
    model._set(
        "J1",
        "=PIVOTBY(A1:A5, B1:B5, C1:C5, PERCENTOF, 0, 0, , 0, , , 1)",
    );
    // relative_to 2: share of the grand total.
    model._set(
        "O1",
        "=PIVOTBY(A1:A5, B1:B5, C1:C5, PERCENTOF, 0, 0, , 0, , , 2)",
    );
    // Round the non-exact row-total shares.
    model._set("A7", "=ROUND(K2, 4)"); // North Apple 60/90
    model._set("A8", "=ROUND(L3, 4)"); // South Banana 40/60
    model.evaluate();

    // Column totals: Apple 80, Banana 70.
    assert_eq!(model._get_text("F2"), *"0.75"); // North Apple 60/80
    assert_eq!(model._get_text("F3"), *"0.25"); // South Apple 20/80
                                                // Row totals: North 90, South 60.
    assert_eq!(model._get_text("A7"), *"0.6667");
    assert_eq!(model._get_text("A8"), *"0.6667");
    // Grand total 150.
    assert_eq!(model._get_text("P2"), *"0.4"); // North Apple 60/150
    assert_eq!(model._get_text("Q2"), *"0.2"); // North Banana 30/150
}

#[test]
fn fn_pivotby_percentof_parent_totals() {
    let mut model = new_empty_model();
    // rows: x; cols: (a,s) 1, (a,l) 3, (b,s) 4
    model._set("A1", "x");
    model._set("B1", "a");
    model._set("C1", "s");
    model._set("D1", "1");
    model._set("A2", "x");
    model._set("B2", "a");
    model._set("C2", "l");
    model._set("D2", "3");
    model._set("A3", "x");
    model._set("B3", "b");
    model._set("C3", "s");
    model._set("D3", "4");
    // relative_to 3: share of the parent column total.
    model._set(
        "F1",
        "=PIVOTBY(A1:A3, B1:C3, D1:D3, PERCENTOF, 0, 0, , 0, , , 3)",
    );
    model.evaluate();

    // Columns sorted: (a,l), (a,s), (b,s). Parent of (a,*) totals 4.
    assert_eq!(model._get_text("G3"), *"0.75"); // (a,l): 3/4
    assert_eq!(model._get_text("H3"), *"0.25"); // (a,s): 1/4
    assert_eq!(model._get_text("I3"), *"1"); // (b,s): 4/4
}

#[test]
fn fn_pivotby_multi_column_fields() {
    let mut model = new_empty_model();
    // col fields: (product, size)
    model._set("A1", "x");
    model._set("B1", "a");
    model._set("C1", "s");
    model._set("D1", "1");
    model._set("A2", "x");
    model._set("B2", "a");
    model._set("C2", "l");
    model._set("D2", "2");
    model._set("A3", "x");
    model._set("B3", "b");
    model._set("C3", "s");
    model._set("D3", "4");
    model._set("F1", "=PIVOTBY(A1:A3, B1:C3, D1:D3, SUM, 0, 0, , 2)");
    model.evaluate();

    // Header rows (two column fields) with per-group subtotal columns:
    //        a      a      a Total  b      b Total  Total
    //        l      s               s
    // x      2      1      3        4      4        7
    assert_eq!(model._get_text("G1"), *"a");
    assert_eq!(model._get_text("G2"), *"l");
    assert_eq!(model._get_text("H1"), *"a");
    assert_eq!(model._get_text("H2"), *"s");
    assert_eq!(model._get_text("I1"), *"a Total");
    assert_eq!(model._get_text("I2"), *"");
    assert_eq!(model._get_text("J1"), *"b");
    assert_eq!(model._get_text("J2"), *"s");
    assert_eq!(model._get_text("K1"), *"b Total");
    assert_eq!(model._get_text("L1"), *"Total");
    assert_eq!(model._get_text("F3"), *"x");
    assert_eq!(model._get_text("G3"), *"2");
    assert_eq!(model._get_text("H3"), *"1");
    assert_eq!(model._get_text("I3"), *"3");
    assert_eq!(model._get_text("J3"), *"4");
    assert_eq!(model._get_text("K3"), *"4");
    assert_eq!(model._get_text("L3"), *"7");
}

#[test]
fn fn_pivotby_filter_array() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // Keep rows with sales > 25 (rows 3, 4, 5).
    model._set(
        "E1",
        "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM, 0, 1, , 1, , C1:C5>25)",
    );
    model.evaluate();

    //         Apple  Banana  Total
    // North   50     30      80
    // South   0      40      40
    // Total   50     70      120
    assert_eq!(model._get_text("F2"), *"50");
    assert_eq!(model._get_text("G2"), *"30");
    assert_eq!(model._get_text("H2"), *"80");
    assert_eq!(model._get_text("F3"), *"0");
    assert_eq!(model._get_text("G3"), *"40");
    assert_eq!(model._get_text("H4"), *"120");
}

#[test]
fn fn_pivotby_gated_options() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    // field_headers 2 and 3 are not implemented in PIVOTBY.
    model._set("E1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM, 2)");
    model._set("H1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM, 3)");
    // Sorting by aggregate columns is not implemented in PIVOTBY.
    model._set("K1", "=PIVOTBY(A1:A5, B1:B5, C1:C5, SUM, 0, 1, 2)");
    // More than one values column is not implemented.
    model._set("N1", "=PIVOTBY(A1:A5, B1:B5, B1:C5, SUM)");
    // relative_to out of range.
    model._set(
        "Q1",
        "=PIVOTBY(A1:A5, B1:B5, C1:C5, PERCENTOF, 0, 1, , 1, , , 5)",
    );
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"#VALUE!");
    assert_eq!(model._get_text("H1"), *"#VALUE!");
    assert_eq!(model._get_text("K1"), *"#VALUE!");
    assert_eq!(model._get_text("N1"), *"#VALUE!");
    assert_eq!(model._get_text("Q1"), *"#VALUE!");
}

#[test]
fn fn_pivotby_lambda() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set(
        "E1",
        "=PIVOTBY(A1:A5, B1:B5, C1:C5, LAMBDA(x, MAX(x)), 0, 0, , 0)",
    );
    model.evaluate();

    assert_eq!(model._get_text("F2"), *"50"); // North Apple: max(10, 50)
    assert_eq!(model._get_text("G3"), *"40"); // South Banana
}

#[test]
fn fn_groupby_formula_roundtrip() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5,C1:C5,SUM)");
    model.evaluate();

    // The eta-reduced function name survives formula serialization.
    assert_eq!(model._get_formula("E1"), *"=GROUPBY(A1:A5,C1:C5,SUM)");
    assert_eq!(model._get_text("F1"), *"90");
}

#[test]
fn fn_groupby_lowercase_eta_name() {
    let mut model = new_empty_model();
    set_sales_data(&mut model);
    model._set("E1", "=GROUPBY(A1:A5, C1:C5, sum, 0, 0)");
    model.evaluate();

    assert_eq!(model._get_text("F1"), *"90");
    assert_eq!(model._get_text("F2"), *"60");
}

#[test]
fn fn_groupby_negative_zero_groups_with_zero() {
    let mut model = new_empty_model();
    // 0.0 and -0.0 have different f64 bit patterns but are the same Excel
    // value: they must form a single group.
    model._set("A1", "=0*1");
    model._set("A2", "=0*-1");
    model._set("B1", "1");
    model._set("B2", "2");
    model._set("E1", "=GROUPBY(A1:A2, B1:B2, SUM, 0, 0)");
    model.evaluate();

    assert_eq!(model._get_text("E1"), *"0");
    assert_eq!(model._get_text("F1"), *"3");
    // A second "0" row would mean the zeros were grouped separately.
    assert_eq!(model._get_text("E2"), *"");
}

#[test]
fn fn_groupby_empty_key_subtotal_label_distinct_from_grand_total() {
    let mut model = new_empty_model();
    // A1 evaluates to "" — its subtotal label must not collide with the
    // grand-total row's bare "Total".
    model._set("A1", "=\"\"");
    model._set("B1", "x");
    model._set("C1", "1");
    model._set("A2", "West");
    model._set("B2", "y");
    model._set("C2", "2");
    model._set("E1", "=GROUPBY(A1:B2, C1:C2, SUM, 0, 2)");
    model.evaluate();

    // Rows: ["", x, 1] / [" Total", , 1] / [West, y, 2] /
    //       [West Total, , 2] / [Total, , 3]
    assert_eq!(model._get_text("E2"), *" Total");
    assert_eq!(model._get_text("G2"), *"1");
    assert_eq!(model._get_text("E4"), *"West Total");
    assert_eq!(model._get_text("G4"), *"2");
    assert_eq!(model._get_text("E5"), *"Total");
    assert_eq!(model._get_text("G5"), *"3");
}
