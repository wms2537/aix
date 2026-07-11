//! `xlq calc` — headless recalculation, report-only.
//!
//! CONTRACT: pub fn run(path: &str) -> anyhow::Result<serde_json::Value>
//!
//! Semantics (from the design doc):
//! - Load the file, snapshot every populated cell's stored value (the value
//!   Excel last saved), then model.evaluate(), then snapshot again.
//! - Report cells whose recomputed value differs from the stored value —
//!   these are either (a) stale caches, (b) engine/Excel disagreement, or
//!   (c) volatile functions. Never write the file.
//! - Values are compared RAW (Model::get_cell_value_by_index — exact number/
//!   string/bool equality), never as formatted display strings: drift below
//!   the cell's number-format resolution must not be silently reported as
//!   "no change". Each change record carries both the raw values (the
//!   comparison basis) and the formatted strings (human display).
//! - Coverage honesty ("no silent incorrectness"): run the function census;
//!   when unsupported, policy-limited, or user-defined callables exist, set
//!   coverage.reliable=false and list them; when volatile functions exist,
//!   list them and mark changes "volatile": true when the cell's formula
//!   calls one OR the cell transitively depends on a cell that does
//!   (volatility propagates through the dependency graph — otherwise one
//!   =TODAY() cell makes every downstream change look like an unexplained
//!   engine disagreement).
//!
//! Output schema:
//! {
//!   "xlq": {"version": ..., "command": "calc"},
//!   "file": {"name": <basename>, "sha256": ...},
//!   "changed": [{"sheet", "cell", "row", "col",
//!                "stored", "recomputed",           // formatted, display-only
//!                "stored_raw", "recomputed_raw",   // comparison basis
//!                "formula", "volatile": bool}],
//!   "summary": {"cells": n, "formulas": n, "changed": n},
//!   "coverage": {"engine": "ironcalc <ver>", "reliable": bool,
//!                "unsupported_functions": [...],
//!                "policy_limited_functions": {"NAME": "<literal>", ...},
//!                "volatile_functions": [...],
//!                "user_defined_functions": [...]}
//! }
//! - Changed list capped at 10_000 with explicit "truncated": true;
//!   summary reflects full totals.
//! - Uses census::function_census and census::extract_function_names for
//!   volatile seeds; the dependency closure comes from lexing each formula's
//!   Reference/Range tokens (ironcalc's public lexer).

use anyhow::{anyhow, Context, Result};
use ironcalc::base::expressions::lexer::{Lexer, LexerMode};
use ironcalc::base::expressions::token::TokenType;
use ironcalc::base::expressions::utils::number_to_column;
use ironcalc::base::language::get_language;
use ironcalc::base::locale::get_locale;
use ironcalc::base::Model;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};

// Single-sourced from the vendored engine (base/src/constants.rs); aliased so
// the JSON-emitting code below is unchanged and the string can never disagree
// with the linked engine. Kept truthful by pinning `ironcalc = "=0.7.1"`.
use ironcalc::base::ENGINE_PROVENANCE as ENGINE;
const CHANGED_CAP: usize = 10_000;

// Same 8-name volatile set as census.rs (Excel semantics); census does not
// export it, so it is duplicated here per the module contract.
const VOLATILE_FUNCTIONS: [&str; 8] = [
    "NOW",
    "TODAY",
    "RAND",
    "RANDBETWEEN",
    "OFFSET",
    "INDIRECT",
    "CELL",
    "INFO",
];

#[derive(Debug, Serialize)]
struct Change {
    sheet: String,
    cell: String,
    row: i32,
    col: i32,
    stored: String,
    recomputed: String,
    stored_raw: serde_json::Value,
    recomputed_raw: serde_json::Value,
    formula: Option<String>,
    volatile: bool,
}

fn formula_is_volatile(formula: &str) -> bool {
    crate::census::extract_function_names(formula)
        .iter()
        .any(|name| VOLATILE_FUNCTIONS.contains(&name.as_str()))
}

fn cell_reference(col: i32, row: i32) -> String {
    match number_to_column(col) {
        Some(letters) => format!("{letters}{row}"),
        None => format!("R{row}C{col}"),
    }
}

/// A rectangular sheet area referenced by a formula.
struct RefArea {
    sheet: u32,
    min_row: i32,
    min_col: i32,
    max_row: i32,
    max_col: i32,
}

