#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

// WEBSERVICE

#[test]
fn fn_webservice_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=WEBSERVICE()");
    model._set("A2", r#"=WEBSERVICE("https://example.com", "b")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_webservice_invalid_url() {
    let mut model = new_empty_model();

    // longer than 2048 characters
    model._set("A1", r#"=WEBSERVICE("https://"&REPT("a", 2048))"#);
    // not http(s)
    model._set("A2", r#"=WEBSERVICE("ftp://example.com/data")"#);
    model._set("A3", r#"=WEBSERVICE("example.com")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
}

#[test]
fn fn_webservice_literal() {
    let mut model = new_empty_model();

    // Excel returns #VALUE! for every failure to fetch, including offline
    model._set("A1", r#"=WEBSERVICE("https://example.com/api")"#);
    model._set("A2", r#"=WEBSERVICE("HTTP://EXAMPLE.COM")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
}

#[test]
fn fn_webservice_propagates_errors() {
    let mut model = new_empty_model();

    model._set("B1", "=NA()");
    model._set("A1", "=WEBSERVICE(B1)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#N/A");
}

// RTD

#[test]
fn fn_rtd_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=RTD()");
    model._set("A2", r#"=RTD("prog.id")"#);
    model._set("A3", r#"=RTD("prog.id", "server")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
}

#[test]
fn fn_rtd_literal() {
    let mut model = new_empty_model();

    // no real-time data server is available
    model._set("A1", r#"=RTD("prog.id", "", "topic")"#);
    model._set("A2", r#"=RTD("prog.id", "server", "topic1", "topic2")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#N/A");
    assert_eq!(model._get_text("A2"), *"#N/A");
}

// STOCKHISTORY

#[test]
fn fn_stockhistory_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=STOCKHISTORY()");
    model._set("A2", r#"=STOCKHISTORY("MSFT")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_stockhistory_invalid_arguments() {
    let mut model = new_empty_model();

    // interval must be 0, 1 or 2
    model._set(
        "A1",
        r#"=STOCKHISTORY("MSFT", DATE(2024,1,1), DATE(2024,2,1), 5)"#,
    );
    // headers must be 0, 1 or 2
    model._set(
        "A2",
        r#"=STOCKHISTORY("MSFT", DATE(2024,1,1), DATE(2024,2,1), 0, 9)"#,
    );
    // properties must be between 0 and 5
    model._set(
        "A3",
        r#"=STOCKHISTORY("MSFT", DATE(2024,1,1), DATE(2024,2,1), 0, 1, 0, 6)"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
}

#[test]
fn fn_stockhistory_literal() {
    let mut model = new_empty_model();

    model._set("A1", r#"=STOCKHISTORY("MSFT", DATE(2024,1,1))"#);
    model._set(
        "A2",
        r#"=STOCKHISTORY("MSFT", DATE(2024,1,1), DATE(2024,2,1), 0, 1, 0, 1)"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#CONNECT!");
    assert_eq!(model._get_text("A2"), *"#CONNECT!");
}

// DETECTLANGUAGE

#[test]
fn fn_detectlanguage() {
    let mut model = new_empty_model();

    model._set("A1", "=DETECTLANGUAGE()");
    model._set("A2", r#"=DETECTLANGUAGE("a", "b")"#);
    model._set("A3", r#"=DETECTLANGUAGE("¿Dónde está la biblioteca?")"#);
    // casts its argument
    model._set("A4", "=DETECTLANGUAGE(123)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#CONNECT!");
    assert_eq!(model._get_text("A4"), *"#CONNECT!");
}

// TRANSLATE

#[test]
fn fn_translate() {
    let mut model = new_empty_model();

    model._set("A1", "=TRANSLATE()");
    // invalid language code
    model._set("A2", r#"=TRANSLATE("hello", "notalanguage", "fr")"#);
    model._set("A3", r#"=TRANSLATE("hello", "en", "12")"#);
    // valid shapes reach the service refusal
    model._set("A4", r#"=TRANSLATE("hello")"#);
    model._set("A5", r#"=TRANSLATE("hello", "en", "fr-CA")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#CONNECT!");
    assert_eq!(model._get_text("A5"), *"#CONNECT!");
}

// COPILOT

#[test]
fn fn_copilot() {
    let mut model = new_empty_model();

    model._set("B1", "10");
    model._set("B2", "20");

    model._set("A1", "=COPILOT()");
    model._set("A2", r#"=COPILOT("Summarize this data")"#);
    // context arguments may be ranges
    model._set("A3", r#"=COPILOT("Summarize this data", B1:B2)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#CONNECT!");
    assert_eq!(model._get_text("A3"), *"#CONNECT!");
}

// IMAGE

#[test]
fn fn_image_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=IMAGE()");
    model._set(
        "A2",
        r#"=IMAGE("https://example.com/a.png", "alt", 3, 10, 10, 1)"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_image_sizing_rules() {
    let mut model = new_empty_model();

    // sizing must be between 0 and 3
    model._set("A1", r#"=IMAGE("https://example.com/a.png", "alt", 4)"#);
    // custom size requires both height and width
    model._set("A2", r#"=IMAGE("https://example.com/a.png", "alt", 3)"#);
    model._set(
        "A3",
        r#"=IMAGE("https://example.com/a.png", "alt", 3, 100)"#,
    );
    // height/width are only used when sizing is 3
    model._set(
        "A4",
        r#"=IMAGE("https://example.com/a.png", "alt", 0, 100, 100)"#,
    );
    // height and width must be positive
    model._set(
        "A5",
        r#"=IMAGE("https://example.com/a.png", "alt", 3, -1, 100)"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
    assert_eq!(model._get_text("A5"), *"#VALUE!");
}

#[test]
fn fn_image_literal() {
    let mut model = new_empty_model();

    model._set("A1", r#"=IMAGE("https://example.com/a.png")"#);
    model._set(
        "A2",
        r#"=IMAGE("https://example.com/a.png", "alt", 3, 100, 100)"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#CONNECT!");
    assert_eq!(model._get_text("A2"), *"#CONNECT!");
}

// CALL and REGISTER.ID

#[test]
fn fn_call() {
    let mut model = new_empty_model();

    model._set("A1", "=CALL()");
    model._set("A2", r#"=CALL("Kernel32", "GetTickCount", "J")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#BLOCKED!");
}

#[test]
fn fn_register_id() {
    let mut model = new_empty_model();

    model._set("A1", r#"=REGISTER.ID("Kernel32")"#);
    model._set("A2", r#"=REGISTER.ID("Kernel32", "GetTickCount")"#);
    model._set("A3", r#"=REGISTER.ID("Kernel32", "GetTickCount", "J")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#BLOCKED!");
    assert_eq!(model._get_text("A3"), *"#BLOCKED!");
}

// CUBE functions

#[test]
fn fn_cubevalue() {
    let mut model = new_empty_model();

    model._set("A1", "=CUBEVALUE()");
    model._set("A2", r#"=CUBEVALUE("Sales")"#);
    model._set("A3", r#"=CUBEVALUE("Sales", "[Measures].[Profit]")"#);
    // member expressions longer than 255 characters
    model._set("A4", r#"=CUBEVALUE("Sales", REPT("a", 256))"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#NAME?");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
}

#[test]
fn fn_cubemember() {
    let mut model = new_empty_model();

    model._set("A1", r#"=CUBEMEMBER("Sales")"#);
    model._set("A2", r#"=CUBEMEMBER("Sales", "[Time].[2024]")"#);
    model._set("A3", r#"=CUBEMEMBER("Sales", "[Time].[2024]", "caption")"#);
    model._set("A4", r#"=CUBEMEMBER("Sales", REPT("a", 256))"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#NAME?");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
}

#[test]
fn fn_cubeset() {
    let mut model = new_empty_model();

    model._set("A1", r#"=CUBESET("Sales")"#);
    model._set("A2", r#"=CUBESET("Sales", "[Product].children")"#);
    // sort_order must be between 0 and 6
    model._set(
        "A3",
        r#"=CUBESET("Sales", "[Product].children", "caption", 9)"#,
    );
    model._set("A4", r#"=CUBESET("Sales", REPT("a", 256))"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
}

#[test]
fn fn_cubesetcount() {
    let mut model = new_empty_model();

    model._set("A1", "=CUBESETCOUNT()");
    // the inner CUBESET error propagates
    model._set(
        "A2",
        r#"=CUBESETCOUNT(CUBESET("Sales", "[Product].children"))"#,
    );
    // a non-error value is not a set
    model._set("A3", "=CUBESETCOUNT(5)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
}

#[test]
fn fn_cuberankedmember() {
    let mut model = new_empty_model();

    model._set("A1", r#"=CUBERANKEDMEMBER("Sales", "[Product].children")"#);
    model._set(
        "A2",
        r#"=CUBERANKEDMEMBER("Sales", "[Product].children", 1)"#,
    );
    model._set(
        "A3",
        r#"=CUBERANKEDMEMBER("Sales", "[Product].children", 1, "caption")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#NAME?");
}

#[test]
fn fn_cubekpimember() {
    let mut model = new_empty_model();

    model._set("A1", r#"=CUBEKPIMEMBER("Sales", "SalesKPI")"#);
    // kpi_property must be between 1 and 6
    model._set("A2", r#"=CUBEKPIMEMBER("Sales", "SalesKPI", 7)"#);
    model._set("A3", r#"=CUBEKPIMEMBER("Sales", "SalesKPI", 1)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#NAME?");
}

#[test]
fn fn_cubememberproperty() {
    let mut model = new_empty_model();

    model._set("A1", r#"=CUBEMEMBERPROPERTY("Sales", "[Time].[2024]")"#);
    model._set(
        "A2",
        r#"=CUBEMEMBERPROPERTY("Sales", "[Time].[2024]", "property")"#,
    );
    model._set(
        "A3",
        r#"=CUBEMEMBERPROPERTY("Sales", REPT("a", 256), "property")"#,
    );

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#NAME?");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
}

// GETPIVOTDATA

#[test]
fn fn_getpivotdata() {
    let mut model = new_empty_model();

    model._set("A1", "=GETPIVOTDATA()");
    model._set("A2", r#"=GETPIVOTDATA("Sales")"#);
    // field/item arguments come in pairs
    model._set("A3", r#"=GETPIVOTDATA("Sales", C1, "Month")"#);
    model._set("A4", r#"=GETPIVOTDATA("Sales", C1)"#);
    model._set("A5", r#"=GETPIVOTDATA("Sales", C1, "Month", "March")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
    assert_eq!(model._get_text("A4"), *"#REF!");
    assert_eq!(model._get_text("A5"), *"#REF!");
}
