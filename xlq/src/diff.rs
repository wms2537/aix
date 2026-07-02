//! `xlq diff` — cell-level positional diff of two workbooks.
//!
//! CONTRACT: pub fn run(old_path: &str, new_path: &str) -> anyhow::Result<serde_json::Value>
//!
//! Semantics (from the design doc — v1 is strictly positional):
//! - Sheets are matched BY NAME, not index. Sheets present in only one file
//!   are reported as added/removed (with cell counts, not contents).
//! - For sheets present in both: compare the union of populated cells at
//!   each (row, col): formula string (canonical, via get_cell_formula) and
//!   RAW stored value (via get_cell_value_by_index — do NOT evaluate; diff
//!   compares the files as they are on disk). Raw values are the comparison
//!   basis; formatted strings (get_formatted_cell_value) are display-only —
//!   comparing formatted strings would both hide on-disk value differences
//!   below display precision and misreport number-format-only edits as
//!   data changes.
//! - A cell differs if formula differs, OR (both have the SAME formula but
//!   different cached raw results — kind "cached_value": a tool stripped or
//!   rewrote the stored results without touching formulas; openpyxl does
//!   this to every formula cell it saves, and Excel shows those numbers
//!   until a recalc), OR (both non-formula and raw value differs), OR (both
//!   non-formula, raw values equal, formatted rendering differs — a
//!   formatting-only change). Report kind:
//!   "value" | "formula" | "cached_value" | "format" | "added" | "removed".
//!   cached_value counts in its own summary bucket, not in "changed".
//! - An inserted row WILL report many changed cells; that is documented
//!   v1 behavior (no alignment/move detection).
//!
//! Output schema:
//! {
//!   "xlq": {"version": ..., "command": "diff"},
//!   "old": {"name": <basename>, "sha256": ...},
//!   "new": {"name": <basename>, "sha256": ...},
//!   "sheets_added": [...], "sheets_removed": [...],
//!   "changes": [{"sheet": "S", "cell": "B7", "row": 7, "col": 2,
//!                "kind": "formula|value|format|added|removed",
//!                "old": {"formula": ..., "value": <formatted>, "raw": ...} | null,
//!                "new": {"formula": ..., "value": <formatted>, "raw": ...} | null}],
//!   "summary": {"changed": n, "added": n, "removed": n, "by_sheet": {...}}
//! }
//!   ("format" changes count toward "changed" in the summary.)
//! - Cell refs in A1 notation: use
//!   ironcalc::base::expressions::utils::number_to_column for the letters.
//! - Changes list is capped at 10_000 entries with "truncated": true set —
//!   never silently; summary counts always reflect the full totals.
//!
//! NOTE: diff DOES include cell values/formulas by design (unlike inspect) —
//! it is a comparison tool for the file owner, not a shareable census.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ironcalc::base::expressions::utils::number_to_column;
use ironcalc::base::Model;
use serde_json::json;

const MAX_CHANGES: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
struct CellSnap {
    formula: Option<String>,
    /// Formatted rendering — display only, never the comparison basis.
    value: String,
    /// Raw stored value (null | string | number | bool) — comparison basis.
    raw: serde_json::Value,
}

type SheetSnap = BTreeMap<(i32, i32), CellSnap>;
type WorkbookSnap = BTreeMap<String, SheetSnap>;

struct DiffReport {
    sheets_added: Vec<serde_json::Value>,
    sheets_removed: Vec<serde_json::Value>,
    changes: Vec<serde_json::Value>,
    truncated: bool,
    summary: serde_json::Value,
}

