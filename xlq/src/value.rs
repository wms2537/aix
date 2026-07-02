//! Raw (unformatted) cell value access shared by `calc` and `diff`.
//!
//! Both commands must compare STORED values exactly, not their formatted
//! renderings: any drift smaller than the cell's number-format resolution
//! (e.g. 100.41 vs 100.44 under "0.0") would otherwise vanish. The raw
//! value is the comparison key; the formatted string is display-only.

use anyhow::{anyhow, Result};
use ironcalc::base::cell::CellValue;
use ironcalc::base::Model;

/// The raw stored value of a cell as JSON: null | string | number | bool.
/// Numbers are compared and serialized as their exact f64 payloads
/// (non-finite values fall back to their string rendering).
pub fn raw_cell_value(
    model: &Model,
    sheet: u32,
    row: i32,
    column: i32,
) -> Result<serde_json::Value> {
    let value = model
        .get_cell_value_by_index(sheet, row, column)
        .map_err(|e| anyhow!(e))?;
    Ok(match value {
        CellValue::None => serde_json::Value::Null,
        CellValue::String(s) => serde_json::Value::String(s),
        CellValue::Boolean(b) => serde_json::Value::Bool(b),
        CellValue::Number(n) => serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(n.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> Model<'static> {
        let mut model = Model::new_empty("t", "en", "UTC", "en").expect("new model");
        model.set_user_input(0, 1, 1, "hello".to_string()).unwrap();
        model.set_user_input(0, 2, 1, "TRUE".to_string()).unwrap();
        model.set_user_input(0, 3, 1, "2.5".to_string()).unwrap();
        model.evaluate();
        model
    }

    #[test]
    fn maps_each_cell_value_variant_to_json() {
        let model = model();
        assert_eq!(raw_cell_value(&model, 0, 1, 1).unwrap(), json!("hello"));
        assert_eq!(raw_cell_value(&model, 0, 2, 1).unwrap(), json!(true));
        assert_eq!(raw_cell_value(&model, 0, 3, 1).unwrap(), json!(2.5));
        // An unpopulated cell is CellValue::None -> JSON null.
        assert_eq!(
            raw_cell_value(&model, 0, 9, 9).unwrap(),
            serde_json::Value::Null
        );
    }

    #[test]
    fn non_finite_numbers_fall_back_to_string_rendering() {
        // JSON has no representation for inf/NaN; the raw value must not be
        // silently dropped, so it degrades to the f64's string rendering.
        let mut model = model();
        let ws = model.workbook.worksheet_mut(0).unwrap();
        ws.set_cell_with_number(10, 1, f64::INFINITY, 0).unwrap();
        ws.set_cell_with_number(11, 1, f64::NEG_INFINITY, 0)
            .unwrap();
        ws.set_cell_with_number(12, 1, f64::NAN, 0).unwrap();
        assert_eq!(raw_cell_value(&model, 0, 10, 1).unwrap(), json!("inf"));
        assert_eq!(raw_cell_value(&model, 0, 11, 1).unwrap(), json!("-inf"));
        assert_eq!(raw_cell_value(&model, 0, 12, 1).unwrap(), json!("NaN"));
    }

    #[test]
    fn invalid_sheet_index_is_an_error() {
        let model = model();
        assert!(raw_cell_value(&model, 99, 1, 1).is_err());
    }
}
