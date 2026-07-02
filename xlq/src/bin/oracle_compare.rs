//! Differential oracle: ironcalc vs LibreOffice on the shared case table.
//!
//! Usage: `cargo run --bin oracle-compare -- <oracle-cases.json> <lo-converted.xlsx>`
//!
//! - Builds ONE in-memory ironcalc model containing the shared data block
//!   (identical to the block gen_oracle_workbook.py writes on sheet "T").
//! - Sets every case formula (canonical Excel spelling, verbatim from the
//!   JSON) in column G, row = case index (same layout as the workbook),
//!   evaluates once, and reads both the raw and the formatted value.
//! - Loads the LibreOffice-converted workbook through ironcalc's importer
//!   (battle-testing import at the same time) and reads LibreOffice's CACHED
//!   computed values — the model is deliberately NOT re-evaluated, so the
//!   values are exactly what LibreOffice wrote into `<v>`.
//!
//! Comparison policy (see docs/AGREEMENT.md):
//! - numbers: relative tolerance 1e-9, absolute 1e-12 near zero — but an
//!   exact zero on one side only matches an exact zero on the other
//!   (zero-vs-tiny is underflow or residue, a real signal for triage);
//!   agreements that hold only under tolerance are counted separately
//! - text / booleans: exact
//! - errors: agree that "both are errors" => verdict `both_error`
//!   (error-CODE equality is reported separately, never as a disagreement;
//!   rows where LibreOffice's error is #NAME? — LO does not know the
//!   function, so there is no oracle at all — are flagged and counted)
//! - LibreOffice empty string vs ironcalc empty cell => agree
//! - verdict `engine_error` = ironcalc could not even accept/read the case
//!
//! Output: JSON report on stdout (per-case rows, per-function rollup, totals).

use ironcalc::base::cell::CellValue;
use ironcalc::base::Model;
use serde_json::{json, Map, Value};
use std::process::ExitCode;

const FORMULA_COLUMN: i32 = 7; // column G

/// Excel error literals (plus ironcalc/LibreOffice-specific ones). A cell
/// value equal to one of these is an error value on either side.
const ERROR_CODES: &[&str] = &[
    "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#REF!", "#VALUE!",
    "#ERROR!", "#N/IMPL!", "#SPILL!", "#CALC!", "#CIRC!", "#GETTING_DATA",
];

fn is_error(v: &CellValue) -> bool {
    matches!(v, CellValue::String(s) if ERROR_CODES.contains(&s.as_str()))
}

fn is_empty(v: &CellValue) -> bool {
    matches!(v, CellValue::None) || matches!(v, CellValue::String(s) if s.is_empty())
}

fn numbers_agree(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a == 0.0 || b == 0.0 {
        // Exact zero vs nonzero is never absorbed by the absolute tolerance:
        // it is either an underflow on the zero side (e.g. a tail
        // probability collapsing to 0.0) or rounding residue on the nonzero
        // side — both are real signals that belong in the triage table.
        return false;
    }
    let diff = (a - b).abs();
    diff <= 1e-12 || diff <= 1e-9 * a.abs().max(b.abs())
}

fn cell_value_to_json(v: &CellValue) -> Value {
    match v {
        CellValue::None => Value::Null,
        CellValue::String(s) => json!(s),
        CellValue::Number(n) => json!(n),
        CellValue::Boolean(b) => json!(b),
    }
}

/// Shared data block — MUST stay identical to gen_oracle_workbook.py and to
/// the `_meta.data_block` entry in benchmarks/oracle-cases.json.
fn write_data_block(model: &mut Model) -> Result<(), String> {
    let a = [2.0, 4.0, 6.0, 8.0, 10.0, -3.0, 0.0, 7.5, 100.0, 1.0];
    for (i, v) in a.iter().enumerate() {
        model.update_cell_with_number(0, i as i32 + 1, 1, *v)?;
    }
    let b = [
        "alpha", "Beta", "gamma DELTA", "2026-03-15", "x,y;z", " padded ", "", "MiXeD", "100",
        "-5",
    ];
    for (i, v) in b.iter().enumerate() {
        if v.is_empty() {
            continue; // B7 stays blank (see _meta note in oracle-cases.json)
        }
        model.update_cell_with_text(0, i as i32 + 1, 2, v)?;
    }
    for i in 0..5 {
        model.update_cell_with_number(0, i + 1, 3, (i + 1) as f64)?;
        model.update_cell_with_number(0, i + 1, 4, ((i + 1) * 10) as f64)?;
    }
    model.update_cell_with_bool(0, 1, 5, true)?;
    model.update_cell_with_bool(0, 2, 5, false)?;
    Ok(())
}

struct CaseResult {
    function: String,
    formula: String,
    row: i32,
    iron_value: Option<CellValue>,
    iron_formatted: Option<String>,
    iron_setup_error: Option<String>,
    lo_value: CellValue,
    lo_formatted: String,
}

