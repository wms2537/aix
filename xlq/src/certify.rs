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
    let mut expected_model =
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
        // A faithful foreign edit (the normal Excel/LibreOffice save) PRESERVES each shifted
        // formula's correct stored cache, but xlq's own transform BLANKS them (it cannot
        // recompute engine-free) — so a stored-cache-vs-stored-cache comparison alone refuses
        // the common case. When the engine fully and deterministically covers xlq's proven
        // transform, evaluate it and vouch each foreign cache against the TRUE computed value:
        // a correct cache is certified, a fabricated or stale one still differs (a strict
        // strengthening — the prior comparison could not tell 55 from 999). Gated on coverage
        // so an unsupported/volatile function never launders a wrong value.
        //
        // The oracle is ALSO disabled under "precision as displayed" (`<calcPr
        // fullPrecision="0">`): there Excel computes on the ROUNDED DISPLAYED value of each cell,
        // but ironcalc's `evaluate()` always computes at FULL precision, so its value diverges
        // from Excel's true result (`=A1` with `A1`=1.4 formatted "0" is 1 in Excel, 1.4 in
        // ironcalc). Vouching the full-precision cache would CERTIFY a wrong value and REFUSE the
        // faithful displayed-precision one; without the oracle a present cache under this mode
        // stays unverified (the safe, conservative refusal).
        let oracle =
            if precision_as_displayed(&expected_bytes) || precision_as_displayed(&edited_bytes) {
                None
            } else {
                // date1904 read from EITHER file (they must agree — a flip is caught separately by
                // sheet_order_and_settings; reading both is belt-and-suspenders).
                let date1904 =
                    workbook_is_date1904(&expected_bytes) || workbook_is_date1904(&edited_bytes);
                build_cache_oracle(&mut expected_model, date1904)
            };
        // A volatile cell's cache is self-healing ONLY when Excel recomputes it on load — i.e.
        // AUTO calc mode (we are already in the branch where fullCalcOnLoad is NOT set on the
        // edited file). Under MANUAL mode Excel shows the stored cache verbatim, so a volatile
        // cache must be verified like any other; the skip set is empty there (fail-closed). The
        // set is TRANSITIVE (a non-volatile dependent of a volatile cell is included), computed
        // from xlq's proven transform via the engine's dependency graph.
        let volatile_tainted = if manual_calc_mode(&edited_bytes) {
            std::collections::HashSet::new()
        } else {
            volatile_tainted_cells(&expected_bytes, original)
        };
        unverified_formula_caches(
            &expected_bytes,
            &edited_bytes,
            recalc_on_load_forced(&expected_bytes),
            oracle.as_ref(),
            &volatile_tainted,
        )
    };
    // A `format` (number-format) difference is normally benign — display only. But it becomes a
    // VALUE input in two cases: (1) under "precision as displayed" (`<calcPr fullPrecision="0">`)
    // Excel computes formulas on the ROUNDED displayed values, so changing `A1`'s format from
    // "0.00" to "0" rounds 1.44→1 and recomputes `=A1*10` as 10 instead of 14.4; (2) a
    // `CELL("format"/"color"/"parentheses", A1)` formula reads `A1`'s number format directly, so
    // restyling `A1` changes that formula's result. In either case format diffs are disqualifying.
    let format_disqualifying =
        if precision_as_displayed(&edited_bytes) || has_format_sensitive_cell_fn(&edited_bytes) {
            counts.format
        } else {
            0
        };
    let disqualifying = counts.formula
        + counts.value
        + counts.added
        + counts.removed
        + unverified_caches as u64
        + format_disqualifying;
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
    // PIVOT tables/caches carry a source range (`<worksheetSource ref>`), a render location, and
    // a connection binding the cell diff never sees. The transform shifts the edited-sheet
    // source and preserves the rest, so a faithful edit matches and a mangle (a repointed
    // source, a moved render extent, a re-bound connection) differs. COMPARED, not
    // presence-refused — refusing on presence rejected xlq's own transform of ANY pivot workbook.
    if pivot_refs(expected) != pivot_refs(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "pivot_reference_mismatch",
            "detail": "a PivotTable/PivotCache source range, render location, or connection \
                       binding differs from xlq's transform — a reference the cell diff misses",
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
    // EXTERNAL DATA-SOURCE targets (connections.xml url/command/connection string), their
    // query-table connection bindings, and customUI autorun callbacks — allowlisted as
    // carrying no shiftable cell coordinate, but never compared. xlq's transform copies them
    // verbatim, so a foreign edit that REPOINTS a data source (SSRF/exfiltration + injected
    // refresh data) or INJECTS an autorun ribbon callback must not certify. Compare them.
    if opaque_target_signature(expected) != opaque_target_signature(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "external_target_mismatch",
            "detail": "an external data-source target (a connections.xml URL / SQL command / \
                       connection string), a query-table connection binding, or a customUI \
                       autorun callback differs from xlq's transform — a value/security change \
                       the cell diff cannot see",
        }));
    }
    // Fail-closed ALLOWLIST over PARTS. certify positionally compares only worksheet cells
    // (diff::snapshot), defined names, and the mergeCell/hyperlink/autoFilter refs above.
    // Any OTHER part can carry a cell reference that comparison never sees — charts,
    // drawings, tables, pivots, external links, comments, form controls, but also the
    // long tail (queryTables, metadata/richData, slicerCaches, timelineCaches,
    // connections, customXml, volatileDependencies, …). Rather than enumerate that open-ended
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
        || low == "xl/volatiledependencies.xml"      // rebuildable volatile/RTD dep cache (the
                                                     // volatile analog of calcChain); restructure
                                                     // DROPS it, but a foreign edit may keep it —
                                                     // value-inert, no verifiable coordinate
        || low == "xl/metadata.xml"                  // dynamic-array/rich-value metadata: index-
                                                     // linked to cells (cm/vm), no shiftable coord
        || low.starts_with("xl/richdata/")           // rich values (=IMAGE(), Stocks/Geography):
                                                     // index-linked from cells via `vm`, no coord
        || low.starts_with("customui/")              // ribbon extensibility XML: no cell coord
                                                     // (callbacks are VBA macro-name strings)
        || low == "xl/connections.xml"               // external data source defs, no cell coord
        || low.starts_with("xl/querytables/")        // query-table field defs (extent is in the
                                                     // associated table part, compared there)
        || low.starts_with("xl/ctrlprops/")          // modern form-control props — its fmlaLink/
                                                     // fmlaRange bindings ARE compared (below)
        || (low.starts_with("xl/pivotcache/") && low.ends_with(".xml"))  // pivot cache defn/records:
                                                     // worksheetSource ref compared via pivot_refs
        || (low.starts_with("xl/pivottables/") && low.ends_with(".xml")) // pivot table: location/
                                                     // source refs compared via pivot_refs
        || low.starts_with("xl/theme/")              // colors/fonts
        || low.starts_with("docprops/")              // document metadata
        || low.starts_with("customxml/")             // inert custom-XML data island: Excel
                                                     // formulas cannot read it, no coordinate
        || low.starts_with("xl/media/")              // embedded images
        || low.starts_with("xl/printersettings/")    // opaque binary print settings
        || low.starts_with("xl/charts/")             // chart data refs — compared semantically
        || low.starts_with("xl/drawings/")           // drawing anchors — compared semantically
        || low.starts_with("xl/tables/")             // Excel Table — ref/name/formulas compared
        || low.starts_with("xl/comments")            // cell comment/note: display anchor + text,
        || low.starts_with("xl/threadedcomments/")   // no value-affecting reference (an anchor on
        || low.starts_with("xl/persons/")            // the EDITED sheet is caught upstream as a
                                                     // bad attachment before certify runs)
        || low.starts_with("xl/slicercaches/")       // slicer / timeline filter widgets: bind to a
        || low.starts_with("xl/slicers/")            // pivot/table by NAME/ID and hold selection
        || low.starts_with("xl/timelinecaches/")     // state — no shiftable A1 coordinate (like
        || low.starts_with("xl/timelines/")          // the pivot parts). Their filter effect
                                                     // surfaces in the pivot's cached output cells.
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

/// A normalized, order-independent signature of the SECURITY-relevant parts certify
/// otherwise passes through untouched via the fail-closed allowlist: the external DATA
/// SOURCES (`xl/connections.xml` — a `<webPr url>` web query, a `<dbPr command>` SQL
/// string, an ODBC/OLEDB `connection` string, an OLAP source) and their query-table
/// bindings (`xl/queryTables/*`, whose `connectionId` selects which source fills a range),
/// plus the RIBBON extensibility callbacks (`customUI/*` — an `onLoad`/`onAction` names a
/// macro that autoruns on open). xlq's transform copies every one of these verbatim (they
/// carry no shiftable cell coordinate, which is WHY they are allowlisted), so a faithful
/// edit's signature matches — while a foreign edit that REPOINTS a data source (an SSRF /
/// intranet-URL exfiltration, with attacker-controlled data injected into the connected
/// cells on the next refresh — a value change no cell diff sees) or INJECTS an autorun
/// callback differs and is refused. Allowlisting without comparing them was a reachable
/// false-certification of a security change. Keyed by part CLASS, not exact name, so a
/// benign part renumber does not false-refuse; element/attribute order is normalized away
/// so a foreign tool's benign reserialization (which a byte compare would refuse) does not.
fn opaque_target_signature(bytes: &[u8]) -> Vec<String> {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        let class = if low == "xl/connections.xml" {
            "connections"
        } else if low.starts_with("xl/querytables/") && low.ends_with(".xml") {
            "querytable"
        } else if low.starts_with("customui/") && low.ends_with(".xml") {
            "customui"
        } else {
            continue;
        };
        let Ok(part) = crate::ooxml::read_part(bytes, n) else {
            continue;
        };
        for sig in element_attr_signatures(&part) {
            out.push(format!("{class}|{sig}"));
        }
    }
    out.sort();
    out
}

