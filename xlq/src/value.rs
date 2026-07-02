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
pub fn raw_cell_value(model: &Model, sheet: u32, row: i32, column: i32) -> Result<serde_json::Value> {
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