#[derive(Default)]
struct Verdict {
    verdict: &'static str,
    /// `Some` only for `both_error`: whether the error codes are equal.
    codes_match: Option<bool>,
    /// `both_error` where LibreOffice's side is `#NAME?`: LO does not know
    /// the function at all, so the row carries no oracle signal — ironcalc's
    /// own error (whatever it is) went entirely unchecked.
    lo_name_error: bool,
    /// `agree` on numbers that are NOT bit-identical: the agreement holds
    /// only under the comparison tolerance.
    within_tolerance: bool,
}

fn verdict(case: &CaseResult) -> Verdict {
    if case.iron_setup_error.is_some() {
        return Verdict {
            verdict: "engine_error",
            ..Default::default()
        };
    }
    let iron = case.iron_value.as_ref().expect("value set when no setup error");
    let lo = &case.lo_value;
    let iron_err = is_error(iron);
    let lo_err = is_error(lo);
    if iron_err && lo_err {
        // Both engines say "error": that is agreement under the policy.
        // Whether the error CODES match is reported separately, and rows
        // where LibreOffice answers #NAME? are flagged: they have no oracle.
        let codes_match = cell_value_to_json(iron) == cell_value_to_json(lo);
        let lo_name_error = matches!(lo, CellValue::String(s) if s == "#NAME?");
        return Verdict {
            verdict: "both_error",
            codes_match: Some(codes_match),
            lo_name_error,
            ..Default::default()
        };
    }
    if iron_err != lo_err {
        return Verdict {
            verdict: "disagree",
            ..Default::default()
        };
    }
    if is_empty(iron) && is_empty(lo) {
        return Verdict {
            verdict: "agree",
            ..Default::default()
        };
    }
    let (agree, exact) = match (iron, lo) {
        (CellValue::Number(a), CellValue::Number(b)) => (numbers_agree(*a, *b), a == b),
        (CellValue::String(a), CellValue::String(b)) => (a == b, a == b),
        (CellValue::Boolean(a), CellValue::Boolean(b)) => (a == b, a == b),
        _ => (false, false), // type mismatch (incl. empty vs non-empty)
    };
    Verdict {
        verdict: if agree { "agree" } else { "disagree" },
        within_tolerance: agree && !exact,
        ..Default::default()
    }
}