/// Every element in `xml` rendered as `local(attr=val;attr=val;…)` with attributes SORTED,
/// plus each non-empty trimmed text run as `#text(…)` — an element/attribute-order- and
/// whitespace-independent view of the part's content. A byte comparison would spuriously
/// refuse a foreign tool's benign reserialization; this catches any attribute-value or
/// text change (a repointed URL/command/connectionId/callback) while tolerating formatting.
/// Namespace-prefix-agnostic (local names only).
fn element_attr_signatures(xml: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local =
                    String::from_utf8_lossy(structural::local_of(e.name().as_ref())).into_owned();
                let mut attrs: Vec<String> = e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .map(|a| {
                        let key = String::from_utf8_lossy(structural::local_of(a.key.as_ref()))
                            .into_owned();
                        let val = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map(|v| v.into_owned())
                            .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
                        format!("{key}={val}")
                    })
                    .collect();
                attrs.sort();
                out.push(format!("{local}({})", attrs.join(";")));
            }
            Ok(Event::Text(t)) => {
                let text = String::from_utf8_lossy(t.as_ref());
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    out.push(format!("#text({trimmed})"));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
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
                // Canonicalize redundant sheet-name quoting: openpyxl/xlq write a chart series ref
                // QUOTED (`'Data'!$D$3`) while Excel/LibreOffice write it unquoted (`Data!$D$3`) —
                // semantically identical, so the raw compare spuriously refused a faithful chart
                // edit. (Same normalization already applied on the defined-name/CF/DV surfaces.)
                charts.extend(
                    structural::element_text_semantics(&x, &[b"f"])
                        .iter()
                        .map(|s| structural::canonicalize_sheet_quotes(s)),
                );
            }
        } else if low.starts_with("xl/drawings/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, n) {
                // Only the `<from>` anchor corner — tolerates the value-neutral oneCellAnchor <->
                // twoCellAnchor re-encoding every desktop editor performs (which changed the
                // col/row token count and spuriously refused a faithful chart re-save).
                drawings.extend(structural::drawing_from_anchors(&x));
                // A graphic-frame formula (`<xdr:f>`) — a linked OLE/picture object's source
                // cell — and a linked shape/textbox's `textlink="Sheet1!$A$8"` attribute are
                // LIVE cell references the shape mirrors. The transform refuses an edit that
                // moves one and copies the drawing verbatim otherwise, so a foreign RE-POINT
                // (mirroring a different cell) must differ. The cell diff never sees them.
                drawings.extend(
                    structural::element_text_semantics(&x, &[b"f"])
                        .iter()
                        .map(|s| structural::canonicalize_sheet_quotes(s)),
                );
                for (_, attrs) in structural::element_attr_semantics(
                    &x,
                    &[b"sp", b"cxnSp", b"pic", b"graphicFrame"],
                ) {
                    if let Some(tl) = attrs
                        .split_whitespace()
                        .find_map(|kv| kv.strip_prefix("textlink="))
                    {
                        drawings.push(format!("textlink={tl}"));
                    }
                }
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

/// The reference/extent surface of every PivotTable and PivotCache across
/// `xl/pivotCache/*` + `xl/pivotTables/*`, as a sorted list (keyed by neither part name nor
/// order, so a benign part renumber does not false-refuse). The transform SHIFTS a
/// `<worksheetSource ref>` whose `sheet` is the edited one and REFUSES any other pivot
/// reference to the edited sheet, so a pivot that survives to certify is one xlq left faithful;
/// a foreign edit that mangles the source range, the render location, or the connection binding
/// differs and is refused. Comparing this lets certify allowlist pivots (which carry a cell
/// coordinate the cell diff misses) instead of blanket-refusing every workbook that has one —
/// including xlq's own correct transform.
fn pivot_refs(bytes: &[u8]) -> Vec<String> {
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if (low.starts_with("xl/pivotcache/") || low.starts_with("xl/pivottables/"))
            && low.ends_with(".xml")
        {
            let Ok(x) = crate::ooxml::read_part(bytes, n) else {
                continue;
            };
            for (tag, attrs) in structural::element_attr_semantics(
                &x,
                &[
                    b"worksheetSource",
                    b"rangeSet",
                    b"location",
                    b"cacheSource",
                    b"dataField",
                    b"pivotCacheDefinition",
                ],
            ) {
                let pick = |key: &str| {
                    attrs
                        .split_whitespace()
                        .find_map(|kv| kv.strip_prefix(key))
                        .unwrap_or("")
                        .to_string()
                };
                let sig = match tag.as_str() {
                    // A `<dataField>`'s aggregation (`subtotal`, default "sum" when absent) is the
                    // VALUE the pivot materializes — a SUM->COUNT flip changes the output column.
                    "dataField" => {
                        let st = pick("subtotal=");
                        let st = if st.is_empty() { "sum".to_string() } else { st };
                        format!(
                            "dataField|name={}|fld={}|subtotal={st}|baseField={}|baseItem={}",
                            pick("name="),
                            pick("fld="),
                            pick("baseField="),
                            pick("baseItem="),
                        )
                    }
                    // `refreshOnLoad="1"` makes Excel recompute the pivot cache on open with no user
                    // action — so an injected refresh + an aggregation/source change materializes a
                    // corrupted value on load. Normalize to a bool (absent/0/false all mean off).
                    "pivotCacheDefinition" => {
                        let rol = pick("refreshOnLoad=");
                        let rol = if rol == "1" || rol.eq_ignore_ascii_case("true") {
                            "1"
                        } else {
                            "0"
                        };
                        format!("pivotCacheDefinition|refreshOnLoad={rol}")
                    }
                    _ => format!(
                        "{tag}|sheet={}|ref={}|name={}|conn={}|type={}",
                        pick("sheet="),
                        pick("ref="),
                        pick("name="),
                        pick("connectionId="),
                        pick("type="),
                    ),
                };
                out.push(sig);
            }
        }
    }
    out.sort();
    out
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
        if (low.starts_with("xl/worksheets/") && low.ends_with(".xml"))
            || low.starts_with("xl/ctrlprops/")
        {
            // Worksheet controlPr bindings AND modern `xl/ctrlProps/*` <formControlPr> bindings
            // (fmlaLink/fmlaRange/…) — the allowlist marks ctrlProps known-safe only because its
            // bindings are compared here.
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

/// The set of manually hidden rows on each worksheet, keyed by sheet, sorted — but ONLY when the
/// workbook uses a hidden-row-excluding aggregate (`SUBTOTAL(101–111)` / hidden-ignoring
/// `AGGREGATE`) SOMEWHERE. Such an aggregate can reference ANY sheet's rows — a cross-sheet
/// `Sheet2!B1 = SUBTOTAL(109, Sheet1!A1:A10)` takes its hidden-row input from the REFERENCED sheet
/// (Sheet1), not the aggregate's own sheet — so keying the guard to each aggregate's own sheet
/// let a foreign edit hide a data row on the referenced sheet and certify a value change. If any
/// aggregate is present, a manually hidden row on ANY sheet is potentially value-affecting, so
/// compare every sheet's hidden-row set (a sound over-approximation); with no such aggregate,
/// a hidden row is pure display state and ignored.
fn subtotal_hidden_rows(bytes: &[u8]) -> Vec<(String, Vec<String>)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let sheet_xml: Vec<(String, Vec<u8>)> = sheets
        .into_iter()
        .filter_map(|(name, part)| {
            crate::ooxml::read_part(bytes, &part)
                .ok()
                .map(|x| (name, x))
        })
        .collect();
    let any_aggregate = sheet_xml
        .iter()
        .any(|(_, x)| structural::hidden_row_exclusion_present(x));
    if !any_aggregate {
        return Vec::new();
    }
    let mut out: Vec<(String, Vec<String>)> = sheet_xml
        .iter()
        .map(|(name, x)| (name.clone(), structural::hidden_rows(x)))
        .filter(|(_, rows)| !rows.is_empty())
        .collect();
    out.sort();
    out
}

/// The autoFilter FILTER-CRITERION elements across every worksheet AND every Excel Table
/// (`xl/tables/*.xml`), keyed by owner, as sorted attribute strings. The transform preserves
/// these verbatim, so a foreign change to which rows the filter hides — a value input to
/// `SUBTOTAL(1–11)` (excludes FILTER-hidden rows) / `SUBTOTAL(101–111)` / hidden-ignoring
/// `AGGREGATE` — is caught. A TABLE carries its own `<autoFilter>`, so scanning only worksheets
/// let a table-filter change (feeding a table `SUBTOTAL`) certify silently.
fn autofilter_criteria(bytes: &[u8]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut extract = |owner: &str, x: &[u8]| {
        for (elem, attrs) in structural::element_attr_semantics(
            x,
            &[
                b"filterColumn",
                // Container attributes that change WHICH rows are hidden: `<customFilters and>`
                // (the AND/OR combinator over two predicates) and `<filters blank>` (show-blanks).
                b"customFilters",
                b"customFilter",
                b"filters",
                b"filter",
                b"dynamicFilter",
                b"top10",
                b"dateGroupItem",
                b"colorFilter",
                b"iconFilter",
            ],
        ) {
            // On a `<filterColumn>`, `hiddenButton`/`showButton` govern ONLY the filter DROPDOWN
            // BUTTON's visibility (pure display), so drop them (openpyxl writes them at defaults).
            let attrs = if elem == "filterColumn" {
                attrs
                    .split(' ')
                    .filter(|t| !t.starts_with("hiddenButton=") && !t.starts_with("showButton="))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                attrs
            };
            out.push((owner.to_string(), elem, attrs));
        }
    };
    if let Ok(sheets) = crate::ooxml::all_sheets(bytes) {
        for (sheet_name, part) in sheets {
            if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
                extract(&sheet_name, &x);
            }
        }
    }
    // Table autoFilters, keyed by CLASS ("table") not part name so a benign renumber does not
    // false-refuse (a real filter change still differs within the sorted set).
    for n in structural::archive_names(bytes).unwrap_or_default() {
        let low = n.to_ascii_lowercase();
        if low.starts_with("xl/tables/") && low.ends_with(".xml") {
            if let Ok(x) = crate::ooxml::read_part(bytes, &n) {
                extract("table", &x);
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
        // `<workbookProtection>` (structure/window lock) and `<fileSharing>` (the workbook-level
        // WRITE-RESERVATION password — reservationPassword / the modern algorithmName+hashValue+
        // saltValue+spinCount hash — plus readOnlyRecommended). Stripping or weakening either is a
        // security downgrade the cell diff never sees.
        for (elem, attrs) in
            structural::element_attr_semantics(&wb, &[b"workbookProtection", b"fileSharing"])
        {
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
    // Canonicalize REDUNDANT sheet-name quoting in the refers-to body: openpyxl writes the
    // autofilter `_xlnm._FilterDatabase` name QUOTED (`'Data'!$A$1:$B$10`) while Excel/
    // LibreOffice write it unquoted (`Data!$A$1:$B$10`) — semantically identical, so comparing
    // the raw bodies spuriously refused a faithful edit of a ubiquitous autofilter workbook.
    // Re-sort afterward because the canonical body can reorder the (name, scope, refers) key.
    let mut names: Vec<(String, String, String)> = structural::defined_names(&wb)
        .into_iter()
        .map(|(name, scope, refers)| (name, scope, structural::canonicalize_sheet_quotes(&refers)))
        .collect();
    names.sort();
    names
}

/// (sheet-name, element, ref) for every mergeCell/hyperlink/autoFilter, sorted — the
/// semantic structural references the transform shifts. The owning sheet's NAME is part
/// of the key (resolved via the workbook relationships, robust to a foreign tool
/// renumbering sheet PARTS) so that RELOCATING a reference to a different sheet — which
/// leaves the cross-sheet multiset unchanged — is still detected as a divergence.
fn structural_ref_attrs(bytes: &[u8]) -> Vec<(String, String, String)> {
    use quick_xml::events::Event;
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part_path) in sheets {
        let Ok(part) = crate::ooxml::read_part(bytes, &part_path) else {
            continue;
        };
        // Per-sheet relationship targets, to resolve an external hyperlink's r:id -> URL.
        let rels = rels_targets(bytes, &part_path);
        // Namespace-aware walk keyed by LOCAL name. A raw `<hyperlink` substring scan (the old
        // code) was blind to a prefixed `<x:hyperlink>` (x bound to the spreadsheetML main
        // namespace) — a foreign editor injecting a prefixed external (phishing) hyperlink, or a
        // prefixed mergeCell/autoFilter change, evaded the comparison and CERTIFIED. This mirrors
        // the same fix already applied to defined_names().
        let mut reader = quick_xml::Reader::from_reader(part.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            let mut item = None;
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let elem = match structural::local_of(e.name().as_ref()) {
                        b"mergeCell" => Some("mergeCell"),
                        b"hyperlink" => Some("hyperlink"),
                        b"autoFilter" => Some("autoFilter"),
                        _ => None,
                    };
                    if let (Some(elem), Some(r)) = (elem, attr_local(&e, b"ref")) {
                        // For a hyperlink, the DESTINATION is also part of the semantic identity
                        // and is preserved verbatim by xlq's transform: the internal `location`
                        // (in-workbook jump) and the external `r:id` -> rels Target (the URL). A
                        // foreign edit that retargets either — an internal mispoint or a phishing
                        // URL swap — would otherwise leave (sheet, elem, ref) unchanged and certify.
                        let key = if elem == "hyperlink" {
                            let location = attr_local(&e, b"location").unwrap_or_default();
                            let target = rel_id(&e)
                                .and_then(|id| rels.get(&id).cloned())
                                .unwrap_or_default();
                            // A trailing slash on a URL navigates to the same resource;
                            // openpyxl/Excel add one to a bare authority
                            // (`https://example.com` -> `…/`). Strip a single trailing `/` so a
                            // benign renormalization is not a spurious mismatch — a real retarget
                            // (different host/path) still differs.
                            let target = target.strip_suffix('/').unwrap_or(&target);
                            // An in-workbook (internal) jump has two EQUIVALENT OOXML encodings for
                            // the SAME destination cell: (A) a relationship Target `#Data!A1` with
                            // no `location` (openpyxl and other library writers), and (B) a
                            // `location="Data!A1"` attribute with no relationship (Excel/
                            // LibreOffice). Comparing (location, target) as independent fields
                            // refused a faithful edit that merely round-tripped the encoding, so
                            // fold both to (dest, ext=""). A genuine external target (URL / other-
                            // workbook file) still lands in `ext`, so a real retarget (a phishing
                            // swap, a mispoint to another file) differs.
                            let (dest, ext) = if let Some(internal) = target.strip_prefix('#') {
                                (internal.to_string(), String::new())
                            } else if target.is_empty() {
                                (location.clone(), String::new())
                            } else {
                                (location.clone(), target.to_string())
                            };
                            format!("ref={r}|dest={dest}|ext={ext}")
                        } else {
                            format!("ref={r}")
                        };
                        item = Some((sheet_name.clone(), elem.to_string(), key));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            if let Some(it) = item {
                out.push(it);
            }
            buf.clear();
        }
    }
    out.sort();
    out
}

/// Value of the attribute whose LOCAL name is `local` (namespace-prefix-insensitive),
/// XML-attribute-normalized. Returns None if the attribute is absent.
fn attr_local(e: &quick_xml::events::BytesStart, local: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (structural::local_of(a.key.as_ref()) == local).then(|| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|c| c.into_owned())
                .unwrap_or_default()
        })
    })
}

