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
    dest: u32,
) -> Result<Value> {
    // Parse the op into the shift-algebra axis/operation. Reuses main.rs's
    // single mapping so `certify` and `restructure` can never diverge.
    let Some((axis, operation)) = crate::parse_structural_op(op) else {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "bad_op",
            "detail": "--op must be insert-rows | delete-rows | insert-cols | delete-cols | move-rows",
        }));
    };
    if at == 0 || count == 0 {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "bad_args",
            "detail": "--at is 1-based and --count must be >= 1",
        }));
    }
    if operation == crate::refshift::Op::Move && dest == 0 {
        return Ok(json!({
            "status": "REFUSED",
            "reason": "bad_args",
            "detail": "move-rows requires --dest >= 1 (the 1-based row the block was moved before)",
        }));
    }
    let op_label = if operation == crate::refshift::Op::Move {
        format!("{op}@{at}x{count}->{dest} on {sheet}")
    } else {
        format!("{op}@{at}x{count} on {sheet}")
    };

    // (1) xlq's OWN faithful transform of the original.
    let original_bytes = std::fs::read(original)
        .with_context(|| format!("read {}", diff::basename(original)))?;
    let edit = StructuralEdit {
        axis,
        at,
        count,
        op: operation,
        sheet: sheet.to_string(),
        dest,
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

    // (1b) NON-CELL references. diff::snapshot (below) compares only sheet CELLS, so
    // a foreign edit that shifts every cell formula correctly but leaves a defined
    // name / data-validation / conditional-formatting / chart reference unshifted
    // would be invisible to it — a reachable false certification. We close it here:
    // defined names must match xlq's (proven) transform exactly, and any other
    // reference-bearing part certify does not compare fails closed.
    let edited_bytes = std::fs::read(edited)
        .with_context(|| format!("read {}", diff::basename(edited)))?;
    if let Some(refusal) = verify_noncell_refs(&expected_bytes, &edited_bytes) {
        return Ok(refusal);
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

/// Verify the reference-bearing content diff::snapshot (sheet cells only) does not
/// compare. Returns Some(refusal) if the foreign edit's defined names differ from
/// xlq's transform, or if the workbook carries a reference-bearing part certify cannot
/// verify (fail closed). None if all clear.
fn verify_noncell_refs(expected: &[u8], edited: &[u8]) -> Option<Value> {
    // defined names must match xlq's proven transform exactly (name -> refers-to)
    if defined_names(expected) != defined_names(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "defined_name_mismatch",
            "detail": "a defined name's target differs from xlq's transform — a non-cell \
                       reference was not shifted faithfully",
        }));
    }
    // SEMANTIC structural references the transform shifts (mergeCell / hyperlink /
    // autoFilter `ref`) must also match xlq's transform. These are the ref-bearing
    // elements a foreign edit can revert while shifting cells (the reviewer's merge
    // exploit); comparing them keeps certify's surface a superset of the transform's
    // value/structure write-surface. Pure view-state (dimension/selection/pane/brk)
    // is deliberately excluded — it is non-semantic and foreign tools legitimately
    // vary it; it does not affect computed values.
    if structural_ref_attrs(expected) != structural_ref_attrs(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "structural_ref_mismatch",
            "detail": "a mergeCell/hyperlink/autoFilter reference differs from xlq's \
                       transform — a structural reference was not shifted faithfully",
        }));
    }
    // fail closed on SHEET-level reference constructs certify does not compare
    for (needle, label) in [
        ("<dataValidation", "data_validation"),
        ("<conditionalFormatting", "conditional_formatting"),
        ("sparkline", "sparkline"),
    ] {
        if sheets_contain(edited, needle) || sheets_contain(expected, needle) {
            return Some(json!({
                "status": "REFUSED",
                "reason": "unverified_reference_part",
                "detail": format!("{label} may carry references certify does not compare — \
                                   refused (fail-closed; outside the verified surface)"),
            }));
        }
    }
    // fail closed on whole reference-bearing PARTS certify does not compare
    for (prefix, label) in [
        ("xl/charts/", "chart"),
        ("xl/pivotTables/", "pivot_table"),
        ("xl/pivotCache/", "pivot_cache"),
        ("xl/externalLinks/", "external_link"),
    ] {
        let present = |b: &[u8]| {
            structural::archive_names(b)
                .map(|ns| ns.iter().any(|n| n.starts_with(prefix)))
                .unwrap_or(false)
        };
        if present(edited) || present(expected) {
            return Some(json!({
                "status": "REFUSED",
                "reason": "unverified_reference_part",
                "detail": format!("{label} references are not compared — refused (fail-closed; \
                                   outside the verified surface)"),
            }));
        }
    }
    None
}

/// (name, refers-to) for every defined name in workbook.xml, sorted.
fn defined_names(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&wb);
    let mut out = Vec::new();
    let mut rest: &str = &text;
    while let Some(p) = rest.find("<definedName") {
        rest = &rest[p..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        let name = attr(tag, "name").unwrap_or_default();
        let after = &rest[gt + 1..];
        let refers = after.find("</definedName>").map(|e| &after[..e]).unwrap_or("");
        out.push((name, refers.to_string()));
        rest = after;
    }
    out.sort();
    out
}

/// (element, ref) for every mergeCell/hyperlink/autoFilter across all sheets, sorted
/// — the semantic structural references the transform shifts. Part names are excluded
/// (robust to a foreign tool renumbering sheet parts); the multiset is compared.
fn structural_ref_attrs(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(names) = structural::archive_names(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in names {
        if !(n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")) {
            continue;
        }
        let Ok(part) = crate::ooxml::read_part(bytes, &n) else {
            continue;
        };
        let text = String::from_utf8_lossy(&part);
        for elem in ["mergeCell", "hyperlink", "autoFilter"] {
            let open = format!("<{elem}");
            let mut rest: &str = &text;
            while let Some(p) = rest.find(&open) {
                rest = &rest[p..];
                let Some(gt) = rest.find('>') else { break };
                if let Some(r) = attr(&rest[..gt], "ref") {
                    out.push((elem.to_string(), r));
                }
                rest = &rest[gt..];
            }
        }
    }
    out.sort();
    out
}

/// Value of attribute `key` in a start tag (quote-agnostic).
fn attr(tag: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=");
    let i = tag.find(&pat)? + pat.len();
    let q = *tag.as_bytes().get(i)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let rest = &tag[i + 1..];
    let end = rest.find(q as char)?;
    Some(rest[..end].to_string())
}

/// True if any worksheet part contains `needle`.
fn sheets_contain(bytes: &[u8], needle: &str) -> bool {
    let Ok(names) = structural::archive_names(bytes) else {
        return false;
    };
    for n in names {
        if n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml") {
            if let Ok(part) = crate::ooxml::read_part(bytes, &n) {
                if String::from_utf8_lossy(&part).contains(needle) {
                    return true;
                }
            }
        }
    }
    false
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
