#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn fn_euroconvert_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=EUROCONVERT()");
    model._set("A2", "=EUROCONVERT(1)");
    model._set("A3", r#"=EUROCONVERT(1, "FRF")"#);
    model._set("A4", r#"=EUROCONVERT(1, "FRF", "DEM", TRUE, 3, 4)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
    assert_eq!(model._get_text("A4"), *"#ERROR!");
}

#[test]
fn fn_euroconvert() {
    let mut model = new_empty_model();

    model._set("A1", r#"=EUROCONVERT(1, "EUR", "DEM")"#);
    model._set("A2", r#"=EUROCONVERT(1, "EUR", "DEM", TRUE)"#);
    model._set("A3", r#"=EUROCONVERT(1.20, "DEM", "EUR")"#);
    // BEF uses 0 decimal places for the currency-specific rounding
    model._set("A4", r#"=EUROCONVERT(100, "EUR", "BEF")"#);
    // Same source and target: value unchanged
    model._set("A5", r#"=EUROCONVERT(3.14, "FRF", "FRF")"#);
    // Codes are accepted case-insensitively
    model._set("A6", r#"=EUROCONVERT(1, "eur", "dem")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"1.96");
    assert_eq!(model._get_text("A2"), *"1.95583");
    assert_eq!(model._get_text("A3"), *"0.61");
    assert_eq!(model._get_text("A4"), *"4034");
    assert_eq!(model._get_text("A5"), *"3.14");
    assert_eq!(model._get_text("A6"), *"1.96");
}

#[test]
fn fn_euroconvert_triangulation() {
    let mut model = new_empty_model();

    // The worked example from Excel's documentation: the intermediate euro
    // value 1/6.55957 = 0.152449... is rounded to 3 DECIMAL PLACES (0.152)
    // before multiplying by 1.95583.
    model._set("A1", r#"=EUROCONVERT(1, "FRF", "DEM", TRUE, 3)"#);
    model._set("A2", r#"=EUROCONVERT(1, "FRF", "DEM", FALSE, 3)"#);
    model._set("A3", r#"=ROUND(EUROCONVERT(1, "FRF", "EUR", TRUE, 3), 9)"#);
    model._set("A4", r#"=EUROCONVERT(1, "FRF", "EUR", FALSE, 3)"#);
    // Without triangulation precision the intermediate value is not rounded
    model._set("A5", r#"=ROUND(EUROCONVERT(1, "FRF", "DEM", TRUE), 9)"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"0.29728616");
    assert_eq!(model._get_text("A2"), *"0.3");
    assert_eq!(model._get_text("A3"), *"0.152449017");
    assert_eq!(model._get_text("A4"), *"0.15");
    assert_eq!(model._get_text("A5"), *"0.298164361");
}

#[test]
fn fn_euroconvert_invalid_params() {
    let mut model = new_empty_model();

    model._set("A1", r#"=EUROCONVERT(1, "USD", "EUR")"#);
    model._set("A2", r#"=EUROCONVERT(1, "EUR", "GBP")"#);
    // CYP is not in Excel's table
    model._set("A3", r#"=EUROCONVERT(1, "CYP", "EUR")"#);
    // triangulation_precision must be at least 3
    model._set("A4", r#"=EUROCONVERT(1, "FRF", "DEM", TRUE, 2)"#);
    model._set("A5", r#"=EUROCONVERT("text", "FRF", "DEM")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#VALUE!");
    assert_eq!(model._get_text("A2"), *"#VALUE!");
    assert_eq!(model._get_text("A3"), *"#VALUE!");
    assert_eq!(model._get_text("A4"), *"#VALUE!");
    assert_eq!(model._get_text("A5"), *"#VALUE!");
}

#[test]
fn fn_dbcs_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=DBCS()");
    model._set("A2", r#"=DBCS("a", "b")"#);
    model._set("A3", "=JIS()");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
    assert_eq!(model._get_text("A3"), *"#ERROR!");
}

#[test]
fn fn_dbcs_ascii() {
    let mut model = new_empty_model();

    model._set("A1", r#"=DBCS("abcABC123")"#);
    model._set("A2", r#"=DBCS("!~")"#);
    // The space U+0020 is outside U+0021..=U+007E and is left unchanged
    model._set("A3", r#"=DBCS("a b")"#);
    model._set("A4", r#"=DBCS("")"#);
    // JIS is the same built-in under its ECMA-376 name
    model._set("A5", r#"=JIS("abc")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"ａｂｃＡＢＣ１２３");
    assert_eq!(model._get_text("A2"), *"！～");
    assert_eq!(model._get_text("A3"), *"ａ ｂ");
    assert_eq!(model._get_text("A4"), *"");
    assert_eq!(model._get_text("A5"), *"ａｂｃ");
}

#[test]
fn fn_dbcs_katakana() {
    let mut model = new_empty_model();

    model._set("A1", r#"=DBCS("ｱｲｳｴｵ")"#);
    // Voiced sound mark composes with the preceding katakana
    model._set("A2", r#"=DBCS("ｶﾞ")"#);
    model._set("A3", r#"=DBCS("ﾃﾞｼﾞﾀﾙ")"#);
    // Semi-voiced sound mark composes only with the ﾊ row
    model._set("A4", r#"=DBCS("ﾊﾟ")"#);
    model._set("A5", r#"=DBCS("ｳﾞ")"#);
    // A sound mark with nothing to compose with becomes the standalone mark
    model._set("A6", r#"=DBCS("ﾞ")"#);
    model._set("A7", r#"=DBCS("ｱﾟ")"#);
    // Punctuation and unaffected text
    model._set("A8", r#"=DBCS("ｶﾀｶﾅ｡｢｣､･ｰ")"#);
    model._set("A9", r#"=DBCS("日本語はそのまま")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"アイウエオ");
    assert_eq!(model._get_text("A2"), *"ガ");
    assert_eq!(model._get_text("A3"), *"デジタル");
    assert_eq!(model._get_text("A4"), *"パ");
    assert_eq!(model._get_text("A5"), *"ヴ");
    assert_eq!(model._get_text("A6"), *"゛");
    assert_eq!(model._get_text("A7"), *"ア゜");
    assert_eq!(model._get_text("A8"), *"カタカナ。「」、・ー");
    assert_eq!(model._get_text("A9"), *"日本語はそのまま");
}

#[test]
fn fn_bahttext_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=BAHTTEXT()");
    model._set("A2", "=BAHTTEXT(1, 2)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_bahttext() {
    let mut model = new_empty_model();

    model._set("A1", "=BAHTTEXT(0)");
    // 1 in the units place stays หนึ่ง when the tens digit is zero
    model._set("A2", "=BAHTTEXT(101)");
    model._set("A3", "=BAHTTEXT(11)");
    model._set("A4", "=BAHTTEXT(21)");
    model._set("A5", "=BAHTTEXT(1234)");
    model._set("A6", "=BAHTTEXT(123456)");
    model._set("A7", "=BAHTTEXT(1234567.89)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"ศูนย์บาทถ้วน");
    assert_eq!(model._get_text("A2"), *"หนึ่งร้อยหนึ่งบาทถ้วน");
    assert_eq!(model._get_text("A3"), *"สิบเอ็ดบาทถ้วน");
    assert_eq!(model._get_text("A4"), *"ยี่สิบเอ็ดบาทถ้วน");
    assert_eq!(model._get_text("A5"), *"หนึ่งพันสองร้อยสามสิบสี่บาทถ้วน");
    assert_eq!(model._get_text("A6"), *"หนึ่งแสนสองหมื่นสามพันสี่ร้อยห้าสิบหกบาทถ้วน");
    assert_eq!(
        model._get_text("A7"),
        *"หนึ่งล้านสองแสนสามหมื่นสี่พันห้าร้อยหกสิบเจ็ดบาทแปดสิบเก้าสตางค์"
    );
}

#[test]
fn fn_bahttext_satang() {
    let mut model = new_empty_model();

    // No บาท when the amount is under one baht
    model._set("A1", "=BAHTTEXT(0.25)");
    model._set("A2", "=BAHTTEXT(-0.25)");
    model._set("A3", "=BAHTTEXT(1.5)");
    model._set("A4", "=BAHTTEXT(0.01)");
    // The value is rounded to two decimal places first
    model._set("A5", "=BAHTTEXT(1.995)");
    // Negative amounts that round to zero take the zero form, without ลบ
    model._set("A6", "=BAHTTEXT(-0.001)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"ยี่สิบห้าสตางค์");
    assert_eq!(model._get_text("A2"), *"ลบยี่สิบห้าสตางค์");
    assert_eq!(model._get_text("A3"), *"หนึ่งบาทห้าสิบสตางค์");
    assert_eq!(model._get_text("A4"), *"หนึ่งสตางค์");
    assert_eq!(model._get_text("A5"), *"สองบาทถ้วน");
    assert_eq!(model._get_text("A6"), *"ศูนย์บาทถ้วน");
}

#[test]
fn fn_bahttext_millions() {
    let mut model = new_empty_model();

    model._set("A1", "=BAHTTEXT(1000000)");
    model._set("A2", "=BAHTTEXT(2000003)");
    // ล้าน stacks once per six-digit block
    model._set("A3", "=BAHTTEXT(1000000000000)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"หนึ่งล้านบาทถ้วน");
    assert_eq!(model._get_text("A2"), *"สองล้านสามบาทถ้วน");
    assert_eq!(model._get_text("A3"), *"หนึ่งล้านล้านบาทถ้วน");
}

#[test]
fn fn_phonetic_args_number() {
    let mut model = new_empty_model();

    model._set("A1", "=PHONETIC()");
    model._set("A2", "=PHONETIC(B1, B2)");

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#ERROR!");
    assert_eq!(model._get_text("A2"), *"#ERROR!");
}

#[test]
fn fn_phonetic() {
    let mut model = new_empty_model();

    model._set("B1", "東京");
    model._set("B2", "hello");
    model._set("B3", "42");

    // No phonetic runs are available, so the cell's own text is returned
    // unchanged.
    model._set("A1", "=PHONETIC(B1)");
    model._set("A2", "=PHONETIC(B2)");
    model._set("A3", "=PHONETIC(B3)");
    // A multi-cell reference uses its upper-left cell
    model._set("A4", "=PHONETIC(B1:B3)");
    model._set("A5", "=PHONETIC(C1)");
    model._set("A6", r#"=PHONETIC("ふりがな")"#);

    model.evaluate();

    assert_eq!(model._get_text("A1"), *"東京");
    assert_eq!(model._get_text("A2"), *"hello");
    assert_eq!(model._get_text("A3"), *"42");
    assert_eq!(model._get_text("A4"), *"東京");
    assert_eq!(model._get_text("A5"), *"");
    assert_eq!(model._get_text("A6"), *"ふりがな");
}
