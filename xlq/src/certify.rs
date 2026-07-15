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
//!   3. CERTIFIES iff every formula is identical at every position, all
//!      non-formula raw data matches, and the foreign file carries no PRESENT
//!      formula cache that xlq's transform did not vouch (unless it forces a
//!      full recalc-on-load). A foreign editor like openpyxl routinely DROPS
//!      formula caches and touches number formats; those are benign because
//!      Excel recomputes a cacheless formula on load. But a foreign file that
//!      FILLS a differing cache and does not force recalc would display that
//!      (possibly fabricated) value verbatim — so those caches are compared
//!      directly. ANY `formula` | `value` | `added` | `removed` difference, or
//!      an unvouched present cache, means the foreign edit is NOT xlq's
//!      faithful transform → REFUSE.

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
    let original_bytes =
        std::fs::read(original).with_context(|| format!("read {}", diff::basename(original)))?;
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
    let edited_bytes =
        std::fs::read(edited).with_context(|| format!("read {}", diff::basename(edited)))?;
    if let Some(refusal) = verify_noncell_refs(&expected_bytes, &edited_bytes) {
        return Ok(refusal);
    }

    // (2) Load xlq's transform (from a unique temp file, same discipline as
    // restructure.rs's proof-carrying re-open) and the foreign edited file.
    let expected_model =
        load_from_bytes(&expected_bytes, original).context("load xlq structural transform")?;
    // Anti-bomb preflight on the untrusted foreign edit before ironcalc loads it.
    crate::ooxml::guard_decompression(edited)
        .map_err(|e| anyhow!("guard {}: {e}", diff::basename(edited)))?;
    let edited_model = ironcalc::import::load_from_xlsx(edited, "en", "UTC", "en")
        .map_err(|e| anyhow!("load {}: {e}", diff::basename(edited)))?;

    // Positional snapshots — STORED formulas + raw values, no evaluation.
    let expected_snap =
        diff::snapshot(&expected_model).context("snapshot xlq structural transform")?;
    let edited_snap = diff::snapshot(&edited_model)
        .with_context(|| format!("snapshot {}", diff::basename(edited)))?;

    // (3) Compare and classify every differing cell exactly as diff.rs does.
    let (counts, samples) = compare(&expected_snap, &edited_snap);

    // A `cached_value` difference is benign ONLY when Excel will RECOMPUTE the formula on
    // load. Excel does that for a formula cell that carries NO stored cache (what a
    // cache-dropping tool like openpyxl leaves, and what xlq writes for every shifted cell),
    // or when the workbook forces a full recalc-on-load (`<calcPr fullCalcOnLoad="1">`).
    // Absent that, Excel displays the stored cache VERBATIM — so a foreign file carrying a
    // PRESENT formula cache that xlq's proven transform did not vouch (a fabricated or stale
    // value) would show a different result than xlq's faithful transform, with no formula or
    // input-value diff for the cell diff to catch. Per ECMA-376 `fullCalcOnLoad` defaults to
    // false, so its ABSENCE is as unsafe as an explicit "0"; we compare the stored caches
    // directly rather than trusting the recalc-on-load assumption.
    let unverified_caches = if recalc_on_load_forced(&edited_bytes) {
        0
    } else {
        unverified_formula_caches(
            &expected_bytes,
            &edited_bytes,
            recalc_on_load_forced(&expected_bytes),
        )
    };
    let disqualifying =
        counts.formula + counts.value + counts.added + counts.removed + unverified_caches as u64;
    let status = if disqualifying == 0 {
        "CERTIFIED"
    } else {
        "REFUSED"
    };

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
        "unverified_caches": unverified_caches,
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
    // Sheet ORDER (3D references `Sheet1:Sheet3!` depend on tab order, and the default
    // sheet is the first) and the workbook `<calcPr>` (calc mode / iterative calc) both
    // affect computed values and are preserved verbatim by xlq's transform, so a foreign
    // edit that reorders sheets or changes a calc setting must not certify.
    if sheet_order_and_settings(expected) != sheet_order_and_settings(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "workbook_settings_mismatch",
            "detail": "the sheet order, date system, or calc settings differ from xlq's \
                       transform — a value-affecting workbook property was changed",
        }));
    }
    // SHEET-level reference constructs — conditional formatting, data validation, and any
    // `<extLst>` reference subtree (x14 CF/DV, sparklines) — are COMPARED, not refused on
    // presence. xlq's transform shifts them (edited sheet) or preserves them (foreign
    // sheet), so a faithful edit's semantics match the transform's and a mangle differs.
    // (Presence-refusal rejected xlq's own transform of any workbook carrying a dropdown
    // or CF rule — ubiquitous, and non-value-bearing.) Namespace-/path-robust: every
    // worksheet is enumerated through the workbook relationships and matched by local name.
    if sheet_ref_constructs(expected) != sheet_ref_constructs(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "sheet_construct_mismatch",
            "detail": "a conditional-formatting / data-validation / extension reference differs \
                       from xlq's transform — it was not shifted faithfully",
        }));
    }
    // AutoFilter FILTER CRITERIA (the customFilter/filter/… predicate) are a value input:
    // SUBTOTAL(1xx,…) and AGGREGATE exclude autofilter-hidden rows, so changing which rows
    // the filter hides changes those formulas' results. The transform preserves the criteria
    // verbatim (it shifts only the autoFilter `ref`), so compare them.
    if autofilter_criteria(expected) != autofilter_criteria(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "autofilter_criteria_mismatch",
            "detail": "an autoFilter filter criterion differs from xlq's transform — it changes \
                       which rows are hidden, a value input to SUBTOTAL/AGGREGATE",
        }));
    }
    // MANUALLY hidden rows are a value input to SUBTOTAL(101–111) / hidden-ignoring AGGREGATE
    // (they exclude a hidden row from the aggregate), so a foreign edit that hides a data row
    // inside such a range changes the result with NO formula or cached-value diff for the cell
    // diff to catch. On sheets carrying such a function, compare the hidden-row set; elsewhere
    // a hidden row is pure display state and is ignored (not compared) to avoid over-refusal.
    if subtotal_hidden_rows(expected) != subtotal_hidden_rows(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "hidden_row_subtotal_mismatch",
            "detail": "a manually hidden row differs from xlq's transform on a sheet using \
                       SUBTOTAL(101-111)/AGGREGATE — a value input to those aggregates",
        }));
    }
    // EXCEL TABLES (ListObjects) are COMPARED, not refused on presence — refusing them
    // rejected xlq's own faithful transform of ANY workbook containing a table (Ctrl+T) on any
    // sheet. restructure refuses an edit that would MOVE a table (on the edited sheet, or one
    // carrying a cross-sheet formula), so a table that survives to here is one the transform
    // left unchanged; a faithful edit matches its ref/name/column-formula surface, a mangle
    // (or a re-scoped structured reference) differs.
    if table_refs(expected) != table_refs(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "table_reference_mismatch",
            "detail": "an Excel Table's extent, name, column, or formula differs from xlq's \
                       transform — a reference/value change the cell diff does not compare",
        }));
    }
    // CHART data references (which the transform shifts) and DRAWING cell anchors are
    // COMPARED, not refused on presence — refusing them rejected xlq's own transform of any
    // charted or logo-bearing workbook. A faithful edit's chart refs / anchors match the
    // transform's; a mangle differs.
    if chart_drawing_refs(expected) != chart_drawing_refs(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "chart_drawing_mismatch",
            "detail": "a chart data reference or drawing anchor differs from xlq's transform",
        }));
    }
    // Tokens the engine NORMALIZES AWAY on load — the required `_xlfn.` prefix on post-2007
    // functions (dropping it makes Excel show `#NAME?`) and the implicit-intersection `@`
    // operator (`@A1:A10` scalar vs the bare `A1:A10` spilling array) — are invisible to the
    // loaded-model cell diff. Compare them per CELL, so a same-sheet RELOCATION (which a
    // per-sheet count would miss) is caught alongside a plain drop/add.
    if hidden_tokens_all(expected) != hidden_tokens_all(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "normalized_token_mismatch",
            "detail": "a formula's `_xlfn.` prefix or implicit-intersection `@` operator was \
                       added, dropped, or relocated versus xlq's transform — a `#NAME?` or \
                       spill-vs-scalar value change the loaded-model diff cannot see",
        }));
    }
    // The `<f>` TYPE attribute `t="array"` (legacy CSE array) / `t="dataTable"` is likewise
    // stripped by the engine on load. A foreign edit that turns a plain formula into a CSE
    // array (or widens the array `ref`) changes the computed value on non-dynamic-array Excel
    // with no formula/value diff the cell diff can see. Compare the array/table flag + extent
    // per cell.
    if array_formula_all(expected) != array_formula_all(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "array_formula_mismatch",
            "detail": "a formula's array/data-table flag or extent (t=\"array\"/\"dataTable\" \
                       ref) differs from xlq's transform — a CSE-array value change the \
                       loaded-model diff cannot see",
        }));
    }
    // FORM-CONTROL / OLE data bindings (a checkbox/spinner's linkedCell/fmlaLink, a listbox's
    // listFillRange, a web-publish sourceRef) — including the legacy VML form-control formulas
    // (`<x:FmlaLink>`/`<x:FmlaMacro>`) — are the cell a control reads, writes, or runs. The
    // cell diff never sees them, so a foreign edit that RE-POINTS a binding (to read a
    // different value, or run a different macro) would otherwise be certified. Compare them.
    if control_bindings(expected) != control_bindings(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "control_binding_mismatch",
            "detail": "a form-control / OLE data binding (linkedCell/fmlaLink/listFillRange/\
                       sourceRef, or a VML FmlaLink/FmlaMacro) differs from xlq's transform — \
                       a value/behavior change the cell diff cannot see",
        }));
    }
    // The VBA macro binary is executable code the transform preserves verbatim. The cell
    // diff never sees it, so a foreign edit that injects or swaps it (arbitrary macro code)
    // would otherwise be certified — a security laundering. Compare the bytes and presence.
    if vba_parts(expected) != vba_parts(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "vba_project_mismatch",
            "detail": "the VBA macro project was added, removed, or changed — refused (a \
                       structural edit never alters executable code)",
        }));
    }
    // Sheet/workbook PROTECTION (a password/hash-backed security control the transform
    // preserves verbatim). Stripping or weakening it is a security change the cell diff
    // cannot see; compare the protection elements' attributes across every sheet + workbook.
    if protection_semantics(expected) != protection_semantics(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "protection_mismatch",
            "detail": "sheet or workbook protection differs from xlq's transform — a security \
                       control was stripped or weakened",
        }));
    }
    // Fail-closed ALLOWLIST over PARTS. certify positionally compares only worksheet cells
    // (diff::snapshot), defined names, and the mergeCell/hyperlink/autoFilter refs above.
    // Any OTHER part can carry a cell reference that comparison never sees — charts,
    // drawings, tables, pivots, external links, comments, form controls, but also the
    // long tail (volatileDependencies, queryTables, metadata/richData, slicerCaches,
    // timelineCaches, connections, customXml, …). Rather than enumerate that open-ended
    // DENYLIST (its incompleteness was a real false-certification), we enumerate the
    // KNOWN-SAFE set — parts certify compares, or that carry no shiftable coordinate — and
    // refuse everything else. A foreign tool that mangles or drops a reference-bearing part
    // while shifting cells can no longer be certified.
    for wb in [edited, expected] {
        let Ok(names) = structural::archive_names(wb) else {
            continue;
        };
        let sheet_parts: BTreeSet<String> = crate::ooxml::all_sheets(wb)
            .map(|v| v.into_iter().map(|(_, p)| p).collect())
            .unwrap_or_default();
        for n in &names {
            if !part_is_certify_safe(n, &sheet_parts) {
                return Some(json!({
                    "status": "REFUSED",
                    "reason": "unverified_reference_part",
                    "detail": format!("part `{n}` is outside certify's verified/known-safe \
                                       surface — it may carry a reference the cell diff does not \
                                       compare; refused (fail-closed)"),
                }));
            }
        }
    }
    None
}