impl RefArea {
    fn contains(&self, pos: &(u32, i32, i32)) -> bool {
        pos.0 == self.sheet
            && pos.1 >= self.min_row
            && pos.1 <= self.max_row
            && pos.2 >= self.min_col
            && pos.2 <= self.max_col
    }
}

/// Areas referenced by a formula plus every bare identifier in it (used to
/// spot references to volatile defined names). Sheet-qualified references
/// resolve through `sheet_ids` (lowercased name -> index); unqualified ones
/// belong to `own_sheet`.
fn formula_deps(
    formula: &str,
    own_sheet: u32,
    sheet_ids: &HashMap<String, u32>,
) -> (Vec<RefArea>, Vec<String>) {
    let locale = get_locale("en").expect("en locale is compiled into ironcalc");
    let language = get_language("en").expect("en language is compiled into ironcalc");
    let mut lexer = Lexer::new(formula, LexerMode::A1, locale, language);
    let mut areas = Vec::new();
    let mut idents = Vec::new();
    let resolve = |sheet: Option<String>| -> Option<u32> {
        match sheet {
            None => Some(own_sheet),
            Some(name) => sheet_ids.get(&name.to_lowercase()).copied(),
        }
    };
    loop {
        match lexer.next_token() {
            TokenType::EOF | TokenType::Illegal(_) => break,
            TokenType::Ident(name) => idents.push(name.to_uppercase()),
            TokenType::Reference {
                sheet, row, column, ..
            } => {
                if let Some(s) = resolve(sheet) {
                    areas.push(RefArea {
                        sheet: s,
                        min_row: row,
                        min_col: column,
                        max_row: row,
                        max_col: column,
                    });
                }
            }
            TokenType::Range { sheet, left, right } => {
                if let Some(s) = resolve(sheet) {
                    areas.push(RefArea {
                        sheet: s,
                        min_row: left.row.min(right.row),
                        min_col: left.column.min(right.column),
                        max_row: left.row.max(right.row),
                        max_col: left.column.max(right.column),
                    });
                }
            }
            _ => {}
        }
    }
    (areas, idents)
}

/// The set of cells whose recomputed value is legitimately time/randomness
/// dependent: cells whose own formula calls a volatile function (or a
/// volatile defined name), plus — transitively — every formula cell that
/// reads from one of those. Without the closure, one =TODAY() cell would
/// make all downstream changes look like unexplained engine disagreements.
fn volatile_taint(
    model: &Model,
    cells: &[(u32, i32, i32)],
    formulas: &[Option<String>],
) -> HashSet<(u32, i32, i32)> {
    let sheet_ids: HashMap<String, u32> = model
        .get_worksheets_properties()
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p.name.to_lowercase(), i as u32))
        .collect();
    let volatile_defined: HashSet<String> = model
        .workbook
        .defined_names
        .iter()
        .filter(|d| formula_is_volatile(&d.formula))
        .map(|d| d.name.to_uppercase())
        .collect();

    let mut tainted: HashSet<(u32, i32, i32)> = HashSet::new();
    let mut dependents: Vec<((u32, i32, i32), Vec<RefArea>)> = Vec::new();
    for (pos, formula) in cells.iter().zip(formulas) {
        let Some(formula) = formula else { continue };
        let (areas, idents) = formula_deps(formula, pos.0, &sheet_ids);
        let volatile_self =
            formula_is_volatile(formula) || idents.iter().any(|i| volatile_defined.contains(i));
        if volatile_self {
            tainted.insert(*pos);
        } else {
            dependents.push((*pos, areas));
        }
    }

    loop {
        let mut grew = false;
        dependents.retain(|(pos, areas)| {
            let hit = areas.iter().any(|a| tainted.iter().any(|t| a.contains(t)));
            if hit {
                tainted.insert(*pos);
                grew = true;
            }
            !hit
        });
        if !grew {
            break;
        }
    }
    tainted
}

