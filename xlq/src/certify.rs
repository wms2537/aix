//! `xlq certify` — ENGINE-FREE certification that a FOREIGN edited workbook
//! equals xlq's own proven-faithful structural transform of the original.
//!
//! This is the production certifier for untrusted foreign edits. Given an
//! `original`, an `edited` file (produced by some other, untrusted tool), and
//! the structural op the edit is *claimed* to be, it:
//!
//!   1. Computes xlq's OWN faithful transform of `original` via the proven
//!      reference-shift algebra (`structural::structural_edit`) — the same
//!      transform `xlq restructure` commits. If xlq cannot express this op on
//!      this file as a pure coordinate shift (residuals present), it REFUSES:
//!      xlq will not certify what it cannot itself prove.
//!   2. Loads both xlq's transform and the foreign `edited` file and compares
//!      them positionally, cell by cell, using the exact same snapshot +
//!      diff-kind classification as `xlq diff` (crate::diff). No recalculation
//!      engine is run over the comparison — the certification is over the
//!      STORED formulas and raw data, so a foreign tool cannot launder a wrong
//!      answer through a matching cached value.
//!   3. CERTIFIES iff the ONLY differences are `cached_value` and/or `format`
//!      (every formula is identical at every position and all non-formula raw
//!      data matches). A foreign editor like openpyxl routinely drops or
//!      rewrites formula caches and touches number formats; those are benign.
//!      ANY `formula` | `value` | `added` | `removed` difference means the
//!      foreign edit is NOT xlq's faithful transform → REFUSE.

use crate::diff::{self, SheetSnap, WorkbookSnap};
use crate::refshift::StructuralEdit;
use crate::structural;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn run(
    original: &str,
    edited: &str,
    sheet: &str,
    op: &str,
    at: u32,
    count: u32,
) -> Result<Value> {
    // Parse the op into the shift-algebra axis/operation. Reuses main.rs's
    // single mapping so `certify` and `restructure` can never diverge.
    let Some((axis, operation)) = crate::parse_structural_op(op) else {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "bad_op",
            "detail": "--op must be insert-rows | delete-rows | insert-cols | delete-cols",
        }));
    };
    if at == 0 || count == 0 {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "bad_args",
            "detail": "--at is 1-based and --count must be >= 1",
        }));
    }
    let op_label = format!("{op}@{at}x{count} on {sheet}");

    // (1) xlq's OWN faithful transform of the original.
    let original_bytes = std::fs::read(original)
        .with_context(|| format!("read {}", diff::basename(original)))?;
    let edit = StructuralEdit {
        axis,
        at,
        count,
        op: operation,
        sheet: sheet.to_string(),
    };
    let (expected_bytes, report) = structural::structural_edit(&original_bytes, &edit)
        .with_context(|| format!("structural edit on {sheet}"))?;

    // Residual gate: if xlq cannot express this op on this file as a pure
    // coordinate shift, it declines to certify — the sound response.
    if !report.residuals.is_empty() {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "residuals",
            "residuals": report.residuals.iter().map(|r| json!({
                "part": r.part, "reason": r.reason, "detail": r.detail
            })).collect::<Vec<_>>(),
        }));
    }

    // (2) Load xlq's transform (from a unique temp file, same discipline as
    // restructure.rs's proof-carrying re-open) and the foreign edited file.
    let expected_model = load_from_bytes(&expected_bytes, original)
        .context("load xlq structural transform")?;
    let edited_model = ironcalc::import::load_from_xlsx(edited, "en", "UTC", "en")
        .map_err(|e| anyhow!("load {}: {e}", diff::basename(edited)))?;

    // Positional snapshots — STORED formulas + raw values, no evaluation.
    let expected_snap = diff::snapshot(&expected_model)
        .context("snapshot xlq structural transform")?;
    let edited_snap = diff::snapshot(&edited_model)
        .with_context(|| format!("snapshot {}", diff::basename(edited)))?;

    // (3) Compare and classify every differing cell exactly as diff.rs does.
    let (counts, samples) = compare(&expected_snap, &edited_snap);

    let disqualifying = counts.formula + counts.value + counts.added + counts.removed;
    let status = if disqualifying == 0 { "CERTIFIED" } else { "REFUSED" };

    Ok(json!({
        "status": status,
        "op": op_label,
        "diff_counts": {
            "formula": counts.formula,
            "value": counts.value,
            "cached_value": counts.cached_value,
            "format": counts.format,
            "added": counts.added,
            "removed": counts.removed,
        },
        "sample_diffs": samples,
    }))
}

#[derive(Default)]
struct DiffCounts {
    formula: u64,
    value: u64,
    cached_value: u64,
    format: u64,
    added: u64,
    removed: u64,
}

/// Compare xlq's transform (`expected`) against the foreign edit (`edited`)
/// over the UNION of sheets and cells. A sheet present on only one side has
/// every cell classified added/removed — so a foreign edit that adds or drops
/// a whole sheet can never be certified. `sample_diffs` carries up to 8 of the
/// DISQUALIFYING differences (formula/value/added/removed); benign
/// cached_value/format diffs never appear there, so a CERTIFIED result always
/// has an empty sample list.
fn compare(expected: &WorkbookSnap, edited: &WorkbookSnap) -> (DiffCounts, Vec<Value>) {
    let mut counts = DiffCounts::default();
    let mut samples: Vec<Value> = Vec::new();
    let empty = SheetSnap::new();

    let sheet_names: BTreeSet<&String> = expected.keys().chain(edited.keys()).collect();
    for name in sheet_names {
        let exp_cells = expected.get(name).unwrap_or(&empty);
        let edt_cells = edited.get(name).unwrap_or(&empty);
        let coords: BTreeSet<(i32, i32)> =
            exp_cells.keys().chain(edt_cells.keys()).copied().collect();
        for (row, col) in coords {
            let e = exp_cells.get(&(row, col));
            let n = edt_cells.get(&(row, col));
            let Some(kind) = diff::classify_kind(e, n) else {
                continue;
            };
            match kind {
                "formula" => counts.formula += 1,
                "value" => counts.value += 1,
                "cached_value" => counts.cached_value += 1,
                "format" => counts.format += 1,
                "added" => counts.added += 1,
                "removed" => counts.removed += 1,
                _ => {}
            }
            let disqualifying = matches!(kind, "formula" | "value" | "added" | "removed");
            if disqualifying && samples.len() < 8 {
                samples.push(json!({
                    "sheet": name,
                    "cell": diff::a1(row, col).unwrap_or_default(),
                    "kind": kind,
                    "expected_formula": e.and_then(|s| s.formula.clone()),
                    "edited_formula": n.and_then(|s| s.formula.clone()),
                }));
            }
        }
    }
    (counts, samples)
}

/// Load an in-memory xlsx by writing it to a UNIQUE temp file next to `near`
/// and importing it the way a consumer would — mirroring restructure.rs's
/// pid+AtomicU64 naming so parallel calls never collide. The temp file is
/// removed before we return (on success and on failure).
fn load_from_bytes(bytes: &[u8], near: &str) -> Result<ironcalc::base::Model<'static>> {
    let dir = Path::new(near)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".xlq-certify-expected-{}-{}.xlsx",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::write(&tmp, bytes).with_context(|| "write expected transform to temp")?;
    let tmp_str = tmp.to_string_lossy().to_string();
    let loaded = ironcalc::import::load_from_xlsx(&tmp_str, "en", "UTC", "en");
    let _ = std::fs::remove_file(&tmp);
    loaded.map_err(|e| anyhow!("{e}"))
}
