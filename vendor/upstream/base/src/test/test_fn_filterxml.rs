#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn fn_filterxml_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=FILTERXML()");
    model._set("A2", r#"=FILTERXML("<a/>")"#);
    model._set("A3", r#"=FILTERXML("<a/>", "//a", 1)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
}

#[test]
fn fn_filterxml_single_match() {
    let mut model = new_empty_model();

    model._set(
        "A1",
        r#"=FILTERXML("<order><id>A-1</id><total>26</total></order>", "/order/id")"#,
    );
    // Numeric text comes back as a number
    model._set(
        "A2",
        r#"=FILTERXML("<order><id>A-1</id><total>26</total></order>", "/order/total")*2"#,
    );
    // The string value of an element concatenates nested text
    model._set("A3", r#"=FILTERXML("<a>he<i>l</i>lo</a>", "/a")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"A-1");
    assert_eq!(model._get_text("A2"), *"52");
    assert_eq!(model._get_text("A3"), *"hello");
}

#[test]
fn fn_filterxml_multiple_matches_spill_vertically() {
    let mut model = new_empty_model();

    model._set(
        "A1",
        r#"=FILTERXML("<list><v>10</v><v>twenty</v><v>30</v></list>", "//v")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"10");
    assert_eq!(model._get_text("A2"), *"twenty");
    assert_eq!(model._get_text("A3"), *"30");
    // Vertical spill: one column, nothing to the right
    assert_eq!(model._get_text("B1"), *"");
    assert_eq!(model._get_text("A4"), *"");
}

#[test]
fn fn_filterxml_attributes() {
    let mut model = new_empty_model();

    model._set(
        "A1",
        r#"=FILTERXML("<a><b id='p1'>x</b><b id='p2'>y</b></a>", "//b/@id")"#,
    );
    model._set(
        "C1",
        r#"=FILTERXML("<a><b id='p1'>x</b><b id='p2'>y</b></a>", "/a/b[2]/@id")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"p1");
    assert_eq!(model._get_text("A2"), *"p2");
    assert_eq!(model._get_text("C1"), *"p2");
}

#[test]
fn fn_filterxml_positional_predicates() {
    let mut model = new_empty_model();

    model._set(
        "A1",
        r#"=FILTERXML("<a><b>1</b><b>2</b><b>3</b></a>", "/a/b[2]")"#,
    );
    model._set(
        "A2",
        r#"=FILTERXML("<a><b>1</b><b>2</b><b>3</b></a>", "/a/b[last()]")"#,
    );
    // Out of range position -> no match
    model._set(
        "A3",
        r#"=FILTERXML("<a><b>1</b><b>2</b><b>3</b></a>", "/a/b[4]")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"2");
    assert_eq!(model._get_text("A2"), *"3");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
}

#[test]
fn fn_filterxml_positions_are_relative_to_each_parent() {
    let mut model = new_empty_model();

    // XPath semantics: //b[1] is the first b of *each* parent
    model._set(
        "A1",
        r#"=FILTERXML("<r><g><b>1</b><b>2</b></g><g><b>3</b></g></r>", "//b[1]")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"1");
    assert_eq!(model._get_text("A2"), *"3");
}

#[test]
fn fn_filterxml_text_node_test() {
    let mut model = new_empty_model();

    model._set("A1", r#"=FILTERXML("<a>hi<b>x</b></a>", "/a/text()")"#);
    model._set(
        "A2",
        r#"=FILTERXML("<a>hi<b>x</b>bye</a>", "/a/text()[2]")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"hi");
    assert_eq!(model._get_text("A2"), *"bye");
}

#[test]
fn fn_filterxml_boolean_predicates() {
    let mut model = new_empty_model();

    let xml = "<a><b t='x'>alpha</b><b t='y'>beta</b><b t='y'>alphabet</b></a>";
    model._set(
        "A1",
        &format!(r#"=FILTERXML("{xml}", "//b[contains(text(),'et')]")"#),
    );
    model._set(
        "B1",
        &format!(r#"=FILTERXML("{xml}", "//b[starts-with(text(),'alpha')]")"#),
    );
    model._set(
        "C1",
        &format!(r#"=FILTERXML("{xml}", "//b[not(contains(@t,'y'))]")"#),
    );
    model._set(
        "D1",
        &format!(r#"=FILTERXML("{xml}", "//b[contains(@t,'y')][last()]")"#),
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"beta");
    assert_eq!(model._get_text("A2"), *"alphabet");
    assert_eq!(model._get_text("B1"), *"alpha");
    assert_eq!(model._get_text("B2"), *"alphabet");
    assert_eq!(model._get_text("C1"), *"alpha");
    assert_eq!(model._get_text("D1"), *"alphabet");
}

#[test]
fn fn_filterxml_multibyte_content() {
    let mut model = new_empty_model();

    model._set(
        "A1",
        r#"=FILTERXML("<品目><名前>りんご</名前><名前>みかん</名前></品目>", "/品目/名前")"#,
    );
    model._set(
        "B1",
        r#"=FILTERXML("<a><b>ünïcode·€</b></a>", "//b[contains(text(),'€')]")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"りんご");
    assert_eq!(model._get_text("A2"), *"みかん");
    assert_eq!(model._get_text("B1"), *"ünïcode·€");
}

#[test]
fn fn_filterxml_namespaces() {
    let mut model = new_empty_model();

    let xml = "<r xmlns:m='urn:x'><m:a>1</m:a><a>2</a></r>";
    model._set("A1", &format!(r#"=FILTERXML("{xml}", "//m:a")"#));
    // An unprefixed test does not match namespaced elements
    model._set("A2", &format!(r#"=FILTERXML("{xml}", "//a")"#));
    // A prefix that is not declared in the document
    model._set("A3", &format!(r#"=FILTERXML("{xml}", "//q:a")"#));
    // An undeclared prefix in the XML itself is invalid XML
    model._set("A4", r#"=FILTERXML("<r><q:a>1</q:a></r>", "//r")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"1");
    assert_eq!(model._get_text("A2"), *"2");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
}

#[test]
fn fn_filterxml_errors() {
    let mut model = new_empty_model();

    // Invalid XML
    model._set("A1", r#"=FILTERXML("<a><b></a>", "//b")"#);
    // No match
    model._set("A2", r#"=FILTERXML("<a><b>1</b></a>", "/a/c")"#);
    // Bad xpaths
    model._set("A3", r#"=FILTERXML("<a><b>1</b></a>", "b")"#);
    model._set(
        "A4",
        r#"=FILTERXML("<a><b>1</b></a>", "//b[position()=1]")"#,
    );
    model._set("A5", r#"=FILTERXML("<a><b>1</b></a>", "//b/@id/x")"#);
    model._set("A6", r#"=FILTERXML("<a><b>1</b></a>", "")"#);
    // Errors in arguments propagate
    model._set("A7", r#"=FILTERXML(1/0, "//b")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
    assert_eq!(model._get_text("A5"), *"#VALUE!");
    assert_eq!(model._get_text("A6"), *"#VALUE!");
    assert_eq!(model._get_text("A7"), *"#DIV/0!");
}

#[test]
fn fn_filterxml_descendant_shorthand_midpath() {
    let mut model = new_empty_model();

    let xml = "<r><x><g><v>1</v></g></x><g><v>2</v></g></r>";
    model._set("A1", &format!(r#"=FILTERXML("{xml}", "/r//g/v")"#));

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"1");
    assert_eq!(model._get_text("A2"), *"2");
}

#[test]
fn fn_filterxml_to_xlsx_string() {
    let mut model = new_empty_model();

    model._set("A1", r#"=FILTERXML("<a><b>1</b></a>", "//b")"#);

    model.evaluate();

    assert_eq!(
        model._get_formula("A1"),
        *r#"=FILTERXML("<a><b>1</b></a>","//b")"#
    );
}
