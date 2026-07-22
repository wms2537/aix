#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn fn_concatenate_args_number() {
    let mut model = new_empty_model();
    model._set("A1", "=CONCATENATE()");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
}

#[test]
fn number_to_text_coercion_uses_excel_general_precision() {
    // Coercing a fractional number to text (`&`, CONCATENATE) must render at Excel's General
    // precision (15 significant figures), not the raw f64 repr: `="" & (0.1+0.2)` is "0.3", not
    // "0.30000000000000004". (A raw-repr coercion diverged from Excel and broke certify's oracle.)
    let mut model = new_empty_model();
    model._set("A1", r#"="" & (0.1+0.2)"#);
    model._set("A2", r#"="rate=" & (1/3)"#);
    model._set("A3", r#"=CONCATENATE("", 0.1+0.2)"#);
    model._set("A4", r#"="" & 0.0000001"#);
    model._set("A5", r#"="" & 12345.678"#);
    model.evaluate();
    assert_eq!(model._get_text("A1"), *"0.3");
    assert_eq!(model._get_text("A2"), *"rate=0.333333333333333");
    assert_eq!(model._get_text("A3"), *"0.3");
    // Small magnitudes render in FIXED notation (not "1e-7"), at 15 significant figures.
    assert_eq!(model._get_text("A4"), *"0.0000001");
    assert_eq!(model._get_text("A5"), *"12345.678");
}

#[test]
fn fn_concatenate() {
    let mut model = new_empty_model();
    model._set("A1", "Hello");
    model._set("A2", " my ");
    model._set("A3", "World");

    model._set("B1", r#"=CONCATENATE(A1, A2, A3, "!")"#);
    // This will break once we implement the implicit intersection operator
    // It should be:
    model._set("C2", r#"=CONCATENATE(@A1:A3, "!")"#);
    model._set("B2", r#"=CONCATENATE(A1:A3, "!")"#);
    model._set("B3", r#"=CONCAT(A1:A3, "!")"#);

    model.evaluate();

    assert_eq!(model._get_text("B1"), *"Hello my World!");
    assert_eq!(model._get_text("B2"), *"#N/IMPL!");
    assert_eq!(model._get_text("B3"), *"Hello my World!");
    assert_eq!(model._get_text("C2"), *" my !");
}