/// Is `name` a part certify either COMPARES or that provably carries no shiftable cell
/// coordinate? Everything else is refused (fail-closed allowlist). OPC part names are
/// case-insensitive, so the match is case-folded.
fn part_is_certify_safe(name: &str, sheet_parts: &BTreeSet<String>) -> bool {
    // Worksheet parts (resolved through the workbook rels — covers nonstandard paths) are
    // compared cell-by-cell plus the sheet-construct scan.
    if sheet_parts.contains(name) {
        return true;
    }
    let low = name.to_ascii_lowercase();
    // Zip directory entries are not parts.
    if low.ends_with('/') {
        return true;
    }
    low == "[content_types].xml"
        || low.ends_with(".rels")                    // packaging relationships
        || (low.starts_with("xl/worksheets/") && low.ends_with(".xml")) // worksheets (fallback if rels unreadable)
        || low == "xl/workbook.xml"                  // compared (defined names, sheets)
        || low == "xl/sharedstrings.xml"             // string pool (compared via cells)
        || low == "xl/styles.xml"                    // number formats (format diffs are benign)
        || low == "xl/calcchain.xml"                 // rebuildable calc order, no semantic ref
        || low == "xl/metadata.xml"                  // dynamic-array/rich-value metadata: index-
                                                     // linked to cells (cm/vm), no shiftable coord
        || low.starts_with("xl/theme/")              // colors/fonts
        || low.starts_with("docprops/")              // document metadata
        || low.starts_with("xl/media/")              // embedded images
        || low.starts_with("xl/printersettings/")    // opaque binary print settings
        || low.starts_with("xl/charts/")             // chart data refs — compared semantically
        || low.starts_with("xl/drawings/")           // drawing anchors — compared semantically
        || low.starts_with("xl/tables/")             // Excel Table — ref/name/formulas compared
        || low.starts_with("xl/comments")            // cell comment/note: display anchor + text,
        || low.starts_with("xl/threadedcomments/")   // no value-affecting reference (an anchor on
        || low.starts_with("xl/persons/")            // the EDITED sheet is caught upstream as a
                                                     // bad attachment before certify runs)
        || low.starts_with("xl/vbaproject") // macro binary — byte-compared for a swap
}