/// The relationship id (`r:id`) of a start tag: the attribute whose LOCAL name is `id` AND
/// which carries a namespace prefix (`r:id`, not a bare `id`), matching `attr_relid`'s intent.
fn rel_id(e: &quick_xml::events::BytesStart) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let key = a.key.as_ref();
        (structural::local_of(key) == b"id" && key.contains(&b':')).then(|| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|c| c.into_owned())
                .unwrap_or_default()
        })
    })
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
        let truthy = |v: Option<String>| matches!(v.as_deref(), Some("1") | Some("true"));
        let wbpr = local_element_tag(&text, "workbookPr").unwrap_or_default();
        let calcpr = local_element_tag(&text, "calcPr").unwrap_or_default();
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
    let Some(tag) = local_element_tag(&text, "calcPr") else {
        return false;
    };
    matches!(
        attr(&tag, "fullCalcOnLoad").as_deref(),
        Some("1") | Some("true")
    )
}

/// True if the workbook is in MANUAL calc mode (`<calcPr calcMode="manual"/"manualNoRecalc">`).
/// In manual mode Excel does NOT recalculate on open — it displays every stored cache VERBATIM,
/// including a volatile cell's, until the user presses F9 — so a volatile cell's cache is NOT
/// self-healing there and must be verified like any other (its skip is unsound under manual mode).
fn manual_calc_mode(bytes: &[u8]) -> bool {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return false;
    };
    let text = String::from_utf8_lossy(&wb);
    let Some(tag) = local_element_tag(&text, "calcPr") else {
        return false;
    };
    matches!(
        attr(&tag, "calcMode").as_deref(),
        Some("manual") | Some("manualNoRecalc")
    )
}

/// True if the workbook computes formulas on the ROUNDED DISPLAYED values
/// (`<calcPr fullPrecision="0"/"false">`, "precision as displayed"). In that mode a cell's
/// number format is a value input to any formula reading it, so a format change is not benign.
fn precision_as_displayed(bytes: &[u8]) -> bool {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return false;
    };
    let text = String::from_utf8_lossy(&wb);
    let Some(tag) = local_element_tag(&text, "calcPr") else {
        return false;
    };
    matches!(
        attr(&tag, "fullPrecision").as_deref(),
        Some("0") | Some("false")
    )
}

/// True when any worksheet formula calls `CELL()` with a NUMBER-FORMAT-sensitive info type,
/// making a "format-only" foreign edit value-affecting. `CELL("format"/"color"/"parentheses",
/// A1)` returns a value DERIVED from `A1`'s number format, so restyling `A1` (numFmtId
/// `0`→`2`) changes the formula's Excel result — a difference `diff::classify_kind` labels
/// `format` and certify would otherwise treat as benign. A `CELL()` call whose first argument
/// is NOT a string literal has an info type certify cannot resolve, so it is treated
/// conservatively as sensitive. Info types that do not depend on the number format
/// (`contents`, `type`, `row`, `col`, `address`, …) do not trip this.
fn has_format_sensitive_cell_fn(bytes: &[u8]) -> bool {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return false;
    };
    sheets.into_iter().any(|(_name, part)| {
        crate::ooxml::read_part(bytes, &part).is_ok_and(|xml| {
            structural::element_text_semantics(&xml, &[b"f"])
                .iter()
                .any(|f| formula_calls_sensitive_cell(f))
        })
    })
}

/// The number-format-sensitive `CELL()` info types: a change to a cell's number format alters
/// each. (`prefix`/`protect`/`width` depend on alignment/protection/column width — style, not
/// number format — and so do not affect a `format`-classified diff.)
const CELL_FORMAT_SENSITIVE: [&str; 3] = ["format", "color", "parentheses"];