pub fn run(old_path: &str, new_path: &str) -> Result<serde_json::Value> {
    // Error contexts carry basenames only: main.rs echoes error messages
    // into the stdout JSON payload, which must never contain full paths.
    let old_name = basename(old_path);
    let new_name = basename(new_path);
    let old_model = ironcalc::import::load_from_xlsx(old_path, "en", "UTC", "en")
        .with_context(|| format!("load {old_name}"))?;
    let new_model = ironcalc::import::load_from_xlsx(new_path, "en", "UTC", "en")
        .with_context(|| format!("load {new_name}"))?;

    let old_sha = crate::hash::sha256_file(old_path)?;
    let new_sha = crate::hash::sha256_file(new_path)?;

    let old_snap = snapshot(&old_model).with_context(|| format!("snapshot {old_name}"))?;
    let new_snap = snapshot(&new_model).with_context(|| format!("snapshot {new_name}"))?;

    let report = diff_snapshots(&old_snap, &new_snap)?;

    Ok(json!({
        "xlq": {"version": env!("CARGO_PKG_VERSION"), "command": "diff"},
        "old": {"name": basename(old_path), "sha256": old_sha},
        "new": {"name": basename(new_path), "sha256": new_sha},
        "sheets_added": report.sheets_added,
        "sheets_removed": report.sheets_removed,
        "changes": report.changes,
        "truncated": report.truncated,
        "summary": report.summary,
    }))
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn snapshot(model: &Model) -> Result<WorkbookSnap> {
    let names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();
    let mut snap: WorkbookSnap = names
        .iter()
        .map(|n| (n.clone(), SheetSnap::new()))
        .collect();
    for cell in model.get_all_cells() {
        let name = names
            .get(cell.index as usize)
            .ok_or_else(|| anyhow!("cell references unknown sheet index {}", cell.index))?;
        let formula = model
            .get_cell_formula(cell.index, cell.row, cell.column)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("read formula at {}!({},{})", name, cell.row, cell.column))?;
        let value = model
            .get_formatted_cell_value(cell.index, cell.row, cell.column)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("read value at {}!({},{})", name, cell.row, cell.column))?;
        let raw = crate::value::raw_cell_value(model, cell.index, cell.row, cell.column)
            .with_context(|| {
                format!("read raw value at {}!({},{})", name, cell.row, cell.column)
            })?;
        snap.get_mut(name)
            .expect("sheet key inserted above")
            .insert(
                (cell.row, cell.column),
                CellSnap {
                    formula,
                    value,
                    raw,
                },
            );
    }
    Ok(snap)
}

fn a1(row: i32, col: i32) -> Result<String> {
    let letters = number_to_column(col).ok_or_else(|| anyhow!("column {col} out of A1 range"))?;
    Ok(format!("{letters}{row}"))
}

fn snap_json(snap: &CellSnap) -> serde_json::Value {
    json!({"formula": snap.formula, "value": snap.value, "raw": snap.raw})
}