/// The reference/value surface of every Excel Table across `xl/tables/*.xml`, as a sorted
/// list (keyed by neither part nor sheet, so a benign part renumber does not false-refuse).
fn table_refs(bytes: &[u8]) -> Vec<String> {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if low.starts_with("xl/tables/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                out.extend(structural::table_semantics(&x));
            }
        }
    }
    out.sort();
    out
}

/// Chart data references (`<f>`) and drawing cell anchors (`<col>`/`<row>`) across ALL
/// chart/drawing parts, as two sorted lists (keyed by neither part name nor sheet, so a
/// foreign tool renumbering parts does not false-refuse). The transform shifts chart refs
/// and preserves drawing anchors, so a faithful edit matches and a mangle differs.
fn chart_drawing_refs(bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut charts = Vec::new();
    let mut drawings = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if low.starts_with("xl/charts/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                charts.extend(structural::element_text_semantics(&x, &[b"f"]));
            }
        } else if low.starts_with("xl/drawings/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                drawings.extend(structural::element_text_semantics(&x, &[b"col", b"row"]));
                // A shape/image hyperlink (`<a:hlinkClick r:id>`) resolves through the
                // drawing's own rels to an external URL — a phishing-swap target the cell
                // diff and the worksheet hyperlink scan never see.
                let rels = rels_targets(bytes, n);
                for (_, attrs) in structural::element_attr_semantics(&x, &[b"hlinkClick"]) {
                    if let Some(id) = attrs
                        .split_whitespace()
                        .find_map(|kv| kv.strip_prefix("id="))
                    {
                        drawings.push(format!(
                            "hlink={}",
                            rels.get(id).cloned().unwrap_or_default()
                        ));
                    }
                }
            }
        }
    }
    charts.sort();
    drawings.sort();
    (charts, drawings)
}

/// Every form-control / OLE / web-publish data binding across the workbook: worksheet
/// `linkedCell`/`fmlaLink`/`listFillRange`/`sourceRef` attributes and legacy VML form-control
/// formulas (`<x:FmlaLink>`/`<x:FmlaMacro>`/…). Collected as a sorted VALUE multiset (keyed by
/// neither sheet nor part, so a benign VML-part renumber does not false-refuse); a re-point
/// changes a value and is caught.
fn control_bindings(bytes: &[u8]) -> Vec<String> {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if low.starts_with("xl/worksheets/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                out.extend(structural::control_binding_attrs(&x));
            }
        } else if low.ends_with(".vml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                for t in structural::element_text_semantics(
                    &x,
                    &[
                        b"FmlaLink",
                        b"FmlaMacro",
                        b"FmlaRange",
                        b"FmlaTxbx",
                        b"FmlaGroup",
                    ],
                ) {
                    out.push(format!("vml:{t}"));
                }
            }
        }
    }
    out.sort();
    out
}

/// The engine-normalized-away formula tokens (`_xlfn.` prefixes and implicit-intersection
/// `@` operators) across every worksheet, keyed by (sheet, cell) so a same-sheet relocation
/// is visible, sorted. Compared between xlq's transform and the foreign edit.
fn hidden_tokens_all(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part) in sheets {
        if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
            for (cell, sig) in structural::formula_hidden_tokens(&x) {
                out.push((sheet_name.clone(), cell, sig));
            }
        }
    }
    out.sort();
    out
}

/// The CSE-array / data-table `<f>` type flag + extent per (sheet, cell), sorted. Stripped by
/// the engine on load, so compared here between xlq's transform and the foreign edit.
fn array_formula_all(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part) in sheets {
        if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
            for (cell, sig) in structural::array_formula_cells(&x) {
                out.push((sheet_name.clone(), cell, sig));
            }
        }
    }
    out.sort();
    out
}

/// The set of manually hidden rows on each worksheet that USES a hidden-row-excluding
/// aggregate (`SUBTOTAL(101–111)` / hidden-ignoring `AGGREGATE`), keyed by sheet, sorted.
/// A sheet without such a function contributes nothing — a hidden row there is pure display
/// state — so the comparison only fires where a hidden row actually changes a computed value.
fn subtotal_hidden_rows(bytes: &[u8]) -> Vec<(String, Vec<String>)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part) in sheets {
        if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
            if structural::hidden_row_exclusion_present(&x) {
                out.push((sheet_name, structural::hidden_rows(&x)));
            }
        }
    }
    out.sort();
    out
}

/// The autoFilter FILTER-CRITERION elements across every worksheet, keyed by sheet, as
/// sorted attribute strings. The transform preserves these verbatim, so a foreign change to
/// which rows the filter hides (a value input to SUBTOTAL/AGGREGATE) is caught.
fn autofilter_criteria(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part) in sheets {
        if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
            for (elem, attrs) in structural::element_attr_semantics(
                &x,
                &[
                    b"filterColumn",
                    b"customFilter",
                    b"filter",
                    b"dynamicFilter",
                    b"top10",
                    b"dateGroupItem",
                    b"colorFilter",
                    b"iconFilter",
                ],
            ) {
                out.push((sheet_name.clone(), elem, attrs));
            }
        }
    }
    out.sort();
    out
}

/// The bytes of every `xl/vbaProject*` part (macro binary + signature), keyed by name,
/// sorted. Compared so a foreign inject/swap of executable macro code cannot be certified.
fn vba_parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out: Vec<(String, Vec<u8>)> = names
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().starts_with("xl/vbaproject"))
        .filter_map(|n| crate::ooxml::read_part(bytes, &n).ok().map(|b| (n, b)))
        .collect();
    out.sort();
    out
}