fn run() -> Result<Value, String> {
    let mut args = std::env::args().skip(1);
    let (Some(cases_path), Some(lo_path)) = (args.next(), args.next()) else {
        return Err("usage: oracle-compare <oracle-cases.json> <lo-converted.xlsx>".to_string());
    };

    // Case table, in the exact row order used by gen_oracle_workbook.py:
    // functions in sorted-key order, cases in listed order.
    let text = std::fs::read_to_string(&cases_path)
        .map_err(|e| format!("read {cases_path}: {e}"))?;
    let table: Value =
        serde_json::from_str(&text).map_err(|e| format!("parse {cases_path}: {e}"))?;
    let obj = table.as_object().ok_or("case table must be a JSON object")?;
    let mut functions: Vec<&String> = obj.keys().filter(|k| *k != "_meta").collect();
    functions.sort(); // byte order == python sorted() for these ASCII names
    let mut cases: Vec<(String, String)> = Vec::new();
    for func in functions {
        let list = obj[func]
            .as_array()
            .ok_or_else(|| format!("cases for {func} must be an array"))?;
        for f in list {
            let formula = f
                .as_str()
                .ok_or_else(|| format!("case for {func} must be a string"))?;
            cases.push((func.clone(), formula.to_string()));
        }
    }

    // ironcalc side: one in-memory model, same layout as the workbook.
    let mut model = Model::new_empty("oracle", "en", "UTC", "en")
        .map_err(|e| format!("new_empty: {e}"))?;
    write_data_block(&mut model).map_err(|e| format!("data block: {e}"))?;
    let mut results: Vec<CaseResult> = Vec::with_capacity(cases.len());
    for (i, (function, formula)) in cases.iter().enumerate() {
        let row = i as i32 + 1;
        let setup = model
            .update_cell_with_formula(0, row, FORMULA_COLUMN, formula.clone())
            .err();
        results.push(CaseResult {
            function: function.clone(),
            formula: formula.clone(),
            row,
            iron_value: None,
            iron_formatted: None,
            iron_setup_error: setup,
            lo_value: CellValue::None,
            lo_formatted: String::new(),
        });
    }
    model.evaluate();
    for case in &mut results {
        if case.iron_setup_error.is_some() {
            continue;
        }
        match model.get_cell_value_by_index(0, case.row, FORMULA_COLUMN) {
            Ok(v) => case.iron_value = Some(v),
            Err(e) => {
                case.iron_setup_error = Some(format!("read value: {e}"));
                continue;
            }
        }
        case.iron_formatted = model
            .get_formatted_cell_value(0, case.row, FORMULA_COLUMN)
            .ok();
    }

    // LibreOffice side: load the converted workbook through ironcalc's
    // importer and read the CACHED values (no evaluate() — the whole point
    // is to read what LibreOffice computed).
    let lo_model = ironcalc::import::load_from_xlsx(&lo_path, "en", "UTC", "en")
        .map_err(|e| format!("load {lo_path}: {e}"))?;
    let lo_sheet = lo_model
        .get_worksheets_properties()
        .iter()
        .position(|p| p.name == "T")
        .ok_or("sheet 'T' not found in LibreOffice workbook")? as u32;
    for case in &mut results {
        case.lo_value = lo_model
            .get_cell_value_by_index(lo_sheet, case.row, FORMULA_COLUMN)
            .map_err(|e| format!("LO read row {}: {e}", case.row))?;
        case.lo_formatted = lo_model
            .get_formatted_cell_value(lo_sheet, case.row, FORMULA_COLUMN)
            .unwrap_or_default();
    }

    // Report.
    let mut per_case = Vec::with_capacity(results.len());
    let mut rollup: std::collections::BTreeMap<String, [u64; 5]> = Default::default();
    let mut totals = [0u64; 5]; // cases, agree, disagree, both_error, engine_error
    let mut error_code_matches = 0u64;
    let mut error_code_mismatches = 0u64;
    let mut both_error_lo_name = 0u64;
    let mut agree_exact = 0u64;
    let mut agree_within_tolerance = 0u64;
    for case in &results {
        let v_full = verdict(case);
        let v = v_full.verdict;
        let codes_match = v_full.codes_match;
        let slot = match v {
            "agree" => 1,
            "disagree" => 2,
            "both_error" => 3,
            _ => 4,
        };
        let entry = rollup.entry(case.function.clone()).or_default();
        entry[0] += 1;
        entry[slot] += 1;
        totals[0] += 1;
        totals[slot] += 1;
        match codes_match {
            Some(true) => error_code_matches += 1,
            Some(false) => error_code_mismatches += 1,
            None => {}
        }
        if v_full.lo_name_error {
            both_error_lo_name += 1;
        }
        if v == "agree" {
            if v_full.within_tolerance {
                agree_within_tolerance += 1;
            } else {
                agree_exact += 1;
            }
        }
        let mut row = Map::new();
        row.insert("row".into(), json!(case.row));
        row.insert("function".into(), json!(case.function));
        row.insert("formula".into(), json!(case.formula));
        row.insert(
            "ironcalc".into(),
            match &case.iron_setup_error {
                Some(e) => json!({ "engine_error": e }),
                None => json!({
                    "value": cell_value_to_json(case.iron_value.as_ref().unwrap()),
                    "formatted": case.iron_formatted,
                }),
            },
        );
        row.insert(
            "libreoffice".into(),
            json!({
                "value": cell_value_to_json(&case.lo_value),
                "formatted": case.lo_formatted,
            }),
        );
        row.insert("verdict".into(), json!(v));
        if let Some(m) = codes_match {
            row.insert("error_codes_match".into(), json!(m));
        }
        if v_full.lo_name_error {
            // LibreOffice does not know the function: no oracle for this row.
            row.insert("lo_name_error".into(), json!(true));
        }
        if v_full.within_tolerance {
            // Numeric agreement that holds only under the tolerance policy.
            row.insert("within_tolerance".into(), json!(true));
        }
        per_case.push(Value::Object(row));
    }

    let per_function: Map<String, Value> = rollup
        .into_iter()
        .map(|(f, c)| {
            (
                f,
                json!({
                    "cases": c[0], "agree": c[1], "disagree": c[2],
                    "both_error": c[3], "engine_error": c[4],
                }),
            )
        })
        .collect();

    Ok(json!({
        "meta": {
            // Keep in sync with the ENGINE constant in xlq/src/calc.rs (the
            // xlq bin targets share no library crate, so it is duplicated):
            // this artifact must identify the engine that actually ran.
            "engine": "ironcalc 0.7.1+e50ccea8 (vendored master)",
            "reference": "LibreOffice-computed cached values from the converted workbook",
            "policy": {
                "numbers": "relative 1e-9, absolute 1e-12 near zero; exact zero only matches exact zero; non-bit-identical agreements counted as within_tolerance",
                "text": "exact", "booleans": "exact",
                "errors": "both-error = agreement class both_error; code equality reported separately; LO #NAME? rows flagged lo_name_error (no oracle)",
                "empty": "LO empty string == ironcalc empty cell",
            },
            "lo_workbook": lo_path,
        },
        "totals": {
            "cases": totals[0], "agree": totals[1], "disagree": totals[2],
            "both_error": totals[3], "engine_error": totals[4],
            "agree_exact": agree_exact,
            "agree_within_tolerance": agree_within_tolerance,
            "both_error_code_matches": error_code_matches,
            "both_error_code_mismatches": error_code_mismatches,
            "both_error_lo_name": both_error_lo_name,
        },
        "per_function": per_function,
        "cases": per_case,
    }))
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