fn compute_changes(model: &mut Model) -> Result<(Vec<Change>, usize, usize)> {
    let cells = model.get_all_cells();
    let sheet_names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();

    let positions: Vec<(u32, i32, i32)> =
        cells.iter().map(|c| (c.index, c.row, c.column)).collect();

    let mut before: Vec<(String, serde_json::Value, Option<String>)> =
        Vec::with_capacity(cells.len());
    let mut formula_count = 0usize;
    for c in &cells {
        let stored = model
            .get_formatted_cell_value(c.index, c.row, c.column)
            .map_err(|e| anyhow!(e))
            .with_context(|| {
                format!(
                    "read stored value sheet {} r{} c{}",
                    c.index, c.row, c.column
                )
            })?;
        let stored_raw = crate::value::raw_cell_value(model, c.index, c.row, c.column)
            .with_context(|| {
                format!(
                    "read stored raw value sheet {} r{} c{}",
                    c.index, c.row, c.column
                )
            })?;
        let formula = model
            .get_cell_formula(c.index, c.row, c.column)
            .map_err(|e| anyhow!(e))
            .with_context(|| format!("read formula sheet {} r{} c{}", c.index, c.row, c.column))?;
        if formula.is_some() {
            formula_count += 1;
        }
        before.push((stored, stored_raw, formula));
    }

    let formulas: Vec<Option<String>> = before.iter().map(|(_, _, f)| f.clone()).collect();
    let tainted = volatile_taint(model, &positions, &formulas);

    model.evaluate();

    let mut changes = Vec::new();
    for (c, (stored, stored_raw, formula)) in cells.iter().zip(before) {
        let recomputed_raw = crate::value::raw_cell_value(model, c.index, c.row, c.column)
            .with_context(|| {
                format!(
                    "read recomputed raw value sheet {} r{} c{}",
                    c.index, c.row, c.column
                )
            })?;
        // Compare RAW values: formatted strings hide drift below the number
        // format's resolution (e.g. 100.41 vs 100.44 both render "100.4").
        if recomputed_raw == stored_raw {
            continue;
        }
        let recomputed = model
            .get_formatted_cell_value(c.index, c.row, c.column)
            .map_err(|e| anyhow!(e))
            .with_context(|| {
                format!(
                    "read recomputed value sheet {} r{} c{}",
                    c.index, c.row, c.column
                )
            })?;
        let volatile = tainted.contains(&(c.index, c.row, c.column));
        let sheet = sheet_names
            .get(c.index as usize)
            .cloned()
            .unwrap_or_else(|| format!("sheet_{}", c.index));
        changes.push(Change {
            sheet,
            cell: cell_reference(c.column, c.row),
            row: c.row,
            col: c.column,
            stored,
            recomputed,
            stored_raw,
            recomputed_raw,
            formula,
            volatile,
        });
    }

    Ok((changes, cells.len(), formula_count))
}