/// `<sheetProtection>`/`<protectedRange>` (worksheets) and `<workbookProtection>`
/// (workbook), keyed by sheet name, as sorted attribute strings — so stripping or weakening
/// a password-backed protection control (invisible to the cell diff) is caught.
fn protection_semantics(bytes: &[u8]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") {
        for (elem, attrs) in structural::element_attr_semantics(&wb, &[b"workbookProtection"]) {
            out.push(("(workbook)".to_string(), elem, attrs));
        }
    }
    if let Ok(sheets) = crate::ooxml::all_sheets(bytes) {
        for (sheet_name, part) in sheets {
            if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
                for (elem, attrs) in
                    structural::element_attr_semantics(&x, &[b"sheetProtection", b"protectedRange"])
                {
                    out.push((sheet_name.clone(), elem, attrs));
                }
            }
        }
    }
    out.sort();
    out
}

/// (name, refers-to) for every defined name in workbook.xml, sorted. Delegates to the
/// shared namespace-aware, entity-resolving parser so it sees exactly what the shifter
/// rewrites — a prefixed `<x:definedName>` included. A raw-substring scan (the old code)
/// was blind to the prefixed form, so a foreign edit that left a prefixed defined name
/// stale compared equal to xlq's shifted transform — a false certification.
fn defined_names(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return Vec::new();
    };
    structural::defined_names(&wb)
}

/// (sheet-name, element, ref) for every mergeCell/hyperlink/autoFilter, sorted — the
/// semantic structural references the transform shifts. The owning sheet's NAME is part
/// of the key (resolved via the workbook relationships, robust to a foreign tool
/// renumbering sheet PARTS) so that RELOCATING a reference to a different sheet — which
/// leaves the cross-sheet multiset unchanged — is still detected as a divergence.
fn structural_ref_attrs(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part_path) in sheets {
        let Ok(part) = crate::ooxml::read_part(bytes, &part_path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&part);
        // Per-sheet relationship targets, to resolve an external hyperlink's r:id -> URL.
        let rels = rels_targets(bytes, &part_path);
        for elem in ["mergeCell", "hyperlink", "autoFilter"] {
            let open = format!("<{elem}");
            let mut rest: &str = &text;
            while let Some(p) = rest.find(&open) {
                rest = &rest[p..];
                let Some(gt) = rest.find('>') else { break };
                let tag = &rest[..gt];
                if let Some(r) = attr(tag, "ref") {
                    // For a hyperlink, the DESTINATION is also part of the semantic identity
                    // and is preserved verbatim by xlq's transform: the internal `location`
                    // (in-workbook jump) and the external `r:id` -> rels Target (the URL). A
                    // foreign edit that retargets either — an internal mispoint or a phishing
                    // URL swap — would otherwise leave (sheet, elem, ref) unchanged and certify.
                    let key = if elem == "hyperlink" {
                        let location = attr(tag, "location").unwrap_or_default();
                        let target = attr_relid(tag)
                            .and_then(|id| rels.get(&id).cloned())
                            .unwrap_or_default();
                        format!("ref={r}|loc={location}|tgt={target}")
                    } else {
                        format!("ref={r}")
                    };
                    out.push((sheet_name.clone(), elem.to_string(), key));
                }
                rest = &rest[gt..];
            }
        }
    }
    out.sort();
    out
}

/// (relationship-Id -> Target) for the relationships part of `sheet_part`. Used to resolve
/// an external hyperlink's `r:id` to its URL so a foreign Target repoint is detected.
fn rels_targets(bytes: &[u8], sheet_part: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let Some((dir, file)) = sheet_part.rsplit_once('/') else {
        return map;
    };
    let rels_part = format!("{dir}/_rels/{file}.rels");
    let Ok(part) = crate::ooxml::read_part(bytes, &rels_part) else {
        return map;
    };
    let text = String::from_utf8_lossy(&part);
    let mut rest: &str = &text;
    while let Some(p) = rest.find("<Relationship ") {
        rest = &rest[p..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        if let (Some(id), Some(target)) = (attr(tag, "Id"), attr(tag, "Target")) {
            map.insert(id, target);
        }
        rest = &rest[gt..];
    }
    map
}

/// The workbook's sheet names IN ORDER plus the VALUE-affecting workbook settings, sorted.
/// Sheet order is value-affecting (3D-span endpoints, the default first sheet). Settings
/// captured: the date epoch (`workbookPr@date1904` — a foreign flip shifts every date value
/// by 1462 days, invisible to a serial-vs-serial cell diff), the calc mode
/// (`calcPr@calcMode`), and whether iterative calc is on (`calcPr@iterate`). Each is
/// NORMALIZED to its semantic default so a foreign tool merely writing out a default value
/// (or a benign `calcId`/`fullCalcOnLoad`) does not spuriously refuse a faithful edit.
fn sheet_order_and_settings(bytes: &[u8]) -> (Vec<String>, Vec<(String, String)>) {
    let order: Vec<String> = crate::ooxml::all_sheets(bytes)
        .map(|v| v.into_iter().map(|(n, _)| n).collect())
        .unwrap_or_default();
    let mut settings: Vec<(String, String)> = Vec::new();
    if let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") {
        let text = String::from_utf8_lossy(&wb);
        let start_tag = |elem: &str| -> Option<String> {
            let p = text.find(elem)?;
            let rest = &text[p..];
            let gt = rest.find('>')?;
            Some(rest[..gt].to_string())
        };
        let truthy = |v: Option<String>| matches!(v.as_deref(), Some("1") | Some("true"));
        let wbpr = start_tag("<workbookPr").unwrap_or_default();
        let calcpr = start_tag("<calcPr").unwrap_or_default();
        settings.push((
            "date_epoch".into(),
            if truthy(attr(&wbpr, "date1904")) {
                "1904"
            } else {
                "1900"
            }
            .into(),
        ));
        settings.push((
            "calc_mode".into(),
            attr(&calcpr, "calcMode").unwrap_or_else(|| "auto".into()),
        ));
        let iterate = truthy(attr(&calcpr, "iterate"));
        settings.push(("iterate".into(), if iterate { "1" } else { "0" }.into()));
        // fullPrecision="0" ("precision as displayed") makes every formula compute on the
        // ROUNDED displayed values instead of the stored values — a workbook-global value
        // change. Default is full precision (true).
        settings.push((
            "full_precision".into(),
            if matches!(
                attr(&calcpr, "fullPrecision").as_deref(),
                Some("0") | Some("false")
            ) {
                "0"
            } else {
                "1"
            }
            .into(),
        ));
        // When iterative calc is ON, iterateCount / iterateDelta control which value a
        // circular reference converges to — a foreign change alters computed values.
        if iterate {
            settings.push((
                "iterate_count".into(),
                attr(&calcpr, "iterateCount").unwrap_or_else(|| "100".into()),
            ));
            settings.push((
                "iterate_delta".into(),
                attr(&calcpr, "iterateDelta").unwrap_or_else(|| "0.001".into()),
            ));
        }
    }
    settings.sort();
    (order, settings)
}