fn diff_snapshots(old: &WorkbookSnap, new: &WorkbookSnap) -> Result<DiffReport> {
    let sheets_added: Vec<serde_json::Value> = new
        .iter()
        .filter(|(name, _)| !old.contains_key(*name))
        .map(|(name, cells)| json!({"name": name, "cells": cells.len()}))
        .collect();
    let sheets_removed: Vec<serde_json::Value> = old
        .iter()
        .filter(|(name, _)| !new.contains_key(*name))
        .map(|(name, cells)| json!({"name": name, "cells": cells.len()}))
        .collect();

    let mut changes = Vec::new();
    let mut truncated = false;
    let (mut total_changed, mut total_added, mut total_removed) = (0u64, 0u64, 0u64);
    let mut total_cached = 0u64;
    let mut by_sheet = serde_json::Map::new();

    for (name, old_cells) in old {
        let Some(new_cells) = new.get(name) else {
            continue;
        };
        let (mut s_changed, mut s_added, mut s_removed) = (0u64, 0u64, 0u64);
        let mut s_cached = 0u64;
        let coords: BTreeSet<(i32, i32)> =
            old_cells.keys().chain(new_cells.keys()).copied().collect();
        for (row, col) in coords {
            let old_snap = old_cells.get(&(row, col));
            let new_snap = new_cells.get(&(row, col));
            let kind = match (old_snap, new_snap) {
                (Some(o), Some(n)) => {
                    if o.formula != n.formula {
                        "formula"
                    } else if o.formula.is_some() && o.raw != n.raw {
                        // Equal formulas, different cached results: the
                        // formula is intact but the stored value diverged —
                        // typically a tool (openpyxl) stripping caches, or a
                        // save from an engine that computed different values.
                        // Excel users see these numbers until a recalc, so a
                        // diff that ignores them under-reports the change.
                        "cached_value"
                    } else if o.formula.is_none() && o.raw != n.raw {
                        // Raw values are the on-disk truth; formatted strings
                        // would hide drift below display precision.
                        "value"
                    } else if o.formula.is_none() && o.value != n.value {
                        // Same stored value, different rendering: a
                        // number-format change, not a data change.
                        "format"
                    } else {
                        continue;
                    }
                }
                (Some(_), None) => "removed",
                (None, Some(_)) => "added",
                (None, None) => unreachable!("coord came from union of keys"),
            };
            match kind {
                "added" => {
                    total_added += 1;
                    s_added += 1;
                }
                "removed" => {
                    total_removed += 1;
                    s_removed += 1;
                }
                "cached_value" => {
                    total_cached += 1;
                    s_cached += 1;
                }
                _ => {
                    total_changed += 1;
                    s_changed += 1;
                }
            }
            if changes.len() < MAX_CHANGES {
                changes.push(json!({
                    "sheet": name,
                    "cell": a1(row, col)?,
                    "row": row,
                    "col": col,
                    "kind": kind,
                    "old": old_snap.map(snap_json),
                    "new": new_snap.map(snap_json),
                }));
            } else {
                truncated = true;
            }
        }
        if s_changed + s_added + s_removed + s_cached > 0 {
            by_sheet.insert(
                name.clone(),
                json!({"changed": s_changed, "added": s_added, "removed": s_removed,
                       "cached_value": s_cached}),
            );
        }
    }

    let summary = json!({
        "changed": total_changed,
        "added": total_added,
        "removed": total_removed,
        "cached_value": total_cached,
        "by_sheet": by_sheet,
    });

    Ok(DiffReport {
        sheets_added,
        sheets_removed,
        changes,
        truncated,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with(cells: &[(i32, i32, &str)]) -> Model<'static> {
        let mut model = Model::new_empty("t", "en", "UTC", "en").expect("new model");
        for (row, col, input) in cells {
            model
                .set_user_input(0, *row, *col, input.to_string())
                .expect("set input");
        }
        model.evaluate();
        model
    }

    #[test]
    fn detects_value_formula_and_added_cell_changes() {
        let old_model = model_with(&[(1, 1, "1"), (2, 1, "=SUM(1,2)")]);
        let new_model = model_with(&[(1, 1, "2"), (2, 1, "=SUM(1,3)"), (1, 2, "hello")]);

        let old_snap = snapshot(&old_model).unwrap();
        let new_snap = snapshot(&new_model).unwrap();
        let report = diff_snapshots(&old_snap, &new_snap).unwrap();

        assert!(report.sheets_added.is_empty());
        assert!(report.sheets_removed.is_empty());
        assert!(!report.truncated);
        assert_eq!(report.changes.len(), 3);

        let kinds: Vec<(&str, &str)> = report
            .changes
            .iter()
            .map(|c| (c["cell"].as_str().unwrap(), c["kind"].as_str().unwrap()))
            .collect();
        assert_eq!(
            kinds,
            vec![("A1", "value"), ("B1", "added"), ("A2", "formula")]
        );

        assert_eq!(report.summary["changed"], 2);
        assert_eq!(report.summary["added"], 1);
        assert_eq!(report.summary["removed"], 0);
        assert_eq!(report.summary["by_sheet"]["Sheet1"]["changed"], 2);
        assert_eq!(report.summary["by_sheet"]["Sheet1"]["added"], 1);

        let a1 = &report.changes[0];
        assert_eq!(a1["old"]["value"], "1");
        assert_eq!(a1["new"]["value"], "2");
        assert_eq!(a1["old"]["formula"], serde_json::Value::Null);

        let b1 = &report.changes[1];
        assert_eq!(b1["old"], serde_json::Value::Null);
        assert_eq!(b1["new"]["value"], "hello");

        let a2 = &report.changes[2];
        assert_eq!(a2["kind"], "formula");
        assert!(a2["old"]["formula"].as_str().unwrap().contains("SUM(1,2)"));
        assert!(a2["new"]["formula"].as_str().unwrap().contains("SUM(1,3)"));
    }

    #[test]
    fn reports_added_and_removed_sheets_with_counts_only() {
        let old_model = model_with(&[(1, 1, "x")]);
        let mut new_model = model_with(&[(1, 1, "x")]);
        new_model.add_sheet("Extra").unwrap();
        new_model
            .set_user_input(1, 1, 1, "secret".to_string())
            .unwrap();
        new_model
            .set_user_input(1, 2, 1, "secret2".to_string())
            .unwrap();
        new_model.evaluate();

        let old_snap = snapshot(&old_model).unwrap();
        let new_snap = snapshot(&new_model).unwrap();
        let report = diff_snapshots(&old_snap, &new_snap).unwrap();

        assert_eq!(
            report.sheets_added,
            vec![json!({"name": "Extra", "cells": 2})]
        );
        assert!(report.sheets_removed.is_empty());
        assert!(report.changes.is_empty());
        assert_eq!(report.summary["changed"], 0);

        let reversed = diff_snapshots(&new_snap, &old_snap).unwrap();
        assert_eq!(
            reversed.sheets_removed,
            vec![json!({"name": "Extra", "cells": 2})]
        );
    }

    fn single_cell_snap(
        formula: Option<&str>,
        value: &str,
        raw: serde_json::Value,
    ) -> WorkbookSnap {
        [(
            "Sheet1".to_string(),
            [(
                (1, 1),
                CellSnap {
                    formula: formula.map(str::to_string),
                    value: value.to_string(),
                    raw,
                },
            )]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect()
    }

    #[test]
    fn same_formula_with_stale_cached_value_is_cached_value_not_changed() {
        // Contract updated after surface verification: an openpyxl re-save
        // strips every formula cache; reporting "no change" hid 442 stale
        // cells in a real workbook. Same formula + different cached result
        // is now kind "cached_value", bucketed apart from "changed".
        let old_snap = single_cell_snap(Some("=A2+1"), "3", json!(3.0));
        let new_snap = single_cell_snap(Some("=A2+1"), "99", json!(99.0));

        let report = diff_snapshots(&old_snap, &new_snap).unwrap();
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0]["kind"], "cached_value");
        assert_eq!(report.summary["changed"], 0);
        assert_eq!(report.summary["cached_value"], 1);
    }

    #[test]
    fn raw_value_drift_below_display_precision_is_a_value_change() {
        // Both render "100.4" under a "0.0" number format, but the stored
        // values on disk differ: this MUST be reported.
        let old_snap = single_cell_snap(None, "100.4", json!(100.44));
        let new_snap = single_cell_snap(None, "100.4", json!(100.41));

        let report = diff_snapshots(&old_snap, &new_snap).unwrap();
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0]["kind"], "value");
        assert_eq!(report.changes[0]["old"]["raw"], json!(100.44));
        assert_eq!(report.changes[0]["new"]["raw"], json!(100.41));
        assert_eq!(report.summary["changed"], 1);
    }

    #[test]
    fn format_only_change_is_kind_format_not_value() {
        // Identical stored value, different number format rendering.
        let old_snap = single_cell_snap(None, "100.4", json!(100.44));
        let new_snap = single_cell_snap(None, "100.44", json!(100.44));

        let report = diff_snapshots(&old_snap, &new_snap).unwrap();
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0]["kind"], "format");
        assert_eq!(report.summary["changed"], 1);
    }

    #[test]
    fn stripped_formula_cache_is_kind_cached_value() {
        // openpyxl-style save: formula intact, cached result replaced (it
        // writes <v/> for every formula cell). The diff must surface this —
        // Excel displays the cached numbers until a recalc.
        let old_snap = single_cell_snap(Some("=A2-A3"), "101597", json!(101597.0));
        let new_snap = single_cell_snap(Some("=A2-A3"), "0", json!(0.0));

        let report = diff_snapshots(&old_snap, &new_snap).unwrap();
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0]["kind"], "cached_value");
        assert_eq!(report.summary["cached_value"], 1);
        assert_eq!(report.summary["changed"], 0);
    }

    #[test]
    fn truncates_at_cap_but_summary_keeps_full_totals() {
        let mut old_snap: WorkbookSnap = BTreeMap::new();
        let mut new_snap: WorkbookSnap = BTreeMap::new();
        let mut old_cells = SheetSnap::new();
        let mut new_cells = SheetSnap::new();
        for i in 0..(MAX_CHANGES as i32 + 5) {
            let coord = (i / 100 + 1, i % 100 + 1);
            old_cells.insert(
                coord,
                CellSnap {
                    formula: None,
                    value: "a".to_string(),
                    raw: json!("a"),
                },
            );
            new_cells.insert(
                coord,
                CellSnap {
                    formula: None,
                    value: "b".to_string(),
                    raw: json!("b"),
                },
            );
        }
        old_snap.insert("S".to_string(), old_cells);
        new_snap.insert("S".to_string(), new_cells);

        let report = diff_snapshots(&old_snap, &new_snap).unwrap();
        assert!(report.truncated);
        assert_eq!(report.changes.len(), MAX_CHANGES);
        assert_eq!(report.summary["changed"], MAX_CHANGES as u64 + 5);
    }

    #[test]
    fn removed_cell_in_a_common_sheet_is_kind_removed() {
        let old_model = model_with(&[(1, 1, "keep"), (2, 3, "gone")]);
        let new_model = model_with(&[(1, 1, "keep")]);

        let old_snap = snapshot(&old_model).unwrap();
        let new_snap = snapshot(&new_model).unwrap();
        let report = diff_snapshots(&old_snap, &new_snap).unwrap();

        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0]["kind"], "removed");
        assert_eq!(report.changes[0]["cell"], "C2");
        assert_eq!(report.changes[0]["new"], serde_json::Value::Null);
        assert_eq!(report.changes[0]["old"]["value"], "gone");
        assert_eq!(report.summary["removed"], 1);
        assert_eq!(report.summary["changed"], 0);
        assert_eq!(report.summary["by_sheet"]["Sheet1"]["removed"], 1);
    }

    #[test]
    fn basename_falls_back_to_the_input_for_component_free_paths() {
        assert_eq!(basename("/tmp/dir/book.xlsx"), "book.xlsx");
        assert_eq!(basename(".."), "..");
    }

    #[test]
    fn load_errors_carry_basenames_only() {
        // Missing OLD file.
        let err = run(
            "/tmp/xlq-diff-secret-dir/old.xlsx",
            "/tmp/xlq-diff-secret-dir/new.xlsx",
        )
        .expect_err("missing files must fail");
        let text = format!("{err:#}");
        assert!(text.contains("old.xlsx"), "old basename missing: {text}");
        assert!(
            !text.contains("xlq-diff-secret-dir"),
            "directory leaked: {text}"
        );

        // OLD loads fine, NEW is missing: the second load context is hit.
        let model = model_with(&[(1, 1, "x")]);
        let dir = std::env::temp_dir().join("xlq-diff-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join(format!("good-{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&good);
        let good = good.to_string_lossy().into_owned();
        ironcalc::export::save_to_xlsx(&model, &good).unwrap();
        let err = run(&good, "/tmp/xlq-diff-secret-dir/new.xlsx")
            .expect_err("missing new file must fail");
        let text = format!("{err:#}");
        assert!(text.contains("new.xlsx"), "new basename missing: {text}");
        assert!(
            !text.contains("xlq-diff-secret-dir"),
            "directory leaked: {text}"
        );
        let _ = std::fs::remove_file(&good);
    }

    #[test]
    fn run_end_to_end_reports_shas_and_changes() {
        let dir = std::env::temp_dir().join("xlq-diff-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let old_path = dir.join(format!("run-old-{}.xlsx", std::process::id()));
        let new_path = dir.join(format!("run-new-{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&old_path);
        let _ = std::fs::remove_file(&new_path);
        let old_path = old_path.to_string_lossy().into_owned();
        let new_path = new_path.to_string_lossy().into_owned();
        ironcalc::export::save_to_xlsx(&model_with(&[(1, 1, "1")]), &old_path).unwrap();
        ironcalc::export::save_to_xlsx(&model_with(&[(1, 1, "2")]), &new_path).unwrap();

        let report = run(&old_path, &new_path).expect("diff runs");
        assert_eq!(report["xlq"]["command"], "diff");
        assert_eq!(report["old"]["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(report["new"]["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(report["summary"]["changed"], 1);
        assert_eq!(report["truncated"], false);

        let _ = std::fs::remove_file(&old_path);
        let _ = std::fs::remove_file(&new_path);
    }

    #[test]
    fn a1_notation_is_correct() {
        assert_eq!(a1(7, 2).unwrap(), "B7");
        assert_eq!(a1(1, 27).unwrap(), "AA1");
        assert_eq!(a1(100, 703).unwrap(), "AAA100");
        assert!(a1(1, 0).is_err());
    }
}
