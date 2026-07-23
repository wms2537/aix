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
    // The expected bytes are xlq's transform of `original`, so their self-referential hyperlink
    // Targets (if any) name `original`; the edited bytes' name `edited`. Passing each basename lets
    // an internal hyperlink encoded as a self-file external Target (LibreOffice) match faithfully.
    if let Some(refusal) = verify_noncell_refs_named(
        &expected_bytes,
        &edited_bytes,
        &diff::basename(original),
        &diff::basename(edited),
    ) {
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
                build_cache_oracle(
                    &mut expected_model,
                    date1904,
                    &intersection_cells(&expected_bytes),
                )
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
    let cell_reads_format = has_format_sensitive_cell_fn(&edited_bytes);
    let mut format_disqualifying = if precision_as_displayed(&edited_bytes) || cell_reads_format {
        counts.format
    } else {
        0
    };
    // The display-based `format` diff misses a number-format CODE change that leaves the RENDERED
    // value unchanged (numFmtId 1 "0" -> 0 General both render 5 as "5"). But `CELL("format")` reads
    // the CODE, so that restyle DOES change the formula's value — compare the resolved per-cell
    // format codes directly and disqualify a mismatch when such a formula is present.
    if cell_reads_format
        && cell_number_formats(&expected_bytes) != cell_number_formats(&edited_bytes)
    {
        format_disqualifying += 1;
    }
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
// Test-only thin wrapper: the many `verify_noncell_refs(expected, edited)` unit tests don't carry
// file names, so the hyperlink self-file fold is disabled (conservative default: never folds an
// external target to internal). Production always calls `verify_noncell_refs_named` with basenames.
#[cfg(test)]
fn verify_noncell_refs(expected: &[u8], edited: &[u8]) -> Option<Value> {
    verify_noncell_refs_named(expected, edited, "", "")
}

/// As `verify_noncell_refs`, but with each workbook's own basename so an internal hyperlink
/// encoded as a self-referential external Target (LibreOffice) is recognised as internal.
fn verify_noncell_refs_named(
    expected: &[u8],
    edited: &[u8],
    expected_name: &str,
    edited_name: &str,
) -> Option<Value> {
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
    if structural_ref_attrs(expected, expected_name) != structural_ref_attrs(edited, edited_name) {
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
    // ISO-8601 date VALUE cells (`t="d"`) are DISCARDED by ironcalc's importer (loaded as a
    // constant NIMPL error), so the engine snapshot cannot see a change to their stored date — a
    // foreign edit could rewrite 2020-01-01 to 2099-12-31 with no cell-value diff for compare() to
    // catch. xlq's transform copies these cells verbatim at shifted coordinates, so compare them at
    // the byte level: a faithful edit matches, a value change (or a moved/added/removed date cell)
    // differs.
    if date_value_cells(expected) != date_value_cells(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "date_value_mismatch",
            "detail": "an ISO-8601 date value cell (t=\"d\") differs from xlq's transform — a value \
                       the engine cannot load and the cell diff cannot see",
        }));
    }
    // A cell with TWO OR MORE `<v>` children is malformed (CT_Cell permits one). Excel/LibreOffice
    // take the LAST `<v>` while ironcalc misreads the cell as empty/error — so certify's engine
    // snapshot is blind to a value smuggled in as a second `<v>`. Refuse a workbook carrying one
    // (fail-closed): a well-formed workbook never has this, so there is no over-refusal.
    if has_repeated_value_cell(expected) || has_repeated_value_cell(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "malformed_multi_value_cell",
            "detail": "a cell has more than one <v> value child (schema-invalid) — the engine \
                       misreads it, so its value cannot be verified",
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
    // EXTERNAL relationship targets (linked image `<a:blip r:link>`, hover hyperlink, linked OLE /
    // media, external-workbook link) live inside allowlisted `.rels` parts and are resolved by no
    // other comparator — a repoint to an attacker URL/UNC would otherwise CERTIFY. xlq copies them
    // verbatim, so a faithful edit matches; a repoint / insertion / removal differs. (Hyperlinks are
    // excluded — compared with their own internal-jump / self-file folds above.)
    if external_rels_targets(expected) != external_rels_targets(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "external_relationship_mismatch",
            "detail": "an external relationship target (linked image / OLE / media / workbook link) \
                       differs from xlq's transform — a repointed external target",
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
    if rich_data_values(expected) != rich_data_values(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "rich_data_mismatch",
            "detail": "a rich value / linked-data-type field (a Stocks/Geography display string or \
                       property, an =IMAGE store) in xl/richData differs from xlq's transform — the \
                       cell's persisted OFFLINE value, which the sheet cell (a `vm`-indexed fallback) \
                       does not carry, so the cell diff misses it",
        }));
    }
    if cell_metadata_bindings(expected) != cell_metadata_bindings(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "cell_metadata_mismatch",
            "detail": "a cell's value-metadata/cell-metadata binding (the `vm`/`cm` pointer to its \
                       rich value or dynamic-array metadata) differs from xlq's transform — a repoint \
                       silently swaps the cell's real offline value while its text stays identical",
        }));
    }
    if metadata_index_chain(expected) != metadata_index_chain(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "metadata_index_mismatch",
            "detail": "the xl/metadata.xml index mapping (the `rc`/`rvb`/`cm` chain that resolves a \
                       cell's `vm`/`cm` to a rich-value record) differs from xlq's transform — a \
                       reindex repoints which record a cell shows with both endpoints unchanged",
        }));
    }
    // A cell's LOCKED state is a style attribute the cell diff and the style-is-benign rule ignore,
    // but `CELL("protect", A1)` reads it: unlocking a cell flips that formula's result. Compare the
    // unlocked-cell set only when such a formula is present (a rare, targeted check).
    if (workbook_has_cell_info_fn(expected, &["protect"])
        || workbook_has_cell_info_fn(edited, &["protect"]))
        && cell_lock_states(expected) != cell_lock_states(edited)
    {
        return Some(json!({
            "status": "REFUSED",
            "reason": "cell_lock_state_mismatch",
            "detail": "a cell's protection (locked) state differs from xlq's transform and a \
                       CELL(\"protect\",…) formula reads it — unlocking a cell changes that formula's \
                       value with no cell/formula diff",
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
        || low.starts_with("customxml/")             // custom-XML data island: no worksheet
                                                     // coordinate, but its CONTENT (Power Query
                                                     // DataMashup source URLs) is compared by
                                                     // opaque_target_signature, not security-inert

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
        } else if low.starts_with("customxml/") && low.ends_with(".xml") {
            // A custom-XML data island is NOT security-inert: Power Query stores its M queries and
            // their EXTERNAL data-source URLs (Web.Contents/OData/SQL, executed on refresh) inline
            // as a `<DataMashup>base64…</DataMashup>` blob here, while `connections.xml` only names
            // the query. A repoint rewrites only this part; xlq copies it verbatim, so a faithful
            // edit keeps it identical and a source repoint differs (round-56 defect 10).
            "customxml"
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

/// Every element in `xml` rendered as `elem:<ancestor-path>local[attr=val;…]` (attributes SORTED),
/// plus, per element, its FULL reassembled text as `text:<ancestor-path>local[…]=<content>`. Both
/// carry the element's ROOT-TO-NODE ancestor path (each ancestor's `local[attrs]`), so a child moved
/// under a DIFFERENT parent — a `<dbPr>`/`<webPr>` data-source relocated to another `<connection
/// id>` — changes its signature and is refused (round-59 defect 4); the flat sorted multiset lost
/// that binding. The text is accumulated across Text + GeneralRef + CData events (an entity/char-ref
/// like `&#57;` arrives as a separate GeneralRef, previously DROPPED — round-59 defect 6 — and CDATA
/// as CData) so an entity/CDATA-encoded tamper of a customXml/connections value differs. A byte
/// comparison would spuriously refuse a benign reserialization; this tolerates attribute/whitespace
/// order while catching any value/target/binding change. Namespace-prefix-agnostic (local names).
fn element_attr_signatures(xml: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // Stack of (element signature `local[attrs]`, accumulated raw text of that element).
    let mut stack: Vec<(String, String)> = Vec::new();
    let elem_sig = |e: &quick_xml::events::BytesStart| -> String {
        let local = String::from_utf8_lossy(structural::local_of(e.name().as_ref())).into_owned();
        let mut attrs: Vec<String> = e
            .attributes()
            .filter_map(|a| a.ok())
            // A namespace DECLARATION (`xmlns` / `xmlns:foo`) is a prefix BINDING, not content:
            // `local_of` would leak the prefix ("foo=uri"), so a benign prefix rename that preserves
            // every local name and bound URI would falsely differ. Skip them to keep this function
            // genuinely namespace-prefix-agnostic (round-60 defect 8).
            .filter(|a| {
                let k = a.key.as_ref();
                k != b"xmlns" && !k.starts_with(b"xmlns:")
            })
            .map(|a| {
                let key =
                    String::from_utf8_lossy(structural::local_of(a.key.as_ref())).into_owned();
                let val = a
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
                format!("{key}={val}")
            })
            .collect();
        attrs.sort();
        format!("{local}[{}]", attrs.join(";"))
    };
    let path_prefix = |stack: &[(String, String)], sig: &str| -> String {
        if stack.is_empty() {
            sig.to_string()
        } else {
            let mut p: String = stack
                .iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>()
                .join(">");
            p.push('>');
            p.push_str(sig);
            p
        }
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let sig = elem_sig(&e);
                out.push(format!("elem:{}", path_prefix(&stack, &sig)));
                stack.push((sig, String::new()));
            }
            Ok(Event::Empty(e)) => {
                let sig = elem_sig(&e);
                out.push(format!("elem:{}", path_prefix(&stack, &sig)));
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.1.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            // An entity / numeric char-reference (`&#57;`, `&amp;`) inside text arrives as a SEPARATE
            // GeneralRef event; reassemble it raw so an entity insert/delete/substitution differs.
            Ok(Event::GeneralRef(r)) => {
                if let Some(top) = stack.last_mut() {
                    top.1.push('&');
                    top.1.push_str(&r.decode().unwrap_or_default());
                    top.1.push(';');
                }
            }
            Ok(Event::CData(c)) => {
                if let Some(top) = stack.last_mut() {
                    top.1.push_str(&String::from_utf8_lossy(c.as_ref()));
                }
            }
            Ok(Event::End(_)) => {
                if let Some((sig, text)) = stack.pop() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        out.push(format!("text:{}={trimmed}", path_prefix(&stack, &sig)));
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Per-SHAPE value/security bindings in a drawing part — a linked-cell `textlink`, an "Assign Macro"
/// `macro`, and a resolved `<a:hlinkClick r:id>` external URL — each KEYED BY the owning shape's
/// STABLE identity (its cNvPr `name`, else `id`). A flat multiset lost the shape<->target binding, so
/// SWAPPING two shapes' hyperlink targets (the "Download report" button now points at the attacker
/// URL) certified; keying by the shape identity makes a swap differ (round-59 defect 5). The final
/// sort keeps benign shape reordering tolerant.
fn drawing_shape_links(x: &[u8], rels: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(x);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // Stack of (shape identity, buffered `kind=value` links) for (possibly nested) shapes.
    let mut stack: Vec<(String, Vec<String>)> = Vec::new();
    let is_shape = |n: &[u8]| matches!(n, b"sp" | b"cxnSp" | b"pic" | b"graphicFrame");
    let flush = |out: &mut Vec<String>, id: &str, links: Vec<String>| {
        for l in links {
            out.push(format!("shape[{id}]|{l}"));
        }
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = structural::local_of(name.as_ref());
                if is_shape(local) {
                    let mut links = Vec::new();
                    if let Some(tl) = attr_local(&e, b"textlink").filter(|v| !v.is_empty()) {
                        links.push(format!("textlink={tl}"));
                    }
                    if let Some(m) = attr_local(&e, b"macro").filter(|v| !v.is_empty()) {
                        links.push(format!("macro={m}"));
                    }
                    stack.push((String::new(), links));
                } else if local == b"cNvPr" {
                    if let Some(top) = stack.last_mut() {
                        top.0 = attr_local(&e, b"name")
                            .filter(|v| !v.is_empty())
                            .or_else(|| attr_local(&e, b"id"))
                            .unwrap_or_default();
                    }
                } else if local == b"hlinkClick" {
                    if let Some(id) = rel_id(&e) {
                        let url = rels.get(&id).cloned().unwrap_or_default();
                        if let Some(top) = stack.last_mut() {
                            top.1.push(format!("hlink={url}"));
                        } else {
                            out.push(format!("shape[]|hlink={url}"));
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = structural::local_of(name.as_ref());
                if is_shape(local) {
                    // A childless shape can still carry textlink/macro attributes.
                    let mut links = Vec::new();
                    if let Some(tl) = attr_local(&e, b"textlink").filter(|v| !v.is_empty()) {
                        links.push(format!("textlink={tl}"));
                    }
                    if let Some(m) = attr_local(&e, b"macro").filter(|v| !v.is_empty()) {
                        links.push(format!("macro={m}"));
                    }
                    flush(&mut out, "", links);
                } else if local == b"cNvPr" {
                    if let Some(top) = stack.last_mut() {
                        top.0 = attr_local(&e, b"name")
                            .filter(|v| !v.is_empty())
                            .or_else(|| attr_local(&e, b"id"))
                            .unwrap_or_default();
                    }
                } else if local == b"hlinkClick" {
                    if let Some(id) = rel_id(&e) {
                        let url = rels.get(&id).cloned().unwrap_or_default();
                        if let Some(top) = stack.last_mut() {
                            top.1.push(format!("hlink={url}"));
                        } else {
                            out.push(format!("shape[]|hlink={url}"));
                        }
                    }
                }
            }
            Ok(Event::End(e)) if is_shape(structural::local_of(e.name().as_ref())) => {
                if let Some((id, links)) = stack.pop() {
                    flush(&mut out, &id, links);
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
                // A drawing's cell ANCHOR (`<from>` col/row/colOff/rowOff) is pure DISPLAY position
                // and is NOT compared: a desktop editor's oneCellAnchor<->twoCellAnchor re-encode
                // can move the anchor to the previous cell with a compensating EMU offset for the
                // IDENTICAL on-screen position (row=2,rowOff=0 == row=1,rowOff=190500), so any
                // col/row comparison spuriously refuses a positionally-faithful re-save. Chart
                // position changes no value and is outside certify's value/security charter; the
                // value-bearing drawing references (`<f>`, textlink, hlink) below ARE compared.
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
                // A linked-shape `textlink`, an "Assign Macro" `macro`, and a shape hyperlink
                // (`<a:hlinkClick r:id>` resolved through the drawing rels to an external URL) — each
                // keyed by the owning shape's stable identity so a SWAP of two shapes' targets differs
                // (a phishing retarget the cell diff / worksheet hyperlink scan never see).
                let rels = rels_targets(bytes, n);
                drawings.extend(drawing_shape_links(&x, &rels));
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
/// The persisted rich-value fields across `xl/richData/*.xml`, sorted (keyed by neither part name
/// nor order, so a benign renumber does not false-refuse). A rich value — a linked data type
/// (Stocks/Geography entity fields: `_DisplayString`, `Price`, …) or an `=IMAGE` store — holds the
/// cell's real OFFLINE value in `<v>` elements, reached from the cell via its `vm` (value-metadata)
/// index; the sheet cell carries only a `vm` pointer and a fallback `<v>` (e.g. `#VALUE!`). xlq's
/// transform copies richData verbatim, so a foreign REWRITE of a field (`420.5`→`999999`,
/// `MSFT`→`EVIL`) — a value/security change every cell diff misses because the cell text is
/// unchanged — differs here and is refused. Rich values do NOT auto-refresh on open, so this is a
/// static persisted value, not a volatile one; comparing it does not over-refuse a legitimate edit.
/// Every worksheet cell's value-metadata / cell-metadata binding (`vm`/`cm` attributes), keyed by
/// sheet and (shifted) cell ref, sorted. A rich-value cell points to its persisted value in
/// `xl/richData` through `vm` -> `xl/metadata.xml` valueMetadata -> rich value; the cell text is
/// only a `#VALUE!` fallback. A foreign edit that SWAPS `vm` (repointing `A1` from the MSFT rich
/// value to the AAPL one) changes the cell's real offline value with the richData store and cell
/// text both byte-identical, so neither `rich_data_values` nor the cell diff catches it — the
/// binding itself must be compared. xlq's transform shifts the cell (carrying `vm`/`cm` with it),
/// so a faithful edit keys identically and only a genuine repoint differs.
fn cell_metadata_bindings(bytes: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, part) in sheets {
        let Ok(xml) = crate::ooxml::read_part(bytes, &part) else {
            continue;
        };
        let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if structural::local_of(e.name().as_ref()) == b"c" =>
                {
                    let vm = attr_local(&e, b"vm");
                    let cm = attr_local(&e, b"cm");
                    if vm.is_some() || cm.is_some() {
                        out.push(format!(
                            "{name}|{}|vm={}|cm={}",
                            attr_local(&e, b"r").unwrap_or_default(),
                            vm.unwrap_or_default(),
                            cm.unwrap_or_default(),
                        ));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out.sort();
    out
}

/// The LOCKED state of each `<xf>` in styles.xml `<cellXfs>`, in document order. The default is
/// LOCKED (`true`); an `<xf>` carrying `<protection locked="0"/>` (or "false") is unlocked. The
/// resolution need not be perfectly Excel-accurate — only CONSISTENT between the two files, so a
/// genuine unlock (a change to the xf's protection) differs and a benign edit does not.
fn cellxfs_locked(styles: &[u8]) -> Vec<bool> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(styles);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut in_cellxfs = false;
    let mut in_xf = false;
    let mut cur = true;
    let locked_of = |e: &quick_xml::events::BytesStart| {
        attr_local(e, b"locked").map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match structural::local_of(e.name().as_ref()) {
                b"cellXfs" => in_cellxfs = true,
                b"xf" if in_cellxfs => {
                    in_xf = true;
                    cur = true;
                }
                b"protection" if in_xf => {
                    if let Some(l) = locked_of(&e) {
                        cur = l;
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match structural::local_of(e.name().as_ref()) {
                b"xf" if in_cellxfs => out.push(true),
                b"protection" if in_xf => {
                    if let Some(l) = locked_of(&e) {
                        cur = l;
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match structural::local_of(e.name().as_ref()) {
                b"cellXfs" => in_cellxfs = false,
                b"xf" if in_xf => {
                    out.push(cur);
                    in_xf = false;
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The (sheet, cell) of every UNLOCKED cell, sorted — compared ONLY when a `CELL("protect", …)`
/// formula reads a cell's lock state. Excel's cell diff and certify's style-is-benign rule both
/// miss an unlock (repointing a cell to an xf with `<protection locked="0"/>`), but
/// `CELL("protect", A1)` turns it into a computed-value change (`1`→`0`). Only unlocked cells are
/// emitted (locked is the default), so both a new unlock and a re-lock change the set.
fn cell_lock_states(bytes: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let locked = crate::ooxml::read_part(bytes, "xl/styles.xml")
        .map(|s| cellxfs_locked(&s))
        .unwrap_or_default();
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, part) in sheets {
        let Ok(xml) = crate::ooxml::read_part(bytes, &part) else {
            continue;
        };
        let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if structural::local_of(e.name().as_ref()) == b"c" =>
                {
                    let s: usize = attr_local(&e, b"s")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    // Default (no cellXfs / out-of-range index) is LOCKED, so absence is not emitted.
                    if !locked.get(s).copied().unwrap_or(true) {
                        out.push(format!(
                            "{name}|{}",
                            attr_local(&e, b"r").unwrap_or_default()
                        ));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out.sort();
    out
}

/// The resolved number-format CODE of each cellXf (by index) in xl/styles.xml. A custom numFmt (an
/// id declared in `<numFmts>`) resolves to its `formatCode`; a built-in id resolves to
/// `builtin:{id}` (the id IS the canonical key for built-ins). Used to detect a number-format change
/// that `CELL("format")` reads but that leaves the RENDERED value unchanged (so the display-based
/// `format` diff misses it).
fn cellxfs_numfmt_codes(styles: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(styles);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut custom: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut ids: Vec<u32> = Vec::new(); // cellXf index -> numFmtId
    let mut in_cellxfs = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match structural::local_of(e.name().as_ref()) {
                    b"numFmt" => {
                        if let (Some(id), Some(code)) = (
                            attr_local(&e, b"numFmtId").and_then(|v| v.parse::<u32>().ok()),
                            attr_local(&e, b"formatCode"),
                        ) {
                            custom.insert(id, code);
                        }
                    }
                    b"cellXfs" => in_cellxfs = true,
                    b"xf" if in_cellxfs => {
                        ids.push(
                            attr_local(&e, b"numFmtId")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(0),
                        );
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if structural::local_of(e.name().as_ref()) == b"cellXfs" => {
                in_cellxfs = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    ids.into_iter()
        .map(|id| {
            // A cell may carry a builtin numFmtId directly (what Excel/openpyxl emit) OR, after a
            // real-editor re-save, the SAME format materialized as a custom `<numFmt>` with the
            // equivalent formatCode. Resolve a builtin to its canonical ECMA-376 code so the two
            // forms compare EQUAL (else CELL("format") over-refused a faithful builtin->custom
            // expansion). A builtin without a canonical code (locale-reserved) stays `builtin:{id}`.
            custom
                .get(&id)
                .cloned()
                .or_else(|| builtin_numfmt_code(id).map(str::to_string))
                .unwrap_or_else(|| format!("builtin:{id}"))
        })
        .collect()
}

/// The canonical ECMA-376 (§18.8.30) format code of a BUILTIN number-format id, for the ids with a
/// standardized, locale-independent code. Locale-dependent (currency 5-8, 41-44) and reserved ids
/// return None (handled as `builtin:{id}`, fail-safe). Used to fold a builtin id and its expanded
/// custom `<numFmt>` to the same key for the CELL("format") comparison.
fn builtin_numfmt_code(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// The (sheet|cell, number-format-code) of every cell carrying a NON-default (non-General) number
/// format, sorted — compared only when a `CELL("format"/"color"/"parentheses", …)` formula reads a
/// cell's number format. `CELL("format")` returns a code derived from the format, so a restyle that
/// changes the format CODE (numFmtId 1 "0" -> 0 General) changes that formula's value even when the
/// cell's rendered value is identical ("5" either way) — which the display-based `format` diff
/// misses. An unreadable workbook returns a sentinel (fail-closed).
fn cell_number_formats(bytes: &[u8]) -> Vec<(String, String)> {
    use quick_xml::events::Event;
    let codes = crate::ooxml::read_part(bytes, "xl/styles.xml")
        .map(|s| cellxfs_numfmt_codes(&s))
        .unwrap_or_default();
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return vec![("__unreadable__".into(), String::new())];
    };
    let mut out = Vec::new();
    for (name, part) in sheets {
        let Ok(xml) = crate::ooxml::read_part(bytes, &part) else {
            out.push((format!("{name}|__unreadable__"), String::new()));
            continue;
        };
        let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if structural::local_of(e.name().as_ref()) == b"c" =>
                {
                    let s: usize = attr_local(&e, b"s")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let code = codes.get(s).map(String::as_str).unwrap_or("builtin:0");
                    // General (builtin:0) is the default; emit only non-default so a change TO or
                    // FROM General still flips the set, without listing every plain cell.
                    if code != "builtin:0" {
                        out.push((
                            format!("{name}|{}", attr_local(&e, b"r").unwrap_or_default()),
                            code.to_string(),
                        ));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out.sort();
    out
}

fn rich_data_values(bytes: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if !(low.starts_with("xl/richdata/") && low.ends_with(".xml")) {
            continue;
        }
        let base = n.rsplit('/').next().unwrap_or(n).to_ascii_lowercase();
        let Ok(x) = crate::ooxml::read_part(bytes, n) else {
            continue;
        };
        // Each `<v>` field is keyed by its POSITION in the part (round-48): a value-preserving
        // PERMUTATION of two rich-value records transposes which cell shows which value, so an
        // order-independent multiset (round-46) missed it. Position-keying makes a swap differ.
        let mut reader = quick_xml::Reader::from_reader(x.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        let mut cap = false;
        let mut raw = String::new();
        let mut seq = 0usize;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if structural::local_of(e.name().as_ref()) == b"v" => {
                    cap = true;
                    raw.clear();
                }
                Ok(Event::End(e)) if structural::local_of(e.name().as_ref()) == b"v" => {
                    if cap {
                        out.push(format!("{base}[{seq}]={raw}"));
                        seq += 1;
                        cap = false;
                    }
                }
                // A self-closing empty field `<v/>` (Event::Empty) is semantically identical to
                // `<v></v>` (empty content). Without this arm it was neither emitted nor counted,
                // shifting every subsequent field's POSITION key by one -> a faithful re-serialization
                // of an empty rich-value field was refused (round-58 defect 7). Emit the same empty
                // positional entry so the signature is invariant to the empty-element style.
                Ok(Event::Empty(e)) if structural::local_of(e.name().as_ref()) == b"v" => {
                    out.push(format!("{base}[{seq}]="));
                    seq += 1;
                }
                Ok(Event::Text(t)) if cap => {
                    // Keep the raw (still-escaped) bytes — both sides escape identically, so a
                    // rewrite/permutation still differs and a benign re-serialization does not.
                    raw.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
                // REGRESSION (round-54 defect 9, HIGH false-certify): under quick-xml an entity /
                // numeric char-reference (`&amp;`, `&#57;`) inside `<v>` arrives as a SEPARATE
                // GeneralRef event, and CDATA as a CData event — the Text-only capture dropped both,
                // so a tampered rich value (`420.5` -> `&#57;420.5` = "9420.5") whose literal runs
                // stayed byte-identical CERTIFIED. Reassemble the entity/CDATA raw (both sides
                // escape identically), so any entity insert/delete/substitution differs.
                Ok(Event::GeneralRef(r)) if cap => {
                    raw.push('&');
                    raw.push_str(&r.decode().unwrap_or_default());
                    raw.push(';');
                }
                Ok(Event::CData(c)) if cap => {
                    raw.push_str(&String::from_utf8_lossy(c.as_ref()));
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out.sort();
    out
}

/// The index mapping inside `xl/metadata.xml`, captured in DOCUMENT ORDER — the MIDDLE link of the
/// rich-value resolution chain `cell.vm -> valueMetadata <rc v> -> futureMetadata <bk> ...
/// <xlrd:rvb i> -> richData record`. Round 46 compared the richData records and round 47 the cell
/// `vm`, but the `rc v`/`rvb i` remap between them was uncompared — with 2+ records, remapping
/// `rvb i="0"` to `i="1"` repoints WHICH record a cell resolves to while both endpoints stay
/// byte-identical. Each index-bearing element (`rc`, `rvb`, `cm`) is keyed by its position so a
/// reorder or reindex differs; a benign re-serialization (whitespace/attr-order) does not.
fn metadata_index_chain(bytes: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let Ok(x) = crate::ooxml::read_part(bytes, "xl/metadata.xml") else {
        return Vec::new();
    };
    let mut reader = quick_xml::Reader::from_reader(x.as_slice());
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut seq = 0usize;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = structural::local_of(name.as_ref());
                if matches!(local, b"rc" | b"rvb" | b"cm") {
                    let l = String::from_utf8_lossy(local).into_owned();
                    let mut a: Vec<String> = e
                        .attributes()
                        .flatten()
                        .map(|at| {
                            format!(
                                "{}={}",
                                String::from_utf8_lossy(structural::local_of(at.key.as_ref())),
                                at.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .map(|c| c.into_owned())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect();
                    a.sort();
                    out.push(format!("{seq}:{l}|{}", a.join(" ")));
                    seq += 1;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// ORDER-faithful pivot signatures that `element_attr_semantics` (position-blind, sorted) cannot
/// produce: the AXIS-MEMBERSHIP lists — the ORDERED `<field x="N"/>` sequence of
/// `<rowFields>`/`<colFields>`/`<pageFields>` and the ordered `<dataField fld>` sequence of
/// `<dataFields>` — which authoritatively name WHICH cache field sits on WHICH axis and in what
/// order, PLUS each `<pivotField>`'s POSITION-keyed axis. A coherent axis/measure reassignment (group
/// by Product instead of Region; transpose two value-area measure columns) re-aggregates the pivot on
/// refresh but leaves the pooled/sorted pivotField multiset unchanged — this makes it differ
/// (round-59 defect 1), mirroring table_semantics' col[idx] positional keying.
fn pivot_ordered_sigs(xml: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut in_pivotfields = false;
    let mut pf_idx = 0i64;
    // (container local name, ordered child keys)
    let mut container: Option<(&'static str, Vec<String>)> = None;
    // Handle a leaf (pivotField / field / dataField), for both Start and Empty events.
    let leaf = |e: &quick_xml::events::BytesStart,
                in_pf: bool,
                container: &mut Option<(&'static str, Vec<String>)>,
                pf_idx: &mut i64,
                out: &mut Vec<String>| {
        match structural::local_of(e.name().as_ref()) {
            b"pivotField" if in_pf => {
                out.push(format!(
                    "pivotField[{pf_idx}]|axis={}",
                    attr_local(e, b"axis").unwrap_or_default()
                ));
                *pf_idx += 1;
            }
            b"field" => {
                if let Some((_, v)) = container {
                    v.push(attr_local(e, b"x").unwrap_or_default());
                }
            }
            b"dataField" => {
                if let Some((name, v)) = container {
                    if *name == "dataFields" {
                        v.push(format!(
                            "fld={}|name={}",
                            attr_local(e, b"fld").unwrap_or_default(),
                            attr_local(e, b"name").unwrap_or_default()
                        ));
                    }
                }
            }
            _ => {}
        }
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match structural::local_of(e.name().as_ref()) {
                b"pivotFields" => in_pivotfields = true,
                b"rowFields" => container = Some(("rowFields", Vec::new())),
                b"colFields" => container = Some(("colFields", Vec::new())),
                b"pageFields" => container = Some(("pageFields", Vec::new())),
                b"dataFields" => container = Some(("dataFields", Vec::new())),
                _ => leaf(&e, in_pivotfields, &mut container, &mut pf_idx, &mut out),
            },
            Ok(Event::Empty(e)) => leaf(&e, in_pivotfields, &mut container, &mut pf_idx, &mut out),
            Ok(Event::End(e)) => match structural::local_of(e.name().as_ref()) {
                b"pivotFields" => in_pivotfields = false,
                b"rowFields" | b"colFields" | b"pageFields" | b"dataFields" => {
                    if let Some((name, v)) = container.take() {
                        out.push(format!("{name}=[{}]", v.join(",")));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

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
                    // The pivotTable ROOT: grand-total / subtotal-scope toggles that
                    // add/remove/retype materialized total cells on refresh (round-58 defect 1).
                    b"pivotTableDefinition",
                    // Filter/layout surface: which field sits on which axis, which items are
                    // HIDDEN (a manual report filter), the page (report) filter selection, and
                    // label/value filters. A change to any re-aggregates the pivot on refresh
                    // (automatic under refreshOnLoad) — a value the materialized output cells the
                    // cell diff sees do not yet reflect.
                    b"pivotField",
                    b"item",
                    b"pageField",
                    b"filter",
                ],
            ) {
                let pick = |key: &str| {
                    attrs
                        .split(structural::ATTR_SEP)
                        .find_map(|kv| kv.strip_prefix(key))
                        .unwrap_or("")
                        .to_string()
                };
                let boolish = |v: String, default_true: bool| {
                    if v.is_empty() {
                        if default_true {
                            "1"
                        } else {
                            "0"
                        }
                    } else if v == "1" || v.eq_ignore_ascii_case("true") {
                        "1"
                    } else {
                        "0"
                    }
                };
                let sig = match tag.as_str() {
                    // A `<dataField>`'s aggregation (`subtotal`, default "sum" when absent) is the
                    // VALUE the pivot materializes — a SUM->COUNT flip changes the output column.
                    // `showDataAs` (the "Show Values As" operation: percentOfCol/runTotal/…, default
                    // "normal") likewise transforms every data cell on the next refresh (a SUM ->
                    // "% of column" flip) — a silent value corruption of pivot output.
                    "dataField" => {
                        let st = pick("subtotal=");
                        let st = if st.is_empty() { "sum".to_string() } else { st };
                        let sda = pick("showDataAs=");
                        let sda = if sda.is_empty() {
                            "normal".to_string()
                        } else {
                            sda
                        };
                        format!(
                            "dataField|name={}|fld={}|subtotal={st}|showDataAs={sda}|baseField={}|baseItem={}",
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
                        format!(
                            "pivotCacheDefinition|refreshOnLoad={}",
                            boolish(pick("refreshOnLoad="), false)
                        )
                    }
                    // The pivotTable ROOT toggles that add/remove/retype materialized total cells on
                    // refresh: rowGrandTotals/colGrandTotals (default TRUE — "0" removes the grand-
                    // total row/col), subtotalHiddenItems (default FALSE — "1" folds hidden-item data
                    // into every subtotal/grand total), dataOnRows (orientation of the data axis).
                    "pivotTableDefinition" => format!(
                        "pivotTableDefinition|rowGrand={}|colGrand={}|subtotalHiddenItems={}|dataOnRows={}",
                        boolish(pick("rowGrandTotals="), true),
                        boolish(pick("colGrandTotals="), true),
                        boolish(pick("subtotalHiddenItems="), false),
                        boolish(pick("dataOnRows="), false),
                    ),
                    // Which field is placed on which axis (row/col/page/data) — a re-placement
                    // re-pivots the output — PLUS the subtotal-function flags that govern which
                    // subtotal rows the field materializes and with what aggregate: defaultSubtotal
                    // (default TRUE — "0" drops the automatic Sum subtotal) and each per-function
                    // boolean (default FALSE — "1" adds a subtotal row computed with that function).
                    "pivotField" => format!(
                        "pivotField|axis={}|dataField={}|defaultSubtotal={}|{}",
                        pick("axis="),
                        boolish(pick("dataField="), false),
                        boolish(pick("defaultSubtotal="), true),
                        [
                            "sumSubtotal",
                            "countASubtotal",
                            "avgSubtotal",
                            "maxSubtotal",
                            "minSubtotal",
                            "productSubtotal",
                            "countSubtotal",
                            "stdDevSubtotal",
                            "stdDevPSubtotal",
                            "varSubtotal",
                            "varPSubtotal",
                        ]
                        .iter()
                        .map(|f| format!("{f}={}", boolish(pick(&format!("{f}=")), false)))
                        .collect::<Vec<_>>()
                        .join("|"),
                    ),
                    // A pivot field item: `h="1"` HIDES it (a manual filter that drops its row and
                    // changes the grand total); `x` is its cache index, `t` its type (default
                    // "data"). Defaults normalized so a foreign editor writing them explicitly is
                    // not a spurious divergence.
                    "item" => {
                        let t = pick("t=");
                        let t = if t.is_empty() { "data".to_string() } else { t };
                        format!(
                            "item|x={}|h={}|t={t}|sd={}",
                            pick("x="),
                            boolish(pick("h="), false),
                            boolish(pick("sd="), true),
                        )
                    }
                    // The report (page) filter selection.
                    "pageField" => format!(
                        "pageField|fld={}|item={}|hier={}|name={}",
                        pick("fld="),
                        pick("item="),
                        pick("hier="),
                        pick("name="),
                    ),
                    // A label/value/date auto-filter on a pivot field. `stringValue1`/`stringValue2`
                    // hold the comparison THRESHOLD (e.g. "> 1000") — the value that decides which
                    // rows the pivot keeps on refresh; loosening it (1000 -> 0) re-materializes a
                    // larger aggregate. The nested `<autoFilter><customFilter operator val>`
                    // predicate is compared by autofilter_criteria (which now also scans pivots).
                    "filter" => format!(
                        "filter|fld={}|type={}|id={}|iMeasureFld={}|evalOrder={}|sv1={}|sv2={}",
                        pick("fld="),
                        pick("type="),
                        pick("id="),
                        pick("iMeasureFld="),
                        pick("evalOrder="),
                        pick("stringValue1="),
                        pick("stringValue2="),
                    ),
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
            // A pivot CALCULATED FIELD (`<cacheField formula="Revenue-Cost" databaseField="0"/>`)
            // and calculated item/member (`<calculatedItem>`/`<calculatedMember formula=…>`) are
            // re-aggregation INPUTS: on refresh the pivot recomputes every data cell from these
            // formulas, so tampering one silently corrupts the output. `element_attr_semantics`
            // space-joins its attribute string, which truncates a formula containing spaces, so read
            // these formula attributes DIRECTLY (full value) instead.
            out.extend(pivot_calc_formula_sigs(&x));
            // ORDER-faithful axis-membership + position-keyed pivotField axis (element_attr_semantics
            // is position-blind and sorted, so a coherent axis/measure swap is invisible to it).
            out.extend(pivot_ordered_sigs(&x));
        }
    }
    out.sort();
    out
}

/// Full-value signatures for a pivot part's calculated-field / calculated-item / calculated-member
/// FORMULAS (read directly, so a formula containing spaces is not truncated). A cache field is
/// emitted only when it carries a `formula` (a calculated field); a plain source `cacheField` has
/// none and is skipped (its identity is compared elsewhere via the dataField `fld` index).
fn pivot_calc_formula_sigs(xml: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let kind = match structural::local_of(name.as_ref()) {
                    b"cacheField" => "cacheField",
                    b"calculatedItem" => "calculatedItem",
                    b"calculatedMember" => "calculatedMember",
                    _ => {
                        buf.clear();
                        continue;
                    }
                };
                let formula = attr_local(&e, b"formula");
                // Only a cacheField WITH a formula is a calculated field; a plain source column has
                // none. calculatedItem/Member always carry one.
                if kind == "cacheField" && formula.is_none() {
                    buf.clear();
                    continue;
                }
                out.push(format!(
                    "{kind}|name={}|field={}|formula={}|databaseField={}",
                    attr_local(&e, b"name").unwrap_or_default(),
                    attr_local(&e, b"field").unwrap_or_default(),
                    formula.unwrap_or_default(),
                    attr_local(&e, b"databaseField").unwrap_or_default(),
                ));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
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
            // EXPAND shared formulas first: a shared group stores the body (and its hidden token)
            // only on the MASTER cell, so scanning the raw XML sees the token on one cell in xlq's
            // (shared-preserving) transform but on EVERY cell in a foreign edit that un-shares the
            // group (openpyxl/LibreOffice). Expanding both sides makes the per-cell token map
            // invariant to the shared<->expanded encoding, closing that over-refusal while a genuine
            // token add/drop/relocation still differs.
            let x = structural::expand_shared_in_sheet(&x).unwrap_or(x);
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
            // Expand shared formulas for symmetry with hidden_tokens_all (array formulas are never
            // shared, so this is a no-op for them — but it keeps the scan encoding-invariant).
            let x = structural::expand_shared_in_sheet(&x).unwrap_or(x);
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
/// Canonicalize an autofilter criterion element's `element_attr_semantics` attribute string so a
/// benign cross-tool re-serialization of DEFAULT-valued attributes is not refused: fold each boolean
/// literal `true`/`false` to `1`/`0`, then DROP the tokens whose value equals the ECMA-376 default
/// (`top=1`, `percent=0` on `<top10>`; `and=0` on `<customFilters>`; `blank=0` on `<filters>`). All
/// OTHER attributes are kept verbatim, so a genuine criterion change still differs (no false
/// certify). Keeping the whole attribute set — rather than PICKING known keys like pivot_refs — is
/// the safe direction here (a missed value-affecting attr would be a false-certify).
fn normalize_filter_attrs(attrs: &str) -> String {
    attrs
        .split(structural::ATTR_SEP)
        .filter_map(|tok| {
            let (k, v) = tok.split_once('=')?;
            let v = match v {
                "true" => "1",
                "false" => "0",
                other => other,
            };
            let is_default = matches!(
                (k, v),
                ("top", "1")
                    | ("percent", "0")
                    | ("and", "0")
                    | ("blank", "0")
                    | ("operator", "equal")
            );
            if is_default {
                None
            } else {
                Some(format!("{k}={v}"))
            }
        })
        .collect::<Vec<_>>()
        .join(structural::ATTR_SEP)
}

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
                    .split(structural::ATTR_SEP)
                    .filter(|t| !t.starts_with("hiddenButton=") && !t.starts_with("showButton="))
                    .collect::<Vec<_>>()
                    .join(structural::ATTR_SEP)
            } else {
                attrs
            };
            // Fold ECMA-376 DEFAULT attributes and canonicalize boolean literals, so a benign
            // cross-tool re-serialization (openpyxl omits `<top10 top percent>` / `and`; LibreOffice
            // writes them explicitly as `top="true" percent="false"` / `and="true"`) is not refused
            // while a genuine criterion change (top<->bottom, AND<->OR, a changed threshold/operator)
            // still differs (round-57 defect 3).
            let attrs = normalize_filter_attrs(&attrs);
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
    // false-refuse (a real filter change still differs within the sorted set). Pivot filters carry
    // the SAME nested `<autoFilter><filterColumn><customFilter operator val>` predicate under a
    // `<filter>` (CT_PivotFilter) — the value/label THRESHOLD that decides which rows the pivot
    // materializes on refresh — so scan pivotTable parts with the same proven comparator instead of
    // re-implementing predicate parsing in pivot_refs.
    for n in structural::archive_names(bytes).unwrap_or_default() {
        let low = n.to_ascii_lowercase();
        if low.ends_with(".xml") {
            let owner = if low.starts_with("xl/tables/") {
                "table"
            } else if low.starts_with("xl/pivottables/") {
                "pivot"
            } else {
                continue;
            };
            if let Ok(x) = crate::ooxml::read_part(bytes, &n) {
                extract(owner, &x);
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
/// Canonicalize a protection element's attribute string so a value-identical re-serialization is
/// not refused: the `sqref` is an UNORDERED cell SET (a protectedRange written `A1:A5 B1:B5` vs
/// `B1:B5 A1:A5`, or `A1:B2` vs `A1:A2 B1:B2`, is the same set) — fold it through `canonical_sqref`
/// exactly as the CF/DV sqref is; and fold xsd:boolean literals `true`/`false` to `1`/`0`
/// (`sheet="true"` == `sheet="1"`). All other attribute values (name, password hash) are kept
/// verbatim, so a genuine protection change still differs (round-58 defect 3).
fn canonicalize_protection_attrs(attrs: &str) -> String {
    attrs
        .split(structural::ATTR_SEP)
        .map(|tok| match tok.split_once('=') {
            Some(("sqref", v)) => format!("sqref={}", structural::canonical_sqref(v)),
            Some((k, "true")) => format!("{k}=1"),
            Some((k, "false")) => format!("{k}=0"),
            _ => tok.to_string(),
        })
        .collect::<Vec<_>>()
        .join(structural::ATTR_SEP)
}

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
            out.push((
                "(workbook)".to_string(),
                elem,
                canonicalize_protection_attrs(&attrs),
            ));
        }
    }
    if let Ok(sheets) = crate::ooxml::all_sheets(bytes) {
        for (sheet_name, part) in sheets {
            if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
                for (elem, attrs) in
                    structural::element_attr_semantics(&x, &[b"sheetProtection", b"protectedRange"])
                {
                    out.push((
                        sheet_name.clone(),
                        elem,
                        canonicalize_protection_attrs(&attrs),
                    ));
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
/// True when a rels hyperlink `target` denotes the workbook's OWN file — a bare relative
/// filename (no directory separator, no scheme) equal to `own_name`. Such a target resolves,
/// relative to the workbook's own directory, to the workbook itself, so it is semantically an
/// INTERNAL jump (LibreOffice encodes same-document links this way). The bare-name requirement
/// keeps this SOUND: a path- or scheme-qualified `../min.xlsx` / `file:///x/min.xlsx` could name
/// a DIFFERENT file, so it is left external (a fail-safe over-refusal, never a false certify).
fn hyperlink_target_is_own_file(target: &str, own_name: &str) -> bool {
    !own_name.is_empty() && target == own_name && !target.contains('/') && !target.contains('\\')
}

/// Canonicalize the SHEET QUALIFIER of an internal-hyperlink destination to its bare (unquoted)
/// form, so every encoding of one destination folds to a single key: `'My Data'!A8`, `My Data!A8`,
/// `'Data'!A8`, and `Data!A8` all normalize to `<bare sheet>!<cell>`. Tools disagree on whether to
/// quote a (space-bearing) sheet name in a `location` / rel-target, so the hyperlink dest needs the
/// same quote-normalization every other reference surface already gets. A DIFFERENT sheet, cell, or
/// external target still differs, so a real mispoint / phishing retarget is still caught.
fn canonicalize_hyperlink_dest(dest: &str) -> String {
    // A sheet name cannot contain `!`, so the first `!` separates `sheet!cell`.
    let Some((sheet, cell)) = dest.split_once('!') else {
        return dest.to_string();
    };
    let bare = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        Some(inner) => inner.replace("''", "'"), // '' is an escaped quote inside a quoted name
        None => sheet.to_string(),
    };
    format!("{bare}!{cell}")
}

fn structural_ref_attrs(bytes: &[u8], own_name: &str) -> Vec<(String, String, String)> {
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
                            // swap, a mispoint to another file) differs. A THIRD encoding of the
                            // same internal jump is a self-referential external Target naming the
                            // workbook's own file (`Target="min.xlsx" TargetMode="External"` +
                            // `location`, written by LibreOffice) — folded to internal too, so a
                            // faithful cross-tool edit is not refused.
                            let (dest, ext) = if let Some(internal) = target.strip_prefix('#') {
                                (internal.to_string(), String::new())
                            } else if target.is_empty()
                                || hyperlink_target_is_own_file(target, own_name)
                            {
                                (location.clone(), String::new())
                            } else {
                                (location.clone(), target.to_string())
                            };
                            // Normalize the dest's sheet-quote so a faithful edit that quotes (or
                            // unquotes) the sheet name of the same destination is not refused.
                            let dest = canonicalize_hyperlink_dest(&dest);
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
    use quick_xml::events::Event;
    let mut map = std::collections::BTreeMap::new();
    let Some((dir, file)) = sheet_part.rsplit_once('/') else {
        return map;
    };
    let rels_part = format!("{dir}/_rels/{file}.rels");
    let Ok(part) = crate::ooxml::read_part(bytes, &rels_part) else {
        return map;
    };
    // Namespace-aware walk (NOT a `<Relationship ` substring scan): a prefixed `<pr:Relationship>`
    // bound to the packaging namespace, or a non-space whitespace after the element name
    // (`<Relationship\nId=…>`), is valid XML that a literal substring misses — letting an injected
    // external target evade resolution and CERTIFY. Matched by LOCAL name, attributes by local name.
    let mut reader = quick_xml::Reader::from_reader(part.as_slice());
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if structural::local_of(e.name().as_ref()) == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (attr_local(&e, b"Id"), attr_local(&e, b"Target"))
                {
                    map.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// Every EXTERNAL relationship target across ALL `*.rels` parts. Hyperlink-typed relationships are
/// skipped ONLY for WORKSHEET-owned rels — those have their own normalized comparator
/// (`structural_ref_attrs`, with the internal-jump / self-file folds), so re-emitting them here
/// would double-refuse the folded forms. A hyperlink external owned by any OTHER part (a chart /
/// drawing `hlinkClick` / `hlinkHover`) is folded nowhere, so it IS emitted and compared here.
/// This closes the hole the blanket `.rels` allowlist left open: certify resolved external targets
/// for only worksheet hyperlink + drawing `hlinkClick`, so a LINKED image (`<a:blip r:link>`), a
/// drawing hover hyperlink, a CHART-part hyperlink (title/label), a linked OLE server, linked
/// media, or an external-workbook link — all `TargetMode="External"` in an allowlisted `.rels`
/// with a byte-identical owning part — could be repointed to an attacker URL/UNC and CERTIFY. xlq's
/// transform copies these verbatim, so a faithful edit keys identically; only a genuine repoint
/// (or an inserted/removed external link) changes the sorted multiset. Keyed by relationship TYPE +
/// TARGET, not by part name, so a benign part renumber does not false-refuse.
fn external_rels_targets(bytes: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    let names = structural::archive_names(bytes).unwrap_or_default();
    let mut out = Vec::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        if !low.ends_with(".rels") {
            continue;
        }
        // A worksheet's own rels (`xl/worksheets/_rels/sheetN.xml.rels`) — its hyperlinks are folded
        // by structural_ref_attrs and must not be double-compared here.
        let worksheet_owned = low.starts_with("xl/worksheets/_rels/");
        let Ok(part) = crate::ooxml::read_part(bytes, n) else {
            continue;
        };
        // Namespace-aware walk (see rels_targets): a prefixed / whitespace-varied `<Relationship>`
        // must NOT evade the external-target comparison, else an injected linked-image / OLE / media
        // / hyperlink target hides from the signature and CERTIFIES (SSRF / NTLM-UNC leak / phishing).
        let mut reader = quick_xml::Reader::from_reader(part.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if structural::local_of(e.name().as_ref()) == b"Relationship" =>
                {
                    // Only externally-resolved targets are a repoint surface; internal (package)
                    // targets are parts compared by the part allowlist / their own bytes.
                    let is_external = attr_local(&e, b"TargetMode")
                        .is_some_and(|m| m.eq_ignore_ascii_case("External"));
                    if is_external {
                        if let (Some(ty), Some(target)) =
                            (attr_local(&e, b"Type"), attr_local(&e, b"Target"))
                        {
                            // The type's local segment (`.../relationships/image` -> `image`).
                            let ty_local = ty.rsplit(['/', ':']).next().unwrap_or(&ty).to_string();
                            // A WORKSHEET hyperlink is folded elsewhere — skip it here; a
                            // chart/drawing hyperlink is folded nowhere, so compare it.
                            if !(worksheet_owned && ty_local.eq_ignore_ascii_case("hyperlink")) {
                                // A single trailing `/` on a bare-authority URL is a benign
                                // renormalization; a real retarget still differs on host/path.
                                let target = target.strip_suffix('/').unwrap_or(&target);
                                // Bind the target to its relationship Id so TRANSPOSING two same-type
                                // external targets within a part (a chart-title vs series hyperlink,
                                // two linked-image blips) — which left the pooled (type,target)
                                // multiset unchanged — now differs (round-60 defect 5). The rId is
                                // intra-part (xlq copies rels verbatim; a referencing element resolves
                                // it), so a benign part renumber still keys identically.
                                let rid = attr_local(&e, b"Id").unwrap_or_default();
                                out.push(format!("ext|{ty_local}|{rid}|{target}"));
                            }
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out.sort();
    out
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
    workbook_has_cell_info_fn(bytes, &CELL_FORMAT_SENSITIVE)
}

/// True when any worksheet formula calls `CELL()` with one of the given info types (or a non-literal
/// info type — conservatively treated as any).
fn workbook_has_cell_info_fn(bytes: &[u8], info: &[&str]) -> bool {
    // (1) Worksheet formula bodies.
    let in_sheets = crate::ooxml::all_sheets(bytes)
        .map(|sheets| {
            sheets.into_iter().any(|(_name, part)| {
                crate::ooxml::read_part(bytes, &part).is_ok_and(|xml| {
                    structural::element_text_semantics(&xml, &[b"f"])
                        .iter()
                        .any(|f| formula_calls_sensitive_cell(f, info))
                })
            })
        })
        .unwrap_or(false);
    if in_sheets {
        return true;
    }
    // (2) DEFINED-NAME refers-to bodies (workbook.xml `<definedName>`): a name `FA=CELL("format",A1)`
    // reached through a cell `=FA` calls CELL indirectly, bypassing the worksheet-only scan — so a
    // foreign restyle that changes what CELL reads would false-certify as a benign `format` diff.
    crate::ooxml::read_part(bytes, "xl/workbook.xml").is_ok_and(|wb| {
        structural::defined_names(&wb)
            .iter()
            .any(|(_n, _scope, refers)| formula_calls_sensitive_cell(refers, info))
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
fn formula_calls_sensitive_cell(f: &str, sensitive: &[&str]) -> bool {
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
                            if sensitive.iter().any(|t| f[s..k].eq_ignore_ascii_case(t)) {
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

/// The first REAL start-tag in `text` whose element LOCAL name is `local`, namespace-prefix
/// agnostic — both `<calcPr …>` and `<x:calcPr …>` match — returned from its `<` up to (not
/// including) the closing `>`. A raw `text.find("<calcPr")` missed a prefixed `<x:calcPr>`, and a
/// naive scan read INTO XML comments / CDATA / PIs and terminated at a `>` inside a quoted attribute
/// value — letting a commented-out DECOY `<!--<workbookPr date1904="0"/>-->` (or a `>` inside a
/// quoted attr) hide a real value-affecting setting (date1904/fullPrecision/calcMode) so certify
/// read the default and false-certified an epoch/precision flip (round-57 defect 7). This scanner
/// SKIPS comment/CDATA/PI/decl spans and finds the tag end with a quote-state machine.
fn local_element_tag(text: &str, local: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(rel) = text[i..].find('<') {
        let lt = i + rel;
        // Skip non-element spans whose content is not markup.
        if text[lt..].starts_with("<!--") {
            i = text[lt + 4..]
                .find("-->")
                .map_or(bytes.len(), |p| lt + 4 + p + 3);
            continue;
        }
        if text[lt..].starts_with("<![CDATA[") {
            i = text[lt + 9..]
                .find("]]>")
                .map_or(bytes.len(), |p| lt + 9 + p + 3);
            continue;
        }
        if text[lt..].starts_with("<?") {
            i = text[lt + 2..]
                .find("?>")
                .map_or(bytes.len(), |p| lt + 2 + p + 2);
            continue;
        }
        if text[lt..].starts_with("<!") {
            i = text[lt..].find('>').map_or(bytes.len(), |p| lt + p + 1);
            continue;
        }
        // A real element. Read its (possibly prefixed) name.
        let name_start = lt + 1;
        if bytes.get(name_start) == Some(&b'/') {
            // an end tag — not a start tag; advance past it.
            i = text[lt..].find('>').map_or(bytes.len(), |p| lt + p + 1);
            continue;
        }
        let mut j = name_start;
        while j < bytes.len() && !matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') {
            j += 1;
        }
        let local_name = {
            let name = &text[name_start..j];
            name.rsplit(':').next().unwrap_or(name)
        };
        // Find the tag-closing '>' with a quote-state machine so a '>' inside a quoted attribute
        // value is not mistaken for the tag end.
        let mut k = j;
        let mut quote = 0u8;
        let tag_end = loop {
            match bytes.get(k) {
                None => return None, // unterminated tag
                Some(&b) if quote != 0 => {
                    if b == quote {
                        quote = 0;
                    }
                }
                Some(&b) if b == b'"' || b == b'\'' => quote = b,
                Some(&b'>') => break k,
                _ => {}
            }
            k += 1;
        };
        if local_name == local {
            return Some(text[lt..tag_end].to_string());
        }
        i = tag_end + 1;
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
/// IronCalc's NOT-IMPLEMENTED sentinel (its `en` rendering of `Error::NIMPL`). The importer loads
/// a `t="d"` (ISO-8601 date) VALUE cell as this, and propagates it through any reading formula. It
/// is the engine explicitly admitting it cannot reproduce Excel, so it must never vouch a cache.
const NIMPL_SENTINEL: &str = "#N/IMPL!";

/// True when the engine evaluates the cell to its NOT-IMPLEMENTED sentinel.
fn cell_is_nimpl(model: &ironcalc::base::Model, sheet: u32, row: i32, col: i32) -> bool {
    use ironcalc::base::cell::CellValue;
    matches!(
        model.get_cell_value_by_index(sheet, row, col),
        Ok(CellValue::String(s)) if s == NIMPL_SENTINEL
    )
}

fn cell_value_sig(model: &ironcalc::base::Model, sheet: u32, row: i32, col: i32) -> Option<String> {
    use ironcalc::base::cell::CellValue;
    match model.get_cell_value_by_index(sheet, row, col) {
        Ok(CellValue::Number(n)) => Some(format!("n:{n}")),
        Ok(CellValue::Boolean(b)) => Some(format!("b:{}", if b { "1" } else { "0" })),
        // The engine's NOT-IMPLEMENTED sentinel is unvouchable — emit no signature so a preserved
        // foreign cache is never matched against a value the engine could not actually compute
        // (a `t="d"` date cell, or a formula that reads one).
        Ok(CellValue::String(s)) if s == NIMPL_SENTINEL => None,
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

/// The subset of DATE_EPOCH_FUNCTIONS that PRODUCE a date serial (rather than consuming one to
/// return a calendar part or a difference). Under the DEFAULT 1900 date system the engine follows
/// Google-Docs / LibreOffice semantics — it omits Excel's phantom 1900-02-29 leap day (a
/// deliberate engine design choice, not a bug: see base `test_date_early_dates`) — so a serial one
/// of these produces that lands BEFORE 1900-03-01 (value < 61) is off by one from Excel's stored
/// serial (`DATE(1900,1,1)` = 2 here, 1 in Excel). A preserved foreign cache carrying either
/// serial is therefore unvouchable: excluded value-gated in `build_cache_oracle` so that the
/// ubiquitous MODERN date cache (serial >= 61, where the engine and Excel agree) stays vouchable.
/// Uppercase to match `extract_function_names`.
const DATE_SERIAL_PRODUCERS: &[&str] = &[
    "DATE",
    "DATEVALUE",
    "EDATE",
    "EOMONTH",
    "WORKDAY",
    "WORKDAY.INTL",
];

/// The subset of DATE_EPOCH_FUNCTIONS that CONSUME a serial to return a calendar component or a
/// day-count. The engine routes these through `from_excel_date`, which omits Excel's phantom
/// 1900-02-29 — so for an INPUT serial < 61 the engine's day/month/year/weekday is off by one from
/// Excel (`DAY(59)` = 27 here, 28 in Excel). `DAYS360` is deliberately EXCLUDED: it already uses the
/// phantom-leap-day-aware `excel_serial_to_ymd`, so it matches Excel for all serials. A consumer
/// reading an early serial is value-gated out of the oracle in `build_cache_oracle` (else the
/// engine's wrong value would be vouched); a consumer reading only modern serials (>= 61) stays
/// vouchable — no blanket over-refusal of the ubiquitous DAY/MONTH/YEAR.
const DATE_CONSUMER_FUNCTIONS: &[&str] = &[
    "YEAR",
    "MONTH",
    "DAY",
    "WEEKDAY",
    "WEEKNUM",
    "ISOWEEKNUM",
    "NETWORKDAYS",
    "NETWORKDAYS.INTL",
    "DAYS",
    "YEARFRAC",
    "DATEDIF",
    // EDATE/EOMONTH/WORKDAY/WORKDAY.INTL also PRODUCE a serial, but they CONSUME one in arg 0 too:
    // reading a pre-1900-03-01 input (serial < 61) lands the engine up to a month off from Excel even
    // when the OUTPUT is modern, so the producer's output-only gate misses it (round-60 defect 4). The
    // serial-arg index is the default [0]; the word-boundary matcher separates WORKDAY/WORKDAY.INTL.
    "EDATE",
    "EOMONTH",
    "WORKDAY",
    "WORKDAY.INTL",
];

/// The serial-valued ARGUMENT positions (0-based) of a date-consumer function — the arguments whose
/// value determines whether the engine's phantom-leap-day omission makes the result diverge from
/// Excel (divergent only for an INPUT serial < 61). `WEEKDAY`/`WEEKNUM`'s 2nd argument is a
/// return-type code, NOT a serial, so it is deliberately excluded; the day-difference functions take
/// two serials.
fn date_consumer_serial_arg_indices(fname: &str) -> &'static [usize] {
    match fname {
        "NETWORKDAYS" | "NETWORKDAYS.INTL" | "DAYS" | "YEARFRAC" | "DATEDIF" => &[0, 1],
        _ => &[0],
    }
}

/// Split a function call's argument list — `inner` is the formula text AFTER the opening `(` — into
/// its top-level comma-separated argument expressions, stopping at the matching `)`. Depth- and
/// quote-aware: a comma nested in inner parens/brackets/braces, inside a `"string literal"`, or
/// inside a `'Sheet Name'!` qualifier does NOT split an argument.
fn split_top_level_args(inner: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_dquote = false;
    let mut in_squote = false;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if in_dquote {
            cur.push(c);
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push(chars.next().unwrap()); // an escaped "" inside the string
                } else {
                    in_dquote = false;
                }
            }
            continue;
        }
        if in_squote {
            cur.push(c);
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    cur.push(chars.next().unwrap()); // an escaped '' inside a sheet name
                } else {
                    in_squote = false;
                }
            }
            continue;
        }
        match c {
            '"' => {
                in_dquote = true;
                cur.push(c);
            }
            '\'' => {
                in_squote = true;
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                if c == ')' && depth == 0 {
                    args.push(cur.trim().to_string());
                    return args;
                }
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                args.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    // Unterminated (malformed) — return what was parsed; the caller fails closed on a bad probe.
    let last = cur.trim();
    if !last.is_empty() {
        args.push(last.to_string());
    }
    args
}

/// Every serial-valued ARGUMENT EXPRESSION of every date-consumer function call in `formula`, plus a
/// flag reporting whether at least one consumer call was recognized. Evaluating these expressions
/// yields the consumer's actual INPUT serial, which detects a divergent early-date input no matter
/// HOW it arrives — a literal `DAY(59)`, a cell ref `DAY(A1)`, a formula-produced serial, or an
/// INLINE expression `DAY(700-645)` that no value-cell poison can reach. The function-name match is
/// bounded by non-identifier characters (so `MYDAY(`/`DAYS360(` do not match `DAY`) and case-folded.
fn date_consumer_serial_args(formula: &str) -> (Vec<String>, bool) {
    let hay = formula.as_bytes();
    // Treat non-ASCII bytes as identifier bytes (conservative — a non-ASCII sheet-name char adjacent
    // to a name is not a word boundary), plus the ASCII identifier set used in Excel function names.
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b >= 0x80;
    let mut out = Vec::new();
    let mut found = false;
    for fname in DATE_CONSUMER_FUNCTIONS {
        let nb = fname.as_bytes();
        let mut from = 0usize;
        while from + nb.len() <= hay.len() {
            // Case-insensitive occurrence of the function name.
            let Some(rel) = (from..=hay.len() - nb.len())
                .find(|&i| hay[i..i + nb.len()].eq_ignore_ascii_case(nb))
            else {
                break;
            };
            let start = rel;
            let end = start + nb.len();
            from = start + 1;
            // Word boundary: not glued to an identifier byte before, nor continued by one after (so a
            // longer name — `DAYS` when matching `DAY`, `NETWORKDAYS.INTL` when matching `NETWORKDAYS`
            // — is not mistaken for this one).
            if start > 0 && is_ident(hay[start - 1]) {
                continue;
            }
            if hay.get(end).is_some_and(|&b| is_ident(b)) {
                continue;
            }
            // The next non-whitespace byte must open the argument list.
            let mut p = end;
            while p < hay.len() && hay[p].is_ascii_whitespace() {
                p += 1;
            }
            if hay.get(p) != Some(&b'(') {
                continue;
            }
            found = true;
            let args = split_top_level_args(&formula[p + 1..]);
            for &i in date_consumer_serial_arg_indices(fname) {
                if let Some(a) = args.get(i) {
                    if !a.is_empty() {
                        out.push(a.clone());
                    }
                }
            }
        }
    }
    (out, found)
}

/// A per-run UNPREDICTABLE numeric probe (as a decimal string) for poison-and-diff. The value MUST
/// NOT be knowable to an adversary crafting the workbook: a fixed public constant could be encoded
/// into a source-dependent formula invariant under exactly that probe, laundering the engine's wrong
/// value into the oracle (round-56 defect 1). Seeded from the OS RNG via std's `RandomState` (each
/// `new()` reseeds), which is unpredictable at file-craft time regardless of the RNG's strength.
fn random_probe() -> String {
    use std::hash::{BuildHasher, Hasher};
    let n = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    // A large value with a fractional part, unlikely to collide with a real cell value.
    let int = 1_000_000_000u64 + (n % 8_000_000_000);
    let frac = (n >> 21) % 1_000_000;
    format!("{int}.{frac:06}")
}

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
/// Two PER-RUN RANDOM constants plus the normal (error-valued) evaluation are used. The randomness
/// is load-bearing for SOUNDNESS: with a fixed public probe an adversary could craft a
/// source-dependent formula invariant under exactly that value yet different for the source's true
/// value; because the workbook is crafted before certify runs, it cannot pre-encode the run-time
/// random probes, so such a formula is no longer invariant under them and is correctly excluded. A
/// cell that survives all probes is independent of every unsupported result (a genuine constant like
/// `=A1*0`), so the engine's value for it equals Excel's and vouching a matching cache is sound.
/// The (sheet-name, A1) of every cell whose `<f>` body carries a top-level range-intersection —
/// excluded from the cache oracle (the engine cannot evaluate the operator). Read from the raw XML
/// because the engine's reparse drops it.
fn intersection_cells(bytes: &[u8]) -> std::collections::HashSet<(String, String)> {
    let mut set = std::collections::HashSet::new();
    if let Ok(sheets) = crate::ooxml::all_sheets(bytes) {
        for (name, part) in sheets {
            if let Ok(xml) = crate::ooxml::read_part(bytes, &part) {
                // Expand shared formulas so a shared-group body carrying a top-level intersection is
                // seen on EVERY follower cell (matching the expanded encoding a foreign editor writes)
                // — the oracle then excludes the same cells regardless of shared vs expanded storage.
                let xml = structural::expand_shared_in_sheet(&xml).unwrap_or(xml);
                for cell in structural::cells_with_range_intersection(&xml) {
                    set.insert((name.clone(), cell));
                }
            }
        }
    }
    set
}

/// True if `name_lower` (a lower-cased defined-name identifier) occurs in `formula` as a WHOLE
/// token — bounded by non-identifier characters — matched case-insensitively. Over-approximates a
/// reference (a name that appears inside a quoted string literal also matches), which is the SOUND
/// direction here: it can only over-exclude a cell from the oracle (a preserved cache stays
/// unverified -> refused), never miss a genuine dependence.
fn formula_references_name(formula: &str, name_lower: &str) -> bool {
    if name_lower.is_empty() {
        return false;
    }
    // Excel names are case-insensitive and may contain letters/digits/`_`/`.`/`\`/non-ASCII.
    let is_ident =
        |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'\\' || b >= 0x80;
    let hay = formula.to_lowercase();
    let hb = hay.as_bytes();
    let nlen = name_lower.len();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(name_lower) {
        let s = from + rel;
        let e = s + nlen;
        let before_ok = s == 0 || !is_ident(hb[s - 1]);
        let after_ok = e >= hb.len() || !is_ident(hb[e]);
        if before_ok && after_ok {
            return true;
        }
        from = s + 1;
    }
    false
}

/// The transitive-closure set (lower-cased) of DEFINED-NAME identifiers whose refers-to body
/// reaches a function in `targets` — directly (a call to a target function) or indirectly (a
/// reference to another defined name already in the set). Used to find cells whose value launders a
/// bad/date function through a defined name, which cell-level poison-and-diff cannot isolate.
fn defined_names_reaching(
    model: &ironcalc::base::Model,
    targets: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut reaching = std::collections::HashSet::new();
    if targets.is_empty() {
        return reaching;
    }
    let defined: Vec<(String, String)> = model
        .workbook
        .defined_names
        .iter()
        .map(|d| (d.name.to_lowercase(), d.formula.clone()))
        .collect();
    // Seed: a body that directly calls a target function.
    for (n, f) in &defined {
        if crate::census::extract_function_names(f)
            .iter()
            .any(|x| targets.contains(x))
        {
            reaching.insert(n.clone());
        }
    }
    // Fixpoint: a body that references a name already known to reach a target.
    loop {
        let mut grew = false;
        for (n, f) in &defined {
            if reaching.contains(n) {
                continue;
            }
            if reaching.iter().any(|bn| formula_references_name(f, bn)) {
                reaching.insert(n.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    reaching
}

fn build_cache_oracle(
    model: &mut ironcalc::base::Model,
    date1904: bool,
    intersection_excluded: &std::collections::HashSet<(String, String)>,
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
    // DEFINED-NAME laundering: `bad` (from the census) includes a bad function that appears inside a
    // DEFINED NAME's refers-to body, not only in a cell formula. Poison-and-diff isolates a cell's
    // dependence on a bad *cell* (it poisons the source cell and diffs) and on a bad function reached
    // *through* a defined name that resolves to a bad cell (the alias re-resolves during evaluation) —
    // but it CANNOT isolate a bad FUNCTION living in a defined-name body, because a defined name is
    // not a cell it can poison. So a cell `=IFERROR(MyUDF_name, 999)` would survive with the engine's
    // WRONG value (999) and vouch a forged cache. Close the gap: compute the transitive-closure set of
    // defined names whose body (directly or via another such name) reaches a bad function, then mark
    // any cell that references one as a `source` so poison-and-diff drops it and its dependents.
    let bad_names = defined_names_reaching(model, &bad);
    let name_produces_date: std::collections::HashSet<String> = defined_names_reaching(
        model,
        &DATE_SERIAL_PRODUCERS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );
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
    // Cells whose formula PRODUCES a date serial — value-gated into `sources` below once the model
    // is evaluated (a pre-1900 serial the engine computes off-by-one from Excel is unvouchable).
    let mut date_producers: Vec<(u32, i32, i32)> = Vec::new();
    // Cells whose formula uses a 3D span — value-gated into `sources` below (excluded only if the
    // engine still cannot evaluate the span, i.e. its value is an error).
    let mut three_d_span_cells: Vec<(u32, i32, i32)> = Vec::new();
    // Cells whose formula CONSUMES a date serial (DAY/MONTH/YEAR/WEEKDAY/…). `prune_early_date_consumers`
    // (after the oracle is built) evaluates each consumer's actual INPUT serial and drops it — and its
    // dependents — iff that serial is in the engine's divergent pre-1900-03-01 range (< 61).
    let mut date_consumers: Vec<(u32, i32, i32)> = Vec::new();
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
        // A top-level range-INTERSECTION (`A1:A10 A3:A5`, Excel's space operator) is an OPERATOR the
        // engine cannot evaluate — it collapses to #ERROR! or a wrong scalar — so exclude the cell
        // (fail-closed): its cache is refused rather than vouched against the engine's wrong value.
        if intersection_excluded.contains(&(name.clone(), a1.clone())) {
            sources.push((cell.index, cell.row, cell.column));
        }
        formula_cells.push(((cell.index, cell.row, cell.column), (name.clone(), a1)));
        let fns = crate::census::extract_function_names(&f);
        if !bad.is_empty() && fns.iter().any(|n| bad.contains(n)) {
            sources.push((cell.index, cell.row, cell.column));
        }
        // ...or the cell references a defined name whose body reaches a bad function (laundering the
        // engine's wrong value through the name). Over-approximates via whole-word match, so at worst
        // it over-excludes (a preserved cache stays unverified -> refused) — never under-excludes.
        if !bad_names.is_empty() && bad_names.iter().any(|n| formula_references_name(&f, n)) {
            sources.push((cell.index, cell.row, cell.column));
        }
        // A 3D (multi-sheet) span `Sheet1:Sheet3!A5`: the vendored engine now EVALUATES these in the
        // common consolidation aggregates (SUM/AVERAGE/COUNT/COUNTA/MIN/MAX/PRODUCT/…), so a
        // correctly-computed span cache is vouchable. But a span used in a function the engine still
        // cannot evaluate returns an ERROR — value-gated below (after evaluate) so only the still-
        // unevaluable ones are excluded, closing the forged-#VALUE! false-certify without refusing
        // the correct value.
        if crate::refshift::formula_contains_3d_span(&f) {
            three_d_span_cells.push((cell.index, cell.row, cell.column));
        }
        if fns
            .iter()
            .any(|n| DATE_SERIAL_PRODUCERS.contains(&n.as_str()))
            || (!name_produces_date.is_empty()
                && name_produces_date
                    .iter()
                    .any(|n| formula_references_name(&f, n)))
        {
            // Value-gated below (only a produced serial < 61 is off-by-one from Excel), so this also
            // covers a date serial produced through a defined name, not just an inline call.
            date_producers.push((cell.index, cell.row, cell.column));
        }
        if fns
            .iter()
            .any(|n| DATE_CONSUMER_FUNCTIONS.contains(&n.as_str()))
        {
            date_consumers.push((cell.index, cell.row, cell.column));
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
    // Exclude off-by-one PRE-1900 date serials (see DATE_SERIAL_PRODUCERS). The engine omits
    // Excel's phantom 1900-02-29, so a produced serial < 61 differs from Excel's stored value under
    // BOTH date systems (the 1904 blanket exclusion above only guards the 1904 direction). Adding
    // such a cell to `sources` makes poison-and-diff drop it AND its dependents, so a preserved
    // foreign cache carrying the engine's off-by-one serial is refused, not vouched. Value-gated so
    // the ubiquitous MODERN date cache (serial >= 61, engine == Excel) stays vouchable.
    for &(s, r, c) in &date_producers {
        let serial = cell_value_sig(model, s, r, c)
            .and_then(|sig| sig.strip_prefix("n:").and_then(|x| x.parse::<f64>().ok()));
        if matches!(serial, Some(v) if v < 61.0) {
            sources.push((s, r, c));
        }
    }
    // Exclude a 3D-span cell ONLY if the engine still returns an ERROR for it (a span used in a
    // function the engine cannot yet aggregate across sheets). A correctly-evaluated span (a number
    // from SUM/AVERAGE/…) stays vouchable — the over-refusal fix. A no-value cell fails closed.
    for &(s, r, c) in &three_d_span_cells {
        if cell_value_sig(model, s, r, c).is_none_or(|sig| sig.starts_with("e:")) {
            sources.push((s, r, c));
        }
    }
    // Exclude every cell the engine evaluates to its NOT-IMPLEMENTED sentinel (#N/IMPL!) — notably a
    // `t="d"` ISO-date VALUE cell the importer cannot load, and any formula that reads one. Adding it
    // to `sources` makes poison-and-diff drop it AND its transitive dependents, so a formula reading
    // a `t="d"` cell — even one masking the error to a clean number via `IFERROR(A1+1,0)` — is not
    // vouched against a value the engine could not actually compute. (cell_value_sig already emits no
    // signature for a cell that IS #N/IMPL!; poisoning closes the error-masked amplification.)
    for cell in model.get_all_cells() {
        if cell_is_nimpl(model, cell.index, cell.row, cell.column) {
            sources.push((cell.index, cell.row, cell.column));
        }
    }
    sources.sort();
    sources.dedup();
    // Fast path: no unsupported/policy/UDF function -> every formula cell is trustworthy. (Still
    // subject to the early-date-consumer prune below.)
    let oracle = if sources.is_empty() {
        snap(model)
    } else {
        // Poison-and-diff taint isolation, with PER-RUN RANDOM probe values (round-56 defect 1). A
        // FIXED, publicly-known probe let an adversary craft a source-dependent formula invariant
        // under exactly those constants (`IF(OR(A1=1234567,A1=-98765.4321),…)`) yet different for the
        // source's true value — laundering the engine's error-masked value into the oracle. The
        // workbook is crafted BEFORE certify runs, so unpredictable run-time probes cannot be
        // pre-encoded: the crafted formula is no longer invariant under the (now random) poisons and
        // is correctly tainted.
        let (p1, p2) = (random_probe(), random_probe());
        let v_err = snap(model); // normal eval: source cells are their (error-valued) formulas
        for &(s, r, c) in &sources {
            let _ = model.set_user_input(s, r, c, p1.clone());
        }
        model.evaluate();
        let v_k1 = snap(model);
        for &(s, r, c) in &sources {
            let _ = model.set_user_input(s, r, c, p2.clone());
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
        out
    };
    // Prune date CONSUMERS whose INPUT serial is in the engine's divergent pre-1900-03-01 range: it
    // evaluates each consumer's actual serial ARGUMENT (covering a literal, a cell ref, a formula-
    // produced serial, and an INLINE `DAY(700-645)`), then drops each divergent consumer AND its
    // transitive dependents. Runs LAST (the model is discarded after), so mutating it is harmless.
    Some(prune_early_date_consumers(
        model,
        &date_consumers,
        &formula_cells,
        &names,
        oracle,
    ))
}

/// A formula cell's engine coordinate `(sheet, row, col)` paired with its `(sheet-name, A1)` oracle key.
type FormulaCellRef = ((u32, i32, i32), (String, String));

/// See the call site in `build_cache_oracle`. A date CONSUMER (DAY/MONTH/YEAR/WEEKDAY/…) whose INPUT
/// serial is in the engine's divergent pre-1900-03-01 range (< 61) computes a value the engine
/// renders off-by-one from Excel (it omits Excel's phantom 1900-02-29). Vouching that value would
/// either false-certify a forged cache carrying the engine's wrong value or — for a cell whose
/// formula the transform shifts/blanks (so `by_stored` cannot vouch it) — refuse the correct one, so
/// this drops every divergent consumer AND its transitive dependents from the oracle (fail-closed):
/// those caches are then unverified and refused.
///
/// Detection EVALUATES each consumer's actual serial ARGUMENT in place — subsuming all four ways an
/// early serial reaches a consumer: a literal `DAY(59)`, a cell ref `DAY(A1)`, a formula-produced
/// serial, and an INLINE `DAY(700-645)` that no value-cell poison can reach. A poison-diff then drops
/// dependents, so `D = C+1` reading a divergent `C` is refused too (closing the dependent gap).
fn prune_early_date_consumers(
    model: &mut ironcalc::base::Model,
    date_consumers: &[(u32, i32, i32)],
    formula_cells: &[FormulaCellRef],
    names: &[String],
    mut oracle: std::collections::HashMap<(String, String), String>,
) -> std::collections::HashMap<(String, String), String> {
    if date_consumers.is_empty() {
        return oracle;
    }
    let numeric = |model: &ironcalc::base::Model, s: u32, r: i32, c: i32| -> Option<f64> {
        cell_value_sig(model, s, r, c)
            .and_then(|sig| sig.strip_prefix("n:").and_then(|x| x.parse::<f64>().ok()))
    };
    // Phase A — identify the divergent consumers by evaluating each serial ARGUMENT expression in the
    // consumer's OWN cell (so relative references resolve at the correct position), then restoring the
    // original formula. Only consumers still IN the oracle are probed: one already excluded (it reads a
    // `source`) needs no check, and its args may read a poisoned source.
    let mut divergent: Vec<(u32, i32, i32)> = Vec::new();
    for &(s, r, c) in date_consumers {
        let key = match (names.get(s as usize), diff::a1(r, c)) {
            (Some(n), Ok(a1)) => (n.clone(), a1),
            _ => continue,
        };
        if !oracle.contains_key(&key) {
            continue;
        }
        let Ok(Some(orig)) = model.get_cell_formula(s, r, c) else {
            continue;
        };
        let (args, found) = date_consumer_serial_args(&orig);
        // A recognized consumer whose serial argument(s) cannot be extracted is dropped (fail-closed):
        // we cannot prove it reads only modern (>= 61) serials.
        let mut is_divergent = !found || args.is_empty();
        for arg in &args {
            let _ = model.set_user_input(s, r, c, format!("=({arg})"));
            model.evaluate();
            match numeric(model, s, r, c) {
                Some(v) if v < 61.0 => {
                    is_divergent = true;
                    break;
                }
                Some(_) => {} // a modern serial -> this argument is fine
                None => {
                    is_divergent = true; // non-numeric (error / range) -> cannot prove modern -> drop
                    break;
                }
            }
        }
        // Restore the original formula so the dependent poison-diff below — and any later consumer
        // that references THIS cell (an adversary-crafted `DAY(other_consumer)`) — sees its true value.
        if found && !args.is_empty() {
            let _ = model.set_user_input(s, r, c, orig);
        }
        if is_divergent {
            divergent.push((s, r, c));
        }
    }
    if divergent.is_empty() {
        return oracle;
    }
    // Settle the restored formulas before snapshotting the dependent-diff baseline.
    model.evaluate();
    // Phase B — drop each divergent consumer AND its transitive dependents. Poison every divergent
    // consumer to a modern serial constant and re-evaluate: any oracle cell whose value CHANGES reads
    // a divergent consumer (directly or through a chain) and so carries the engine's off-by-one value
    // too; it is refused rather than vouched.
    let snap = |model: &ironcalc::base::Model| -> std::collections::HashMap<(String, String), Option<String>> {
        formula_cells
            .iter()
            .map(|(coord, k)| (k.clone(), cell_value_sig(model, coord.0, coord.1, coord.2)))
            .collect()
    };
    let baseline = snap(model);
    for &(s, r, c) in &divergent {
        let _ = model.set_user_input(s, r, c, "44000".to_string());
    }
    model.evaluate();
    let after = snap(model);
    let divergent_keys: std::collections::HashSet<(String, String)> = divergent
        .iter()
        .filter_map(|&(s, r, c)| Some((names.get(s as usize)?.clone(), diff::a1(r, c).ok()?)))
        .collect();
    oracle.retain(|key, _| !divergent_keys.contains(key) && baseline.get(key) == after.get(key));
    oracle
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
    // NOTE (round-52): a zero-snap tolerance (treat a tiny residual as equal to a 0 cache — for the
    // catastrophic-cancellation `0.5-0.4-0.1` -> 0 that ironcalc leaves at ~1e-17) was REMOVED as
    // UNSOUND. A catastrophic-cancellation residual scales with the OPERAND magnitude (~operand *
    // 2^-52), so NO absolute floor distinguishes a large-operand cancellation residual (~1e-10) from
    // a GENUINE small computed value (1/8e9 = 1.25e-10): any floor wide enough to vouch the former
    // FALSE-CERTIFIES a forged 0 hiding the latter. Unlike the RELATIVE 14-sig-fig compare below
    // (whose residual is always at each value's own precision floor), an absolute near-zero snap can
    // corrupt a full-precision small value. So a cancellation cache of exactly 0 stays a fail-safe
    // over-refusal (a sound fix would need operand-scale-aware precision, unavailable here).
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
    let raw = &rest[..end];
    // RESOLVE XML entity / character references, so an entity-encoded value is compared as its
    // real content — `date1904="&#49;"` is spec-equivalent to `date1904="1"` and Excel honors it,
    // but a raw read saw the literal `&#49;`, bucketed it as the 1900 DEFAULT, and CERTIFIED a
    // silent epoch flip (round-56 defect 11). A malformed entity falls back to the raw text.
    Some(
        quick_xml::escape::unescape(raw)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| raw.to_string()),
    )
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

/// Every ISO-8601 date VALUE cell (`t="d"`) across all worksheets as (sheet, A1-ref, value).
/// ironcalc discards these on import, so the engine snapshot is blind to their value — certify
/// compares them here at the byte level (see the `date_value_mismatch` guard). Path-robust: sheets
/// are enumerated through the workbook relationships, keyed by sheet NAME so a cosmetic part-path
/// difference does not spuriously diverge. Fail-closed sentinels on an unreadable workbook/sheet.
fn date_value_cells(bytes: &[u8]) -> Vec<(String, String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return vec![(
            "__unreadable_workbook__".into(),
            String::new(),
            String::new(),
        )];
    };
    let mut out = Vec::new();
    for (sheet_name, part_path) in sheets {
        let Ok(xml) = crate::ooxml::read_part(bytes, &part_path) else {
            out.push((sheet_name, "__unreadable_sheet__".into(), String::new()));
            continue;
        };
        for (cell_ref, value) in structural::date_typed_value_cells(&xml) {
            out.push((sheet_name.clone(), cell_ref, value));
        }
    }
    out.sort();
    out
}

/// True if any worksheet has a `<c>` cell with two or more `<v>` children (see
/// `structural::cell_has_repeated_value`). An unreadable workbook fails closed (true).
fn has_repeated_value_cell(bytes: &[u8]) -> bool {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return true;
    };
    sheets.into_iter().any(|(_name, part)| {
        crate::ooxml::read_part(bytes, &part)
            .map(|xml| structural::cell_has_repeated_value(&xml))
            .unwrap_or(true)
    })
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
            // NOTE (round-46): a literal-value float-noise tolerance was tried here and REVERTED —
            // it was unsound. A literal (`A1`) feeds formulas, and a cache-stripped
            // catastrophic-cancellation dependent (`=(A1-1e12)*1e6`) amplifies even a 1-ULP input
            // residual into the leading result figure with NO counted value-diff (the formula's
            // cache is blank -> recompute-on-load benign). So a value diff on a literal is always
            // disqualifying; the (niche) over-refusal of a frozen `0.30000000000000004` re-rounded
            // to `0.3` is the fail-safe cost.
            match kind {
                "formula" => counts.formula += 1,
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
                && !removed_empty;
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
        // REGRESSION (round-56 defect 11): an ENTITY-encoded value is spec-equivalent and Excel
        // honors it, so it must resolve to true — a raw read saw `&#49;`, bucketed it as the 1900
        // default, and CERTIFIED a silent epoch flip.
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><workbookPr date1904="&#49;"/></workbook>"#
        )));
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><workbookPr date1904="&#116;rue"/></workbook>"#
        )));
        // REGRESSION (round-57 defect 7): a commented-out DECOY workbookPr must NOT hide the real
        // one, a CDATA span is skipped, and a '>' inside a quoted attribute must not truncate the tag.
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><!--<workbookPr date1904="0"/>--><workbookPr codeName="ThisWorkbook" date1904="1"/></workbook>"#
        )));
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><![CDATA[<workbookPr date1904="0"/>]]><workbookPr date1904="1"/></workbook>"#
        )));
        assert!(workbook_is_date1904(&wb_only(
            r#"<workbook><workbookPr codeName="a>b" date1904="1"/></workbook>"#
        )));
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
    fn pivot_show_data_as_flip_is_compared() {
        // REGRESSION (round-52 defect 2): `showDataAs` (the "Show Values As" transform) rewrites
        // every data cell on refresh — a SUM -> "% of column" flip is a silent value corruption the
        // dataField signature must catch. Absent keys the same as explicit "normal" (no over-refusal).
        let pt = |sda: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><dataFields><dataField name="X" fld="1"{sda}/></dataFields></pivotTableDefinition>"#
            )
        };
        let normal = wb("", &[("xl/pivotTables/pivotTable1.xml", &pt(""))]);
        let pct = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(r#" showDataAs="percentOfCol""#),
            )],
        );
        assert_ne!(
            pivot_refs(&normal),
            pivot_refs(&pct),
            "SUM vs % of column must differ"
        );
        let normal_explicit = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(r#" showDataAs="normal""#),
            )],
        );
        assert_eq!(pivot_refs(&normal), pivot_refs(&normal_explicit));
    }

    #[test]
    fn protected_range_sqref_set_is_canonicalized() {
        // REGRESSION (round-58 defect 3): a protectedRange sqref is an UNORDERED cell SET; a faithful
        // re-serialization that reorders or re-decomposes it (openpyxl sorts areas) must not refuse,
        // exactly as the CF/DV sqref is canonicalized. A genuine cell-set change still differs.
        let pr = |sqref: &str| {
            format!(
                r#"<protectedRanges><protectedRange sqref="{sqref}" name="R1"/></protectedRanges>"#
            )
        };
        let good = wb(&pr("A1:A5 B1:B5"), &[]);
        assert!(verify_noncell_refs(&good, &good).is_none());
        assert!(
            verify_noncell_refs(&good, &wb(&pr("B1:B5 A1:A5"), &[])).is_none(),
            "a sqref area reorder (same set) must not refuse"
        );
        let base2 = wb(&pr("A1:B2"), &[]);
        assert!(
            verify_noncell_refs(&base2, &wb(&pr("A1:A2 B1:B2"), &[])).is_none(),
            "a sqref decomposition (same set) must not refuse"
        );
        // A GENUINE protected-range change (different cell set) still refuses.
        assert_eq!(
            verify_noncell_refs(&good, &wb(&pr("A1:A5 C1:C5"), &[]))
                .expect("a genuine protected-range change must refuse")["reason"],
            "protection_mismatch"
        );
        // A boolean-spelling difference on a protection attribute is not refused.
        let sheet_prot = |v: &str| format!(r#"<sheetProtection sheet="{v}" password="ABCD"/>"#);
        assert!(
            verify_noncell_refs(&wb(&sheet_prot("1"), &[]), &wb(&sheet_prot("true"), &[]))
                .is_none(),
            "sheet=\"1\" == sheet=\"true\" must not refuse"
        );
    }

    #[test]
    fn pivot_root_and_field_subtotal_toggles_are_compared() {
        // REGRESSION (round-58 defects 1 & 2): the pivotTable ROOT grand-total / subtotal-scope
        // toggles and the pivotField subtotal-function flags add/remove/retype materialized total
        // cells on refresh — but the root was never scanned and the field signature read only
        // axis+dataField. A toggle must differ; an explicit-default reserialization must not refuse.
        let root = |attrs: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"{attrs}><pivotFields/></pivotTableDefinition>"#
            )
        };
        let base = wb("", &[("xl/pivotTables/pivotTable1.xml", &root(""))]);
        // rowGrandTotals/colGrandTotals default TRUE -> "0" removes them and must differ.
        let no_gt = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &root(r#" rowGrandTotals="0" colGrandTotals="0""#),
            )],
        );
        assert_ne!(
            pivot_refs(&base),
            pivot_refs(&no_gt),
            "removing grand totals must differ"
        );
        // subtotalHiddenItems default FALSE -> "1" must differ.
        let sub_hidden = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &root(r#" subtotalHiddenItems="1""#),
            )],
        );
        assert_ne!(
            pivot_refs(&base),
            pivot_refs(&sub_hidden),
            "subtotalHiddenItems must differ"
        );
        // An explicit default (grand totals ON) matches the absent form — no over-refusal.
        let explicit_default = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &root(r#" rowGrandTotals="1" colGrandTotals="true""#),
            )],
        );
        assert_eq!(pivot_refs(&base), pivot_refs(&explicit_default));

        // pivotField: defaultSubtotal default TRUE ("0" drops the Sum subtotal); avgSubtotal adds one.
        let field = |attrs: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><pivotFields><pivotField axis="axisRow"{attrs}/></pivotFields></pivotTableDefinition>"#
            )
        };
        let fbase = wb("", &[("xl/pivotTables/pivotTable1.xml", &field(""))]);
        let no_defsub = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &field(r#" defaultSubtotal="0""#),
            )],
        );
        assert_ne!(
            pivot_refs(&fbase),
            pivot_refs(&no_defsub),
            "defaultSubtotal off must differ"
        );
        let avg_sub = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &field(r#" avgSubtotal="1""#),
            )],
        );
        assert_ne!(
            pivot_refs(&fbase),
            pivot_refs(&avg_sub),
            "adding an avg subtotal must differ"
        );
    }

    #[test]
    fn pivot_axis_membership_and_measure_order_are_compared() {
        // REGRESSION (round-59 defect 1, HIGH false-certify): a coherent RE-PIVOT (group by a
        // different field; transpose value-area measures) leaves the pooled+sorted pivotField
        // multiset unchanged, so it certified. The ordered axis-membership lists + position-keyed
        // pivotField axis now catch it.
        // (i) rowFields <field x> membership change (row axis now field 1 not field 0) — a full re-pivot.
        let repivot = |rowx: &str, pf: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><location ref="A1"/><rowFields count="1"><field x="{rowx}"/></rowFields><pivotFields>{pf}</pivotFields></pivotTableDefinition>"#
            )
        };
        let region = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &repivot("0", r#"<pivotField axis="axisRow"/><pivotField/>"#),
            )],
        );
        let product = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &repivot("1", r#"<pivotField/><pivotField axis="axisRow"/>"#),
            )],
        );
        assert_ne!(
            pivot_refs(&region),
            pivot_refs(&product),
            "a coherent axis reassignment (group by field 1 not 0) must differ"
        );
        // (ii) dataField (value-area measure) REORDER — output columns transpose on refresh.
        let df = |a: &str, b: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><dataFields count="2"><dataField name="{a}" fld="{a}"/><dataField name="{b}" fld="{b}"/></dataFields></pivotTableDefinition>"#
            )
        };
        let ab = wb("", &[("xl/pivotTables/pivotTable1.xml", &df("0", "1"))]);
        let ba = wb("", &[("xl/pivotTables/pivotTable1.xml", &df("1", "0"))]);
        assert_ne!(
            pivot_refs(&ab),
            pivot_refs(&ba),
            "a dataField (measure) reorder must differ"
        );
        // A benign identical re-serialization still matches (no over-refusal).
        assert_eq!(pivot_refs(&region), pivot_refs(&region));
    }

    #[test]
    fn hidden_tokens_invariant_to_shared_formula_expansion() {
        // REGRESSION (round-55 defect 3, over-refusal): a shared-formula group stores the body — and
        // its hidden token (`_xlfn.`) — only on the MASTER cell; a faithful foreign editor
        // (openpyxl/LibreOffice) un-shares the group, putting the token on EVERY cell. Expanding both
        // sides before scanning makes the per-cell token map invariant to shared<->expanded storage.
        let toks = |xml: &[u8]| {
            let x = structural::expand_shared_in_sheet(xml).unwrap_or_else(|_| xml.to_vec());
            structural::formula_hidden_tokens(&x)
        };
        let shared = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="B1"><f t="shared" ref="B1:B3" si="0">_xlfn.CONCAT(A1,"x")</f></c></row><row r="2"><c r="B2"><f t="shared" si="0"/></c></row><row r="3"><c r="B3"><f t="shared" si="0"/></c></row></sheetData></worksheet>"#;
        let expanded = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="B1"><f>_xlfn.CONCAT(A1,"x")</f></c></row><row r="2"><c r="B2"><f>_xlfn.CONCAT(A2,"x")</f></c></row><row r="3"><c r="B3"><f>_xlfn.CONCAT(A3,"x")</f></c></row></sheetData></worksheet>"#;
        assert_eq!(
            toks(shared),
            toks(expanded),
            "shared and expanded forms of the same hidden-token formula must yield equal token maps"
        );
        assert!(
            !toks(shared).is_empty(),
            "the hidden token IS captured (not a vacuous match)"
        );
        // A genuine token DROP (dropping the `_xlfn.` prefix) still differs.
        let dropped = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="B1"><f>CONCAT(A1,"x")</f></c></row><row r="2"><c r="B2"><f>CONCAT(A2,"x")</f></c></row><row r="3"><c r="B3"><f>CONCAT(A3,"x")</f></c></row></sheetData></worksheet>"#;
        assert_ne!(
            toks(shared),
            toks(dropped),
            "a genuine hidden-token drop still differs"
        );
    }

    #[test]
    fn pivot_source_sheet_with_space_is_compared_whole() {
        // REGRESSION (round-55 defect 7, HIGH false-certify): the pivot worksheetSource `sheet`
        // attribute holds a RAW sheet name that routinely contains a space; the old `pick` re-split
        // element_attr_semantics on whitespace, truncating `Data 2024`/`Data 2099` to `Data` so a
        // source REPOINT between two same-prefix sheets collided to an identical signature. The
        // ATTR_SEP-joined attrs now keep the value whole.
        let cache = |sheet: &str| {
            format!(
                r#"<pivotCacheDefinition xmlns="urn:x"><cacheSource type="worksheet"><worksheetSource ref="$A$1:$D$100" sheet="{sheet}"/></cacheSource></pivotCacheDefinition>"#
            )
        };
        let a = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Data 2024"),
            )],
        );
        let b = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Data 2099"),
            )],
        );
        assert_ne!(
            pivot_refs(&a),
            pivot_refs(&b),
            "a source repoint between two space-bearing same-prefix sheets must differ"
        );
        // A benign re-serialization of the SAME space-bearing name still matches (no over-refusal).
        assert_eq!(pivot_refs(&a), pivot_refs(&a));
    }

    #[test]
    fn pivot_value_filter_threshold_is_compared() {
        // REGRESSION (round-55 defect 2, HIGH false-certify): a pivot VALUE/LABEL filter threshold
        // (the <filter stringValue1> and its nested <customFilter operator val>) decides which rows
        // the pivot materializes on refresh; loosening it (1000 -> 0) corrupts the aggregate. It was
        // in no comparator. Now the <filter> signature carries sv1/sv2 AND autofilter_criteria scans
        // pivotTables for the nested predicate.
        let pv = |threshold: &str| {
            format!(
                r#"<pivotTableDefinition xmlns="urn:x"><filters><filter fld="0" type="valueGreaterThan" id="1" iMeasureFld="0" stringValue1="{threshold}"><autoFilter ref="A1:A1"><filterColumn colId="0"><customFilters><customFilter operator="greaterThan" val="{threshold}"/></customFilters></filterColumn></autoFilter></filter></filters></pivotTableDefinition>"#
            )
        };
        let (good_s, loose_s) = (pv("1000"), pv("0"));
        let good = wb("", &[("xl/pivotTables/pivotTable1.xml", good_s.as_str())]);
        assert!(
            verify_noncell_refs(&good, &good).is_none(),
            "identical pivot filter certifies"
        );
        let loosened = wb("", &[("xl/pivotTables/pivotTable1.xml", loose_s.as_str())]);
        assert!(
            verify_noncell_refs(&good, &loosened).is_some(),
            "a loosened pivot value-filter threshold must be refused"
        );
        // The threshold is captured both in pivot_refs (sv1) and autofilter_criteria (customFilter).
        assert_ne!(pivot_refs(&good), pivot_refs(&loosened));
        assert_ne!(autofilter_criteria(&good), autofilter_criteria(&loosened));
    }

    #[test]
    fn pivot_calculated_field_formula_is_compared() {
        // REGRESSION (round-53 defect 2): a pivot CALCULATED FIELD's formula (<cacheField
        // formula=…>) re-aggregates every data cell on refresh, so tampering it corrupts the pivot
        // output — pivot_refs must compare it. And it must read the FULL formula (a formula with
        // spaces was truncated by the space-joined attr `pick`, so `Revenue - Cost` and
        // `Revenue - Evil` collided).
        let cache = |formula: &str| {
            format!(
                r#"<pivotCacheDefinition xmlns="urn:x"><cacheFields><cacheField name="Revenue"/><cacheField name="Cost"/><cacheField name="Margin" databaseField="0" formula="{formula}"/></cacheFields></pivotCacheDefinition>"#
            )
        };
        let good = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Revenue-Cost"),
            )],
        );
        let evil = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Revenue*100"),
            )],
        );
        assert_ne!(
            pivot_refs(&good),
            pivot_refs(&evil),
            "a calculated-field formula change must be caught"
        );
        // A formula containing SPACES that differs only after the first token must still differ
        // (the full value is read, not the split-whitespace first token).
        let spaced_good = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Revenue - Cost"),
            )],
        );
        let spaced_evil = wb(
            "",
            &[(
                "xl/pivotCache/pivotCacheDefinition1.xml",
                &cache("Revenue - Evil"),
            )],
        );
        assert_ne!(
            pivot_refs(&spaced_good),
            pivot_refs(&spaced_evil),
            "a space-containing calculated-field formula must not be truncated to its first token"
        );
        // A plain source cacheField (no formula) does not spuriously refuse a re-serialization.
        assert_eq!(pivot_refs(&good), pivot_refs(&good));
    }

    #[test]
    fn pivot_filter_surface_is_compared() {
        // REGRESSION (round-47): a manual item filter (`<item h="1">`), a page filter, and a field's
        // axis placement re-aggregate the pivot on refresh — pivot_refs must compare them, not just
        // dataField/refreshOnLoad.
        let pt = |body: &str| {
            format!(r#"<pivotTableDefinition xmlns="urn:x">{body}</pivotTableDefinition>"#)
        };
        let shown = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(
                    r#"<pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1"/></items></pivotField></pivotFields>"#,
                ),
            )],
        );
        let hidden = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(
                    r#"<pivotFields><pivotField axis="axisRow"><items><item x="0"/><item x="1" h="1"/></items></pivotField></pivotFields>"#,
                ),
            )],
        );
        assert_ne!(
            pivot_refs(&shown),
            pivot_refs(&hidden),
            "a hidden-item filter must differ"
        );
        // Absent `h` == h="0" (no spurious refusal on a benign default).
        let shown0 = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(
                    r#"<pivotFields><pivotField axis="axisRow"><items><item x="0" h="0"/><item x="1"/></items></pivotField></pivotFields>"#,
                ),
            )],
        );
        assert_eq!(pivot_refs(&shown), pivot_refs(&shown0));
        // A page (report) filter selection change is caught.
        let p1 = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(r#"<pageFields><pageField fld="0" item="1"/></pageFields>"#),
            )],
        );
        let p2 = wb(
            "",
            &[(
                "xl/pivotTables/pivotTable1.xml",
                &pt(r#"<pageFields><pageField fld="0" item="2"/></pageFields>"#),
            )],
        );
        assert_ne!(
            pivot_refs(&p1),
            pivot_refs(&p2),
            "a page-filter change must differ"
        );
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
    fn drawing_value_refs_compared_but_anchor_position_ignored() {
        // A drawing's VALUE-bearing reference (a graphic-frame `<f>` source cell) is compared: a
        // re-point is caught. But the cell ANCHOR position is DISPLAY-only and NOT compared
        // (round-47): a desktop editor's oneCell<->twoCell re-encode can move the from-anchor to
        // the previous cell with a compensating EMU offset for the identical on-screen position, so
        // comparing col/row spuriously refused a positionally-faithful re-save.
        let draw = |body: &str| {
            (
                "xl/drawings/drawing1.xml",
                format!(r#"<xdr:wsDr xmlns:xdr="urn:xdr">{body}</xdr:wsDr>"#),
            )
        };
        // Same value ref, DIFFERENT anchor decomposition (row 2/off 0 vs row 1/off 190500 = same
        // screen position) -> NOT refused (anchor position ignored).
        let (n, a) = draw(
            r#"<xdr:oneCellAnchor><xdr:from><xdr:col>7</xdr:col><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:graphicFrame><xdr:f>Data!$D$3</xdr:f></xdr:graphicFrame></xdr:oneCellAnchor>"#,
        );
        let (n2, b) = draw(
            r#"<xdr:twoCellAnchor><xdr:from><xdr:col>7</xdr:col><xdr:row>1</xdr:row><xdr:rowOff>190500</xdr:rowOff></xdr:from><xdr:to><xdr:col>12</xdr:col><xdr:row>10</xdr:row></xdr:to><xdr:graphicFrame><xdr:f>Data!$D$3</xdr:f></xdr:graphicFrame></xdr:twoCellAnchor>"#,
        );
        let base = wb("", &[(n, a.as_str())]);
        assert!(
            verify_noncell_refs(&base, &wb("", &[(n2, b.as_str())])).is_none(),
            "a value-neutral anchor re-encode must NOT refuse"
        );
        // A re-pointed graphic-frame source cell IS caught.
        let (n3, evil) = draw(
            r#"<xdr:oneCellAnchor><xdr:from><xdr:col>7</xdr:col><xdr:row>2</xdr:row></xdr:from><xdr:graphicFrame><xdr:f>Data!$Z$99</xdr:f></xdr:graphicFrame></xdr:oneCellAnchor>"#,
        );
        assert_eq!(
            verify_noncell_refs(&base, &wb("", &[(n3, evil.as_str())]))
                .expect("a re-pointed drawing ref must be caught")["reason"],
            "chart_drawing_mismatch"
        );
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
        // REGRESSION (round-57 defect 3): a faithful cross-tool re-serialization of DEFAULT-valued
        // criterion attributes must NOT refuse. openpyxl writes `<top10 val="2"/>`; LibreOffice
        // writes `<top10 top="true" percent="false" val="2"/>` (explicit defaults + true/false
        // literals) — the SAME top-2 filter.
        let openpyxl_top10 = wb(
            r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><top10 val="2"/></filterColumn></autoFilter>"#,
            &[],
        );
        let libre_top10 = wb(
            r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><top10 top="true" percent="false" val="2"/></filterColumn></autoFilter>"#,
            &[],
        );
        assert!(
            verify_noncell_refs(&openpyxl_top10, &libre_top10).is_none(),
            "a default-attribute re-serialization of the same top10 filter must not refuse"
        );
        // But a GENUINE change (top -> bottom, i.e. top="false") still differs.
        let bottom10 = wb(
            r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><top10 top="false" val="2"/></filterColumn></autoFilter>"#,
            &[],
        );
        assert_eq!(
            verify_noncell_refs(&openpyxl_top10, &bottom10).expect("top->bottom must refuse")
                ["reason"],
            "autofilter_criteria_mismatch"
        );
        // REGRESSION (round-58 defect 4): the CT_CustomFilter default operator="equal" written
        // explicitly must fold to the omitted form (direct follow-on to the round-57 fix).
        let explicit_op = wb(
            r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><customFilters><customFilter operator="equal" val="9"/></customFilters></filterColumn></autoFilter>"#,
            &[],
        );
        let omitted_op = wb(
            r#"<autoFilter ref="A1:A10"><filterColumn colId="0"><customFilters><customFilter val="9"/></customFilters></filterColumn></autoFilter>"#,
            &[],
        );
        assert!(
            verify_noncell_refs(&explicit_op, &omitted_op).is_none(),
            "explicit default operator=\"equal\" must fold to the omitted form"
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
    fn rich_data_value_rewrite_is_caught() {
        // REGRESSION (round-46): a rich value (linked data type / =IMAGE) holds the cell's OFFLINE
        // value in xl/richData; the cell carries only a `vm` pointer, so a REWRITE of a field
        // (420.5 -> 999999, MSFT -> EVIL) is invisible to the cell diff — certify must compare it.
        let rv = |name: &str, price: &str| {
            format!(r#"<rvData xmlns="urn:x"><rv s="0"><v>{name}</v><v>{price}</v></rv></rvData>"#)
        };
        let good = wb("", &[("xl/richData/rdrichvalue.xml", &rv("MSFT", "420.5"))]);
        assert!(
            verify_noncell_refs(&good, &good).is_none(),
            "identical richData certifies"
        );
        let evil = wb(
            "",
            &[("xl/richData/rdrichvalue.xml", &rv("EVIL", "999999"))],
        );
        assert_eq!(
            verify_noncell_refs(&good, &evil).expect("a rewritten rich value must refuse")
                ["reason"],
            "rich_data_mismatch"
        );
        // REGRESSION (round-48): a value-preserving PERMUTATION of two records (which transposes
        // which cell shows which value) must ALSO differ — the compare is order-sensitive now.
        let two = |a: &str, b: &str| {
            format!(
                r#"<rvData xmlns="urn:x"><rv s="0"><v>{a}</v></rv><rv s="0"><v>{b}</v></rv></rvData>"#
            )
        };
        let ab = wb(
            "",
            &[("xl/richData/rdrichvalue.xml", &two("Alpha", "Beta"))],
        );
        let ba = wb(
            "",
            &[("xl/richData/rdrichvalue.xml", &two("Beta", "Alpha"))],
        );
        assert_ne!(
            rich_data_values(&ab),
            rich_data_values(&ba),
            "a record permutation must differ"
        );
        // REGRESSION (round-54 defect 9): a tampered field that injects an XML numeric char-ref
        // (`420.5` -> `&#57;420.5` = "9420.5") whose literal text runs stay byte-identical must
        // differ — the entity must be reassembled, not dropped.
        let clean = wb("", &[("xl/richData/rdrichvalue.xml", &rv("MSFT", "420.5"))]);
        let entity = wb(
            "",
            &[(
                "xl/richData/rdrichvalue.xml",
                r#"<rvData xmlns="urn:x"><rv s="0"><v>MSFT</v><v>&#57;420.5</v></rv></rvData>"#,
            )],
        );
        assert_ne!(
            rich_data_values(&clean),
            rich_data_values(&entity),
            "an injected numeric char-reference must differ (not be silently dropped)"
        );
        assert_eq!(
            verify_noncell_refs(&clean, &entity)
                .expect("an entity-injected rich value must refuse")["reason"],
            "rich_data_mismatch"
        );
    }

    #[test]
    fn metadata_index_reindex_is_caught() {
        // REGRESSION (round-48): the MIDDLE link of the rich-value chain — the `rvb i` mapping in
        // metadata.xml — must be compared. Remapping i="0"->i="1" repoints a cell to a different
        // record with both the cell `vm` and the richData store byte-identical.
        let md = |i: &str| {
            wb(
                "",
                &[(
                    "xl/metadata.xml",
                    &format!(
                        r#"<metadata xmlns="urn:x" xmlns:xlrd="urn:xlrd"><valueMetadata><bk><rc t="1" v="0"/></bk></valueMetadata><futureMetadata><bk><ext><xlrd:rvb i="{i}"/></ext></bk></futureMetadata></metadata>"#
                    ),
                )],
            )
        };
        let a = md("0");
        assert_eq!(metadata_index_chain(&a), metadata_index_chain(&md("0")));
        assert_ne!(metadata_index_chain(&a), metadata_index_chain(&md("1")));
        assert_eq!(
            verify_noncell_refs(&a, &md("1")).expect("an rvb reindex must refuse")["reason"],
            "metadata_index_mismatch"
        );
    }

    #[test]
    fn cell_metadata_binding_repoint_is_caught() {
        // REGRESSION (round-47): swapping a cell's `vm` repoints it to a DIFFERENT rich value with
        // the richData store and cell text both byte-identical — the binding itself must be compared.
        let ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let pkg = "http://schemas.openxmlformats.org/package/2006/relationships";
        let mk = |c1: &str, c2: &str| -> Vec<u8> {
            let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let o = zip::write::SimpleFileOptions::default();
            let mut put = |n: &str, b: String| {
                z.start_file(n, o).unwrap();
                z.write_all(b.as_bytes()).unwrap();
            };
            put("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_string());
            put(
                "_rels/.rels",
                format!(
                    r#"<Relationships xmlns="{pkg}"><Relationship Id="rId1" Type="{r}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
                ),
            );
            put(
                "xl/workbook.xml",
                format!(
                    r#"<workbook xmlns="{ns}" xmlns:r="{r}"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#
                ),
            );
            put(
                "xl/_rels/workbook.xml.rels",
                format!(
                    r#"<Relationships xmlns="{pkg}"><Relationship Id="rId1" Type="{r}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
                ),
            );
            put(
                "xl/worksheets/sheet1.xml",
                format!(
                    r#"<worksheet xmlns="{ns}"><sheetData><row r="1"><c r="C1" vm="{c1}" t="e"><v>#VALUE!</v></c><c r="C2" vm="{c2}" t="e"><v>#VALUE!</v></c></row></sheetData></worksheet>"#
                ),
            );
            z.finish().unwrap().into_inner()
        };
        let orig = mk("3", "4");
        assert_eq!(
            cell_metadata_bindings(&orig),
            cell_metadata_bindings(&mk("3", "4"))
        );
        assert_ne!(
            cell_metadata_bindings(&orig),
            cell_metadata_bindings(&mk("4", "3")),
            "a vm repoint must differ"
        );
        assert_eq!(
            verify_noncell_refs(&orig, &mk("4", "3")).expect("a vm repoint must refuse")["reason"],
            "cell_metadata_mismatch"
        );
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

    /// Like `oracle_wb`, but also injects `defined_names_xml` (a `<definedNames>…</definedNames>`
    /// block) into workbook.xml after `</sheets>`. Used to place a function inside a DEFINED NAME —
    /// which the engine's defined-name API validator rejects, so it must come from the loaded XML.
    fn oracle_wb_named(rows: &str, defined_names_xml: &str) -> Vec<u8> {
        use std::io::Read;
        let base = oracle_wb(rows);
        let mut ar = zip::ZipArchive::new(Cursor::new(base.as_slice())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for i in 0..ar.len() {
            let mut f = ar.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            out.start_file(&name, opts).unwrap();
            if name == "xl/workbook.xml" {
                let s = String::from_utf8(b).unwrap();
                let patched = s.replacen("</sheets>", &format!("</sheets>{defined_names_xml}"), 1);
                out.write_all(patched.as_bytes()).unwrap();
            } else {
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
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
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
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
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
    fn pre_1900_date_serial_excluded_but_modern_date_vouchable() {
        // REGRESSION (round-49 defect 5): the engine deliberately omits Excel's phantom 1900-02-29
        // (it follows Google-Docs/LibreOffice), so a DATE result BEFORE 1900-03-01 (serial < 61) is
        // off by one from Excel's stored serial — under the DEFAULT 1900 system, not just 1904. Such
        // a cache (and its dependents) must be EXCLUDED (fail-closed): vouching the engine's serial
        // would CERTIFY a value-corrupting cache and REFUSE the faithful Excel one. A MODERN date
        // (serial >= 61, where engine == Excel) must stay vouchable — value-gated, no over-refusal.
        let rows = r#"<row r="1"><c r="A1"><f>DATE(1900,1,1)</f><v>2</v></c><c r="B1"><f>A1+0</f><v>2</v></c><c r="C1"><f>DATE(2020,1,1)</f><v>43831</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load date workbook");
        // date1904=false: the DEFAULT 1900 system, where the finder's false-certify lived.
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("A1")),
            "a pre-1900 DATE serial must be excluded from the oracle: {oracle:?}"
        );
        assert!(
            !oracle.contains_key(&key("B1")),
            "a cell depending on a pre-1900 DATE is excluded transitively"
        );
        assert!(
            oracle.contains_key(&key("C1")),
            "a modern DATE serial (>= 61) stays vouchable — no over-refusal: {oracle:?}"
        );
    }

    #[test]
    fn poison_diff_excludes_a_probe_crafted_source_dependent() {
        // REGRESSION (round-56 defect 1, HIGH false-certify): a formula crafted to be invariant
        // under the OLD fixed poison constants (1234567 / -98765.4321) but different for the source's
        // true value must NOT be vouched. A1 t="d" loads as #N/IMPL! (a source); B1 launders it.
        // With PER-RUN RANDOM probes B1 is no longer invariant and is correctly excluded.
        let rows = r#"<row r="1"><c r="A1" t="d"><v>2020-01-01T00:00:00</v></c><c r="B1"><f>IF(ISERROR(A1),111,IF(OR(A1=1234567,A1=-98765.4321),111,222))</f><v>111</v></c><c r="D1"><f>SUM(A2:A3)</f><v>0</v></c></row><row r="2"><c r="A2"><v>7</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load probe-craft workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "a probe-crafted laundering formula (invariant under the OLD fixed constants) must be \
             excluded now that the probes are random: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "an unrelated pure SUM stays vouchable"
        );
    }

    #[test]
    fn date_consumer_reading_a_plain_early_value_is_excluded() {
        // REGRESSION (round-56 defect 2, false-certify): DAY(A1) where A1=59 is a PLAIN (non-date-
        // formatted) number, so the round-54 format/literal gates miss it, yet the engine computes
        // DAY(59)=27 (Excel 28). The value-cell poison-diff detects that B1 reads an early serial and
        // excludes it, while a consumer reading a MODERN value (C1=DAY(A2), A2=44000) stays vouchable.
        let rows = r#"<row r="1"><c r="A1"><v>59</v></c><c r="B1"><f>DAY(A1)</f><v>28</v></c><c r="C1"><f>DAY(A2)</f><v>15</v></c></row><row r="2"><c r="A2"><v>44000</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load early-value workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "DAY of a plain early value (A1=59) must be excluded (engine off-by-one): {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("C1")),
            "DAY of a MODERN value (A2=44000) stays vouchable — no over-refusal"
        );
    }

    #[test]
    fn date_consumer_reading_a_formula_produced_early_serial_is_excluded() {
        // REGRESSION (round-57 defect 1): the early serial is PRODUCED BY A FORMULA (A1=44000-43941
        // -> 59), so it is neither a literal value cell nor a DATE_SERIAL_PRODUCER. The prune must
        // still poison it (any cell whose value is a serial < 61, formula or not) and exclude the
        // consumer B1=DAY(A1); a consumer reading a MODERN formula-produced serial stays vouchable.
        let rows = r#"<row r="1"><c r="A1"><f>44000-43941</f><v>59</v></c><c r="B1"><f>DAY(A1)</f><v>28</v></c><c r="C1"><f>DAY(A2)</f><v>13</v></c></row><row r="2"><c r="A2"><f>44000-1</f><v>43999</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load formula-serial workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "DAY of a FORMULA-produced early serial must be excluded: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("C1")),
            "DAY of a formula-produced MODERN serial stays vouchable"
        );
    }

    #[test]
    fn formula_reading_a_t_d_date_cell_is_not_vouched() {
        // REGRESSION (round-55 defect 1, HIGH false-certify): the importer loads a `t="d"` ISO-date
        // VALUE cell as the engine's NOT-IMPLEMENTED sentinel (#N/IMPL!). A formula that READS it
        // (`=A1+1`) propagates that, and `IFERROR(A1+1,0)` masks it to a clean 0 — either way the
        // engine value is NOT Excel's real date, so vouching it would false-certify a forged cache.
        // Both the reader and the error-masked reader must be EXCLUDED; an unrelated cell stays
        // vouchable.
        let rows = r#"<row r="1"><c r="A1" t="d"><v>2020-01-01T00:00:00</v></c><c r="B1"><f>A1+1</f><v>43832</v></c><c r="C1"><f>IFERROR(A1+1,0)</f><v>43832</v></c><c r="D1"><f>SUM(A2:A3)</f><v>0</v></c></row><row r="2"><c r="A2"><v>5</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load t=d workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "a formula reading a t=\"d\" cell must be excluded: {oracle:?}"
        );
        assert!(
            !oracle.contains_key(&key("C1")),
            "an IFERROR-masked reader of a t=\"d\" cell must be excluded (else a forged clean number \
             would be vouched)"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "an unrelated cell independent of the t=\"d\" value stays vouchable"
        );
    }

    #[test]
    fn early_date_consumer_excluded_but_modern_consumer_vouchable() {
        // REGRESSION (round-54 defect 1, false-certify): a date CONSUMER (DAY/MONTH/YEAR/WEEKDAY/…)
        // reading a pre-1900-03-01 serial (< 61) computes an Excel-divergent value on the engine
        // (DAY(59) = 27 here, 28 in Excel), so its cache must be EXCLUDED from the oracle. A modern
        // consumer (input >= 61) stays vouchable — no blanket over-refusal of DAY/MONTH/YEAR.
        let rows = r#"<row r="1"><c r="B1"><f>DAY(59)</f><v>27</v></c><c r="C1"><f>B1+0</f><v>27</v></c><c r="D1"><f>DAY(50000)</f><v>18</v></c><c r="E1"><f>SUM(A1:A1)</f><v>0</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load date-consumer workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "DAY(59) reads an early serial -> excluded (else the engine's off-by-one 27 would be \
             vouched): {oracle:?}"
        );
        assert!(
            !oracle.contains_key(&key("C1")),
            "a dependent of the early-date consumer is excluded transitively"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "DAY(50000) reads a MODERN serial (>= 61) -> stays vouchable, no over-refusal: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("E1")),
            "a pure SUM (no date function) stays vouchable"
        );
    }

    #[test]
    fn inline_early_date_consumer_and_dependent_are_excluded() {
        // REGRESSION (round-59, MEDIUM false-certify): the early serial is computed INLINE inside the
        // consumer (`DAY(700-645)` -> DAY(55), engine 23 vs Excel 24) — no value cell to poison, and
        // no bare literal in [1,60], so both prior gates missed it. The argument-evaluation prune
        // catches it, AND its transitive dependent (`C1 = B1+0`) is dropped too (the dependent gap:
        // otherwise a forged cache on C1 carrying the engine's off-by-one value would be vouched). A
        // modern inline consumer and a two-serial difference over a modern span stay vouchable.
        let rows = r#"<row r="1"><c r="B1"><f>DAY(700-645)</f><v>24</v></c><c r="C1"><f>B1+0</f><v>24</v></c><c r="D1"><f>DAY(50000-1000)</f><v>10</v></c><c r="E1"><f>DAYS(50000,49000)</f><v>1000</v></c><c r="F1"><f>SUM(A1:A1)</f><v>0</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load inline-date workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "DAY(700-645) reads an inline early serial (55) -> excluded: {oracle:?}"
        );
        assert!(
            !oracle.contains_key(&key("C1")),
            "a dependent (C1=B1+0) of the inline early-date consumer must be dropped transitively"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "DAY(50000-1000) is a MODERN inline serial (49000) -> stays vouchable: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("E1")),
            "DAYS over a modern span (both >= 61) stays vouchable — no over-refusal"
        );
        assert!(
            oracle.contains_key(&key("F1")),
            "a pure SUM stays vouchable"
        );
    }

    #[test]
    fn date_producer_consuming_an_early_serial_is_excluded() {
        // REGRESSION (round-60 defect 4, HIGH false-certify): EDATE/EOMONTH/WORKDAY also CONSUME a
        // serial in arg 0. Reading a pre-1900-03-01 input (< 61) lands the engine up to a month off
        // from Excel even when the OUTPUT is modern (EOMONTH(32,2)=91 here vs Excel 121), so the
        // producer's output-only gate missed it. Adding them as consumers evaluates their input arg.
        let rows = r#"<row r="1"><c r="B1"><f>EOMONTH(32,2)</f><v>121</v></c><c r="C1"><f>EDATE(44000,2)</f><v>44059</v></c><c r="D1"><f>SUM(A1:A1)</f><v>0</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load producer-consumer workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "EOMONTH reading an early serial (32) must be excluded (engine off by ~a month): {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("C1")),
            "EDATE reading a MODERN serial (44000) stays vouchable — no over-refusal"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "a pure SUM stays vouchable"
        );
    }

    /// Build a workbook from `rows` with `fullCalcOnLoad` stripped, so a preserved cache is actually
    /// verified (rather than short-circuited by the recalc-on-load path).
    fn e2e_wb_no_recalc(rows: &str) -> Vec<u8> {
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
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            out.start_file(&name, opts).unwrap();
            if name == "xl/worksheets/sheet1.xml" {
                let s = format!(
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:E10"/><sheetData>{rows}</sheetData></worksheet>"#
                );
                out.write_all(s.as_bytes()).unwrap();
            } else if name == "xl/workbook.xml" {
                let s = String::from_utf8(b)
                    .unwrap()
                    .replace(" fullCalcOnLoad=\"1\"", "");
                out.write_all(s.as_bytes()).unwrap();
            } else {
                out.write_all(&b).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn e2e_false_certify_computed_early_serial() {
        use std::io::Read;
        // REGRESSION (round-59, end-to-end): B1 = DAY(700-645) = DAY(55) — Excel renders 24, the engine
        // 23 (phantom-leap-day off-by-one). xlq's transform force-recomputes B1 (it cannot vouch the
        // cache), so the faithful transform carries NO B1 cache. A FORGED foreign edit injects a
        // preserved B1 cache = 23 (the engine's wrong value); because B1 is excluded from the oracle,
        // `by_eval` cannot vouch it and it is REFUSED. Were B1 NOT excluded, the oracle would hold 23
        // and vouch the forgery — the false-certify this guards against.
        let orig = e2e_wb_no_recalc(
            r#"<row r="1"><c r="B1"><f>DAY(700-645)</f><v>24</v></c></row><row r="8"><c r="A8"><v>9</v></c></row>"#,
        );
        let edit = StructuralEdit {
            axis: crate::refshift::Axis::Row,
            at: 5,
            count: 1,
            op: crate::refshift::Op::Insert,
            sheet: "Sheet1".to_string(),
            dest: 0,
        };
        let (expected, _r) = structural::structural_edit(&orig, &edit).unwrap();
        let forge = |bytes: &[u8], from: &str, to: &str| -> Vec<u8> {
            let mut ar = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
            let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default();
            for i in 0..ar.len() {
                let mut f = ar.by_index(i).unwrap();
                let name = f.name().to_string();
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                out.start_file(&name, opts).unwrap();
                if name == "xl/worksheets/sheet1.xml" {
                    let s = String::from_utf8(b).unwrap().replace(from, to);
                    out.write_all(s.as_bytes()).unwrap();
                } else {
                    out.write_all(&b).unwrap();
                }
            }
            out.finish().unwrap().into_inner()
        };
        let expected_blanks_b1 = {
            let mut ar = zip::ZipArchive::new(Cursor::new(expected.clone())).unwrap();
            let mut s = String::new();
            for i in 0..ar.len() {
                let mut f = ar.by_index(i).unwrap();
                if f.name() == "xl/worksheets/sheet1.xml" {
                    f.read_to_string(&mut s).unwrap();
                }
            }
            s.contains(r#"<c r="B1"><f>DAY(700-645)</f></c>"#)
        };
        // Inject a preserved cache carrying the engine's off-by-one value 23 (a forged foreign edit).
        let forged = forge(
            &expected,
            r#"<c r="B1"><f>DAY(700-645)</f></c>"#,
            r#"<c r="B1"><f>DAY(700-645)</f><v>23</v></c>"#,
        );
        let dir = std::env::temp_dir();
        let tag = format!("xlqe2e_{}", std::process::id());
        let op = dir.join(format!("{tag}_o.xlsx"));
        let fp = dir.join(format!("{tag}_f.xlsx"));
        let hp = dir.join(format!("{tag}_h.xlsx"));
        std::fs::write(&op, &orig).unwrap();
        std::fs::write(&fp, &forged).unwrap();
        std::fs::write(&hp, &expected).unwrap();
        let rf = run(
            op.to_str().unwrap(),
            fp.to_str().unwrap(),
            "Sheet1",
            "insert-rows",
            5,
            1,
            0,
        )
        .unwrap();
        let rh = run(
            op.to_str().unwrap(),
            hp.to_str().unwrap(),
            "Sheet1",
            "insert-rows",
            5,
            1,
            0,
        )
        .unwrap();
        let _ = std::fs::remove_file(&op);
        let _ = std::fs::remove_file(&fp);
        let _ = std::fs::remove_file(&hp);
        assert!(
            expected_blanks_b1,
            "the transform force-recomputes the unvouchable inline-date consumer (blanks its cache)"
        );
        assert_eq!(
            rf["status"], "REFUSED",
            "an injected cache carrying the engine's off-by-one DAY(700-645)=23 must be REFUSED \
             end-to-end (B1 is oracle-excluded, so the engine value cannot vouch it): {rf}"
        );
        assert_eq!(
            rh["status"], "CERTIFIED",
            "the faithful transform (B1's cache blanked, nothing to verify) must CERTIFY: {rh}"
        );
    }

    #[test]
    fn date_consumer_serial_argument_extraction() {
        // The serial-argument extractor recognizes each consumer call (bounded by non-identifier
        // characters so a longer name is not mistaken for a shorter one) and returns the correct
        // serial-valued argument expressions — whatever their form.
        assert_eq!(
            date_consumer_serial_args("=DAY(59)"),
            (vec!["59".into()], true)
        );
        assert_eq!(
            date_consumer_serial_args("=DAY(700-645)"),
            (vec!["700-645".into()], true)
        );
        assert_eq!(
            date_consumer_serial_args("=DAY(A1)"),
            (vec!["A1".into()], true)
        );
        // WEEKDAY's 2nd argument is a return-type code, NOT a serial -> only arg 0 is probed.
        assert_eq!(
            date_consumer_serial_args("=WEEKDAY(A1,2)"),
            (vec!["A1".into()], true)
        );
        // The day-difference functions take TWO serials.
        assert_eq!(
            date_consumer_serial_args("=NETWORKDAYS(A1,B2,Holidays)"),
            (vec!["A1".into(), "B2".into()], true)
        );
        assert_eq!(
            date_consumer_serial_args("=DATEDIF(A1,A2,\"D\")"),
            (vec!["A1".into(), "A2".into()], true)
        );
        // A longer name is not mistaken for a shorter one; a non-consumer formula finds nothing.
        assert_eq!(
            date_consumer_serial_args("=DAYS360(A1,A2)"),
            (vec![], false)
        );
        assert_eq!(date_consumer_serial_args("=MYDAY(1)"), (vec![], false));
        assert_eq!(date_consumer_serial_args("=SUM(A1:A9)"), (vec![], false));
        // Nested calls and quoted sheet names with commas do not tear the argument. EOMONTH is now
        // itself a consumer (it reads a serial in arg 0), so BOTH the outer MONTH arg and the inner
        // EOMONTH arg are extracted.
        assert_eq!(
            date_consumer_serial_args("=MONTH(EOMONTH(A1,1))"),
            (vec!["EOMONTH(A1,1)".into(), "A1".into()], true)
        );
        assert_eq!(
            date_consumer_serial_args("=DAY('A,B'!C1)"),
            (vec!["'A,B'!C1".into()], true)
        );
    }

    #[test]
    fn bad_function_laundered_through_a_defined_name_is_excluded() {
        // REGRESSION (round-53 defect 1, HIGH false-certify): a bad (unsupported/UDF) function that
        // lives ONLY inside a DEFINED NAME's body is invisible to the cell-formula scan that builds
        // `sources`, and poison-and-diff cannot poison a name — so a cell `=IFERROR(Bad,999)` used to
        // survive with the engine's WRONG masked value (999) and vouch a forged cache. The
        // defined-name closure must now mark such a cell as a source and EXCLUDE it, while a pure SUM
        // stays vouchable (no blanket over-refusal).
        // `Bad` refers to RTD — a policy-limited function (its value depends on an external service
        // the engine never contacts, so the engine computes it WRONG, a #N/A the IFERROR masks to
        // 999). It stands in for any bad function (UDF / unsupported / engine-divergent). It is
        // injected into workbook.xml directly (the defined-name VALIDATOR rejects a function body via
        // the API), and appears ONLY in the name — no cell formula calls RTD.
        let rows = r#"<row r="1"><c r="A1"><v>10</v></c><c r="B1"><f>IFERROR(Bad,999)</f><v>999</v></c><c r="C1"><f>B1+1</f><v>1000</v></c><c r="D1"><f>SUM(A1:A1)</f><v>10</v></c></row>"#;
        let names =
            r#"<definedNames><definedName name="Bad">RTD("a","","b")</definedName></definedNames>"#;
        let bytes = oracle_wb_named(rows, names);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load udf-name workbook");
        let census = crate::census::function_census(&model);
        assert!(
            census.policy_limited.contains_key("RTD"),
            "the function inside the defined name must be in the bad set: {census:?}"
        );
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        assert!(
            !oracle.contains_key(&key("B1")),
            "a cell laundering a bad function through a defined name must be EXCLUDED (else a forged \
             cache would false-certify): {oracle:?}"
        );
        assert!(
            !oracle.contains_key(&key("C1")),
            "a transitive dependent of the laundering cell is excluded too"
        );
        assert!(
            oracle.contains_key(&key("D1")),
            "a pure SUM independent of the bad name stays vouchable — no blanket over-refusal"
        );
    }

    #[test]
    fn three_d_span_cell_vouched_when_the_engine_evaluates_it() {
        // The vendored engine now EVALUATES 3D (multi-sheet) spans in aggregates (round-51), so a
        // correctly-computed span cache is VOUCHABLE — the round-50 over-refusal is closed. The
        // exclusion is now value-gated: only a span the engine still cannot evaluate (an ERROR
        // value) is excluded, which keeps the forged-#VALUE! false-certify closed.
        let rows = r#"<row r="1"><c r="D1"><f>SUM(Sheet1:Sheet2!A5)</f><v>10</v></c><c r="E1"><f>D1+1</f><v>11</v></c><c r="B1"><f>SUM(C1:C2)</f><v>7</v></c></row><row r="2"><c r="C1"><v>3</v></c><c r="C2"><v>4</v></c></row><row r="5"><c r="A5"><v>10</v></c></row>"#;
        let bytes = oracle_wb(rows);
        let mut model = load_from_bytes(
            &bytes,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/structural/refs.xlsx"
            ),
        )
        .expect("load 3d-span workbook");
        let oracle = build_cache_oracle(&mut model, false, &Default::default())
            .expect("oracle is always Some");
        let key = |c: &str| ("Sheet1".to_string(), c.to_string());
        // SUM(Sheet1:Sheet2!A5) = 10 (Sheet1!A5=10 + Sheet2!A5=empty) — vouched at the true value.
        assert_eq!(
            oracle.get(&key("D1")).map(String::as_str),
            Some("n:10"),
            "an evaluable 3D span must be vouched at its true value: {oracle:?}"
        );
        assert!(
            oracle.contains_key(&key("E1")),
            "a dependent of an evaluable 3D span is vouchable too"
        );
        assert!(
            oracle.contains_key(&key("B1")),
            "a plain SUM stays vouchable: {oracle:?}"
        );
    }

    #[test]
    fn iso_date_value_change_is_refused() {
        // REGRESSION (round-49 defect 3): an ISO-8601 date VALUE cell (t="d") is discarded by
        // ironcalc's importer (loaded as a constant NIMPL error), so the engine snapshot is blind to
        // a change of its stored date. verify_noncell_refs must catch it at the byte level.
        let dt = |v: &str| {
            format!(r#"<sheetData><row r="1"><c r="Z1" t="d"><v>{v}</v></c></row></sheetData>"#)
        };
        let good = wb(&dt("2020-01-01T00:00:00"), &[]);
        assert!(
            verify_noncell_refs(&good, &good).is_none(),
            "an identical t=\"d\" date must not refuse"
        );
        let changed = wb(&dt("2099-12-31T00:00:00"), &[]);
        assert_eq!(
            verify_noncell_refs(&good, &changed).expect("a changed t=\"d\" date must refuse")
                ["reason"],
            "date_value_mismatch"
        );
    }

    #[test]
    fn multi_value_cell_is_refused() {
        // REGRESSION (round-51 defect 2): a cell with two `<v>` children is malformed — the engine
        // misreads it, real readers take the last <v>. verify_noncell_refs refuses a workbook with one.
        let good = wb(
            r#"<sheetData><row r="1"><c r="Z1" t="n"><v>5</v></c></row></sheetData>"#,
            &[],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        let bad = wb(
            r#"<sheetData><row r="1"><c r="Z1" t="n"><v>0</v><v>999</v></c></row></sheetData>"#,
            &[],
        );
        assert_eq!(
            verify_noncell_refs(&good, &bad).expect("a multi-<v> cell must refuse")["reason"],
            "malformed_multi_value_cell"
        );
    }

    #[test]
    fn near_zero_cache_is_not_snapped_to_zero() {
        // REGRESSION (round-52 defect 5): the zero-snap tolerance was UNSOUND and REMOVED — a forged
        // 0 cache must NOT be vouched against a genuine tiny computed value (1.25e-10).
        assert!(!nums_equal_at_excel_precision(0.0, 1.25e-10));
        assert!(!nums_equal_at_excel_precision(0.0, -2.7755575615628914e-17));
        assert!(nums_equal_at_excel_precision(0.0, 0.0));
    }

    #[test]
    fn number_format_code_change_is_detected() {
        // REGRESSION (round-51 defect 5): a numFmt change that leaves the RENDERED value unchanged
        // ("0" vs "General" both show 5) is invisible to the display-based `format` diff, but
        // CELL("format") reads the CODE. Resolve per-cellXf format codes: custom -> formatCode,
        // built-in -> its canonical ECMA-376 code string (round-52 defect 4).
        let styles = br#"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="&quot;$&quot;#,##0.00"/></numFmts><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="1"/><xf numFmtId="164"/></cellXfs></styleSheet>"#;
        let codes = cellxfs_numfmt_codes(styles);
        assert_eq!(codes[0], "General"); // built-in 0
        assert_eq!(codes[1], "0"); // built-in 1
        assert_eq!(codes[2], "\"$\"#,##0.00"); // custom
                                               // The three codes are distinct, so a numFmt CODE change is still detected.
        assert_ne!(codes[0], codes[1]);
    }

    #[test]
    fn builtin_numfmt_expanded_to_equivalent_custom_is_not_refused() {
        // REGRESSION (round-52 defect 4): a faithful editor (LibreOffice) that re-encodes built-in
        // numFmtId 2 as an EQUIVALENT custom `<numFmt formatCode="0.00">` must resolve to the SAME
        // canonical code, so a CELL("format") reader sees no change and certify does not over-refuse.
        let builtin =
            br#"<styleSheet><cellXfs count="1"><xf numFmtId="2"/></cellXfs></styleSheet>"#;
        let expanded = br#"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts><cellXfs count="1"><xf numFmtId="164"/></cellXfs></styleSheet>"#;
        assert_eq!(
            cellxfs_numfmt_codes(builtin),
            cellxfs_numfmt_codes(expanded),
            "built-in 2 and custom \"0.00\" must canonicalize identically"
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
    fn drawing_shape_macro_repoint_is_caught() {
        // REGRESSION (round-56 defect 3, HIGH security): a DrawingML shape's `macro=` click binding
        // (the modern analog of VML FmlaMacro) was scanned but never compared, so re-pointing a
        // button from a benign macro to a destructive one CERTIFIED. Now compared in chart_drawing_refs.
        let shape = |mac: &str| {
            format!(
                r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:sp macro="{mac}"><xdr:nvSpPr/></xdr:sp></xdr:wsDr>"#
            )
        };
        let good = wb(
            "",
            &[(
                "xl/drawings/drawing1.xml",
                shape("Module1.SubmitReport").as_str(),
            )],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        let evil = wb(
            "",
            &[(
                "xl/drawings/drawing1.xml",
                shape("Module1.WipeAndExfiltrate").as_str(),
            )],
        );
        let refusal =
            verify_noncell_refs(&good, &evil).expect("a drawing macro re-point must be caught");
        assert_eq!(refusal["reason"], "chart_drawing_mismatch");
        // A non-macro shape (macro="" / absent) is not spuriously refused.
        let plain = wb(
            "",
            &[(
                "xl/drawings/drawing1.xml",
                r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:sp macro=""><xdr:nvSpPr/></xdr:sp></xdr:wsDr>"#,
            )],
        );
        assert!(verify_noncell_refs(&plain, &plain).is_none());
    }

    #[test]
    fn drawing_hyperlink_swap_between_shapes_is_caught() {
        // REGRESSION (round-59 defect 5, security): two shapes' hyperlink targets, keyed by a flat
        // multiset, were permutation-invariant — swapping the r:ids so the "Download" button points
        // at the attacker URL CERTIFIED. Keying each hlink by the owning shape's identity (cNvPr
        // name) catches the swap.
        let two = |r1: &str, r2: &str| {
            format!(
                r#"<xdr:wsDr xmlns:xdr="urn:xdr" xmlns:a="urn:a"><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="1" name="DownloadBtn"><a:hlinkClick xmlns:r="urn:r" r:id="{r1}"/></xdr:cNvPr></xdr:nvSpPr></xdr:sp><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="2" name="UnsubBtn"><a:hlinkClick xmlns:r="urn:r" r:id="{r2}"/></xdr:cNvPr></xdr:nvSpPr></xdr:sp></xdr:wsDr>"#
            )
        };
        let drels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdRep" Type="x/hyperlink" Target="https://reports.corp.example/q3" TargetMode="External"/><Relationship Id="rIdTrk" Type="x/hyperlink" Target="https://track.example/unsub" TargetMode="External"/></Relationships>"#;
        // Download->reports, Unsub->track.
        let good = wb(
            "",
            &[
                ("xl/drawings/drawing1.xml", two("rIdRep", "rIdTrk").as_str()),
                ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        // SWAP: Download now -> track (attacker/other), Unsub -> reports. Same URL multiset.
        let swapped = wb(
            "",
            &[
                ("xl/drawings/drawing1.xml", two("rIdTrk", "rIdRep").as_str()),
                ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ],
        );
        let refusal = verify_noncell_refs(&good, &swapped)
            .expect("a hyperlink swap between shapes must be caught");
        assert!(
            refusal["reason"] == "chart_drawing_mismatch"
                || refusal["reason"] == "external_relationship_mismatch",
            "reason: {}",
            refusal["reason"]
        );
    }

    #[test]
    fn drawing_linked_image_external_target_repoint_is_caught() {
        // REGRESSION (round-53 defect 7, HIGH security): a drawing LINKED image (`<a:blip r:link>`)
        // resolves through the drawing's `.rels` to a `TargetMode="External"` URL that Excel
        // auto-fetches on open. Only hyperlink + hlinkClick were resolved, so repointing the blip
        // link to an attacker URL/UNC (drawing part byte-identical, change lives in the allowlisted
        // `.rels`) used to CERTIFY. The external-rels comparator now catches it.
        let parts = |target: &str| {
            vec![
                (
                    "xl/drawings/drawing1.xml".to_string(),
                    r#"<xdr:wsDr xmlns:xdr="urn:xdr" xmlns:a="urn:a"><xdr:pic><xdr:blipFill><a:blip xmlns:r="urn:r" r:link="rId1"/></xdr:blipFill></xdr:pic></xdr:wsDr>"#.to_string(),
                ),
                (
                    "xl/drawings/_rels/drawing1.xml.rels".to_string(),
                    format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="{target}" TargetMode="External"/></Relationships>"#),
                ),
            ]
        };
        let mk = |target: &str| {
            let p = parts(target);
            wb(
                "",
                &p.iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect::<Vec<_>>(),
            )
        };
        let good = mk("https://legit.example/logo.png");
        assert!(
            verify_noncell_refs(&good, &good).is_none(),
            "an identical linked image must not refuse"
        );
        let evil = mk(r"\\attacker.example\share\x.png");
        let refusal =
            verify_noncell_refs(&good, &evil).expect("a repointed linked-image target must refuse");
        assert_eq!(refusal["reason"], "external_relationship_mismatch");
        // An EMBEDDED image (internal, no TargetMode) is a package part, not an external target, so
        // it does not enter this comparator (no over-refusal on a benign embed).
        let embed = wb(
            "",
            &[(
                "xl/drawings/_rels/drawing1.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#,
            )],
        );
        assert!(external_rels_targets(&embed).is_empty());
    }

    #[test]
    fn external_rels_target_transposition_is_caught() {
        // REGRESSION (round-60 defect 5, security): two same-type external targets in one rels part,
        // pooled into a flat (type,target) multiset, were permutation-invariant — TRANSPOSING the
        // targets of two chart hyperlinks (or two linked-image blips) left the multiset unchanged and
        // CERTIFIED. Binding each target to its relationship Id makes the transposition differ.
        let rels = |ta: &str, tb: &str| {
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdA" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{ta}" TargetMode="External"/><Relationship Id="rIdB" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{tb}" TargetMode="External"/></Relationships>"#
            )
        };
        let chart = r#"<c:chartSpace xmlns:c="urn:c" xmlns:a="urn:a" xmlns:r="urn:r"><c:title><a:hlinkClick r:id="rIdA"/></c:title><c:ser><a:hlinkClick r:id="rIdB"/></c:ser></c:chartSpace>"#;
        let mk = |ta: &str, tb: &str| {
            wb(
                "",
                &[
                    ("xl/charts/chart1.xml", chart),
                    ("xl/charts/_rels/chart1.xml.rels", &rels(ta, tb)),
                ],
            )
        };
        let good = mk("https://corp.example/report", "https://terms.example/tos");
        assert!(verify_noncell_refs(&good, &good).is_none());
        // TRANSPOSE: rIdA now -> terms, rIdB -> report. Same URL multiset, different rId bindings.
        let swapped = mk("https://terms.example/tos", "https://corp.example/report");
        assert_eq!(
            verify_noncell_refs(&good, &swapped)
                .expect("a transposition of two external targets must refuse")["reason"],
            "external_relationship_mismatch"
        );
    }

    #[test]
    fn external_rels_parser_is_namespace_and_whitespace_robust() {
        // REGRESSION (round-55 defect 6, HIGH security): the `.rels` parsers used a `<Relationship `
        // substring scan that a namespace-PREFIXED `<pr:Relationship>` (bound to the packaging
        // namespace) or a non-space whitespace (`<Relationship\nId=…>`) evades — so an injected
        // external linked-image / OLE / hyperlink target hid from the signature and CERTIFIED.
        let prefixed = wb(
            "",
            &[(
                "xl/drawings/_rels/drawing1.xml.rels",
                "<pr:Relationships xmlns:pr=\"http://schemas.openxmlformats.org/package/2006/relationships\"><pr:Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"\\\\attacker.example\\share\\x.png\" TargetMode=\"External\"/></pr:Relationships>",
            )],
        );
        assert!(
            !external_rels_targets(&prefixed).is_empty(),
            "a prefixed <pr:Relationship> external target must be seen"
        );
        let newline = wb(
            "",
            &[(
                "xl/drawings/_rels/drawing1.xml.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n<Relationship\n\tId=\"rId1\"\n\tType=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\"\n\tTarget=\"https://evil.example/x.png\"\n\tTargetMode=\"External\"/></Relationships>",
            )],
        );
        assert!(
            !external_rels_targets(&newline).is_empty(),
            "a newline/tab-separated <Relationship> external target must be seen"
        );
        // End-to-end: a clean (no external target) transform vs a prefixed-rels external repoint
        // must REFUSE, not certify.
        let clean = wb("", &[]);
        let refusal = verify_noncell_refs(&clean, &prefixed)
            .expect("an injected prefixed external target must refuse");
        assert_eq!(refusal["reason"], "external_relationship_mismatch");
        // rels_targets (hyperlink URL resolution) is likewise robust.
        assert_eq!(
            rels_targets(&newline, "xl/drawings/drawing1.xml")
                .get("rId1")
                .map(String::as_str),
            Some("https://evil.example/x.png")
        );
    }

    #[test]
    fn chart_and_hover_hyperlink_external_targets_are_compared() {
        // REGRESSION (round-54 defects 2/3/8): the round-53 external-rels comparator skipped ALL
        // hyperlink-typed relationships (to avoid double-refusing worksheet folds), but the only
        // dedicated hyperlink comparators were worksheet `<hyperlink>` and drawing `hlinkClick` — so
        // a CHART-part hyperlink and a drawing `hlinkHover` (both Type=hyperlink, External) were
        // compared by nothing and a phishing repoint CERTIFIED. Now scoped to worksheet-owned rels,
        // so chart/drawing hyperlink externals are compared.
        let mk = |rels_part: &str, target: &str| {
            wb(
                "",
                &[(
                    rels_part,
                    // The owning XML part is byte-identical across good/evil; only the rels differs.
                    &format!(
                        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{target}" TargetMode="External"/></Relationships>"#
                    ),
                )],
            )
        };
        for rels in [
            "xl/charts/_rels/chart1.xml.rels",
            "xl/drawings/_rels/drawing1.xml.rels",
        ] {
            let good = mk(rels, "https://good.example.com/x");
            assert!(
                verify_noncell_refs(&good, &good).is_none(),
                "identical {rels} hyperlink must not refuse"
            );
            let evil = mk(rels, "https://evil.example.com/phish");
            let refusal = verify_noncell_refs(&good, &evil)
                .unwrap_or_else(|| panic!("a repointed {rels} hyperlink must refuse"));
            assert_eq!(refusal["reason"], "external_relationship_mismatch");
        }
        // A WORKSHEET-owned hyperlink is still folded (not double-compared) — a benign trailing
        // slash on a bare-authority chart URL is not a spurious mismatch.
        let a = mk(
            "xl/charts/_rels/chart1.xml.rels",
            "https://good.example.com",
        );
        let b = mk(
            "xl/charts/_rels/chart1.xml.rels",
            "https://good.example.com/",
        );
        assert!(
            verify_noncell_refs(&a, &b).is_none(),
            "a trailing-slash renormalization must not refuse"
        );
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
        assert!(formula_calls_sensitive_cell(
            r#"CELL("format",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        assert!(formula_calls_sensitive_cell(
            r#"CELL("color",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        assert!(formula_calls_sensitive_cell(
            r#"IF(CELL("parentheses",B2)=1,"y","n")"#,
            &CELL_FORMAT_SENSITIVE
        ));
        // Case- and _xlfn.-insensitive.
        assert!(formula_calls_sensitive_cell(
            r#"cell("FORMAT",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        assert!(formula_calls_sensitive_cell(
            r#"_xlfn.CELL("format",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        // A NON-literal info type is unresolvable -> conservative true.
        assert!(formula_calls_sensitive_cell(
            "CELL(D1,A1)",
            &CELL_FORMAT_SENSITIVE
        ));
        // Format-INSENSITIVE info types -> not sensitive.
        assert!(!formula_calls_sensitive_cell(
            r#"CELL("contents",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        assert!(!formula_calls_sensitive_cell(
            r#"CELL("row",A1)+CELL("col",A1)"#,
            &CELL_FORMAT_SENSITIVE
        ));
        // A STRING LITERAL that merely contains "CELL(" is not a call.
        assert!(!formula_calls_sensitive_cell(
            r#"CONCAT("CELL(""format"",A1)","x")"#,
            &CELL_FORMAT_SENSITIVE
        ));
        // A sheet named CELL is not the function.
        assert!(!formula_calls_sensitive_cell(
            "'CELL'!A1+1",
            &CELL_FORMAT_SENSITIVE
        ));
        // No CELL at all.
        assert!(!formula_calls_sensitive_cell(
            "SUM(A1:A10)*1.1",
            &CELL_FORMAT_SENSITIVE
        ));
    }

    #[test]
    fn custom_xml_part_is_certify_safe() {
        // A custom-XML data island carries no worksheet coordinate; certify must not refuse xlq's
        // own transform of a workbook containing one (identical content -> no refusal).
        let bytes = wb(
            "",
            &[("customXml/item1.xml", "<root><tag>hello</tag></root>")],
        );
        assert!(verify_noncell_refs(&bytes, &bytes).is_none());
    }

    #[test]
    fn custom_xml_datamashup_repoint_is_caught() {
        // REGRESSION (round-56 defect 10, HIGH security): a Power Query DataMashup source URL lives
        // inline in customXml (base64), which was allowlisted as inert and never compared — a repoint
        // (good -> evil) CERTIFIED. Its CONTENT is now compared via opaque_target_signature.
        let mashup = |host: &str| {
            format!(
                r#"<root><DataMashup>M-source Web.Contents("https://{host}/api")</DataMashup></root>"#
            )
        };
        let good = wb(
            "",
            &[("customXml/item1.xml", mashup("good.example").as_str())],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        let evil = wb(
            "",
            &[("customXml/item1.xml", mashup("evil.example").as_str())],
        );
        let refusal = verify_noncell_refs(&good, &evil)
            .expect("a repointed DataMashup query source must refuse");
        assert_eq!(refusal["reason"], "external_target_mismatch");
        // REGRESSION (round-58 defect 6): the same repoint wrapped in CDATA must also refuse — the
        // Text-only capture dropped CDATA, so a CDATA-encoded DataMashup evaded the comparison.
        let cdata = |host: &str| {
            format!(
                r#"<root><DataMashup><![CDATA[section S; shared Q = Web.Contents("https://{host}/api");]]></DataMashup></root>"#
            )
        };
        let cgood = wb(
            "",
            &[("customXml/item1.xml", cdata("good.example").as_str())],
        );
        assert!(verify_noncell_refs(&cgood, &cgood).is_none());
        let cevil = wb(
            "",
            &[("customXml/item1.xml", cdata("evil.example").as_str())],
        );
        assert_eq!(
            verify_noncell_refs(&cgood, &cevil)
                .expect("a CDATA-wrapped DataMashup repoint must refuse")["reason"],
            "external_target_mismatch"
        );
        // REGRESSION (round-59 defect 6): an ENTITY/char-reference tamper (`420.5` -> `&#57;420.5`
        // = "9420.5") in a customXml value must refuse — element_attr_signatures previously dropped
        // the GeneralRef event and the two bodies signed identically.
        let plain = wb(
            "",
            &[(
                "customXml/item1.xml",
                "<props><amount>420.5</amount></props>",
            )],
        );
        assert!(verify_noncell_refs(&plain, &plain).is_none());
        let entity = wb(
            "",
            &[(
                "customXml/item1.xml",
                "<props><amount>&#57;420.5</amount></props>",
            )],
        );
        assert_eq!(
            verify_noncell_refs(&plain, &entity)
                .expect("an entity-encoded customXml value tamper must refuse")["reason"],
            "external_target_mismatch"
        );
    }

    #[test]
    fn opaque_part_namespace_prefix_rename_is_not_refused() {
        // REGRESSION (round-60 defect 8, over-refusal): element_attr_signatures is meant to be
        // namespace-prefix-agnostic, but a namespace DECLARATION (`xmlns:foo`) was enumerated as an
        // ordinary attribute and local_of leaked the prefix ("foo=uri"), so a foreign tool that
        // re-serialized an opaque part under a different prefix binding the SAME URI was refused.
        let foo = wb(
            "",
            &[(
                "customXml/item1.xml",
                r#"<c xmlns:foo="uri" foo:attr="v"/>"#,
            )],
        );
        let bar = wb(
            "",
            &[(
                "customXml/item1.xml",
                r#"<c xmlns:bar="uri" bar:attr="v"/>"#,
            )],
        );
        assert!(
            verify_noncell_refs(&foo, &bar).is_none(),
            "a pure namespace-prefix rename (same URI, same local names) must still certify"
        );
        // A genuine value tamper is still caught (the fix did not blind the comparison).
        let evil = wb(
            "",
            &[(
                "customXml/item1.xml",
                r#"<c xmlns:bar="uri" bar:attr="TAMPERED"/>"#,
            )],
        );
        assert!(
            verify_noncell_refs(&foo, &evil).is_some(),
            "a customXml attribute-value tamper must still refuse"
        );
    }

    #[test]
    fn connections_datasource_child_swap_is_caught() {
        // REGRESSION (round-59 defect 4, HIGH security): the security-critical data source (dbPr
        // connection/command) lives in a CHILD of <connection>, while the stable handle `id` is on
        // the parent. A flat sorted multiset lost the parent<->child binding, so SWAPPING the <dbPr>
        // source between two <connection id> elements (each keeping its id) certified. The
        // ancestor-path-qualified signature now catches it.
        let conns = |src1: &str, src2: &str| {
            format!(
                r#"<connections><connection id="1" name="A"><dbPr connection="{src1}" command="q1"/></connection><connection id="2" name="B"><dbPr connection="{src2}" command="q2"/></connection></connections>"#
            )
        };
        let good = wb(
            "",
            &[(
                "xl/connections.xml",
                conns("Data Source=public", "Data Source=salaries").as_str(),
            )],
        );
        assert!(verify_noncell_refs(&good, &good).is_none());
        // SWAP the two dbPr sources (connectionId=1 now resolves to the salaries source).
        let swapped = wb(
            "",
            &[(
                "xl/connections.xml",
                conns("Data Source=salaries", "Data Source=public").as_str(),
            )],
        );
        assert_eq!(
            verify_noncell_refs(&good, &swapped)
                .expect("a data-source child swap between connections must refuse")["reason"],
            "external_target_mismatch"
        );
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
        // REGRESSION (round-57 defect 2, HIGH false-certify): the table DATA-BODY extent
        // (headerRowCount/totalsRowCount) re-aggregates every structured-reference formula
        // (`Table1[Col]` resolves to rows [top+header .. bottom-totals]) but was in no signature.
        let counted = |header: &str, totals: &str| {
            format!(
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="T1" displayName="T1" ref="A1:A5" headerRowCount="{header}" totalsRowCount="{totals}"><tableColumns count="1"><tableColumn id="1" name="Amt"/></tableColumns></table>"#
            )
        };
        let base = wb("", &[("xl/tables/table1.xml", &counted("1", "1"))]);
        let totals_flip = wb("", &[("xl/tables/table1.xml", &counted("1", "0"))]);
        assert_eq!(
            verify_noncell_refs(&base, &totals_flip).expect("a totalsRowCount flip must refuse")
                ["reason"],
            "table_reference_mismatch"
        );
        let header_flip = wb("", &[("xl/tables/table1.xml", &counted("0", "1"))]);
        assert_eq!(
            verify_noncell_refs(&base, &header_flip).expect("a headerRowCount flip must refuse")
                ["reason"],
            "table_reference_mismatch"
        );
        // A foreign tool writing the DEFAULT counts explicitly is not over-refused.
        let explicit_default = wb(
            "",
            &[(
                "xl/tables/table1.xml",
                &tbl("A1:B2", "B1*2").replace(
                    r#"ref="A1:B2""#,
                    r#"ref="A1:B2" headerRowCount="1" totalsRowCount="0""#,
                ),
            )],
        );
        assert!(verify_noncell_refs(&good, &explicit_default).is_none());
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
        let keys: Vec<String> = structural_ref_attrs(&bytes, "")
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
        assert_eq!(
            structural_ref_attrs(&form_a, ""),
            structural_ref_attrs(&form_b, "")
        );
        // A genuine external retarget still differs (the equivalence must not blur real swaps).
        let external = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="https://evil.example/x" TargetMode="External"/></Relationships>"#,
            )],
        );
        assert_ne!(
            structural_ref_attrs(&form_a, ""),
            structural_ref_attrs(&external, "")
        );
    }

    #[test]
    fn hyperlink_dest_sheet_quote_is_normalized() {
        // REGRESSION (round-54 defect 4, over-refusal): the hyperlink DEST was the one reference
        // surface missing sheet-quote normalization, so a faithful edit that quotes the sheet name
        // of the SAME destination (`'My Data'!A8` vs the rel form `#My Data!A8`, or `'Data'!A8` vs
        // `Data!A8`) was refused. All encodings of one destination must fold to one key.
        let loc = |dest: &str| {
            wb(
                &format!(r#"<hyperlinks><hyperlink ref="A4" location="{dest}"/></hyperlinks>"#),
                &[],
            )
        };
        let rel = |target: &str| {
            wb(
                r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" r:id="rIdH"/></hyperlinks>"#,
                &[(
                    "xl/worksheets/_rels/sheet2.xml.rels",
                    &format!(
                        r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="#{target}"/></Relationships>"##
                    ),
                )],
            )
        };
        // Quoted vs unquoted space-bearing sheet name — same destination.
        assert_eq!(
            structural_ref_attrs(&loc("'My Data'!A8"), ""),
            structural_ref_attrs(&loc("My Data!A8"), "")
        );
        // Redundantly-quoted simple name vs bare, and location-form vs rel-target-form.
        assert_eq!(
            structural_ref_attrs(&loc("'Data'!A8"), ""),
            structural_ref_attrs(&rel("Data!A8"), "")
        );
        // SOUNDNESS: a genuinely different sheet or cell still differs.
        assert_ne!(
            structural_ref_attrs(&loc("'My Data'!A8"), ""),
            structural_ref_attrs(&loc("'Other'!A8"), "")
        );
        assert_ne!(
            structural_ref_attrs(&loc("'My Data'!A8"), ""),
            structural_ref_attrs(&loc("'My Data'!A9"), "")
        );
    }

    #[test]
    fn hyperlink_self_file_target_folds_to_internal() {
        // REGRESSION (round-52 defect 3): a THIRD encoding of the same in-workbook jump is a
        // self-referential external Target naming the workbook's OWN file (LibreOffice writes
        // `Target="min.xlsx" TargetMode="External"` + `location="Data!A1"`). Given the workbook's
        // own basename, it must fold to the SAME key as the openpyxl `#Data!A1` / bare-`location`
        // forms, so a faithful cross-tool edit is not over-refused.
        let openpyxl = wb(
            r#"<hyperlinks><hyperlink ref="A4" location="Data!A1"/></hyperlinks>"#,
            &[],
        );
        let libre = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" location="Data!A1" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="min.xlsx" TargetMode="External"/></Relationships>"#,
            )],
        );
        // With the own basename, the self-file Target folds to internal -> keys match.
        assert_eq!(
            structural_ref_attrs(&openpyxl, "min.xlsx"),
            structural_ref_attrs(&libre, "min.xlsx"),
            "self-file external Target must fold to the internal jump"
        );
        assert!(verify_noncell_refs_named(&openpyxl, &libre, "min.xlsx", "min.xlsx").is_none());

        // SOUNDNESS: the fold is name-gated. A Target naming a DIFFERENT workbook (`other.xlsx`)
        // stays external and still differs — a real retarget is never blurred to internal.
        let other = wb(
            r#"<hyperlinks><hyperlink xmlns:r="urn:r" ref="A4" location="Data!A1" r:id="rIdH"/></hyperlinks>"#,
            &[(
                "xl/worksheets/_rels/sheet2.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="x/hyperlink" Target="other.xlsx" TargetMode="External"/></Relationships>"#,
            )],
        );
        assert_ne!(
            structural_ref_attrs(&openpyxl, "min.xlsx"),
            structural_ref_attrs(&other, "min.xlsx"),
            "a target to a DIFFERENT workbook must NOT fold to internal"
        );
        // And a path-qualified target that merely ends in the own name is NOT folded (could be a
        // different directory) — conservative fail-safe, never a false certify.
        assert!(!hyperlink_target_is_own_file("../min.xlsx", "min.xlsx"));
        assert!(!hyperlink_target_is_own_file(
            "file:///x/min.xlsx",
            "min.xlsx"
        ));
        assert!(hyperlink_target_is_own_file("min.xlsx", "min.xlsx"));
        // Unknown own-name (empty) never folds.
        assert!(!hyperlink_target_is_own_file("min.xlsx", ""));
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
        assert!(structural_ref_attrs(&clean, "").is_empty());
        // Attacker injects a PREFIXED hyperlink (x bound to the main ns) with an external target.
        let evil = wb(
            &format!(
                r#"<x:hyperlinks xmlns:x="{main}" xmlns:r="{r}"><x:hyperlink ref="A1" r:id="rId100"/></x:hyperlinks>"#
            ),
            &[evil_rels],
        );
        assert!(
            !structural_ref_attrs(&evil, "").is_empty(),
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
            structural_ref_attrs(&evil, ""),
            structural_ref_attrs(&plain, ""),
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
