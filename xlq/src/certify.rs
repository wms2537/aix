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

    // A `cached_value` difference is treated as benign because Excel recomputes formula
    // caches on load. That assumption breaks if the foreign file EXPLICITLY disables
    // recalc-on-load (`<calcPr fullCalcOnLoad="0">`): a fabricated cache would then be shown
    // verbatim. (Benign tools like openpyxl omit the attribute, so this does not over-refuse
    // them.) When recalc is explicitly off and caches differ, the difference is disqualifying.
    let mut disqualifying = counts.formula + counts.value + counts.added + counts.removed;
    if counts.cached_value > 0 && recalc_on_load_disabled(&edited_bytes) {
        disqualifying += counts.cached_value;
    }
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
    // The `_xlfn.` prefix on post-2007 functions is REQUIRED in the persisted format but is
    // stripped by the engine on load, so the cell diff (over the loaded model) cannot see a
    // foreign edit that drops it — which makes Excel render `#NAME?`. Compare the stored
    // prefixed function tokens per sheet.
    if xlfn_tokens_all(expected) != xlfn_tokens_all(edited) {
        return Some(json!({
            "status": "REFUSED",
            "reason": "xlfn_prefix_mismatch",
            "detail": "a required _xlfn. function prefix (post-2007 function) was added or \
                       dropped — the stored formula differs from xlq's transform",
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
        || low.starts_with("xl/vbaproject") // macro binary — byte-compared for a swap
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

/// The `_xlfn.` prefixed function tokens across every worksheet, keyed by sheet, sorted.
fn xlfn_tokens_all(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (sheet_name, part) in sheets {
        if let Ok(x) = crate::ooxml::read_part(bytes, &part) {
            for tok in structural::xlfn_tokens(&x) {
                out.push((sheet_name.clone(), tok));
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

/// True if `xl/workbook.xml`'s `<calcPr>` EXPLICITLY disables recalc-on-load
/// (`fullCalcOnLoad="0"`/`"false"`) — the signal an attacker sets so a fabricated formula
/// cache is displayed without recomputation. Absence (the benign default for most tools)
/// returns false, so a normal cache-dropping edit is not over-refused.
fn recalc_on_load_disabled(bytes: &[u8]) -> bool {
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
    // Recalc-on-load is off if it is explicitly disabled OR the workbook is in manual calc
    // mode — Excel shows stored formula caches verbatim in both cases, so a fabricated cache
    // would be displayed. (`autoNoTable` still recomputes ordinary cells, so it is not off.)
    matches!(
        attr(tag, "fullCalcOnLoad").as_deref(),
        Some("0") | Some("false")
    ) || attr(tag, "calcMode").as_deref() == Some("manual")
}

/// The relationship-id value of a start tag: a namespace-prefixed `*:id="..."` attribute.
/// The relationships-namespace prefix is arbitrary (`r:id`, `x:id`, `r2:id`), so we match
/// by LOCAL name — a literal `r:id` lookup let a rebound prefix hide a hyperlink's target.
fn attr_relid(tag: &str) -> Option<String> {
    let i = tag.find(":id=")? + ":id=".len();
    let q = *tag.as_bytes().get(i)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let rest = &tag[i + 1..];
    let end = rest.find(q as char)?;
    Some(rest[..end].to_string())
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
    fn table_part_is_refused_by_verify_noncell_refs() {
        // A table part carries `ref` + value-bearing `calculatedColumnFormula` certify does
        // not positionally compare — a foreign tool that drops/mangles it while shifting
        // cells must not be certified.
        let bytes = wb(
            "",
            &[(
                "xl/tables/table1.xml",
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="T1" displayName="T1" ref="A1:B2"><tableColumns count="1"><tableColumn id="1" name="A"/></tableColumns></table>"#,
            )],
        );
        let refusal = verify_noncell_refs(&bytes, &bytes).expect("table must refuse");
        assert_eq!(refusal["status"], "REFUSED");
        assert_eq!(refusal["reason"], "unverified_reference_part");
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
