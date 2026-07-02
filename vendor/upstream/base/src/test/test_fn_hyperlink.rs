#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn fn_hyperlink_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=HYPERLINK()");
    model._set("A2", r#"=HYPERLINK("http://example.com", "a", "b")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_hyperlink() {
    let mut model = new_empty_model();

    model._set("A1", r#"=HYPERLINK("http://example.com")"#);
    model._set("A2", r#"=HYPERLINK("http://example.com", "Example")"#);
    model._set("A3", r#"=HYPERLINK("http://example.com", 42)"#);
    model._set("A4", r#"=HYPERLINK("http://example.com", TRUE)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"http://example.com");
    assert_eq!(model._get_text("A2"), *"Example");
    // The friendly name keeps its type
    assert_eq!(model._get_text("A3"), *"42");
    assert_eq!(model._get_text("A4"), *"TRUE");
}

#[test]
fn fn_hyperlink_references() {
    let mut model = new_empty_model();

    model._set("B1", "http://example.com");
    model._set("B2", "Example");

    model._set("A1", "=HYPERLINK(B1)");
    model._set("A2", "=HYPERLINK(B1, B2)");
    model._set("A3", "=HYPERLINK(B1, B3)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"http://example.com");
    assert_eq!(model._get_text("A2"), *"Example");
    // An empty friendly name displays as 0
    assert_eq!(model._get_text("A3"), *"0");
}

#[test]
fn fn_hyperlink_errors_propagate() {
    let mut model = new_empty_model();

    model._set("A1", r#"=HYPERLINK(1/0, "Example")"#);
    model._set("A2", r#"=HYPERLINK("http://example.com", 1/0)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#DIV/0!");
    assert_eq!(model._get_text("A2"), *"#DIV/0!");
}