pub fn run(path: &str) -> Result<serde_json::Value> {
    let sha256 = crate::hash::sha256_file(path)?;
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();

    let mut model = ironcalc::import::load_from_xlsx(path, "en", "UTC", "en")
        .map_err(|e| anyhow!(e))
        .with_context(|| format!("load workbook {file_name}"))?;

    let census = crate::census::function_census(&model);
    let (changes, cell_count, formula_count) = compute_changes(&mut model)?;

    let total_changed = changes.len();
    let truncated = total_changed > CHANGED_CAP;
    let changed_json = changes
        .iter()
        .take(CHANGED_CAP)
        .map(|c| serde_json::to_value(c).context("serialize change record"))
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "xlq": {"version": env!("CARGO_PKG_VERSION"), "command": "calc"},
        "file": {"name": file_name, "sha256": sha256},
        "changed": changed_json,
        "truncated": truncated,
        "summary": {
            "cells": cell_count,
            "formulas": formula_count,
            "changed": total_changed,
        },
        "coverage": {
            "engine": ENGINE,
            // Policy-limited functions keep reliable=false: their values
            // depend on external services/connections xlq never contacts,
            // so the recomputation cannot be verified locally.
            "reliable": census.unsupported.is_empty()
                && census.policy_limited.is_empty()
                && census.user_defined.is_empty(),
            "unsupported_functions": census.unsupported,
            "policy_limited_functions": census.policy_limited,
            "volatile_functions": census.volatile_present,
            "user_defined_functions": census.user_defined.keys().collect::<Vec<_>>(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_model() -> Model<'static> {
        Model::new_empty("test", "en", "UTC", "en").expect("new empty model")
    }

    #[test]
    fn cell_reference_labels() {
        assert_eq!(cell_reference(1, 1), "A1");
        assert_eq!(cell_reference(26, 3), "Z3");
        assert_eq!(cell_reference(27, 10), "AA10");
        assert_eq!(cell_reference(703, 2), "AAA2");
    }

    #[test]
    fn stale_cached_number_reported_as_changed() {
        let mut model = empty_model();
        model.set_user_input(0, 1, 1, "1".to_string()).unwrap();
        model.set_user_input(0, 1, 2, "=A1+1".to_string()).unwrap();
        model.evaluate();
        assert_eq!(model.get_formatted_cell_value(0, 1, 2).unwrap(), "2");

        // Change A1 behind the engine's back so B1's cached "2" goes stale.
        model
            .workbook
            .worksheet_mut(0)
            .unwrap()
            .set_cell_with_number(1, 1, 5.0, 0)
            .unwrap();

        let (changes, cells, formulas) = compute_changes(&mut model).unwrap();
        assert_eq!(cells, 2);
        assert_eq!(formulas, 1);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.cell, "B1");
        assert_eq!(change.row, 1);
        assert_eq!(change.col, 2);
        assert_eq!(change.stored, "2");
        assert_eq!(change.recomputed, "6");
        assert_eq!(change.stored_raw, json!(2.0));
        assert_eq!(change.recomputed_raw, json!(6.0));
        assert_eq!(change.formula.as_deref(), Some("=A1+1"));
        assert_eq!(change.sheet, "Sheet1");
        assert!(!change.volatile);
    }

    #[test]
    fn drift_below_display_precision_is_still_a_change() {
        use ironcalc::base::types::{Cell, FormulaValue};
        let mut model = empty_model();
        model.set_user_input(0, 1, 1, "=1/3".to_string()).unwrap();
        model.evaluate();
        assert_eq!(
            model.get_formatted_cell_value(0, 1, 1).unwrap(),
            "0.333333333"
        );

        // Make the cached value stale by less than the display resolution:
        // both 1/3 and this value render as "0.333333333".
        let ws = model.workbook.worksheet_mut(0).unwrap();
        match ws.sheet_data.get_mut(&1).and_then(|row| row.get_mut(&1)) {
            Some(Cell::CellFormula {
                v: v @ FormulaValue::Number(_),
                ..
            }) => *v = FormulaValue::Number(0.333_333_333_4),
            other => panic!("expected a formula-number cell, got {other:?}"),
        }
        assert_eq!(
            model.get_formatted_cell_value(0, 1, 1).unwrap(),
            "0.333333333"
        );

        let (changes, _, _) = compute_changes(&mut model).unwrap();
        assert_eq!(
            changes.len(),
            1,
            "sub-display-precision drift must be reported"
        );
        let change = &changes[0];
        assert_eq!(change.cell, "A1");
        assert_eq!(
            change.stored, change.recomputed,
            "precondition: the formatted values mask this drift"
        );
        assert_ne!(change.stored_raw, change.recomputed_raw);
        assert!(!change.volatile);
    }

    #[test]
    fn volatility_propagates_to_dependent_cells() {
        let mut model = empty_model();
        model
            .set_user_input(0, 1, 1, "=RAND()".to_string())
            .unwrap();
        model.set_user_input(0, 1, 2, "=A1*2".to_string()).unwrap();
        model.set_user_input(0, 2, 2, "=B1+1".to_string()).unwrap();
        model
            .set_user_input(0, 3, 1, "=SUM(10,20)".to_string())
            .unwrap();
        model.evaluate();

        let (changes, _, _) = compute_changes(&mut model).unwrap();
        for change in &changes {
            match change.cell.as_str() {
                "A1" | "B1" | "B2" => assert!(
                    change.volatile,
                    "{} depends on RAND() and must be volatile",
                    change.cell
                ),
                other => panic!("unexpected change in {other}"),
            }
        }
        // A1 recomputes to a fresh random number, so A1/B1/B2 all change.
        assert_eq!(changes.len(), 3);
    }

    #[test]
    fn volatile_defined_name_reference_is_volatile() {
        let mut model = empty_model();
        // new_defined_name only accepts plain references; function-bearing
        // defined names arrive via xlsx import, which this mirrors.
        model
            .workbook
            .defined_names
            .push(ironcalc::base::types::DefinedName {
                name: "MovingWindow".to_string(),
                formula: "OFFSET(Sheet1!$A$1,1,0)".to_string(),
                sheet_id: None,
            });
        model.set_user_input(0, 2, 1, "7".to_string()).unwrap();
        model
            .set_user_input(0, 1, 2, "=SUM(MovingWindow)".to_string())
            .unwrap();
        // Not evaluated: the recompute produces a change we can inspect.
        let (changes, _, _) = compute_changes(&mut model).unwrap();
        let b1 = changes.iter().find(|c| c.cell == "B1").expect("B1 changed");
        assert!(
            b1.volatile,
            "reference to a volatile defined name must taint the cell"
        );
    }

    #[test]
    fn now_formula_flagged_volatile() {
        let mut model = empty_model();
        model.set_user_input(0, 1, 1, "=NOW()".to_string()).unwrap();
        // Unevaluated formula cells read back "#ERROR!", so evaluation always
        // produces a change here.
        let (changes, cells, formulas) = compute_changes(&mut model).unwrap();
        assert_eq!(cells, 1);
        assert_eq!(formulas, 1);
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.cell, "A1");
        assert!(change.volatile);
        assert_eq!(change.formula.as_deref(), Some("=NOW()"));
    }

    #[test]
    fn non_volatile_formula_not_flagged() {
        assert!(!formula_is_volatile("=SUM(A1:A3)+1"));
        assert!(formula_is_volatile("=OFFSET(A1,1,1)"));
    }

    #[test]
    fn cell_reference_out_of_a1_range_falls_back_to_r1c1() {
        // number_to_column(0) has no A1 letters; the label degrades to R1C1
        // rather than panicking.
        assert_eq!(cell_reference(0, 5), "R5C0");
    }

    #[test]
    fn run_errors_carry_basenames_only() {
        // Missing file: fails at the sha256 step.
        let err = run("/tmp/xlq-calc-secret-dir/payroll.xlsx").expect_err("missing file");
        let text = format!("{err:#}");
        assert!(text.contains("payroll.xlsx"), "basename missing: {text}");
        assert!(
            !text.contains("xlq-calc-secret-dir"),
            "directory leaked: {text}"
        );

        // Present but not an xlsx: fails at the load step with the basename.
        let dir = std::env::temp_dir().join("xlq-calc-secret-dir-2");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("corrupt-{}.xlsx", std::process::id()));
        std::fs::write(&path, b"not a zip").unwrap();
        let err = run(path.to_str().unwrap()).expect_err("corrupt file");
        let text = format!("{err:#}");
        assert!(
            text.contains("load workbook"),
            "load context missing: {text}"
        );
        assert!(
            !text.contains("xlq-calc-secret-dir-2"),
            "directory leaked: {text}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn changed_list_truncates_at_cap_with_full_summary_totals() {
        // 10_005 formula cells all read A1; mutate A1 behind the engine's
        // back and save, so a fresh load + evaluate changes every one of
        // them — 5 past the CHANGED_CAP.
        let n = CHANGED_CAP as i32 + 5;
        let mut model = empty_model();
        model.set_user_input(0, 1, 1, "0".to_string()).unwrap();
        for row in 1..=n {
            model
                .set_user_input(0, row, 2, "=$A$1+1".to_string())
                .unwrap();
        }
        model.evaluate();
        model
            .workbook
            .worksheet_mut(0)
            .unwrap()
            .set_cell_with_number(1, 1, 5.0, 0)
            .unwrap();

        let dir = std::env::temp_dir().join("xlq-calc-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("truncate-{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path = path.to_string_lossy().into_owned();
        ironcalc::export::save_to_xlsx(&model, &path).unwrap();

        let report = run(&path).expect("calc runs");
        assert_eq!(report["truncated"], json!(true));
        assert_eq!(report["changed"].as_array().unwrap().len(), CHANGED_CAP);
        assert_eq!(report["summary"]["changed"], json!(n));
        assert_eq!(report["summary"]["cells"], json!(n as u64 + 1));
        assert_eq!(report["summary"]["formulas"], json!(n));
        assert_eq!(report["coverage"]["reliable"], json!(true));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn policy_limited_functions_break_reliable_with_their_own_bucket() {
        let mut model = empty_model();
        model
            .set_user_input(0, 1, 1, "=CUBEVALUE(\"Sales\")".to_string())
            .unwrap();
        model
            .set_user_input(0, 2, 1, "=SUM(1,2)".to_string())
            .unwrap();
        model.evaluate();

        let dir = std::env::temp_dir().join("xlq-calc-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("policy-limited-{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path = path.to_string_lossy().into_owned();
        ironcalc::export::save_to_xlsx(&model, &path).unwrap();

        let report = run(&path).expect("calc runs");
        // Recognized but OLAP-dependent: distinct bucket, values unverifiable
        // locally, so the run is not reliable — yet nothing is "unsupported".
        assert_eq!(report["coverage"]["unsupported_functions"], json!([]));
        assert_eq!(
            report["coverage"]["policy_limited_functions"],
            json!({"CUBEVALUE": "#NAME?"})
        );
        assert_eq!(report["coverage"]["reliable"], json!(false));

        let _ = std::fs::remove_file(&path);
    }
}