/// True if `xl/workbook.xml`'s `<calcPr>` FORCES a full recalculation on load
/// (`fullCalcOnLoad="1"`/`"true"`). Only then does Excel discard the stored formula caches
/// and recompute, making a differing cache benign. Absence (per ECMA-376 the default,
/// `false`) or an explicit `"0"` means Excel trusts the stored cache, so a present differing
/// cache could be shown verbatim — the caller must then verify the caches directly.
fn recalc_on_load_forced(bytes: &[u8]) -> bool {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return false;
    };
    let text = String::from_utf8_lossy(&wb);
    let Some(p) = text.find("<calcPr") else {
        return false;
    };
    let Some(gt) = text[p..].find('>') else {
        return false;
    };
    let tag = &text[p..p + gt];
    matches!(
        attr(tag, "fullCalcOnLoad").as_deref(),
        Some("1") | Some("true")
    )
}

/// The count of FORMULA cells in the `edited` file whose PRESENT stored cache xlq's proven
/// `expected` transform did not vouch — i.e., the edited cell stores a `<v>` value that is
/// absent in, or differs from, xlq's transform of the same cell. Excel displays such a stored
/// cache verbatim when recalc-on-load is not forced, so each one is a value Excel could show
/// that diverges from xlq's faithful transform. A cache-DROPPING edit (openpyxl leaves no
/// `<v>`; xlq blanks every shifted cell) contributes nothing, so the benign case is not
/// over-refused. Sheets are matched by name through the workbook relationships.
///
/// `expected_forced` is whether xlq's transform ITSELF forces a full recalc-on-load. When it
/// does, the transform DISCARDS its own stored caches and displays recomputed values, so its
/// caches cannot vouch anything — a foreign edit that keeps the (now stale) cache but dropped
/// the recalc-forcing flag would show the stale value while the transform shows the recomputed
/// one. In that case EVERY present edited cache is unverified (certify cannot recompute to
/// check it), so the caller's own-cache comparison must not launder it through a matching but
/// equally-stale expected cache.
fn unverified_formula_caches(expected: &[u8], edited: &[u8], expected_forced: bool) -> usize {
    let exp_by_name: std::collections::HashMap<String, String> = crate::ooxml::all_sheets(expected)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let Ok(edt_sheets) = crate::ooxml::all_sheets(edited) else {
        return 0;
    };
    let mut count = 0;
    for (name, edt_part) in edt_sheets {
        let Ok(edt_xml) = crate::ooxml::read_part(edited, &edt_part) else {
            continue;
        };
        let edt_map = structural::formula_cache_map(&edt_xml);
        if edt_map.is_empty() {
            continue;
        }
        // When the transform force-recomputes, its stored caches are moot: every present
        // edited cache is unverifiable, so count them all.
        if expected_forced {
            count += edt_map.len();
            continue;
        }
        let exp_map = exp_by_name
            .get(&name)
            .and_then(|p| crate::ooxml::read_part(expected, p).ok())
            .map(|x| structural::formula_cache_map(&x))
            .unwrap_or_default();
        for (cell, ev) in &edt_map {
            match exp_map.get(cell) {
                Some(xv) if caches_equal(xv, ev) => {}
                _ => count += 1,
            }
        }
    }
    count
}

/// Two stored cache values are equal if their text matches, or (for numeric caches) their
/// parsed magnitudes match — so a benign renumbering of the same value (`55` vs `55.0`) is
/// not counted as a divergence.
fn caches_equal(a: &str, b: &str) -> bool {
    a == b
        || matches!(
            (a.parse::<f64>(), b.parse::<f64>()),
            (Ok(x), Ok(y)) if x == y
        )
}

/// Parse a quoted attribute value beginning at `name_end` — the byte index just past an
/// attribute NAME — consuming XML `Eq ::= S? '=' S?` then the quoted value. Returns None
/// if what follows is not a well-formed `= "value"`. Handling the optional whitespace
/// around `=` is not cosmetic: `date1904 = "1"` is valid XML that Excel honors, and a
/// literal `find("date1904=")` missed it — letting a foreign edit smuggle a value-affecting
/// workbook setting (date1904, fullPrecision, calcMode) past certify unseen.
fn attr_value_at(tag: &str, name_end: usize) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut j = name_end;
    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
        j += 1;
    }
    if bytes.get(j) != Some(&b'=') {
        return None;
    }
    j += 1;
    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
        j += 1;
    }
    let q = match bytes.get(j) {
        Some(&b) if b == b'"' || b == b'\'' => b,
        _ => return None,
    };
    let rest = &tag[j + 1..];
    let end = rest.find(q as char)?;
    Some(rest[..end].to_string())
}

/// The relationship-id value of a start tag: a namespace-prefixed `*:id="..."` attribute.
/// The relationships-namespace prefix is arbitrary (`r:id`, `x:id`, `r2:id`), so we match
/// by LOCAL name — a literal `r:id` lookup let a rebound prefix hide a hyperlink's target.
fn attr_relid(tag: &str) -> Option<String> {
    let mut from = 0;
    while let Some(rel) = tag[from..].find(":id") {
        let start = from + rel;
        from = start + 1;
        // `:id` must be the tail of a QName (`r:id`), so a prefix name char precedes the
        // colon; `attr_value_at` rejects a longer local name (`:identifier`) by requiring
        // `Eq` immediately after, so no explicit after-boundary check is needed here.
        if let Some(v) = attr_value_at(tag, start + ":id".len()) {
            return Some(v);
        }
    }
    None
}

/// Value of attribute `key` in a start tag (quote-agnostic). `key` is matched only as a
/// whole attribute NAME — preceded by a name boundary (whitespace or the string start) and
/// followed by XML `Eq` — so neither a suffix collision (`id` inside `guid=`) nor legal
/// whitespace around `=` can forge or hide a value.
fn attr(tag: &str, key: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut from = 0;
    while let Some(rel) = tag[from..].find(key) {
        let start = from + rel;
        from = start + 1;
        let boundary_before = start == 0 || bytes[start - 1].is_ascii_whitespace();
        if boundary_before {
            if let Some(v) = attr_value_at(tag, start + key.len()) {
                return Some(v);
            }
        }
    }
    None
}