/// Scan one formula for a `CELL(<info>, …)` call whose `<info>` is a number-format-sensitive
/// literal, or is not a string literal at all (info type unresolvable -> conservative). String
/// literals and single-quoted sheet qualifiers are skipped so `="CELL(...)"` text and a sheet
/// named `CELL` do not false-trip.
fn formula_calls_sensitive_cell(f: &str) -> bool {
    let b = f.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        match b[i] {
            b'"' => {
                i += 1;
                while i < n {
                    if b[i] == b'"' {
                        if i + 1 < n && b[i + 1] == b'"' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                i += 1;
                while i < n {
                    if b[i] == b'\'' {
                        if i + 1 < n && b[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                    i += 1;
                }
                // Function name is the identifier's tail after any `_xlfn.` prefix.
                let name = f[start..i].rsplit('.').next().unwrap_or("");
                if name.eq_ignore_ascii_case("CELL") {
                    let mut j = i;
                    while j < n && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < n && b[j] == b'(' {
                        j += 1;
                        while j < n && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < n && b[j] == b'"' {
                            let s = j + 1;
                            let mut k = s;
                            while k < n && b[k] != b'"' {
                                k += 1;
                            }
                            if CELL_FORMAT_SENSITIVE
                                .iter()
                                .any(|t| f[s..k].eq_ignore_ascii_case(t))
                            {
                                return true;
                            }
                            // A format-INSENSITIVE literal: this call is safe, keep scanning.
                        } else {
                            // Non-literal info type -> cannot resolve -> conservative.
                            return true;
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    false
}

/// The first start-tag in `text` whose element LOCAL name is `local`, namespace-prefix
/// agnostic — both `<calcPr …>` and `<x:calcPr …>` match — returned from its `<` up to (not
/// including) the closing `>`. A raw `text.find("<calcPr")` missed a prefixed `<x:calcPr>`,
/// hiding value-affecting workbook settings (date1904/fullPrecision/calcMode/iterate) from the
/// settings compare — a false certification, since XML namespace resolution is prefix-agnostic
/// and Excel honors the prefixed form.
fn local_element_tag(text: &str, local: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(rel) = text[i..].find('<') {
        let lt = i + rel;
        i = lt + 1;
        let name_start = lt + 1;
        let mut j = name_start;
        while j < bytes.len() && !matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') {
            j += 1;
        }
        let name = &text[name_start..j];
        let local_name = name.rsplit(':').next().unwrap_or(name);
        if local_name == local {
            let gt = text[lt..].find('>')?;
            return Some(text[lt..lt + gt].to_string());
        }
    }
    None
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
fn unverified_formula_caches(
    expected: &[u8],
    edited: &[u8],
    expected_forced: bool,
    oracle: Option<&std::collections::HashMap<(String, String), String>>,
    volatile_tainted: &std::collections::HashSet<(String, String)>,
) -> usize {
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
        // xlq's OWN stored caches (a cell the transform did NOT blank — an unshifted formula).
        // When the transform force-recomputes it discards its own caches, so they vouch nothing.
        let exp_map = if expected_forced {
            Default::default()
        } else {
            exp_by_name
                .get(&name)
                .and_then(|p| crate::ooxml::read_part(expected, p).ok())
                .map(|x| structural::formula_cache_map(&x))
                .unwrap_or_default()
        };
        for (cell, ev) in &edt_map {
            // A cell Excel RECOMPUTES on load — a cell that transitively depends on a VOLATILE
            // function (NOW/RAND/OFFSET/INDIRECT/…) in auto-calc mode — never surfaces a stale
            // stored value, so its preserved cache is skipped. The set is TRANSITIVE (a
            // non-volatile dependent `A2=A1` where `A1=NOW()` is included) and EMPTY under manual
            // calc mode, where Excel shows the stored cache verbatim and it must be verified.
            if volatile_tainted.contains(&(name.clone(), cell.clone())) {
                continue;
            }
            // (a) the transform kept an identical stored cache for this cell, or
            let by_stored =
                !expected_forced && exp_map.get(cell).is_some_and(|xv| caches_equal(xv, ev));
            // (b) the engine's evaluation of the proven transform (when covered) computes it.
            let by_eval = oracle
                .and_then(|o| o.get(&(name.clone(), cell.clone())))
                .is_some_and(|ov| caches_equal(ov, ev));
            if !(by_stored || by_eval) {
                count += 1;
            }
        }
    }
    count
}

/// A cell's evaluated value rendered to the `type:value` signature of [`structural::formula_cache_map`]
/// so [`caches_equal`] compares them directly. None for an empty cell.
fn cell_value_sig(model: &ironcalc::base::Model, sheet: u32, row: i32, col: i32) -> Option<String> {
    use ironcalc::base::cell::CellValue;
    match model.get_cell_value_by_index(sheet, row, col) {
        Ok(CellValue::Number(n)) => Some(format!("n:{n}")),
        Ok(CellValue::Boolean(b)) => Some(format!("b:{}", if b { "1" } else { "0" })),
        Ok(CellValue::String(s)) if is_excel_error(&s) => Some(format!("e:{s}")),
        Ok(CellValue::String(s)) => Some(format!("str:{s}")),
        _ => None,
    }
}

/// True when the workbook uses the 1904 date system (`<workbookPr date1904="1">`).
fn workbook_is_date1904(bytes: &[u8]) -> bool {
    let Ok(wb) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return false;
    };
    let text = String::from_utf8_lossy(&wb);
    local_element_tag(&text, "workbookPr")
        .and_then(|t| attr(&t, "date1904"))
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Functions the engine (ironcalc) evaluates DIFFERENTLY from Excel even though it fully supports
/// them, so its value cannot vouch a preserved cache: a cell using one — or transitively depending
/// on one — is excluded from the oracle (fail-closed) and refused rather than vouched against a
/// value only the engine produces.
///
/// Two kinds remain: number-to-TEXT RENDERING (locale/format-dependent — a fraction or rounding
/// format diverges) and ITERATIVE financial SOLVERS (converge to a different valid root,
/// disagreeing beyond the vouch tolerance). The DECIMAL-ROUNDING family (`ROUND`/`ROUNDUP`/
/// `ROUNDDOWN`/`MROUND`) was ALSO here until the vendored engine's rounding was decimal-corrected
/// to match Excel (`ROUND(1.005,2)=1.01`); it now agrees, so those are vouchable again — which
/// fixes both the false-certify AND the over-refusal for the ubiquitous rounding functions.
const ENGINE_DIVERGENT_FUNCTIONS: &[&str] = &[
    // Number-to-text rendering.
    "TEXT", "FIXED", "DOLLAR", // Iterative financial solvers.
    "IRR", "XIRR", "MIRR", "RATE",
];

/// Functions whose result depends on the workbook DATE SYSTEM (1900 vs 1904): each maps between a
/// serial number and a calendar field, so the engine — which hardcodes the 1900 epoch — computes
/// them off by the 1462-day shift under date1904. In a 1904 workbook a cell using (or depending
/// on) one of these cannot be vouched by the oracle, so it is treated like an unsupported
/// function and excluded (fail-closed). `TEXT` is included because a date format string turns it
/// into a calendar renderer; the difference/decomposition functions (DATEDIF/NETWORKDAYS/…) walk
/// the calendar of their serial inputs. Uppercase to match `extract_function_names`.
const DATE_EPOCH_FUNCTIONS: &[&str] = &[
    "DATE",
    "DATEVALUE",
    "YEAR",
    "MONTH",
    "DAY",
    "WEEKDAY",
    "WEEKNUM",
    "ISOWEEKNUM",
    "EDATE",
    "EOMONTH",
    "WORKDAY",
    "WORKDAY.INTL",
    "NETWORKDAYS",
    "NETWORKDAYS.INTL",
    "DAYS",
    "DAYS360",
    "YEARFRAC",
    "DATEDIF",
    "TEXT",
    "NOW",
    "TODAY",
];

/// An oracle mapping (sheet name, A1 cell) -> the `type:value` cache signature of the TRUE computed
/// value of xlq's proven transform, used to vouch a foreign edit's PRESERVED formula caches (which
/// xlq's own transform blanks). Always returns Some, but INCLUDES ONLY cells whose engine value can
/// be trusted to equal Excel's.
///
/// When the workbook uses an UNSUPPORTED / policy-limited (`RTD`/`WEBSERVICE`/`CUBEVALUE`) /
/// user-defined function, the engine computes those cells (and anything depending on them) WRONG —
/// but a cell whose value does NOT depend on such a function is still computed correctly. Rather
/// than disable the whole oracle (which spuriously refused a preserved pure-`SUM` cache in any
/// live-data workbook) OR trust every clean value (UNSOUND — an `IFERROR(RTD(),5)` wrapper yields a
/// clean-but-WRONG value a fabricated cache could match), it isolates the trustworthy cells by
/// POISON-AND-DIFF: overwrite every "source" cell (whose formula calls such a function) with a
/// constant and re-evaluate; a cell whose value CHANGES depends on a source cell and is EXCLUDED.
/// Two distinct constants plus the normal (error-valued) evaluation are used, so a false
/// "unchanged" requires a formula coincidentally constant on all three probes yet dependent —
/// effectively unconstructable, and such a cell's value is a genuine constant anyway. A cell that
/// survives is provably independent of every unsupported result, so the engine's value for it
/// equals Excel's and vouching a matching cache is sound.
fn build_cache_oracle(
    model: &mut ironcalc::base::Model,
    date1904: bool,
) -> Option<std::collections::HashMap<(String, String), String>> {
    let census = crate::census::function_census(model);
    // Functions whose value the engine cannot faithfully reproduce (external data / not implemented
    // / user code). A cell transitively depending on one of these is not vouchable.
    let mut bad: std::collections::HashSet<String> = census
        .unsupported
        .iter()
        .cloned()
        .chain(census.policy_limited.keys().cloned())
        .chain(census.user_defined.keys().cloned())
        .collect();
    // The engine also diverges from Excel on some FULLY-SUPPORTED functions — decimal rounding
    // (`ROUND(1.005,2)` = 1.01 in Excel, 1.00 on a naive binary round), number-to-text rendering,
    // and iterative financial solvers that converge to a different valid root. Trusting the engine
    // there would CERTIFY a forged cache matching its wrong value (and refuse the correct one), so
    // these are unvouchable too: exclude them (fail-closed). The preserved cache then stays
    // unverified and is refused rather than vouched against a value only the engine would produce.
    bad.extend(ENGINE_DIVERGENT_FUNCTIONS.iter().map(|s| s.to_string()));
    // Under the 1904 date system the engine's 1900-epoch date arithmetic is wrong, so any
    // date-system-dependent function is unvouchable — add it to the bad set (poison-and-diff then
    // excludes those cells and their dependents; their preserved caches stay unverified -> refused
    // rather than vouched against a wrong value).
    if date1904 {
        bad.extend(DATE_EPOCH_FUNCTIONS.iter().map(|s| s.to_string()));
    }
    let names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();
    // Enumerate the formula cells and, among them, the "source" cells (formula calls a bad fn).
    // Each formula cell: its (sheet-index, row, col) coordinate and its (sheet-name, A1) oracle key.
    type FormulaCell = ((u32, i32, i32), (String, String));
    let mut formula_cells: Vec<FormulaCell> = Vec::new();
    let mut sources: Vec<(u32, i32, i32)> = Vec::new();
    for cell in model.get_all_cells() {
        let Ok(Some(f)) = model.get_cell_formula(cell.index, cell.row, cell.column) else {
            continue;
        };
        let (Some(name), Ok(a1)) = (
            names.get(cell.index as usize),
            diff::a1(cell.row, cell.column),
        ) else {
            continue;
        };
        formula_cells.push(((cell.index, cell.row, cell.column), (name.clone(), a1)));
        if !bad.is_empty()
            && crate::census::extract_function_names(&f)
                .iter()
                .any(|n| bad.contains(n))
        {
            sources.push((cell.index, cell.row, cell.column));
        }
    }
    let snap =
        |model: &ironcalc::base::Model| -> std::collections::HashMap<(String, String), String> {
            let mut m = std::collections::HashMap::new();
            for (coord, key) in &formula_cells {
                if let Some(sig) = cell_value_sig(model, coord.0, coord.1, coord.2) {
                    m.insert(key.clone(), sig);
                }
            }
            m
        };
    model.evaluate();
    // Fast path: no unsupported/policy/UDF function -> every formula cell is trustworthy.
    if sources.is_empty() {
        return Some(snap(model));
    }
    // Poison-and-diff taint isolation.
    let v_err = snap(model); // normal eval: source cells are their (error-valued) formulas
    for &(s, r, c) in &sources {
        let _ = model.set_user_input(s, r, c, "1234567".to_string());
    }
    model.evaluate();
    let v_k1 = snap(model);
    for &(s, r, c) in &sources {
        let _ = model.set_user_input(s, r, c, "-98765.4321".to_string());
    }
    model.evaluate();
    let v_k2 = snap(model);
    // Untainted iff the value is IDENTICAL across the normal eval and both poisonings.
    let mut out = std::collections::HashMap::new();
    for (key, sig) in &v_err {
        if v_k1.get(key) == Some(sig) && v_k2.get(key) == Some(sig) {
            out.insert(key.clone(), sig.clone());
        }
    }
    Some(out)
}

/// The set of (sheet-name, A1) cells whose value TRANSITIVELY depends on a VOLATILE function
/// (NOW/TODAY/RAND/RANDBETWEEN/OFFSET/INDIRECT/CELL/INFO) — the cells Excel RECOMPUTES on load in
/// auto-calc mode, so their preserved caches self-heal and must be SKIPPED rather than verified
/// against the (freshly re-rolled, never-matching) engine value. The byte-level
/// `volatile_formula_cells` flags only a cell whose OWN body calls a volatile function; a
/// non-volatile dependent (`A2 = A1` where `A1 = NOW()`) needs the engine's dependency graph.
/// Computed by overwriting every volatile SOURCE cell with a constant and diffing the
/// re-evaluation: a cell whose value CHANGES depends on a volatile input. A cell whose value is
/// constant regardless (`=A1*0`) does NOT change and remains vouchable. Returns empty — with NO
/// model load — when the workbook carries no volatile formula at all.
fn volatile_tainted_cells(bytes: &[u8], near: &str) -> std::collections::HashSet<(String, String)> {
    let mut set: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let has_volatile = crate::ooxml::all_sheets(bytes)
        .map(|v| {
            v.into_iter().any(|(_, p)| {
                crate::ooxml::read_part(bytes, &p)
                    .map(|x| !structural::volatile_formula_cells(&x).is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !has_volatile {
        return set;
    }
    let Ok(mut model) = load_from_bytes(bytes, near) else {
        return set;
    };
    let names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();
    type FormulaCell = ((u32, i32, i32), (String, String));
    let mut cells: Vec<FormulaCell> = Vec::new();
    let mut sources: Vec<(u32, i32, i32)> = Vec::new();
    for cell in model.get_all_cells() {
        let Ok(Some(f)) = model.get_cell_formula(cell.index, cell.row, cell.column) else {
            continue;
        };
        let (Some(name), Ok(a1)) = (
            names.get(cell.index as usize),
            diff::a1(cell.row, cell.column),
        ) else {
            continue;
        };
        cells.push(((cell.index, cell.row, cell.column), (name.clone(), a1)));
        if crate::census::is_volatile_formula(&f) {
            sources.push((cell.index, cell.row, cell.column));
        }
    }
    if sources.is_empty() {
        return set;
    }
    let snap =
        |m: &ironcalc::base::Model| -> std::collections::HashMap<(String, String), Option<String>> {
            cells
                .iter()
                .map(|(c, k)| (k.clone(), cell_value_sig(m, c.0, c.1, c.2)))
                .collect()
        };
    model.evaluate();
    let base = snap(&model);
    // Overwrite every volatile source with a constant (unlikely to equal a NOW/RAND value), so a
    // dependent's value provably shifts off the volatile input.
    for &(s, r, c) in &sources {
        let _ = model.set_user_input(s, r, c, "1234567".to_string());
    }
    model.evaluate();
    let poisoned = snap(&model);
    for (_, k) in &cells {
        if base.get(k) != poisoned.get(k) {
            set.insert(k.clone());
        }
    }
    set
}

/// Whether `s` is exactly an Excel error literal (`#REF!`, `#DIV/0!`, …). An engine-evaluated
/// error cell surfaces as this string; its STORED cache carries `t="e"`, so the oracle
/// signature must use the `e:` type to match it (a plain `str:` would spuriously differ).
fn is_excel_error(s: &str) -> bool {
    matches!(
        s,
        "#DIV/0!"
            | "#N/A"
            | "#NAME?"
            | "#NULL!"
            | "#NUM!"
            | "#REF!"
            | "#VALUE!"
            | "#SPILL!"
            | "#CALC!"
            | "#GETTING_DATA"
            | "#FIELD!"
            | "#BLOCKED!"
            | "#CONNECT!"
            | "#UNKNOWN!"
    )
}

/// Two stored formula caches are equal. Each is `type:value` (the cell `t` plus its `<v>`
/// text). The TYPE must match exactly — a number→text retype (`n:55` vs `str:55`) is a real
/// stored-value-type change — while the value tolerates a benign numeric renumbering (`55` vs
/// `55.0`). The type prefix has no colon, so the first `:` cleanly separates it.
fn caches_equal(a: &str, b: &str) -> bool {
    let (ta, va) = a.split_once(':').unwrap_or(("n", a));
    let (tb, vb) = b.split_once(':').unwrap_or(("n", b));
    if ta != tb {
        return false;
    }
    if va == vb {
        return true;
    }
    // The numeric tolerance applies ONLY to a numeric (`n:`) cache. A `str:` (text), `e:`
    // (error) or `b:` (bool) cache is a NON-numeric value whose text must match exactly — a
    // numeric-looking STRING (`"000123"` vs `"123"`, `"1.50"` vs `"1.5"`) is a DIFFERENT
    // displayed value Excel shows verbatim, even though both parse to the same number. Applying
    // the numeric fallback to `str:` vouched a corrupted zero-padded ID as faithful.
    ta == "n"
        && matches!(
            (va.parse::<f64>(), vb.parse::<f64>()),
            (Ok(x), Ok(y)) if nums_equal_at_excel_precision(x, y)
        )
}

/// True when two numbers denote the SAME value at Excel's storage precision. The engine
/// (ironcalc) returns a raw f64 — `100*1.1` is `110.00000000000001` — while Excel/LibreOffice
/// store the correctly-rounded value `110`; comparing a preserved cache against the oracle with
/// EXACT f64 equality spuriously refused a faithful edit of any fractional-arithmetic workbook.
///
/// Comparison is at 14 significant figures. Excel's stated precision is 15 significant figures,
/// but two INDEPENDENT IEEE-754 implementations of a transcendental/irrational function
/// (`LOG`/`EXP`/trig/`POWER`/financial) legitimately disagree by ~1 unit in the last place, which
/// surfaces at the 15th figure — so a 15-figure compare refused a faithful `LOG` cache a real
/// editor recomputed. Dropping to 14 figures absorbs that last-place disagreement. A genuinely
/// different value (a stale or fabricated cache) differs far above the 14th figure and is still
/// refused; the only residual is a corruption confined to the 15th significant figure, which is
/// at Excel's own precision floor.
fn nums_equal_at_excel_precision(x: f64, y: f64) -> bool {
    if x == y {
        return true;
    }
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    // Canonical 14-significant-figure form (13 fractional digits in scientific notation);
    // 0.0 and -0.0 collapse to one key.
    let round14 = |v: f64| {
        if v == 0.0 {
            "0e0".to_string()
        } else {
            format!("{v:.13e}")
        }
    };
    round14(x) == round14(y)
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
            // A cell present on only ONE side that carries NO value (no formula, null raw) is a
            // STYLE-ONLY empty cell — e.g. the covered cells B1/C1/D1 of a merged title, which
            // Excel/LibreOffice materialize as `<c r="B1" s="1"/>`. It is display-only and cannot
            // change any computed value, so it is not an "added"/"removed" value divergence.
            let added_empty =
                kind == "added" && n.is_some_and(|s| s.formula.is_none() && s.raw.is_null());
            let removed_empty =
                kind == "removed" && e.is_some_and(|s| s.formula.is_none() && s.raw.is_null());
            // A literal-cell "value" diff that is numerically equal at Excel's 14-sig-fig storage
            // precision is benign float noise — a real editor rounding a frozen `0.1+0.2` =
            // `0.30000000000000004` back to `0.3` on re-save. Formula caches already get this
            // tolerance (caches_equal); a literal value must get it too, or the same faithful
            // re-serialization is refused. A genuine value change differs far above the floor.
            let value_float_noise = kind == "value"
                && match (e, n) {
                    (Some(a), Some(b)) => match (a.raw.as_f64(), b.raw.as_f64()) {
                        (Some(x), Some(y)) => nums_equal_at_excel_precision(x, y),
                        _ => false,
                    },
                    _ => false,
                };
            match kind {
                "formula" => counts.formula += 1,
                "value" if value_float_noise => {}
                "value" => counts.value += 1,
                "cached_value" => counts.cached_value += 1,
                "format" => counts.format += 1,
                "added" if added_empty => {}
                "removed" if removed_empty => {}
                "added" => counts.added += 1,
                "removed" => counts.removed += 1,
                _ => {}
            }
            let disqualifying = matches!(kind, "formula" | "value" | "added" | "removed")
                && !added_empty
                && !removed_empty
                && !value_float_noise;
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

    /// A zip holding only `xl/workbook.xml` — enough to exercise workbook-level readers.
    fn wb_only(workbook_xml: &str) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("xl/workbook.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(workbook_xml.as_bytes()).unwrap();
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn date1904_detection_is_namespace_aware() {
        // The oracle keys date-function exclusion off this; a prefixed <x:workbookPr> must match.
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><workbookPr date1904="1"/></workbook>"#
        )));
        assert!(workbook_is_date1904(&wb_only(
            r#"<x:workbook xmlns:x="u"><x:workbookPr date1904="true"/></x:workbook>"#
        )));
        assert!(!workbook_is_date1904(&wb_only(
            r#"<workbook><workbookPr/></workbook>"#
        )));
        assert!(!workbook_is_date1904(&wb_only(r#"<workbook/>"#)));
        // The date-epoch exclusion set covers the calendar decomposition/construction functions.
        for f in ["YEAR", "DATE", "EOMONTH", "WEEKDAY", "TEXT"] {
            assert!(
                DATE_EPOCH_FUNCTIONS.contains(&f),
                "{f} must be excluded under 1904"
            );
        }
    }

    #[test]
    fn chart_ref_redundant_quotes_canonicalized() {
        // REGRESSION (round-42): a chart series ref 'Data'!$D$3 (openpyxl/xlq) vs Data!$D$3
        // (Excel/LibreOffice) is semantically identical; the chart compare now canonicalizes
        // redundant quoting so a faithful re-serialization is not refused.
        let chart = |body: &str| {
            format!(
                r#"<c:chartSpace xmlns:c="urn:c"><c:ser><c:val><c:numRef><c:f>{body}</c:f></c:numRef></c:val></c:ser></c:chartSpace>"#
            )
        };
        let (q, u, z) = (
            chart("'Data'!$D$3:$D$8"),
            chart("Data!$D$3:$D$8"),
            chart("Data!$Z$1:$Z$9"),
        );
        let quoted = wb("", &[("xl/charts/chart1.xml", &q)]);
        let unquoted = wb("", &[("xl/charts/chart1.xml", &u)]);
        assert_eq!(
            chart_drawing_refs(&quoted),
            chart_drawing_refs(&unquoted),
            "redundant quote must not change the chart key"
        );
        // A genuinely different range still differs (canonicalization must not over-merge).
        let diff = wb("", &[("xl/charts/chart1.xml", &z)]);
        assert_ne!(chart_drawing_refs(&quoted), chart_drawing_refs(&diff));
    }

    #[test]
    fn pivot_datafield_and_refresh_are_compared() {
        // REGRESSION (round-44): a pivot dataField aggregation (SUM->COUNT) and a refreshOnLoad
        // injection materialize a corrupted value on open; pivot_refs must compare both.
        let pt = |sub: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><dataFields><dataField name="X" fld="1"{sub}/></dataFields></pivotTableDefinition>"#
            )
        };
        let cache = |rol: &str| {
            format!(
                r#"<pivotCacheDefinition xmlns="urn:x"{rol}><cacheSource type="worksheet"/></pivotCacheDefinition>"#
            )
        };
        let sum = wb("", &[("xl/pivotTables/pivotTable1.xml", &pt(""))]);
        let count = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(r#" subtotal="count""#),
            )],
        );
        assert_ne!(
            pivot_refs(&sum),
            pivot_refs(&count),
            "SUM vs COUNT must differ"
        );
        // Absent subtotal keys the same as explicit "sum" (no over-refusal).
        let sum_explicit = wb(
            "",
            &[("xl/pivotTables/pivotTable1.xml", &pt(r#" subtotal="sum""#))],
        );
        assert_eq!(pivot_refs(&sum), pivot_refs(&sum_explicit));
        // refreshOnLoad injection is caught; absent == "0" (no over-refusal).
        let off = wb(
            "",
            &[("xl/pivotCache/pivotCacheDefinition1.xml", &cache(""))],
        );
        let on = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache(r#" refreshOnLoad="1""#),
            )],
        );
        assert_ne!(
            pivot_refs(&off),
            pivot_refs(&on),
            "refreshOnLoad must be compared"
        );
        let off2 = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache(r#" refreshOnLoad="0""#),
            )],
        );
        assert_eq!(pivot_refs(&off), pivot_refs(&off2));
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
        // The AND/OR COMBINATOR on the <customFilters and> CONTAINER is also a value input
        // (round-26): flipping it changes which rows are filter-hidden.
        let comb = |and: &str| {
            format!(
                r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><customFilters and="{and}"><customFilter operator="greaterThan" val="3"/><customFilter operator="lessThan" val="8"/></customFilters></filterColumn></autoFilter>"#
            )
        };
        let and_on = wb(&comb("1"), &[]);
        assert!(verify_noncell_refs(&and_on, &and_on).is_none());
        assert_eq!(
            verify_noncell_refs(&and_on, &wb(&comb("0"), &[]))
                .expect("combinator flip must refuse")["reason"],
            "autofilter_criteria_mismatch"
        );
    }

    #[test]
    fn table_autofilter_criteria_is_compared() {
        // An Excel Table carries its OWN <autoFilter> in xl/tables/*.xml. A foreign change to a
        // table-filter predicate is a value input to a table SUBTOTAL(1-11) — scanning only
        // worksheets missed it and certified silently. Now the table part is compared too.
        let table = |v: &str| {
            format!(
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="T1" ref="A1:A10"><autoFilter ref="A1:A10"><filterColumn colId="0"><customFilters><customFilter operator="lessThanOrEqual" val="{v}"/></customFilters></filterColumn></autoFilter></table>"#
            )
        };
        let (t5, t9) = (table("5"), table("9"));
        let good = wb("", &[("xl/tables/table1.xml", &t5)]);
        assert!(verify_noncell_refs(&good, &good).is_none());
        let refusal = verify_noncell_refs(&good, &wb("", &[("xl/tables/table1.xml", &t9)]))
            .expect("table filter criterion change must refuse");
        assert_eq!(refusal["reason"], "autofilter_criteria_mismatch");
    }

    #[test]
    fn caches_equal_type_and_value() {
        // `type:value` — type must match, value tolerates a numeric renumber.
        assert!(caches_equal("n:55", "n:55"));
        assert!(caches_equal("n:55", "n:55.0")); // benign renumber of the same value
        assert!(caches_equal("n:5.5E1", "n:55"));
        assert!(!caches_equal("n:55", "n:56"));
        assert!(caches_equal("str:hello", "str:hello"));
        assert!(!caches_equal("str:hello", "str:world"));
        // a number->text retype of the same digit string is NOT equal (round-26).
        assert!(!caches_equal("n:55", "str:55"));
        // a string value containing a colon splits only on the FIRST colon.
        assert!(caches_equal("str:9:30", "str:9:30"));
        // REGRESSION (round-43): the numeric tolerance must NOT leak into str: caches — a
        // numeric-looking STRING is a distinct displayed value Excel shows verbatim. A stale
        // zero-padded ID cache ("123" for a true "000123") must be REFUSED, not vouched.
        assert!(!caches_equal("str:000123", "str:123"));
        assert!(!caches_equal("str:1.50", "str:1.5"));
        assert!(!caches_equal("str:1e3", "str:1000"));
        // REGRESSION (round-41): the engine's raw f64 (100*1.1 = 110.00000000000001) must be
        // vouched against the editor-rounded stored cache (110) — same value at Excel's 15-sig-fig
        // precision. Exact f64 equality spuriously refused every fractional-arithmetic workbook.
        assert!(caches_equal("n:110.00000000000001", "n:110"));
        assert!(caches_equal("n:0.30000000000000004", "n:0.3")); // 0.1+0.2
                                                                 // A genuinely different value (beyond float noise) still differs — no false-certify.
        assert!(!caches_equal("n:110.01", "n:110"));
        assert!(!caches_equal("n:110.0001", "n:110"));
        // Signed zero collapses; sign of a real value is kept.
        assert!(caches_equal("n:-0", "n:0"));
        assert!(!caches_equal("n:-5", "n:5"));
    }

    #[test]
    fn excel_precision_equality_is_sound() {
        // IEEE-754 noise below the 14th figure -> vouched.
        assert!(nums_equal_at_excel_precision(110.00000000000001, 110.0));
        assert!(nums_equal_at_excel_precision(1.0 / 3.0, 0.333333333333333));
        assert!(nums_equal_at_excel_precision(0.0, -0.0));
        // REGRESSION (round-43): two engines' transcendental results disagree by ~1 ULP at the
        // 15th figure (ironcalc LOG(10,3) vs LibreOffice's) — must still be vouched at 14 figs.
        assert!(nums_equal_at_excel_precision(
            2.095903274289385,
            2.09590327428939
        ));
        // A difference at the 14th significant figure or above -> NOT vouched.
        assert!(!nums_equal_at_excel_precision(1.0, 1.0000000000001));
        assert!(!nums_equal_at_excel_precision(1e300, 1.0001e300));
        // A meaningful value difference (a stale/fabricated cache) is far above the floor.
        assert!(!nums_equal_at_excel_precision(5.0, 6.0));
        // Non-finite never silently equal.
        assert!(!nums_equal_at_excel_precision(f64::NAN, f64::NAN));
        assert!(!nums_equal_at_excel_precision(f64::INFINITY, 1e308));
    }

    #[test]
    fn rich_data_part_is_certify_safe() {
        // In-cell images / linked data types (xl/richData/*) are index-linked from cells via
        // `vm`, carry no shiftable coordinate; certify must not refuse xlq's own transform.
        let bytes = wb(
            "",
            &[(
                "xl/richData/rdrichvalue.xml",
                r#"<rvData xmlns="urn:x"><rv><v>0</v></rv></rvData>"#,
            )],
        );
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }

    #[test]
    fn custom_ui_part_is_certify_safe() {
        // Ribbon extensibility XML carries no cell coordinate; certify must not refuse xlq's
        // own transform of a ribbon-customized workbook.
        let bytes = wb(
            "",
            &[(
                "customUI/customUI14.xml",
                r#"<customUI xmlns="urn:ui"><ribbon><tabs><tab id="t"/></tabs></ribbon></customUI>"#,
            )],
        );
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }

    #[test]
    fn external_data_source_repoint_is_refused() {
        // An external data-source part (connections.xml) is allowlisted (no cell coordinate) but
        // must be COMPARED: a foreign edit that repoints its URL/command is a SECURITY change
        // (SSRF/exfiltration + attacker data injected on refresh) the cell diff cannot see.
        let conn = |url: &str| {
            format!(
                r#"<connections xmlns="urn:c"><connection id="1" name="q"><webPr url="{url}"/></connection></connections>"#
            )
        };
        let good_conn = conn("https://data.internal.example/report.xml");
        let good = wb("", &[("xl/connections.xml", good_conn.as_str())]);
        // Identical connection target -> not refused (must not blanket-refuse a data workbook).
        assert!(verify_noncell_refs(&good, &good).is_none());
        // A benign reserialization (attribute reorder / whitespace) is tolerated.
        let reserialized = wb(
            "",
            &[(
                "xl/connections.xml",
                r#"<connections xmlns="urn:c">
                     <connection name="q" id="1"><webPr  url="https://data.internal.example/report.xml" /></connection>
                   </connections>"#,
            )],
        );
        assert!(verify_noncell_refs(&good, &reserialized).is_none());
        // A repointed URL -> refused as an external-target mismatch.
        let evil_conn = conn("https://evil.attacker.example/exfil.xml");
        let evil = wb("", &[("xl/connections.xml", evil_conn.as_str())]);
        let refusal =
            verify_noncell_refs(&good, &evil).expect("a repointed data source must be refused");
        assert_eq!(refusal["reason"], "external_target_mismatch");
    }

    #[test]
    fn pivot_workbook_is_compared_not_presence_refused() {
        // A pivot part carries a source range the cell diff never sees; it was on neither the
        // allowlist nor a comparator, so certify refused EVERY pivot workbook — including xlq's
        // own faithful transform. Now allowlisted + compared: presence is fine, a mangle differs.
        let pivot = |src_ref: &str| {
            format!(
                r#"<pivotCacheDefinition xmlns="urn:x"><cacheSource type="worksheet"><worksheetSource ref="{src_ref}" sheet="Sheet2"/></cacheSource><cacheFields count="1"/></pivotCacheDefinition>"#
            )
        };
        let good_src = pivot("B1:B2");
        let good = wb(
            "",
            &[("xl/pivotCache/pivotCacheDefinition1.xml", good_src.as_str())],
        );
        // Presence of a pivot must NOT refuse (the over-refusal fix).
        assert!(verify_noncell_refs(&good, &good).is_none());
        // A repointed pivot source range IS caught.
        let bad_src = pivot("B1:B999");
        let bad = wb(
            "",
            &[("xl/pivotCache/pivotCacheDefinition1.xml", bad_src.as_str())],
        );
        let refusal =
            verify_noncell_refs(&good, &bad).expect("a repointed pivot source must be caught");
        assert_eq!(refusal["reason"], "pivot_reference_mismatch");
    }

    #[test]
    fn customui_autorun_callback_injection_is_refused() {
        // A customUI ribbon is allowlisted, but injecting an onLoad autorun callback (a macro
        // that runs on open) is a security change certify must catch.
        let inert = wb(
            "",
            &[(
                "customUI/customUI14.xml",
                r#"<customUI xmlns="urn:ui"><ribbon><tabs><tab id="t"/></tabs></ribbon></customUI>"#,
            )],
        );
        let autorun = wb(
            "",
            &[(
                "customUI/customUI14.xml",
                r#"<customUI xmlns="urn:ui" onLoad="Evil"><ribbon><tabs><tab id="t"/></tabs></ribbon></customUI>"#,
            )],
        );
        let refusal = verify_noncell_refs(&inert, &autorun)
            .expect("an injected customUI autorun callback must be refused");
        assert_eq!(refusal["reason"], "external_target_mismatch");
    }

    #[test]
    fn unverified_formula_caches_flags_present_not_dropped() {
        // No volatile cells here, so the transitive-volatile skip set is empty.
        let empty: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        // A formula cell (in Sheet2's body) with various stored caches.
        let cell = |v: &str| format!(r#"<row r="1"><c r="Z1"><f>SUM(A1:A2)</f>{v}</c></row>"#);
        let blank = wb(&cell("<v />"), &[]); // xlq blanks a shifted cache
        let fabricated = wb(&cell("<v>999</v>"), &[]); // foreign fabricates one
        let dropped = wb(&cell(""), &[]); // openpyxl drops it (no <v>)
        let honest = wb(&cell("<v>3</v>"), &[]);
        let honest_renum = wb(&cell("<v>3.0</v>"), &[]);
        // present cache the transform did not vouch (no eval oracle) -> counted.
        assert_eq!(
            unverified_formula_caches(&blank, &fabricated, false, None, &empty),
            1
        );
        // a dropped cache (no <v>) -> Excel recomputes -> not counted.
        assert_eq!(
            unverified_formula_caches(&blank, &dropped, false, None, &empty),
            0
        );
        // identical present caches, and a benign 3 vs 3.0 renumber -> not counted.
        assert_eq!(
            unverified_formula_caches(&honest, &honest, false, None, &empty),
            0
        );
        assert_eq!(
            unverified_formula_caches(&honest, &honest_renum, false, None, &empty),
            0
        );
        // a present cache that DIFFERS from the transform's present cache -> counted.
        assert_eq!(
            unverified_formula_caches(&honest, &fabricated, false, None, &empty),
            1
        );
        // when xlq's transform FORCES recalc, its own caches are moot: an identical present
        // edited cache (which would otherwise verify) is unverifiable because the transform
        // discards it and recomputes -> every present edited cache is counted.
        assert_eq!(
            unverified_formula_caches(&honest, &honest, true, None, &empty),
            1
        );
        // BUT an evaluation oracle (built when the engine covers the workbook) vouches the
        // correct cache even when the transform blanked or force-discarded its own: 3 matches
        // the true SUM, 999 does not — the strengthening the stored-cache compare cannot do.
        let oracle: std::collections::HashMap<(String, String), String> =
            [(("Sheet2".to_string(), "Z1".to_string()), "n:3".to_string())]
                .into_iter()
                .collect();
        assert_eq!(
            unverified_formula_caches(&blank, &honest, false, Some(&oracle), &empty),
            0
        );
        assert_eq!(
            unverified_formula_caches(&honest, &honest, true, Some(&oracle), &empty),
            0
        );
        // a fabricated cache is NOT vouched by the oracle (999 != the true 3).
        assert_eq!(
            unverified_formula_caches(&blank, &fabricated, false, Some(&oracle), &empty),
            1
        );
    }

    /// A loadable single-sheet workbook (refs.xlsx skeleton, so ironcalc loads it) with sheet1's
    /// `<sheetData>` replaced by `rows`.
    fn oracle_wb(rows: &str) -> Vec<u8> {
        use std::io::Read;
        let base = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/structural/refs.xlsx"
        ))
        .unwrap();
        let mut ar = zip::ZipArchive::new(Cursor::new(base.as_slice())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for i in 0..ar.len() {
            let mut f = ar.by_index(i).unwrap();
            let name = f.name().to_string();
            out.start_file(&name, opts).unwrap();
            if name == "xl/worksheets/sheet1.xml" {
                let s = format!(
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:E3"/><sheetData>{rows}</sheetData></worksheet>"#
                );
                out.write_all(s.as_bytes()).unwrap();
            } else {
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                out.write_all(&b).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn cache_oracle_poison_diff_isolates_tainted_cells() {
        // REGRESSION (round-36): a policy-limited/unsupported/UDF function no longer disables the
        // oracle workbook-wide (which refused a preserved pure-SUM cache). Poison-and-diff isolates
        // the cells whose value the engine computes correctly. RTD is policy-limited (-> #N/A).
        let rows = r#"<row r="1"><c r="A1"><v>10</v></c><c r="B1"><f>SUM(A1:A2)</f><v>30</v></c><c r="C1"><f>RTD("a","","b")</f><v>7</v></c><c r="D1"><f>IFERROR(RTD("a","","b"),5)</f><v>5</v></c><c r="E1"><f>RTD("a","","b")+1</f><v>8</v></c></row><row r="2"><c r="A2"><v>20</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load rtd workbook");
        let oracle = build_cache_oracle(&mut model, false).expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        // The PURE SUM cell is provably independent of RTD -> vouchable (the over-refusal fix).
        assert!(
            oracle.contains_key(&key("B1")),
            "pure SUM must be vouchable in a live-data workbook: {oracle:?}"
        );
        // The RTD source cell and everything depending on it — INCLUDING the error-MASKED
        // IFERROR(RTD,5) (the vector a naive clean-value fix would false-certify) and the
        // transitive RTD+1 — must be EXCLUDED (not vouchable).
        assert!(!oracle.contains_key(&key("C1")), "RTD source excluded");
        assert!(
            !oracle.contains_key(&key("D1")),
            "IFERROR(RTD) masked dependent excluded (else a fabricated 5 would false-certify)"
        );
        assert!(
            !oracle.contains_key(&key("E1")),
            "RTD+1 transitive dependent excluded"
        );
    }

    #[test]
    fn engine_divergent_functions_excluded_from_oracle() {
        // ROUND was decimal-corrected in the vendored engine (round-44 follow-up), so it is
        // vouchable AGAIN — B1/C1 must be in the oracle. TEXT (still divergent) and anything
        // depending on it are excluded; a pure SUM is vouchable.
        let rows = r#"<row r="1"><c r="A1"><v>1.005</v></c><c r="B1"><f>ROUND(A1,2)</f><v>1.01</v></c><c r="C1"><f>B1*1000</f><v>1010</v></c><c r="D1"><f>SUM(A1:A1)</f><v>1.005</v></c><c r="E1" t="str"><f>TEXT(A1,"0.00")</f><v>1.01</v></c><c r="F1" t="str"><f>E1&amp;"x"</f><v>1.01x</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load round workbook");
        let oracle = build_cache_oracle(&mut model, false).expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        // ROUND now agrees with Excel -> vouchable (both directions of the old bug fixed).
        assert!(
            oracle.contains_key(&key("B1")),
            "decimal-corrected ROUND is vouchable again: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("C1")),
            "a ROUND dependent is vouchable"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "a pure SUM stays vouchable"
        );
        // TEXT rendering still diverges -> excluded (source + transitive dependent).
        assert!(!oracle.contains_key(&key("E1")), "TEXT source excluded");
        assert!(
            !oracle.contains_key(&key("F1")),
            "a cell depending on TEXT is excluded"
        );
    }

    #[test]
    fn volatile_taint_is_transitive() {
        // REGRESSION (round-43): the volatile-recompute skip must be TRANSITIVE. A1=NOW() is
        // volatile; A2=A1 is a non-volatile DEPENDENT Excel also recomputes on load — both caches
        // self-heal and must be skipped. A pure SUM cell must NOT be tainted (its cache is
        // verifiable). The byte-level check flagged only A1, so A2 was spuriously refused.
        let rows = r#"<row r="1"><c r="A1"><f>NOW()</f></c></row><row r="2"><c r="A2"><f>A1</f></c></row><row r="3"><c r="A3"><f>SUM(A5:A6)</f></c></row><row r="5"><c r="A5"><v>2</v></c></row><row r="6"><c r="A6"><v>3</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/structural/refs.xlsx"
        );
        let tainted = volatile_tainted_cells(&bytes, path);
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            tainted.contains(&key("A1")),
            "NOW() cell is volatile-tainted"
        );
        assert!(
            tainted.contains(&key("A2")),
            "A2=A1 is a TRANSITIVE volatile dependent: {tainted:?}"
        );
        assert!(
            !tainted.contains(&key("A3")),
            "a pure SUM is not volatile-tainted (its cache stays verifiable)"
        );
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
    fn precision_as_displayed_reads_fullprecision() {
        // The value-affecting "precision as displayed" mode, namespace-prefix-agnostic.
        let wb = |cp: &str| {
            format!(
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{cp}</workbook>"#
            )
        };
        // helper reads xl/workbook.xml from a zip; build a tiny one via the test wb() builder is
        // heavier, so exercise the underlying tag reader directly.
        assert!(matches!(
            attr(
                &local_element_tag(&wb(r#"<calcPr fullPrecision="0"/>"#), "calcPr").unwrap(),
                "fullPrecision"
            )
            .as_deref(),
            Some("0")
        ));
        assert!(local_element_tag(&wb(r#"<calcPr calcId="1"/>"#), "calcPr")
            .and_then(|t| attr(&t, "fullPrecision"))
            .is_none());
    }

    #[test]
    fn cell_info_function_sensitivity_scan() {
        // Number-format-sensitive info types -> a format change is value-affecting.
        assert!(formula_calls_sensitive_cell(r#"CELL("format",A1)"#));
        assert!(formula_calls_sensitive_cell(r#"CELL("color",A1)"#));
        assert!(formula_calls_sensitive_cell(
            r#"IF(CELL("parentheses",B2)=1,"y","n")"#
        ));
        // Case- and _xlfn.-insensitive.
        assert!(formula_calls_sensitive_cell(r#"cell("FORMAT",A1)"#));
        assert!(formula_calls_sensitive_cell(r#"_xlfn.CELL("format",A1)"#));
        // A NON-literal info type is unresolvable -> conservative true.
        assert!(formula_calls_sensitive_cell("CELL(D1,A1)"));
        // Format-INSENSITIVE info types -> not sensitive.
        assert!(!formula_calls_sensitive_cell(r#"CELL("contents",A1)"#));
        assert!(!formula_calls_sensitive_cell(
            r#"CELL("row",A1)+CELL("col",A1)"#
        ));
        // A STRING LITERAL that merely contains "CELL(" is not a call.
        assert!(!formula_calls_sensitive_cell(
            r#"CONCAT("CELL(""format"",A1)","x")"#
        ));
        // A sheet named CELL is not the function.
        assert!(!formula_calls_sensitive_cell("'CELL'!A1+1"));
        // No CELL at all.
        assert!(!formula_calls_sensitive_cell("SUM(A1:A10)*1.1"));
    }

    #[test]
    fn custom_xml_part_is_certify_safe() {
        // An inert custom-XML data island carries no worksheet coordinate; certify must not
        // refuse xlq's own transform of a workbook containing one.
        let bytes = wb(
            "",
            &[("customXml/item1.xml", "<root><tag>hello</tag></root>")],
        );
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }

    #[test]
    fn slicer_timeline_parts_are_certify_safe() {
        // REGRESSION (round-36): slicer/timeline widgets bind to a pivot/table by name/ID and carry
        // no shiftable A1 coordinate (like the pivot parts), so certify must not refuse its own
        // transform of a slicer/timeline dashboard.
        let empty = BTreeSet::new();
        for p in [
            "xl/slicerCaches/slicerCache1.xml",
            "xl/slicers/slicer1.xml",
            "xl/timelineCaches/timelineCache1.xml",
            "xl/timelines/timeline1.xml",
        ] {
            assert!(part_is_certify_safe(p, &empty), "{p} must be allowlisted");
        }
        // But a genuinely unknown reference-bearing part still fails closed.
        assert!(!part_is_certify_safe(
            "xl/externalLinks/externalLink1.xml",
            &empty
        ));
    }

    #[test]
    fn volatile_dependencies_part_is_certify_safe() {
        // REGRESSION (round-41): xl/volatileDependencies.xml is the volatile/RTD analog of
        // calcChain — a rebuildable cache restructure now DROPS. certify must not refuse its own
        // faithful transform of a workbook whose foreign editor kept the part.
        let empty = BTreeSet::new();
        assert!(part_is_certify_safe("xl/volatileDependencies.xml", &empty));
        assert!(part_is_certify_safe("xl/calcChain.xml", &empty));
    }

    #[test]
    fn autofilter_ignores_filtercolumn_display_button_attrs() {
        // REGRESSION (round-36): hiddenButton/showButton on <filterColumn> govern only the filter
        // DROPDOWN BUTTON's visibility (pure display), so a foreign editor writing them at their
        // defaults must NOT change the criteria key. The value-affecting predicate is still compared.
        let af = |fc: &str| {
            wb(
                &format!(r#"<autoFilter ref="A1:C10">{fc}</autoFilter>"#),
                &[],
            )
        };
        let plain =
            af(r#"<filterColumn colId="1"><filters><filter val="5"/></filters></filterColumn>"#);
        let with_display = af(
            r#"<filterColumn colId="1" hiddenButton="0" showButton="1"><filters><filter val="5"/></filters></filterColumn>"#,
        );
        assert_eq!(
            autofilter_criteria(&plain),
            autofilter_criteria(&with_display),
            "filterColumn display-button attrs must not change the criteria"
        );
        // A real predicate change (the filter value) still differs.
        let changed =
            af(r#"<filterColumn colId="1"><filters><filter val="9"/></filters></filterColumn>"#);
        assert_ne!(autofilter_criteria(&plain), autofilter_criteria(&changed));
    }

    #[test]
    fn local_element_tag_is_namespace_prefix_agnostic() {
        // REGRESSION (round-21): a raw `find("<calcPr")` missed a prefixed `<x:calcPr>`, hiding
        // value-affecting settings from the compare. Match by LOCAL name.
        assert_eq!(
            local_element_tag(
                r#"<workbook><x:calcPr fullPrecision="0"/></workbook>"#,
                "calcPr"
            )
            .as_deref(),
            Some(r#"<x:calcPr fullPrecision="0"/"#)
        );
        assert_eq!(
            local_element_tag(r#"<workbook><calcPr calcId="1"/></workbook>"#, "calcPr").as_deref(),
            Some(r#"<calcPr calcId="1"/"#)
        );
        // a look-alike element name must not match.
        assert_eq!(
            local_element_tag(r#"<workbook><calcPrExtra/></workbook>"#, "calcPr"),
            None
        );
        // and the extracted tag feeds `attr` correctly through the prefix + Eq whitespace.
        let tag = local_element_tag(
            r#"<workbook><x:calcPr fullPrecision = "0"/></workbook>"#,
            "calcPr",
        )
        .unwrap();
        assert_eq!(attr(&tag, "fullPrecision").as_deref(), Some("0"));
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
            keys.iter().any(|k| k.contains("dest=Sheet2!C3")),
            "internal location captured: {keys:?}"
        );
        assert!(
            keys.iter()
                .any(|k| k.contains("ext=https://good.example.com/safe")),
            "external target captured: {keys:?}"
        );
    }

    #[test]
    fn hyperlink_internal_target_and_location_encodings_are_equivalent() {
        // The SAME in-workbook jump (A4 -> Data!A1) has two standard OOXML encodings: (A) a
        // relationship Target `#Data!A1` with no `location` (openpyxl), and (B) a
        // `location="Data!A1"` attribute with no relationship (Excel/LibreOffice). They must
        // produce the SAME key so a faithful edit that round-trips the encoding is not refused.
        let form_a = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="#Data!A1"/></Relationships>"##,
            )],
        );
        let form_b = wb(
            r#"<hyperlinks><hyperlink ref="A4" location="Data!A1"/></hyperlinks>"#,
            &[],
        );
        assert_eq!(structural_ref_attrs(&form_a), structural_ref_attrs(&form_b));
        // A genuine external retarget still differs (the equivalence must not blur real swaps).
        let external = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="https://evil.example/x" TargetMode="External"/></Relationships>"#,
            )],
        );
        assert_ne!(
            structural_ref_attrs(&form_a),
            structural_ref_attrs(&external)
        );
    }

    #[test]
    fn structural_ref_attrs_is_namespace_prefix_aware() {
        // REGRESSION (round-40 HIGH security): the old raw `<hyperlink` substring scan was blind
        // to a namespace-PREFIXED element. A foreign editor binds a prefix to the spreadsheetML
        // main namespace and injects `<x:hyperlink r:id=…>` at an external phishing URL; the
        // prefixed element evaded the scan, so its ref set stayed empty and matched xlq's own
        // (also empty) transform -> CERTIFIED. The walk is now namespace-aware (by local name).
        let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let evil_rels = (
            "xl/worksheets/_rels/sheet2.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId100" Type="x/hyperlink" Target="https://evil-phishing.example/steal" TargetMode="External"/></Relationships>"#,
        );
        // xlq's own transform: NO hyperlink.
        let clean = wb("", &[]);
        assert!(structural_ref_attrs(&clean).is_empty());
        // Attacker injects a PREFIXED hyperlink (x bound to the main ns) with an external target.
        let evil = wb(
            &format!(
                r#"<x:hyperlinks xmlns:x="{main}" xmlns:r="{r}"><x:hyperlink ref="A1" r:id="rId100"/></x:hyperlinks>"#
            ),
            &[evil_rels],
        );
        assert!(
            !structural_ref_attrs(&evil).is_empty(),
            "prefixed hyperlink must now be captured"
        );
        let refusal = verify_noncell_refs(&clean, &evil)
            .expect("an injected prefixed external hyperlink must refuse");
        assert_eq!(refusal["reason"], "structural_ref_mismatch");
        // DUAL GUARD (no new over-refusal): a benign prefix rebind of the SAME hyperlink keys
        // identically to the unprefixed form, so a faithful re-serialization is not refused.
        let plain = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A1" r:id="rId100"/></hyperlinks>"#,
            &[evil_rels],
        );
        assert_eq!(
            structural_ref_attrs(&evil),
            structural_ref_attrs(&plain),
            "prefixed and unprefixed must key identically"
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