/// The reference SEMANTICS of every conditional-formatting / data-validation / extLst
/// construct across all worksheets, keyed by owning sheet, sorted. Worksheets are
/// enumerated through the workbook relationships (nonstandard paths included). Compared
/// between xlq's transform and the foreign edit: a faithful shift matches, a mangle
/// differs — replacing the old presence-refusal that rejected xlq's own transform of any
/// workbook with a dropdown or CF rule. An UNREADABLE sheet yields a sentinel so the two
/// sides differ meaningfully rather than silently comparing equal.
fn sheet_ref_constructs(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return vec![(String::new(), "unreadable_workbook".into(), String::new())];
    };
    let mut out = Vec::new();
    for (sheet_name, part_path) in sheets {
        let Ok(xml) = crate::ooxml::read_part(bytes, &part_path) else {
            out.push((sheet_name, "unreadable_sheet".into(), String::new()));
            continue;
        };
        for (kind, key) in structural::sheet_ref_construct_semantics(&xml) {
            out.push((sheet_name.clone(), kind, key));
        }
    }
    out.sort();
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Minimal 2-sheet workbook. `sheet2_extra` is appended inside Sheet2's
    /// `<worksheet>` (before `</worksheet>`); `extra_parts` adds arbitrary parts.
    fn wb(sheet2_extra: &str, extra_parts: &[(&str, &str)]) -> Vec<u8> {
        let ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let pkg = "http://schemas.openxmlformats.org/package/2006/relationships";
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, body: &str| {
            z.start_file(name, o).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        };
        put(
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
        );
        put(
            "_rels/.rels",
            &format!(
                r#"<?xml version="1.0"?><Relationships xmlns="{pkg}"><Relationship Id="rId1" Type="{r}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        );
        put(
            "xl/workbook.xml",
            &format!(
                r#"<?xml version="1.0"?><workbook xmlns="{ns}" xmlns:r="{r}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/><sheet name="Sheet2" sheetId="2" r:id="rId2"/></sheets></workbook>"#
            ),
        );
        put(
            "xl/_rels/workbook.xml.rels",
            &format!(
                r#"<?xml version="1.0"?><Relationships xmlns="{pkg}"><Relationship Id="rId1" Type="{r}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{r}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#
            ),
        );
        put(
            "xl/worksheets/sheet1.xml",
            &format!(
                r#"<?xml version="1.0"?><worksheet xmlns="{ns}"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#
            ),
        );
        put(
            "xl/worksheets/sheet2.xml",
            &format!(
                r#"<?xml version="1.0"?><worksheet xmlns="{ns}"><sheetData/>{sheet2_extra}</worksheet>"#
            ),
        );
        for (n, b) in extra_parts {
            put(n, b);
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn x14_conditional_formatting_is_compared_not_presence_refused() {
        // x14 CF (and legacy CF/DV) are now COMPARED, not refused on presence — presence
        // refusal rejected xlq's own transform of any workbook with a CF rule.
        let x14 = |sqref: &str| {
            format!(
                r#"<extLst><ext uri="{{x}}"><x14:conditionalFormatting xmlns:x14="urn:x14"><x14:cfRule type="expression" id="{{1}}"><xm:f xmlns:xm="urn:xm">$D$1&gt;0</xm:f></x14:cfRule><xm:sqref xmlns:xm="urn:xm">{sqref}</xm:sqref></x14:conditionalFormatting></ext></extLst>"#
            )
        };
        let good = wb(&x14("D1:D5"), &[]);
        let mangled = wb(&x14("Z9:Z99"), &[]);
        // identical x14 CF -> NO mismatch (must not blanket-refuse)
        assert!(verify_noncell_refs(&good, &good).is_none());
        // a mangled x14 sqref -> caught as a construct mismatch
        let refusal = verify_noncell_refs(&good, &mangled).expect("mangled x14 CF must be caught");
        assert_eq!(refusal["reason"], "sheet_construct_mismatch");
    }

    #[test]
    fn legacy_conditional_formatting_is_compared_not_presence_refused() {
        let cf = |sqref: &str| {
            format!(
                r#"<conditionalFormatting sqref="{sqref}"><cfRule type="expression" priority="1"><formula>$A1&gt;0</formula></cfRule></conditionalFormatting>"#
            )
        };
        let good = wb(&cf("A1:A10"), &[]);
        // A faithful workbook with a CF rule must NOT be refused (the over-refusal fix).
        assert!(verify_noncell_refs(&good, &good).is_none());
        // A mangled CF sqref is caught.
        let refusal = verify_noncell_refs(&good, &wb(&cf("A1:A99"), &[]))
            .expect("mangled CF sqref must be caught");
        assert_eq!(refusal["reason"], "sheet_construct_mismatch");
    }

    #[test]
    fn drawing_anchor_is_compared_not_presence_refused() {
        // A drawing's cell anchor is COMPARED (not refused on presence) — refusing it
        // rejected xlq's own transform of any workbook with a chart or image.
        let draw = |row: &str| {
            (
                "xl/drawings/drawing1.xml",
                format!(
                    r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>{row}</xdr:row></xdr:from></xdr:oneCellAnchor></xdr:wsDr>"#
                ),
            )
        };
        let (n, g) = draw("1");
        let good = wb("", &[(n, g.as_str())]);
        // identical drawing -> NOT refused
        assert!(verify_noncell_refs(&good, &good).is_none());
        // a mangled anchor row -> caught
        let (n2, m) = draw("99");
        let refusal = verify_noncell_refs(&good, &wb("", &[(n2, m.as_str())]))
            .expect("mangled drawing anchor must be caught");
        assert_eq!(refusal["reason"], "chart_drawing_mismatch");
    }

    #[test]
    fn autofilter_criteria_is_compared() {
        // The filter PREDICATE (customFilter val) is a value input to SUBTOTAL/AGGREGATE.
        let af = |v: &str| {
            format!(
                r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><customFilters><customFilter operator="lessThanOrEqual" val="{v}"/></customFilters></filterColumn></autoFilter>"#
            )
        };
        let good = wb(&af("5"), &[]);
        assert!(verify_noncell_refs(&good, &good).is_none());
        let refusal =
            verify_noncell_refs(&good, &wb(&af("9"), &[])).expect("criterion change must refuse");
        assert_eq!(refusal["reason"], "autofilter_criteria_mismatch");
    }

    #[test]
    fn caches_equal_matches_text_or_number() {
        assert!(caches_equal("55", "55"));
        assert!(caches_equal("55", "55.0")); // benign renumber of the same value
        assert!(caches_equal("5.5E1", "55"));
        assert!(!caches_equal("55", "56"));
        assert!(caches_equal("hello", "hello"));
        assert!(!caches_equal("hello", "world"));
    }

    #[test]
    fn unverified_formula_caches_flags_present_not_dropped() {
        // A formula cell (in Sheet2's body) with various stored caches.
        let cell = |v: &str| format!(r#"<row r="1"><c r="Z1"><f>SUM(A1:A2)</f>{v}</c></row>"#);
        let blank = wb(&cell("<v />"), &[]); // xlq blanks a shifted cache
        let fabricated = wb(&cell("<v>999</v>"), &[]); // foreign fabricates one
        let dropped = wb(&cell(""), &[]); // openpyxl drops it (no <v>)
        let honest = wb(&cell("<v>3</v>"), &[]);
        let honest_renum = wb(&cell("<v>3.0</v>"), &[]);
        // present cache the transform did not vouch -> counted.
        assert_eq!(unverified_formula_caches(&blank, &fabricated, false), 1);
        // a dropped cache (no <v>) -> Excel recomputes -> not counted.
        assert_eq!(unverified_formula_caches(&blank, &dropped, false), 0);
        // identical present caches, and a benign 3 vs 3.0 renumber -> not counted.
        assert_eq!(unverified_formula_caches(&honest, &honest, false), 0);
        assert_eq!(unverified_formula_caches(&honest, &honest_renum, false), 0);
        // a present cache that DIFFERS from the transform's present cache -> counted.
        assert_eq!(unverified_formula_caches(&honest, &fabricated, false), 1);
        // when xlq's transform FORCES recalc, its own caches are moot: an identical present
        // edited cache (which would otherwise verify) is unverifiable because the transform
        // discards it and recomputes -> every present edited cache is counted.
        assert_eq!(unverified_formula_caches(&honest, &honest, true), 1);
    }

    #[test]
    fn hidden_row_under_subtotal_is_compared() {
        // Sheet2 carries SUBTOTAL(109,...) and a data row; hiding that row changes the
        // aggregate with no formula/cache diff, so certify must compare the hidden-row set.
        let sheet = |hidden: &str| {
            format!(
                r#"<sheetData><row r="1"><c r="A1"><f>SUBTOTAL(109,A2:A3)</f></c></row><row r="2"{hidden}><c r="A2"><v>5</v></c></row><row r="3"><c r="A3"><v>5</v></c></row></sheetData>"#
            )
        };
        let good = wb(&sheet(""), &[]);
        assert!(verify_noncell_refs(&good, &good).is_none());
        let hidden = wb(&sheet(r#" hidden="1""#), &[]);
        let refusal = verify_noncell_refs(&good, &hidden)
            .expect("hiding a data row under SUBTOTAL(109) must refuse");
        assert_eq!(refusal["reason"], "hidden_row_subtotal_mismatch");
    }

    #[test]
    fn hidden_row_without_excluding_aggregate_is_ignored() {
        // The same hidden row on a sheet with only SUBTOTAL(9) (or no aggregate) is pure
        // display state -> NOT compared, so no over-refusal.
        let sheet = |hidden: &str| {
            format!(
                r#"<sheetData><row r="1"><c r="A1"><f>SUBTOTAL(9,A2:A3)</f></c></row><row r="2"{hidden}><c r="A2"><v>5</v></c></row></sheetData>"#
            )
        };
        let good = wb(&sheet(""), &[]);
        let hidden = wb(&sheet(r#" hidden="1""#), &[]);
        assert!(verify_noncell_refs(&good, &hidden).is_none());
    }

    #[test]
    fn control_binding_repoint_is_caught() {
        // A form control re-pointed to a different cell (linkedCell $A$5 -> $A$99) is a
        // value/behavior change the cell diff never sees; certify must compare the binding.
        let ctl = |target: &str| {
            format!(r#"<controls><control><controlPr linkedCell="{target}"/></control></controls>"#)
        };
        let good = wb(&ctl("Sheet1!$A$5"), &[]);
        assert!(verify_noncell_refs(&good, &good).is_none());
        let repointed = wb(&ctl("Sheet1!$A$99"), &[]);
        let refusal = verify_noncell_refs(&good, &repointed).expect("control re-point must refuse");
        assert_eq!(refusal["reason"], "control_binding_mismatch");
    }

    #[test]
    fn array_formula_flag_is_compared() {
        // Turning a plain formula into a legacy CSE array (t="array") is value-affecting on
        // non-dynamic Excel but stripped by the engine on load; certify compares the flag.
        let f = |t: &str| format!(r#"<row><c r="C1"><f{t}>SUM(A1:A3*A1:A3)</f></c></row>"#);
        let plain = wb(&f(""), &[]);
        assert!(verify_noncell_refs(&plain, &plain).is_none());
        let array = wb(&f(r#" t="array" ref="C1:C1""#), &[]);
        assert_eq!(
            verify_noncell_refs(&plain, &array).expect("plain->array must refuse")["reason"],
            "array_formula_mismatch"
        );
        // widening the array extent (materializing spilled cells) is caught too.
        let wide = wb(&f(r#" t="array" ref="C1:C3""#), &[]);
        assert_eq!(
            verify_noncell_refs(&array, &wide).expect("widened array ref must refuse")["reason"],
            "array_formula_mismatch"
        );
    }

    #[test]
    fn normalized_tokens_compared_per_cell() {
        // `@A1:A10` (implicit intersection -> a scalar) vs bare `A1:A10` (a spilling array) is
        // a value change the engine normalizes away on load. The compare is PER CELL, so both
        // a drop and a same-sheet RELOCATION (per-sheet count unchanged) are caught.
        let cell = |r: &str, f: &str| format!(r#"<row><c r="{r}"><f>{f}</f></c></row>"#);
        let good = wb(
            &format!("{}{}", cell("C1", "@A1:A10"), cell("C5", "A1:A10")),
            &[],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        // DROP the @.
        let dropped = wb(
            &format!("{}{}", cell("C1", "A1:A10"), cell("C5", "A1:A10")),
            &[],
        );
        assert_eq!(
            verify_noncell_refs(&good, &dropped).expect("@ drop must refuse")["reason"],
            "normalized_token_mismatch"
        );
        // RELOCATE the @ from C1 to C5 — Sheet2's total @ count is still 1, but the per-cell
        // map differs, so it is caught (a per-sheet count would miss this).
        let moved = wb(
            &format!("{}{}", cell("C1", "A1:A10"), cell("C5", "@A1:A10")),
            &[],
        );
        assert_eq!(
            verify_noncell_refs(&good, &moved).expect("@ relocation must refuse")["reason"],
            "normalized_token_mismatch"
        );
    }

    #[test]
    fn drawing_shape_hyperlink_target_is_compared() {
        // A shape hyperlink (a:hlinkClick r:id) resolves via the drawing's rels to a URL;
        // a foreign retarget (phishing swap) must be caught.
        let parts = |url: &str| {
            vec![
                (
                    "xl/drawings/drawing1.xml".to_string(),
                    r#"<xdr:wsDr xmlns:xdr="urn:xdr" xmlns:a="urn:a"><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="1"><a:hlinkClick xmlns:r="urn:r" r:id="rIdH"/></xdr:cNvPr></xdr:nvSpPr></xdr:sp></xdr:wsDr>"#.to_string(),
                ),
                (
                    "xl/drawings/_rels/drawing1.xml.rels".to_string(),
                    format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="{url}" TargetMode="External"/></Relationships>"#),
                ),
            ]
        };
        let g = parts("https://good.example.com");
        let good = wb(
            "",
            &g.iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        let ev = parts("https://evil.example.com/phish");
        let evil = wb(
            "",
            &ev.iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
        );
        let refusal =
            verify_noncell_refs(&good, &evil).expect("drawing hyperlink retarget must be caught");
        assert_eq!(refusal["reason"], "chart_drawing_mismatch");
    }

    #[test]
    fn chart_data_ref_is_compared_not_presence_refused() {
        let chart = |rng: &str| {
            (
                "xl/charts/chart1.xml",
                format!(
                    r#"<c:chartSpace xmlns:c="urn:c"><c:ser><c:val><c:numRef><c:f>Sheet2!{rng}</c:f></c:numRef></c:val></c:ser></c:chartSpace>"#
                ),
            )
        };
        let (n, g) = chart("$B$1:$B$10");
        let good = wb("", &[(n, g.as_str())]);
        // identical chart -> NOT refused (over-refusal fix)
        assert!(verify_noncell_refs(&good, &good).is_none());
        // a mangled chart data range -> caught
        let (n2, m) = chart("$Z$1:$Z$99");
        let refusal = verify_noncell_refs(&good, &wb("", &[(n2, m.as_str())]))
            .expect("mangled chart data ref must be caught");
        assert_eq!(refusal["reason"], "chart_drawing_mismatch");
    }

    #[test]
    fn comment_part_is_certify_safe() {
        // A cell comment/note carries only a display anchor + text (no value-affecting
        // reference); certify must not refuse xlq's own transform of a commented workbook.
        let bytes = wb(
            "",
            &[(
                "xl/comments1.xml",
                r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>A</author></authors><commentList><comment ref="A5" authorId="0"><text><t>note</t></text></comment></commentList></comments>"#,
            )],
        );
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }

    #[test]
    fn table_reference_surface_is_compared_not_refused() {
        // An Excel Table is COMPARED, not refused on presence (over-refusal fix): an identical
        // table certifies, but a mangled `ref` (or renamed table / changed column formula) is
        // caught even though the cell diff never compares the table part.
        let tbl = |rng: &str, colf: &str| {
            format!(
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="T1" displayName="T1" ref="{rng}"><tableColumns count="1"><tableColumn id="1" name="Amt"><calculatedColumnFormula>{colf}</calculatedColumnFormula></tableColumn></tableColumns></table>"#
            )
        };
        let good = wb("", &[("xl/tables/table1.xml", &tbl("A1:B2", "B1*2"))]);
        // identical table -> NOT refused
        assert!(verify_noncell_refs(&good, &good).is_none());
        // a mangled extent -> caught
        let bad_ref = wb("", &[("xl/tables/table1.xml", &tbl("A1:B99", "B1*2"))]);
        assert_eq!(
            verify_noncell_refs(&good, &bad_ref).expect("mangled table ref must refuse")["reason"],
            "table_reference_mismatch"
        );
        // a mangled column formula -> caught
        let bad_f = wb("", &[("xl/tables/table1.xml", &tbl("A1:B2", "B1*999"))]);
        assert_eq!(
            verify_noncell_refs(&good, &bad_f).expect("mangled table formula must refuse")
                ["reason"],
            "table_reference_mismatch"
        );
    }

    #[test]
    fn attr_relid_is_namespace_prefix_insensitive() {
        // REGRESSION: a literal "r:id" lookup let a rebound prefix (x:id) hide a hyperlink's
        // external target — a phishing-URL swap that certified. Match by local name instead.
        assert_eq!(
            attr_relid(r#"<hyperlink ref="A1" r:id="rId5"/"#).as_deref(),
            Some("rId5")
        );
        assert_eq!(
            attr_relid(r#"<hyperlink ref="A1" x:id="rId9"/"#).as_deref(),
            Some("rId9")
        );
        assert_eq!(attr_relid(r#"<hyperlink ref="A1"/"#), None);
        // XML-legal whitespace around `=` (Eq ::= S? '=' S?) must not hide the value.
        assert_eq!(
            attr_relid(r#"<hyperlink ref="A1" r:id = "rId7"/"#).as_deref(),
            Some("rId7")
        );
    }

    #[test]
    fn attr_matches_whole_name_and_tolerates_eq_whitespace() {
        // REGRESSION: `attr` did a literal `key=` substring search, so XML-legal whitespace
        // around `=` (`date1904 = "1"`, which Excel honors) read as the default — a foreign
        // edit could smuggle a value-affecting workbook setting past certify.
        assert_eq!(
            attr(r#"<workbookPr date1904 = "1"/>"#, "date1904").as_deref(),
            Some("1")
        );
        assert_eq!(
            attr(r#"<calcPr fullPrecision  =  '0'/>"#, "fullPrecision").as_deref(),
            Some("0")
        );
        // No collision with a longer attribute that merely ENDS in the key (`guid` vs `id`).
        assert_eq!(attr(r#"<x guid="abc"/>"#, "id"), None);
        assert_eq!(
            attr(r#"<x guid="abc" id="7"/>"#, "id").as_deref(),
            Some("7")
        );
        // A key that is a PREFIX of another attribute is not confused (`iterate` vs
        // `iterateCount`), and the plain no-whitespace form still works.
        assert_eq!(
            attr(r#"<calcPr iterateCount="99" iterate="1"/>"#, "iterate").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn structural_ref_attrs_captures_hyperlink_destination() {
        // The hyperlink's DESTINATION (internal location + external r:id->Target) must be in
        // the comparison key, so a foreign retarget (mispoint / phishing URL) is caught.
        let bytes = wb(
            r#"<hyperlinks><hyperlink ref="A1" location="Sheet2!C3"/><hyperlink xmlns:r="urn:r" ref="A2" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="https://good.example.com/safe" TargetMode="External"/></Relationships>"#,
            )],
        );
        let keys: Vec<String> = structural_ref_attrs(&bytes)
            .into_iter()
            .filter(|(_, e, _)| e == "hyperlink")
            .map(|(_, _, k)| k)
            .collect();
        assert!(
            keys.iter().any(|k| k.contains("loc=Sheet2!C3")),
            "internal location captured: {keys:?}"
        );
        assert!(
            keys.iter()
                .any(|k| k.contains("tgt=https://good.example.com/safe")),
            "external target captured: {keys:?}"
        );
    }

    #[test]
    fn a_plain_two_sheet_workbook_has_no_unverified_construct() {
        // Guard against over-refusal: a workbook with no ref-bearing constructs passes.
        let bytes = wb("", &[]);
        assert!(sheet_ref_constructs(&bytes).is_empty());
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }
}
