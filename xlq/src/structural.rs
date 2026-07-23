//! Surgical STRUCTURAL edits (insert/delete row/column) via the reference-shift
//! algebra σ (refshift.rs). The minimal-patch invariant: the only bytes that
//! differ from the input are reference coordinates that the edit provably
//! shifts (by exactly the shift delta) plus the physically inserted/removed
//! rows/cells. Every non-coordinate byte of every part is identical.
//!
//! Attribute rewrites are done by RAW byte surgery on the tag's inner bytes
//! (replace_attr_value): only the target attribute's value substring changes,
//! so sibling attributes and quoting stay byte-identical — the invariant holds
//! at the tag level, not just the part level.
//!
//! Residuals it cannot guarantee to express as a coordinate shift (shared/array
//! formulas — refused on PRESENCE, a sound conservative gate — plus table parts
//! and 3D spans not anchored on the edited sheet) are REPORTED; the command
//! layer refuses to commit when any residual is present, so a subtly-wrong file
//! is never produced. This is the "shift-correctly-or-refuse" discipline that
//! keeps the never-silently-wrong invariant honest.

use crate::ooxml;
use crate::refshift::{self, Axis, Op, Shift, StructuralEdit};
use anyhow::{anyhow, Result};
use quick_xml::events::{BytesRef, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

#[derive(Debug, Default, Clone)]
pub struct StructuralReport {
    pub refs_shifted: u32,
    pub ref_errors: u32,
    pub rows_inserted: u32,
    pub rows_deleted: u32,
    pub parts_touched: Vec<String>,
    pub residuals: Vec<Residual>,
}

#[derive(Debug, Clone)]
pub struct Residual {
    pub part: String,
    pub reason: String,
    pub detail: String,
}

/// Perform a structural edit and return (new_bytes, report).
pub fn structural_edit(input: &[u8], edit: &StructuralEdit) -> Result<(Vec<u8>, StructuralReport)> {
    // Move is defined only on the row axis (the buffered reorder is a row
    // permutation). A column Move is not reachable from the CLI; reject it
    // defensively rather than silently mis-transform.
    if edit.op == Op::Move && edit.axis != Axis::Row {
        return Err(anyhow!("move is only supported on the row axis"));
    }
    let sheets = ooxml::all_sheets(input)?;
    let edited_part = sheets
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&edit.sheet))
        .map(|(_, p)| p.clone())
        .ok_or_else(|| anyhow!("no sheet named {}", edit.sheet))?;
    let part_sheet: BTreeMap<String, String> =
        sheets.iter().map(|(n, p)| (p.clone(), n.clone())).collect();
    // Sheet names in workbook order — a definedName's `localSheetId` is a 0-based index here.
    let sheet_names: Vec<String> = sheets.iter().map(|(n, _)| n.clone()).collect();
    // Map each drawing part -> the worksheet it floats over, so a live cell reference inside a
    // drawing (a linked shape's `textlink`, a graphic-frame `<xdr:f>`) can scope its UNQUALIFIED
    // references to that sheet when shifted. Built by inverting each worksheet's `/drawing`
    // relationship.
    let drawing_owner = drawing_owner_sheets(input, &part_sheet);

    let mut archive =
        zip::ZipArchive::new(Cursor::new(input)).map_err(|e| anyhow!("open zip: {e}"))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let base_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    let mut report = StructuralReport::default();

    // Conservative pre-scan: constructs the current σ-application cannot
    // guarantee to shift correctly must be REFUSED, never silently corrupted.
    // (a) Tables carry an extent + structured refs we do not yet rewrite.
    // (b) A 3D span not anchored on the edited sheet may cover it as an interior
    //     tab, which we cannot verify. Both are reported as residuals so the
    //     command layer declines the edit — preserving "never silently wrong".
    scan_extra_residuals(
        &archive_names(input)?,
        input,
        edit,
        &edited_part,
        &sheet_names,
        &mut report,
    );

    // One decompression budget across the whole workbook. The declared
    // uncompressed size is attacker-controlled; read_entry_capped bounds BOTH the
    // reservation (defeats the over-allocation attack) AND the actual decompressed
    // length (defeats the real bomb — the old .min(8<<20) clamped only the former,
    // so read_to_end still expanded the whole entry unbounded).
    let mut budget = crate::ooxml::total_cap();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| anyhow!("zip entry: {e}"))?;
        let name = file.name().to_string();
        // Drop rebuildable dependency caches whose cell coordinates would otherwise go stale;
        // Excel rebuilds both on open. (volatileDependencies is the volatile/RTD analog of
        // calcChain — value-inert, so dropping never changes a computed result.)
        if name == "xl/calcChain.xml" || name == "xl/volatileDependencies.xml" {
            continue;
        }
        if file.is_dir() {
            writer
                .add_directory(name, base_opts)
                .map_err(|e| anyhow!("dir: {e}"))?;
            continue;
        }
        let sz = file.size();
        let mut bytes = crate::ooxml::read_entry_capped(file, sz, &name, &mut budget)?;

        // `touched` is derived from CONTENT, not from a shift counter: any part
        // whose bytes actually change must be reported in `parts_touched`, even
        // if it shifted zero references (e.g. a foreign sheet whose only change
        // was shared-formula expansion). Reporting fewer parts than we rewrote
        // would be silently-wrong — the exact property this tool must not have.
        let before = if name == edited_part {
            Vec::new()
        } else {
            bytes.clone()
        };
        if name == edited_part {
            // FAIL-CLOSED (round-53 defect 8): the streaming row insert/delete rewrite matches
            // `<row>`/`<sheetData>` by FULL qualified name, but transform_tag shifts a cell `<c>` by
            // LOCAL name. So a worksheet that binds the spreadsheetML namespace to a PREFIX
            // (`<x:worksheet …><x:row><x:c>`) would have its CELL coordinates shifted while its ROW
            // elements stay stale and the blank-row/delete logic is skipped — a silent value
            // corruption the reopen check does not catch. A fully prefix-aware rewrite would touch
            // the whole hot path (rewrite_edited_sheet[_move], maybe_inject, inject_blanks_at_end,
            // delete_skip, expand_shared_in_sheet, strip_formula_caches, insert_overflows_grid); no
            // real spreadsheet tool emits a prefixed main namespace, so we refuse this pathological
            // encoding rather than risk the common path — never silently wrong.
            if edited_sheet_has_prefixed_grid(&bytes) {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "namespace_prefixed_worksheet".into(),
                    detail: "the edited worksheet binds the spreadsheetML namespace to a PREFIX \
                             (<x:row>/<x:c>); xlq's row insert/delete rewrite is not prefix-aware, \
                             so committing it would shift cells without shifting their rows — edit \
                             refused (fail-closed)"
                        .into(),
                });
            }
            // FAIL-CLOSED (round-59): the `r` attribute on `<row>` is OPTIONAL in OOXML (the index is
            // implied by position). The row insert/delete surgery keys entirely on `attr_u32(row, r)`
            // — a row without `r` is neither renumbered, injected-around, nor dropped — while the
            // per-cell `r` shift still fires, so a DELETE leaves the target row's data in place while
            // cells below shift onto it (duplicate coordinates), and an INSERT mixes positional rows
            // with explicit-`r` blanks. IronCalc's reopen silently overwrites the duplicate, so the
            // corrupt file commits. Refuse rather than renumber (which would change the surgical
            // writer's contract). Row-axis Insert/Delete only — a column edit / move does not renumber
            // rows. (An empty sheetData has no such row, so a data-free workbook is unaffected.)
            if edit.axis == Axis::Row
                && matches!(edit.op, Op::Insert | Op::Delete)
                && edited_sheet_has_row_without_r(&bytes)
            {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "row_without_r_ref".into(),
                    detail: "the edited worksheet has a <row> with no `r` (row-index) attribute \
                             (positionally implied); the streaming row insert/delete surgery cannot \
                             express the edit against implicit rows — edit refused (fail-closed)"
                        .into(),
                });
            }
            // Materialize shared formulas so σ shifts them uniformly, then run
            // the row/cell coordinate + formula surgery on the explicit sheet.
            let expanded = expand_shared_in_sheet(&bytes)?;
            // Re-target autoFilter `<filterColumn colId>` (a column offset the ref shift leaves
            // stale) BEFORE the ref itself is shifted by rewrite_edited_sheet, using the same
            // shift_sqref so the two stay consistent.
            let (expanded, afn, afe) = shift_autofilter_columns(&expanded, &edit.sheet, edit)?;
            report.refs_shifted += afn;
            report.ref_errors += afe;
            bytes = rewrite_edited_sheet(&expanded, edit, &name, &mut report)?;
            // A hyperlink's `location` (its destination) is not touched by rewrite_edited_sheet
            // (which shifts the `ref` it sits on); shift it here so a link into a moved cell
            // follows. Host is the edited sheet, so an unqualified local location shifts too.
            let (out, n, r) = shift_hyperlink_locations(&bytes, &edit.sheet, edit)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
        } else if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            // Only touch a foreign sheet if it cross-references the edited sheet.
            // Sheets that do not stay byte-identical (unexpanded) — a shared formula
            // there must NOT trigger a spurious change.
            //
            // Expand shared formulas BEFORE the gate: a shared-formula DEPENDENT carries
            // its own position-offset reference, so its cross-reference to the edited sheet
            // can cross the edit boundary even when the MASTER body does not. Gating on the
            // master body alone would leave those dependents silently stale. Over the
            // EXPLICIT (expanded) formulas the σ oracle sees each dependent's real
            // reference. The gate is the sound σ oracle (not a substring scan), so an
            // entity/case/3D-span-encoded cross-ref cannot slip past either.
            let expanded = expand_shared_in_sheet(&bytes)?;
            if foreign_sheet_needs_shift(&expanded, edit) {
                let host = part_sheet.get(&name).cloned().unwrap_or_default();
                let (out, n, r, qrisk) = shift_text_in_element(&expanded, b"f", edit, &host)?;
                bytes = out;
                report.refs_shifted += n;
                report.ref_errors += r;
                if qrisk {
                    report.residuals.push(Residual {
                        part: name.clone(),
                        reason: "non_ascii_sheet_qualifier".into(),
                        detail: "unquoted non-ASCII sheet qualifier in a cross-sheet formula \
                                 — edit refused (fail-closed)"
                            .into(),
                    });
                }
            }
            // A hyperlink on this foreign sheet whose `location` is qualified to the edited
            // sheet moves too — shift it (host = this sheet, so its own unqualified links stay
            // local and untouched). Runs regardless of the `<f>` gate: a sheet whose ONLY
            // cross-reference is such a hyperlink still needs it shifted.
            let host = part_sheet.get(&name).cloned().unwrap_or_default();
            let (out, n, r) = shift_hyperlink_locations(&bytes, &host, edit)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            // Legacy conditional-formatting / data-validation formula bodies
            // (`<formula>`/`<formula1>`/`<formula2>`) on a foreign sheet can reference the edited
            // sheet (a dashboard CF rule `Data!$A$11>50`); shift them like `<f>` (host = this sheet,
            // so a foreign-LOCAL ref stays put and only an edited-sheet-qualified ref moves). Runs
            // regardless of the `<f>` gate — a sheet whose ONLY cross-reference is such a formula
            // still needs it shifted (else it is left stale, the over-refusal these used to trip).
            for tag in [b"formula".as_slice(), b"formula1", b"formula2"] {
                let (out, n, r, qrisk) = shift_text_in_element(&bytes, tag, edit, &host)?;
                bytes = out;
                report.refs_shifted += n;
                report.ref_errors += r;
                if qrisk {
                    report.residuals.push(Residual {
                        part: name.clone(),
                        reason: "non_ascii_sheet_qualifier".into(),
                        detail: "unquoted non-ASCII sheet qualifier in a cross-sheet \
                                 conditional-formatting / data-validation formula — edit refused \
                                 (fail-closed)"
                            .into(),
                    });
                }
            }
        } else if name == "xl/workbook.xml" {
            let (out, n, r, qrisk) = shift_defined_names(&bytes, edit, &sheet_names)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            if qrisk {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "non_ascii_sheet_qualifier".into(),
                    detail: "unquoted non-ASCII sheet qualifier in a defined name — edit \
                             refused (fail-closed)"
                        .into(),
                });
            }
        } else if name.starts_with("xl/charts/") && name.ends_with(".xml") {
            let (out, n, r, qrisk) = shift_text_in_element(&bytes, b"f", edit, "")?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            if qrisk {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "non_ascii_sheet_qualifier".into(),
                    detail: "unquoted non-ASCII sheet qualifier in a chart formula — edit \
                             refused (fail-closed)"
                        .into(),
                });
            }
        } else if (name.starts_with("xl/pivotCache/") || name.starts_with("xl/pivotTables/"))
            && name.ends_with(".xml")
        {
            let (out, n, r, unhandled) = rewrite_pivot(&bytes, edit)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            if unhandled {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "pivot_source_unsupported".into(),
                    detail: "a pivot cache source other than a worksheetSource (e.g. a \
                             consolidation rangeSet) references the edited sheet; its grid range \
                             is not shifted — edit refused (fail-closed)"
                        .into(),
                });
            }
        } else if name.starts_with("xl/drawings/") && name.ends_with(".xml") {
            // A drawing floats over one sheet but can carry a LIVE cell reference — a linked
            // shape's `textlink="Sheet1!$A$8"` or a graphic-frame `<xdr:f>` — that mirrors a
            // cell's value. If this edit MOVES that cell, the reference must follow or the shape
            // silently shows a DIFFERENT cell's value. Host = the drawing's OWNING sheet, so an
            // unqualified `$A$8` (local to the sheet the drawing floats over) shifts while a
            // reference to an unrelated sheet is left untouched. An edit on one sheet never moves
            // a drawing's ANCHOR on another sheet, so a foreign drawing needs only its refs
            // shifted; the edited sheet's own drawing anchor is guarded by edited_sheet_bad_attachment.
            let host = drawing_owner.get(&name).cloned().unwrap_or_default();
            let (out, n, r, qrisk) = shift_drawing_refs(&bytes, &host, edit)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            if qrisk {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "non_ascii_sheet_qualifier".into(),
                    detail: "unquoted non-ASCII sheet qualifier in a drawing reference — edit \
                             refused (fail-closed)"
                        .into(),
                });
            }
        } else if name == "xl/_rels/workbook.xml.rels" && mentions_dropped_part(&bytes) {
            // The calcChain / volatileDependencies parts are DROPPED above; prune their
            // workbook-rels <Relationship> too, else the package carries a dangling relationship
            // (OPC-invalid — strict readers reject it and Excel flags the file for repair).
            bytes = prune_relationships_to_dropped(&bytes);
        } else if name == "[Content_Types].xml" && mentions_dropped_part(&bytes) {
            // ...and prune the matching <Override> so no orphaned content-type declaration remains.
            bytes = prune_content_type_overrides_of_dropped(&bytes);
        }
        // Engine-free xlq cannot recompute a formula result, and a structural edit changes
        // computed values transitively across the workbook, so every stored formula cache is now
        // untrustworthy. Invalidate them on EVERY worksheet — the edited sheet, cross-referencing
        // sheets, AND sheets this edit did not otherwise touch (a transitively-affected value can
        // live on any of them) — so no reader ever sees a stale computed value. `part_sheet` is
        // exactly the set of worksheet parts (resolved via the workbook rels), so this is robust
        // to non-standard part paths.
        if part_sheet.contains_key(&name) {
            bytes = strip_formula_caches(&bytes);
        }
        let touched = name != edited_part && bytes != before;
        if touched {
            report.parts_touched.push(name.clone());
        }

        writer
            .start_file(&name, base_opts)
            .map_err(|e| anyhow!("start {name}: {e}"))?;
        writer
            .write_all(&bytes)
            .map_err(|e| anyhow!("write {name}: {e}"))?;
    }
    // Move straddle safety net: under Move a #REF! can ONLY arise from a range
    // that reorders across the move boundary (σ is a total bijection on single
    // cells, so no single cell errors). Any ref error therefore means a straddle
    // the coordinate shift cannot express — refuse (fail-closed) even if it lived
    // in a cross-sheet formula / chart / defined name / pivot the per-sheet scan
    // above did not already flag.
    if edit.op == Op::Move
        && report.ref_errors > 0
        && !report
            .residuals
            .iter()
            .any(|r| r.reason == "move_straddles_range")
    {
        report.residuals.push(Residual {
            part: "(workbook)".into(),
            reason: "move_straddles_range".into(),
            detail: "a range reference reorders under the move (σ(head) > σ(tail)) in a \
                     cross-sheet / chart / defined-name / pivot reference — edit refused"
                .into(),
        });
    }

    report.parts_touched.insert(0, edited_part);
    let cur = writer.finish().map_err(|e| anyhow!("finalize: {e}"))?;
    Ok((cur.into_inner(), report))
}

/// Parts `structural_edit` DROPS (Excel rebuilds them on open). When a part is dropped its
/// workbook-rels `<Relationship>` and `[Content_Types]` `<Override>` must be pruned too, or the
/// package is left with a dangling relationship / orphaned override (OPC-invalid).
const DROPPED_PARTS: &[&str] = &["xl/calcChain.xml", "xl/volatileDependencies.xml"];

/// Cheap gate: do these bytes even mention a dropped part's basename? Avoids re-serializing
/// workbook.xml.rels / [Content_Types].xml (and marking them touched) on the common workbook that
/// has neither part.
fn mentions_dropped_part(bytes: &[u8]) -> bool {
    let hay = String::from_utf8_lossy(bytes);
    DROPPED_PARTS
        .iter()
        .any(|p| p.rsplit('/').next().is_some_and(|base| hay.contains(base)))
}

/// Stream `xml`, dropping every top-level element whose LOCAL name is `local` and for which
/// `drop_it` returns true (an OOXML `<Relationship>`/`<Override>` is always self-closing, but the
/// Start..End form is handled too). On a parse error the ORIGINAL bytes are returned unchanged
/// (fail-safe: never corrupt a part we could not fully parse).
fn prune_elements(xml: &[u8], local: &[u8], drop_it: impl Fn(&BytesStart) -> bool) -> Vec<u8> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut skip_name: Vec<u8> = Vec::new();
    let mut skip_depth = 0u32;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e))
                if skip_depth == 0 && tag_local_eq(e.name().as_ref(), local) && drop_it(&e) => {}
            Ok(Event::Start(e))
                if skip_depth == 0 && tag_local_eq(e.name().as_ref(), local) && drop_it(&e) =>
            {
                skip_name = e.name().as_ref().to_vec();
                skip_depth = 1;
            }
            Ok(Event::Start(e)) if skip_depth > 0 && e.name().as_ref() == skip_name.as_slice() => {
                skip_depth += 1;
            }
            Ok(Event::End(e)) if skip_depth > 0 && e.name().as_ref() == skip_name.as_slice() => {
                skip_depth -= 1;
            }
            Ok(_) if skip_depth > 0 => {}
            Ok(Event::Eof) => break,
            Ok(other) => {
                let _ = writer.write_event(other);
            }
            Err(_) => return xml.to_vec(),
        }
        buf.clear();
    }
    writer.into_inner().into_inner()
}

/// Remove workbook-rels `<Relationship>`s whose `Target` resolves (relative to `xl/`) to a dropped
/// part.
fn prune_relationships_to_dropped(xml: &[u8]) -> Vec<u8> {
    prune_elements(xml, b"Relationship", |e| {
        attr_by_local(e, b"Target")
            .map(|t| crate::ooxml::resolve_target("xl", &t))
            .is_some_and(|p| DROPPED_PARTS.contains(&p.as_str()))
    })
}

/// Remove `[Content_Types]` `<Override>`s whose `PartName` is a dropped part.
fn prune_content_type_overrides_of_dropped(xml: &[u8]) -> Vec<u8> {
    prune_elements(xml, b"Override", |e| {
        attr_by_local(e, b"PartName").is_some_and(|p| {
            let norm = p.trim_start_matches('/');
            DROPPED_PARTS.contains(&norm)
        })
    })
}

/// True if the edited worksheet binds the spreadsheetML MAIN namespace to a PREFIX on the grid
/// structure (`<x:sheetData>`/`<x:row>`/`<x:c>`) — which the non-prefix-aware row/insert/delete
/// rewrite cannot handle, so such a sheet is refused upfront (fail-closed). The match set is
/// restricted to `sheetData`/`row`/`c` (NOT `f`/`v`): a genuinely prefix-bound grid ALWAYS carries
/// those, so detection stays complete, while a FOREIGN-namespace element that merely SHARES the
/// local name `f`/`v` — notably the ubiquitous x14 `<xm:f>` (Office `.../excel/2006/main`, in
/// sparklines / cross-sheet dropdowns / x14 CF) — no longer trips a false `namespace_prefixed_
/// worksheet` refusal (round-57 defect 6). (`sheetData`/`row`/`c` have no foreign-namespace
/// homograph in real workbooks, so matching them by local name is safe.)
fn edited_sheet_has_prefixed_grid(xml: &[u8]) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let full = name.as_ref();
                let local = local_of(full);
                // A prefix is present iff the qualified name differs from its local part.
                if full != local && matches!(local, b"sheetData" | b"row" | b"c") {
                    return true;
                }
            }
            // A parse error here is not our concern — the row/cell path handles malformed input
            // elsewhere; report no prefix so we do not mask a different failure mode.
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    false
}

/// True if any `<row>` element inside `<sheetData>` lacks an `r` (row-index) attribute. The index
/// is OOXML-optional (implied by position), but the streaming insert/delete surgery keys entirely on
/// `r`, so a row without it is silently mishandled — refused up front.
fn edited_sheet_has_row_without_r(xml: &[u8]) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut in_sheetdata = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"sheetData") => {
                in_sheetdata = true;
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"sheetData") => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if in_sheetdata && tag_local_eq(e.name().as_ref(), b"row") =>
            {
                if attr_u32(&e, b"r").is_none() {
                    return true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return true, // unparseable -> fail closed
            _ => {}
        }
        buf.clear();
    }
    false
}

/// Parse a cell coordinate like `B5` into (col, row), 1-based.
fn parse_cell_rc(r: &str) -> Option<(u32, u32)> {
    let bytes = r.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let col = refshift::col_to_num(&r[..i])?;
    let row: u32 = r[i..].parse().ok()?;
    Some((col, row))
}

/// True if `v` is (or contains) a worksheet coordinate: a single cell, a colon range,
/// or a sheet-qualified reference — tolerant of `$` anchors. Used to decide whether an
/// element's `r` attribute is a real cell reference the shift engine would need to move.
/// Broader than `parse_cell_rc` on purpose (fail-closed): `$A$8`, `A8:B9`, `Sheet2!A8`.
fn looks_like_cell_or_range(v: &str) -> bool {
    let v = v.trim();
    if v.is_empty() {
        return false;
    }
    // A sheet qualifier makes it a reference regardless of the endpoint spelling.
    if v.contains('!') {
        return true;
    }
    // Otherwise every colon-separated endpoint (with `$` anchors stripped) must be a cell.
    v.split(':')
        .all(|part| parse_cell_rc(&part.replace('$', "")).is_some())
}

/// Materialize shared-formula groups into explicit per-cell formulas so σ can
/// shift them uniformly (what Excel/LibreOffice do around a structural edit).
/// Pass 1 collects each master's (position, body) by shared index; pass 2
/// rewrites the master to a plain `<f>` and every dependent stub to its explicit
/// formula (master body translated by the dependent's offset). Array formulas
/// are NOT expanded (Excel forbids splitting them) — they remain and are refused
/// upstream. Returns the input unchanged if the sheet has no shared formulas.
pub(crate) fn expand_shared_in_sheet(src: &[u8]) -> Result<Vec<u8>> {
    // ---- pass 1: collect masters: si -> (col, row, body) ----
    let mut masters: BTreeMap<String, (u32, u32, String)> = BTreeMap::new();
    {
        let mut reader = Reader::from_reader(src);
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        let mut cur: Option<(u32, u32)> = None;
        let mut pending_si: Option<String> = None;
        // Master body reassembled across Text + GeneralRef (quick-xml >=0.38
        // splits entities out of Text); captured logical at the closing </f>.
        let mut body_acc = String::new();
        loop {
            match reader
                .read_event_into(&mut buf)
                .map_err(|e| anyhow!("shared-formula xml: {e}"))?
            {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"c" => {
                    cur = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"r")
                        .and_then(|a| parse_cell_rc(&String::from_utf8_lossy(&a.value)));
                }
                Event::Start(e) if e.name().as_ref() == b"f" => {
                    let is_shared = e
                        .attributes()
                        .flatten()
                        .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"shared");
                    let has_ref = e.attributes().flatten().any(|a| a.key.as_ref() == b"ref");
                    let si = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"si")
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
                    if is_shared && has_ref {
                        pending_si = si; // master body accumulates until </f>
                        body_acc.clear();
                    }
                }
                Event::Text(t) if pending_si.is_some() => {
                    push_text_raw(&mut body_acc, &t);
                }
                Event::GeneralRef(r) if pending_si.is_some() => {
                    push_ref_raw(&mut body_acc, &r);
                }
                Event::End(e) if e.name().as_ref() == b"f" && pending_si.is_some() => {
                    if let Some((c, r)) = cur {
                        // Reassembled master body -> logical formula.
                        let body = logical_formula(&body_acc).unwrap_or_default();
                        masters.insert(pending_si.take().unwrap(), (c, r, body));
                    } else {
                        pending_si = None;
                    }
                    body_acc.clear();
                }
                _ => {}
            }
            buf.clear();
        }
    }
    if masters.is_empty() {
        return Ok(src.to_vec()); // no shared formulas → byte-identical
    }

    // ---- pass 2: rewrite masters to plain <f>, dependents to explicit <f> ----
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut cur: Option<(u32, u32)> = None;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(e) if e.name().as_ref() == b"c" => {
                cur = cell_pos(&e);
                writer.write_event(Event::Start(e.into_owned()))?;
            }
            Event::Empty(e) if e.name().as_ref() == b"c" => {
                cur = cell_pos(&e);
                writer.write_event(Event::Empty(e.into_owned()))?;
            }
            Event::Start(e) if e.name().as_ref() == b"f" && is_shared_f(&e) => {
                if has_ref_f(&e) {
                    // master: strip attrs, keep body (body Text + End flow through)
                    writer.write_event(Event::Start(BytesStart::new("f")))?;
                } else {
                    // dependent (Start form): emit explicit formula, consume body
                    emit_dependent(&mut writer, &e, cur, &masters)?;
                    reader.read_to_end(e.name())?;
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"f" && is_shared_f(&e) => {
                if !has_ref_f(&e) {
                    emit_dependent(&mut writer, &e, cur, &masters)?;
                } else {
                    // master with no body (degenerate) — keep as-is
                    writer.write_event(Event::Empty(e.into_owned()))?;
                }
            }
            other => writer.write_event(other.into_owned())?,
        }
        buf.clear();
    }
    Ok(writer.into_inner().into_inner())
}

/// Drop every FORMULA cell's stored result cache (`<v>`) from a worksheet part, leaving the
/// `<f>` intact. Engine-free xlq cannot recompute a formula's value, and a structural edit can
/// change computed values TRANSITIVELY across the whole workbook (a deleted data row changes a
/// `SUM`, which changes a cell that reads that `SUM`, on any sheet), so no stored cache can be
/// trusted afterward. Excel/LibreOffice recompute a cache-less formula on load, and a
/// value-reading tool (openpyxl `data_only`, pandas) then reads NO value rather than a STALE one
/// — the same invalidation openpyxl itself performs on save. A literal (non-formula) cell keeps
/// its `<v>`. The current cell's events are buffered so the drop decision holds regardless of
/// child order; byte-identical when the sheet carries no cached formula result. On an
/// unparseable part it returns the input unchanged (fail-safe: never corrupt a part we cannot
/// model).
fn strip_formula_caches(xml: &[u8]) -> Vec<u8> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut cell: Vec<Event<'static>> = Vec::new();
    let mut in_cell = false;
    let mut has_f = false;
    loop {
        let ev = match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(e) => e.into_owned(),
            Err(_) => return xml.to_vec(),
        };
        buf.clear();
        match &ev {
            Event::Start(e) if e.name().as_ref() == b"c" => {
                in_cell = true;
                has_f = false;
                cell.clear();
                cell.push(ev);
            }
            Event::End(e) if in_cell && e.name().as_ref() == b"c" => {
                cell.push(ev);
                let mut skip_v = false;
                for c in cell.drain(..) {
                    if has_f {
                        match &c {
                            Event::Start(s) if s.name().as_ref() == b"v" => {
                                skip_v = true;
                                continue;
                            }
                            Event::Empty(s) if s.name().as_ref() == b"v" => continue,
                            Event::End(s) if s.name().as_ref() == b"v" => {
                                skip_v = false;
                                continue;
                            }
                            Event::Text(_) | Event::GeneralRef(_) if skip_v => continue,
                            _ => {}
                        }
                    }
                    let _ = writer.write_event(c);
                }
                in_cell = false;
            }
            _ if in_cell => {
                if let Event::Start(e) | Event::Empty(e) = &ev {
                    if e.name().as_ref() == b"f" {
                        has_f = true;
                    }
                }
                cell.push(ev);
            }
            _ => {
                let _ = writer.write_event(ev);
            }
        }
    }
    writer.into_inner().into_inner()
}

/// True if the proven shift algebra σ would CHANGE `logical` for this edit — i.e. the
/// formula carries a reference qualified to the edited sheet that this edit moves.
///
/// This is the sound oracle that replaces every substring "does it name the edited
/// sheet?" test: it delegates ALL sheet-name precision to σ itself — case-insensitivity
/// (`eq_sheet`), quoted/apostrophe-escaped names, and 3D-span endpoints (`Sheet1:Sheet3!`)
/// — so no substring/case/entity/span evasion is possible. A raw substring pre-filter over
/// still-escaped XML is unsound for a "never silently wrong" guarantee.
///
/// The host is a phantom sheet name (a NUL, which no real sheet name can be), so an
/// UNqualified reference — which belongs to the formula's own foreign sheet, never the
/// edited one — is never spuriously counted; only a reference explicitly qualified to the
/// edited sheet does.
fn formula_would_shift(logical: &str, edit: &StructuralEdit) -> bool {
    let (shifted, _n) = refshift::shift_formula(logical, "\u{0}", edit);
    shifted != logical
}

/// Sound replacement for the old substring `references_sheet` gate: does any `<f>` body in
/// this foreign worksheet carry a reference σ would actually shift for THIS edit? Each body
/// is reassembled across Text + `GeneralRef` (resolving XML entities via `logical_formula`)
/// before the σ oracle runs, so an entity-encoded qualifier (`Data&#33;A5`), a case variant
/// (`SHEET1!`), or a 3D span whose first endpoint is the edited sheet (`Sheet1:Sheet3!`) can
/// no longer hide a live cross-reference. Fail closed on a parse error.
fn foreign_sheet_needs_shift(bytes: &[u8], edit: &StructuralEdit) -> bool {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut in_f = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = true;
                raw.clear();
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                if in_f {
                    match logical_formula(&raw) {
                        Some(logical) if formula_would_shift(&logical, edit) => return true,
                        _ => {}
                    }
                    in_f = false;
                }
            }
            Ok(Event::Text(t)) if in_f => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if in_f => push_ref_raw(&mut raw, &r),
            Ok(Event::Eof) => return false,
            // Unparseable: fail closed by attempting the shift (which is itself fail-closed).
            Err(_) => return true,
            _ => {}
        }
        buf.clear();
    }
}

fn cell_pos(e: &BytesStart) -> Option<(u32, u32)> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"r")
        .and_then(|a| parse_cell_rc(&String::from_utf8_lossy(&a.value)))
}

/// True if `e` is a `<c>` cell whose COLUMN falls inside a column-delete's band — such a
/// cell's content must be dropped (not just coordinate-shifted), or it is left stale and an
/// interior delete emits duplicate coordinates.
fn cell_col_deleted(e: &BytesStart, edit: &StructuralEdit) -> bool {
    e.name().as_ref() == b"c"
        && cell_pos(e).is_some_and(|(col, _row)| col >= edit.at && col < edit.at + edit.count)
}

/// True if `e` is a `has_ref_attr` element whose entire `ref`/`sqref` is consumed by a
/// delete — the element (mergeCell / dataValidation / conditionalFormatting / …) must be
/// DROPPED, or shifting its range to the empty string emits a malformed `ref=""`/`sqref=""`
/// that triggers Excel's repair.
fn ref_fully_consumed(e: &BytesStart, sheet: &str, edit: &StructuralEdit) -> bool {
    if edit.op != Op::Delete || !has_ref_attr(e.name().as_ref()) {
        return false;
    }
    e.attributes().flatten().any(|a| {
        let k = local_of(a.key.as_ref());
        if k != b"ref" && k != b"sqref" {
            return false;
        }
        let val = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .unwrap_or_default();
        shift_sqref(&val, sheet, edit).3 // the all-consumed flag
    })
}
fn is_shared_f(e: &BytesStart) -> bool {
    e.attributes()
        .flatten()
        .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"shared")
}
fn is_array_f(e: &BytesStart) -> bool {
    e.attributes()
        .flatten()
        .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"array")
}
fn has_ref_f(e: &BytesStart) -> bool {
    e.attributes().flatten().any(|a| a.key.as_ref() == b"ref")
}

fn emit_dependent(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    e: &BytesStart,
    cur: Option<(u32, u32)>,
    masters: &BTreeMap<String, (u32, u32, String)>,
) -> Result<()> {
    let si = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"si")
        .map(|a| String::from_utf8_lossy(&a.value).into_owned());
    if let (Some(si), Some((cc, cr))) = (si, cur) {
        if let Some((mc, mr, body)) = masters.get(&si) {
            let dr = cr as i64 - *mr as i64;
            let dc = cc as i64 - *mc as i64;
            let explicit = refshift::offset_formula(body, dr, dc);
            writer.write_event(Event::Start(BytesStart::new("f")))?;
            writer.write_event(Event::Text(BytesText::from_escaped(text_escape(&explicit))))?;
            writer.write_event(Event::End(quick_xml::events::BytesEnd::new("f")))?;
            return Ok(());
        }
    }
    // no master found: keep the stub verbatim (safety — upstream will refuse)
    writer.write_event(Event::Empty(e.to_owned()))?;
    Ok(())
}

pub(crate) fn archive_names(input: &[u8]) -> Result<Vec<String>> {
    let mut a = zip::ZipArchive::new(Cursor::new(input)).map_err(|e| anyhow!("zip: {e}"))?;
    Ok((0..a.len())
        .filter_map(|i| a.by_index(i).ok().map(|f| f.name().to_string()))
        .collect())
}

/// Populate residuals for constructs the current implementation cannot safely
/// shift: table parts (unsupported extent/structured refs) and 3D spans not
/// anchored on the edited sheet (interior-tab shift is unverifiable).
/// Value of `name="…"` (or `'…'`) in a raw XML tag fragment.
/// Resolve an OOXML relationship `Target` against the directory of the part that
/// declares it: base `xl/worksheets` + `../tables/table1.xml` -> `xl/tables/table1.xml`.
/// A leading `/` means package-rooted.
fn resolve_rel_target(base_dir: &str, target: &str) -> Option<String> {
    if let Some(abs) = target.strip_prefix('/') {
        return Some(abs.to_string());
    }
    let mut segs: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segs.pop()?;
            }
            s => segs.push(s),
        }
    }
    Some(segs.join("/"))
}

/// A start-tag attribute value matched by LOCAL name (namespace-prefix-insensitive),
/// with XML entities resolved — a raw read would let `location="Sheet1&#33;A11"`
/// (`!` written as `&#33;`) evade a `!`/name scan.
fn attr_by_local(e: &BytesStart, local: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| tag_local_eq(a.key.as_ref(), local))
        .map(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned())
        })
}

/// Whether any element in `xml` has the given LOCAL name (namespace-prefix- and
/// encoding-insensitive — a substring scan would miss `<x:tableParts>` etc.).
/// `unreadable` is returned when the part cannot be parsed (fail-closed callers).
pub(crate) fn xml_has_local_element(xml: &[u8], locals: &[&[u8]], unreadable: bool) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if locals.iter().any(|l| tag_local_eq(e.name().as_ref(), l)) {
                    return true;
                }
            }
            Ok(Event::Eof) => return false,
            Err(_) => return unreadable,
            _ => {}
        }
        buf.clear();
    }
}

/// Table part paths declared by one `.rels` part, resolved against the directory of
/// the part that OWNS those relationships. Namespace-aware: matches the `Relationship`
/// element by local name and reads `Type`/`Target` by local name, so a prefixed
/// `<pr:Relationship>` cannot slip a table past the scan.
fn table_targets_in_rels(input: &[u8], rels_part: &str) -> Vec<String> {
    let base_dir = match rels_part.split_once("/_rels/") {
        Some((d, _)) => d,
        None => return Vec::new(),
    };
    let bytes = match crate::ooxml::read_part(input, rels_part) {
        Ok(b) => b,
        Err(_) => return Vec::new(), // no rels part => nothing declared here
    };
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"Relationship") =>
            {
                let is_table = attr_by_local(&e, b"Type")
                    .map(|t| t.ends_with("/table"))
                    .unwrap_or(false);
                if is_table {
                    if let Some(t) = attr_by_local(&e, b"Target") {
                        if let Some(p) = resolve_rel_target(base_dir, &t) {
                            out.push(p);
                        }
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

/// Zip part paths of the structured tables attached to `sheet_part`. Only these
/// tables have an extent expressed in the EDITED sheet's coordinates.
fn tables_attached_to(input: &[u8], sheet_part: &str) -> Vec<String> {
    let (dir, file) = match sheet_part.rsplit_once('/') {
        Some(x) => x,
        None => return Vec::new(),
    };
    table_targets_in_rels(input, &format!("{dir}/_rels/{file}.rels"))
}

/// The edited sheet's coordinate-bearing ATTACHMENTS (drawings, comments, pivot
/// tables, form controls, …) live in that sheet's coordinates but are copied
/// byte-for-byte — never shifted. Only coordinate-free attachments (external-URL
/// hyperlinks, printer settings) and tables (handled by the dedicated table guard)
/// are safe. Fail-closed WHITELIST: returns the Type of the first attachment that is
/// NOT safe, so an unrecognized (future) attachment type refuses by default rather
/// than being silently left stale. `None` = every attachment is safe.
fn edited_sheet_bad_attachment(
    input: &[u8],
    sheet_part: &str,
    edit: &StructuralEdit,
) -> Option<String> {
    // `/image` at the WORKSHEET level is the sheet-background picture (`<picture r:id>` /
    // CT_SheetBackgroundPicture) — it tiles the whole sheet and carries NO cell anchor, so a
    // row/column edit never moves it and xlq's verbatim copy is a faithful identity. (Images
    // embedded in a drawing are referenced from the DRAWING part's own rels, not the worksheet's, so
    // a worksheet `/image` is unconditionally coordinate-free.) Refusing it was an over-refusal on
    // every sheet that has a background image.
    const SAFE: &[&str] = &["/hyperlink", "/printerSettings", "/table", "/image"];
    let (dir, file) = sheet_part.rsplit_once('/')?;
    let rels_part = format!("{dir}/_rels/{file}.rels");
    let bytes = crate::ooxml::read_part(input, &rels_part).ok()?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"Relationship") =>
            {
                if let Some(ty) = attr_by_local(&e, b"Type") {
                    if SAFE.iter().any(|s| ty.ends_with(s)) {
                        continue;
                    }
                    // A DRAWING (image / chart / shape) is now fully handled by the drawing
                    // dispatch: its live cell references (`textlink` / `<xdr:f>`) are SHIFTED, and its
                    // display ANCHORS (`<xdr:from>/<xdr:to>`) are shifted for the edited sheet so the
                    // chart/image relocates with the rows/cols the edit moves. The anchor is display
                    // position only (rounds 47/48; certify does not compare it), so it is never a
                    // value/validity hazard — refusing on it spuriously rejected the canonical
                    // data-table-plus-adjacent-chart dashboard layout. Safe to commit.
                    if ty.ends_with("/drawing") {
                        continue;
                    }
                    // A legacy note / threaded comment is copied verbatim, so — like a drawing —
                    // it is a hazard only if THIS edit would MOVE its anchor cell (`<comment ref>`
                    // / `<threadedComment ref>`). A comment far from the edit is unaffected;
                    // presence-refusing it rejected almost every real annotated workbook.
                    if ty.ends_with("/comments") || ty.ends_with("/threadedComment") {
                        let affected = attr_by_local(&e, b"Target")
                            .map(|t| crate::ooxml::resolve_target(dir, &t))
                            .and_then(|p| crate::ooxml::read_part(input, &p).ok())
                            .map(|x| comment_refs_affected(&x, edit))
                            .unwrap_or(true); // unresolved/unreadable -> fail closed
                        if affected {
                            return Some("comment".into());
                        }
                        continue;
                    }
                    // A legacy VML drawing carries note/control DISPLAY anchors AND form-control
                    // cell BINDINGS. Copied verbatim, so it is a hazard iff the edit MOVES an
                    // anchor (note/control displaced) OR moves a cell a binding names (control
                    // re-bound to the wrong cell). The binding check uses the EDITED sheet as host
                    // so a LOCAL unqualified `$A$8` counts — otherwise walking back the presence-
                    // refuse would open a silent-wrong hole. An unaffected comment-only VML commits.
                    if ty.ends_with("/vmlDrawing") {
                        let affected = attr_by_local(&e, b"Target")
                            .map(|t| crate::ooxml::resolve_target(dir, &t))
                            .and_then(|p| crate::ooxml::read_part(input, &p).ok())
                            .map(|x| {
                                vml_anchor_affected(&x, edit)
                                    || vml_binding_affected_on_host(&x, edit, &edit.sheet)
                            })
                            .unwrap_or(true); // unresolved/unreadable -> fail closed
                        if affected {
                            return Some("vmlDrawing".into());
                        }
                        continue;
                    }
                    // A PivotTable renders on this sheet at its `<location ref>` (its display
                    // extent); its data SOURCE (in the pivot cache) is shifted independently by
                    // rewrite_pivot on the main loop. So — like a comment/drawing anchor — it is a
                    // hazard only if THIS edit MOVES the render extent; a pivot whose location is
                    // outside the edit's band is faithful and must not be presence-refused. Fail
                    // closed on an unresolved/unreadable part.
                    if ty.ends_with("/pivotTable") {
                        let affected = attr_by_local(&e, b"Target")
                            .map(|t| crate::ooxml::resolve_target(dir, &t))
                            .and_then(|p| crate::ooxml::read_part(input, &p).ok())
                            .map(|x| pivot_location_affected(&x, edit))
                            .unwrap_or(true);
                        if affected {
                            return Some("pivotTable".into());
                        }
                        continue;
                    }
                    // Any other attachment (OLE/ActiveX controls, …) is presence-refused —
                    // its coordinate parsing differs and is left fail-closed.
                    return Some(ty.rsplit('/').next().unwrap_or(&ty).to_string());
                }
            }
            Ok(Event::Eof) => break,
            // A present-but-unparseable rels part fails CLOSED: we cannot enumerate
            // its relationships, so we cannot prove there is no unshiftable attachment.
            Err(_) => return Some("unparseable_rels".into()),
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Shift a drawing's cell ANCHORS — the `<xdr:from>`/`<xdr:to>` `<row>` (row edit) or `<col>`
/// (column edit), which are 0-based — so a chart/image on the EDITED sheet RELOCATES with the
/// rows/cols the edit moves, exactly as Excel does. The anchor is DISPLAY position only (a chart's
/// data/category references live in the chart part and shift separately; certify deliberately does
/// not compare the anchor — rounds 47/48), so this is a fidelity shift, never a value change. A
/// deleted anchor row/col clamps to the edit point; an `absoluteAnchor` (EMU-positioned) has no
/// `<from>`/`<to>` and is untouched. Only meaningful for the edited sheet's own drawing (a foreign
/// sheet's anchors do not move under an edit elsewhere), so the caller gates on host == edit.sheet.
/// A parse/write error fails closed by propagating.
fn shift_drawing_anchor(src: &[u8], edit: &StructuralEdit) -> Result<Vec<u8>> {
    let want: &[u8] = if edit.axis == Axis::Row {
        b"row"
    } else {
        b"col"
    };
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut in_pt = false; // inside <from> / <to>
    let mut cap = false; // capturing the <row>/<col> text
    let mut txt = String::new();
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e)
                if tag_local_eq(e.name().as_ref(), b"from")
                    || tag_local_eq(e.name().as_ref(), b"to") =>
            {
                in_pt = true;
                writer
                    .write_event(Event::Start(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::End(e)
                if tag_local_eq(e.name().as_ref(), b"from")
                    || tag_local_eq(e.name().as_ref(), b"to") =>
            {
                in_pt = false;
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Start(e) if in_pt && tag_local_eq(e.name().as_ref(), want) => {
                cap = true;
                txt.clear();
                writer
                    .write_event(Event::Start(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Text(t) if cap => txt.push_str(&String::from_utf8_lossy(t.as_ref())),
            Event::End(e) if cap && tag_local_eq(e.name().as_ref(), want) => {
                let out_val = match txt.trim().parse::<u32>() {
                    Ok(idx0) => {
                        let one_based = idx0 + 1; // xdr anchors are 0-based
                        let new_one = shift_line(one_based, edit).unwrap_or(edit.at); // deleted -> clamp
                        new_one.saturating_sub(1).to_string()
                    }
                    Err(_) => std::mem::take(&mut txt), // non-numeric -> leave verbatim
                };
                writer
                    .write_event(Event::Text(BytesText::new(&out_val)))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
                cap = false;
            }
            other => {
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok(writer.into_inner().into_inner())
}

/// Map each drawing part -> the worksheet that OWNS it, by inverting every worksheet's `/drawing`
/// relationship. A drawing floats over exactly one sheet, and an UNQUALIFIED reference inside it
/// (`textlink="$A$8"`, an `<xdr:f>` body) is local to that owning sheet — so shifting the
/// reference needs the owning sheet as its σ host. Drawings not owned by any WORKSHEET (orphans, a
/// chartsheet's drawing) are absent from the map; their unqualified references are not
/// worksheet-cell-local and a qualified reference shifts without a host, so `host = ""` is sound.
fn drawing_owner_sheets(
    input: &[u8],
    part_sheet: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (sheet_part, sheet_name) in part_sheet {
        let (dir, file) = match sheet_part.rsplit_once('/') {
            Some(x) => x,
            None => continue,
        };
        let rels_part = format!("{dir}/_rels/{file}.rels");
        let bytes = match crate::ooxml::read_part(input, &rels_part) {
            Ok(b) => b,
            Err(_) => continue, // no rels => this sheet has no drawing
        };
        let mut reader = Reader::from_reader(bytes.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if tag_local_eq(e.name().as_ref(), b"Relationship") =>
                {
                    let is_drawing = attr_by_local(&e, b"Type")
                        .map(|t| t.ends_with("/drawing"))
                        .unwrap_or(false);
                    if is_drawing {
                        if let Some(t) = attr_by_local(&e, b"Target") {
                            out.insert(crate::ooxml::resolve_target(dir, &t), sheet_name.clone());
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    out
}

/// Shift the live cell references carried inside a drawing part so they follow the cells this edit
/// moves: a graphic-frame `<xdr:f>` formula body and a linked shape's `textlink` attribute (the
/// two channels certify treats as value-bearing in `chart_drawing_refs`). `host` is the drawing's
/// OWNING sheet, scoping unqualified references. Returns (bytes, refs_shifted, ref_errors,
/// qualifier_risk); a parse/write error fails closed by propagating.
fn shift_drawing_refs(
    src: &[u8],
    host: &str,
    edit: &StructuralEdit,
) -> Result<(Vec<u8>, u32, u32, bool)> {
    // `<xdr:f>` bodies (local name `f`) — reuse the hardened `<f>`-body shifter.
    let (b1, n1, e1, q1) = shift_text_in_element(src, b"f", edit, host)?;
    // Linked-shape `textlink="…"` attributes.
    let (b2, n2, e2, q2) = shift_textlink_attrs(&b1, host, edit)?;
    // Display anchors move only for the drawing on the EDITED sheet (a foreign sheet's drawing does
    // not move under an edit elsewhere). Shift them for fidelity so the chart/image relocates.
    let b3 = if host.eq_ignore_ascii_case(&edit.sheet) {
        shift_drawing_anchor(&b2, edit)?
    } else {
        b2
    };
    Ok((b3, n1 + n2, e1 + e2, q1 || q2))
}

/// Rewrite the shifted `textlink` on a single drawing element, if present. Excel writes
/// `textlink="Sheet1!$A$8"` when a shape's text is bound to a cell (`=A8` in the formula bar); the
/// shape mirrors that cell's value, so if the edit MOVES the cell the attribute must follow.
/// Fail-closed on an unquoted non-ASCII sheet qualifier (flag, do not shift). Returns the (possibly
/// unchanged) tag plus (refs_shifted, ref_errors, qualifier_risk).
fn shift_textlink_in_tag(
    e: &BytesStart,
    host: &str,
    edit: &StructuralEdit,
) -> (BytesStart<'static>, u32, u32, bool) {
    let tl = match attr_by_local(e, b"textlink") {
        Some(v) if !v.trim().is_empty() => v,
        _ => return (e.to_owned(), 0, 0, false),
    };
    if refshift::has_unquoted_non_ascii_qualifier(&tl) {
        // On an ASCII host (edited) sheet a non-ASCII qualifier necessarily names a DIFFERENT sheet
        // the edit cannot move, so — like the main `<f>` path — neutralize those refs and refuse
        // ONLY if the remaining host-sheet portion actually shifts (or the span is non-neutralizable,
        // or the host sheet is itself non-ASCII). A textlink to only a non-ASCII sheet is copied
        // verbatim, not refused (round-56 defect 7).
        if host.is_ascii() {
            match refshift::neutralize_non_ascii_quals(&tl) {
                Some(resid) if refshift::shift_formula(&resid, host, edit).0 == resid => {
                    return (e.to_owned(), 0, 0, false);
                }
                _ => return (e.to_owned(), 0, 0, true),
            }
        }
        return (e.to_owned(), 0, 0, true);
    }
    let before_ref = tl.matches("#REF!").count();
    let (nv, n) = refshift::shift_formula(&tl, host, edit);
    // Count only NEWLY introduced #REF! (a genuine straddle/overflow), matching shift_text_in_element.
    let errs = nv.matches("#REF!").count().saturating_sub(before_ref) as u32;
    if nv == tl {
        (e.to_owned(), n, errs, false)
    } else {
        (set_attrs(e, &[(&b"textlink"[..], nv)]), n, errs, false)
    }
}

/// Shift every linked-shape `textlink` attribute in a drawing part (see `shift_textlink_in_tag`).
fn shift_textlink_attrs(
    src: &[u8],
    host: &str,
    edit: &StructuralEdit,
) -> Result<(Vec<u8>, u32, u32, bool)> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    let mut qualifier_risk = false;
    loop {
        // Fail closed: a mid-stream parse or write error must NOT commit a truncated drawing.
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e) => {
                let (t, n, r, q) = shift_textlink_in_tag(&e, host, edit);
                shifted += n;
                errs += r;
                qualifier_risk |= q;
                writer
                    .write_event(Event::Start(t))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Empty(e) => {
                let (t, n, r, q) = shift_textlink_in_tag(&e, host, edit);
                shifted += n;
                errs += r;
                qualifier_risk |= q;
                writer
                    .write_event(Event::Empty(t))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            other => {
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((
        writer.into_inner().into_inner(),
        shifted,
        errs,
        qualifier_risk,
    ))
}

/// True if an `<extLst>` reference on the edited sheet — an `<xm:sqref>` range for x14
/// conditional formatting / data validation / a sparkline draw location — would be MOVED by
/// this edit and the base shift does not rewrite it. AFFECT-based, not presence-based: Excel
/// writes a data bar / color scale / icon set / sparkline as an x14 extLst on essentially
/// every real workbook, so refusing on mere presence rejected almost every legitimate edit;
/// an extLst whose ranges the edit does not touch is unaffected. (`<xm:f>` bodies have local
/// name `f`, so they ARE shifted by the edited-sheet formula path; only `<xm:sqref>` is
/// left stale.) Fail-closed on a parse error.
pub(crate) fn sheet_extlst_affected(xml: &[u8], sheet: &str, edit: &StructuralEdit) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut ext_depth = 0u32;
    let mut cap = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"extLst") => {
                ext_depth += 1;
            }
            Ok(Event::Start(e)) if ext_depth > 0 && tag_local_eq(e.name().as_ref(), b"sqref") => {
                cap = true;
                raw.clear();
            }
            Ok(Event::End(e)) if cap && tag_local_eq(e.name().as_ref(), b"sqref") => {
                let text = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                let (nv, _n, consumed, _all) = shift_sqref(&text, sheet, edit);
                if nv != text || consumed > 0 {
                    return true;
                }
                cap = false;
                raw.clear();
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"extLst") => {
                ext_depth = ext_depth.saturating_sub(1);
            }
            Ok(Event::Text(t)) if cap => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if cap => push_ref_raw(&mut raw, &r),
            Ok(Event::Eof) => return false,
            Err(_) => return true, // unparseable -> fail closed
            _ => {}
        }
        buf.clear();
    }
}

/// Fail-closed scan of the EDITED worksheet body for a coordinate-bearing construct the
/// shift engine copies verbatim instead of shifting. `transform_tag` shifts `ref`/`sqref`
/// only for the `has_ref_attr` set (mergeCell/hyperlink/CF/DV/dimension/selection/pane/
/// autoFilter) and shifts `r` only on cells — every OTHER element on the edited sheet that
/// carries a `ref`/`sqref`, or a cell-shaped `r`, is emitted STALE. That covers, among
/// others: `<protectedRange sqref>` (which cells are locked — a SECURITY reference),
/// `<scenario><inputCells r>` (Scenario Manager's write target), and `<dataConsolidate>
/// <dataRef ref>`. Per the fail-closed-by-default whitelist we refuse rather than commit any of
/// them stale. Returns the offending element's local name, else `None`. Fail-closed on a
/// parse error. (Cells/rows are handled by the row/cell path; formula tags by the formula
/// path; pure view-state `pane topLeftCell`/`selection activeCell`/`sheetView topLeftCell`
/// carries no `ref`/`sqref` and no `r`, so it is correctly not flagged.)
fn edited_sheet_body_unshifted_ref(
    xml: &[u8],
    sheet: &str,
    edit: &StructuralEdit,
) -> Option<String> {
    // AFFECT-based: a stale coordinate is a defect only if THIS edit would actually move it.
    // A construct whose range is nowhere near the edit is unaffected, so refusing it is a
    // spurious over-refusal (e.g. a protectedRange on A1:A5 while inserting at row 50).
    let would_shift = |val: &str| -> bool {
        let (nv, _n, consumed, _all) = shift_sqref(val, sheet, edit);
        nv != val || consumed > 0
    };
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        let ev = reader.read_event_into(&mut buf);
        match ev {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let full = name.as_ref();
                let local = local_of(full);
                // Handled elsewhere: cells/rows (row/cell path), formulas (formula path).
                if local == b"c" || local == b"row" || is_formula_tag(full) {
                    buf.clear();
                    continue;
                }
                // A `<dataRef ref sheet>` (CT_DataRef) scopes its BARE `ref` to a SEPARATE `sheet`
                // attribute; treating that ref as edited-sheet-local over-refuses a consolidation
                // whose source is a DIFFERENT, unaffected sheet (round-60 defect 6). Read the sibling
                // `sheet` once so a bare ref/sqref can be qualified before the affect test.
                let sheet_attr = attr_by_local(&e, b"sheet");
                let flagged = e.attributes().flatten().any(|a| {
                    let k = local_of(a.key.as_ref());
                    let val = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap_or_default();
                    // `ref`/`sqref` is shifted ONLY for the has_ref_attr elements; on anything
                    // else it is left stale — but flag it only if this edit would move it. Matched
                    // by FULL name (see shift_ref_attrs): a namespace-prefixed element is left to
                    // refuse here because resolving whether its prefix is spreadsheetML (shift) or a
                    // foreign namespace (must not shift the attribute) needs namespace resolution.
                    if (k == b"ref" || k == b"sqref") && !has_ref_attr(full) {
                        // A bare ref (no `!`) with a sibling `sheet` names its source sheet there;
                        // qualify each token by it so a ref on a non-edited sheet is not flagged
                        // (mirrors foreign_sheet_ref_attr_crosses). When `sheet` is absent or names
                        // the edited sheet, the edited-sheet-local reading is correct and preserved.
                        if let Some(s) = &sheet_attr {
                            if !val.contains('!') {
                                return split_sqref_tokens(&val).into_iter().any(|tok| {
                                    let q = format!("'{}'!{}", s.replace('\'', "''"), tok);
                                    formula_would_shift(&q, edit)
                                });
                            }
                        }
                        return would_shift(&val);
                    }
                    // Other cell-range attributes the shift never rewrites: form-control / OLE
                    // data bindings (linkedCell/fmlaLink/listFillRange/fmlaRange — the cell a
                    // control reads/writes or a list/combo box's SOURCE range; `link` — an
                    // `<oleObject link>` linked-cell source) and a web-publish source range
                    // (sourceRef). Guarded by would_shift, so a `link` holding a non-coordinate
                    // value is not flagged. Flag if this edit would move them.
                    if matches!(
                        k,
                        b"linkedCell"
                            | b"fmlaLink"
                            | b"listFillRange"
                            | b"fmlaRange"
                            | b"sourceRef"
                            | b"link"
                            // Modern CT_FormControlPr links the foreign-sheet scan + certify already
                            // cover (round 37): an option-button-GROUP result cell and an edit-box
                            // bound cell. On the EDITED sheet they are copied verbatim too, so a
                            // shift leaves them stale — flag if this edit would move them.
                            | b"fmlaGroup"
                            | b"fmlaTxbx"
                    ) {
                        return would_shift(&val);
                    }
                    // A cell/range-shaped `r` on a NON-cell element (`<inputCells r>`,
                    // `<cellWatch r>`) is never shifted — flag only if this edit moves it.
                    k == b"r" && looks_like_cell_or_range(&val) && would_shift(&val)
                });
                if flagged {
                    return Some(String::from_utf8_lossy(local).into_owned());
                }
            }
            Ok(Event::Eof) => return None,
            Err(_) => return Some("unparseable_sheet".into()),
            _ => {}
        }
        buf.clear();
    }
}

/// The reference SEMANTICS of every conditional-formatting / data-validation element and
/// every `<extLst>` reference subtree on a sheet, sorted: for legacy CF/DV, its `sqref`
/// attribute plus its `<formula>`/`<formula1>`/`<formula2>` bodies (logical form); for an
/// `<extLst>`, the collected `<xm:sqref>`/`<xm:f>` texts (x14 CF/DV, sparklines). These are
/// the references xlq's transform SHIFTS (edited sheet) or preserves (foreign sheet), so
/// certify COMPARES them against its transform — a faithful edit matches, a mangle differs
/// — instead of refusing on their mere PRESENCE, which rejected xlq's own transform of any
/// workbook carrying a dropdown or CF rule (ubiquitous constructs).
/// Canonicalize a `sqref` (space-separated A1 ranges) so that a foreign editor coalescing or
/// splitting ADJACENT ranges over the SAME cells does not change it: `B1:B11 C1:C11` and `B1:C11`
/// both canonicalize to `B1:C11`. Enumerates the covered cells (capped); when their union is a
/// full rectangle it emits that rectangle (so a single range like `A1:A10` is unchanged — its own
/// canonical form), otherwise the sorted cell list. A very large coverage (whole rows/columns) or
/// an unparseable range falls back to a sorted-token join (a coalesce of huge ranges is rare, and
/// refusing it is the safe direction).
pub(crate) fn canonical_sqref(sqref: &str) -> String {
    const CAP: usize = 262_144;
    let token_sort = || {
        let mut toks: Vec<&str> = sqref.split_whitespace().collect();
        toks.sort_unstable();
        toks.join(" ")
    };
    let mut cells: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for range in sqref.split_whitespace() {
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        let (Some((c0, r0)), Some((c1, r1))) = (parse_cell_rc(a), parse_cell_rc(b)) else {
            return token_sort();
        };
        let (cmin, cmax) = (c0.min(c1), c0.max(c1));
        let (rmin, rmax) = (r0.min(r1), r0.max(r1));
        total += (cmax - cmin + 1) as usize * (rmax - rmin + 1) as usize;
        if total > CAP {
            return token_sort();
        }
        for c in cmin..=cmax {
            for r in rmin..=rmax {
                cells.insert((c, r));
            }
        }
    }
    if cells.is_empty() {
        return String::new();
    }
    let cmin = cells.iter().map(|(c, _)| *c).min().unwrap();
    let cmax = cells.iter().map(|(c, _)| *c).max().unwrap();
    let rmin = cells.iter().map(|(_, r)| *r).min().unwrap();
    let rmax = cells.iter().map(|(_, r)| *r).max().unwrap();
    let a1 = |c: u32, r: u32| crate::diff::a1(r as i32, c as i32).unwrap_or_default();
    let area = (cmax - cmin + 1) as usize * (rmax - rmin + 1) as usize;
    if cells.len() == area {
        format!("{}:{}", a1(cmin, rmin), a1(cmax, rmax))
    } else {
        cells
            .iter()
            .map(|(c, r)| a1(*c, *r))
            .collect::<Vec<_>>()
            .join(",")
    }
}

pub(crate) fn sheet_ref_construct_semantics(xml: &[u8]) -> Vec<(String, String)> {
    #[derive(PartialEq)]
    enum Cap {
        None,
        Legacy,
        Ext,
    }
    let is_cfdv =
        |n: &[u8]| tag_local_eq(n, b"conditionalFormatting") || tag_local_eq(n, b"dataValidation");
    let is_legacy_f = |n: &[u8]| {
        tag_local_eq(n, b"formula") || tag_local_eq(n, b"formula1") || tag_local_eq(n, b"formula2")
    };
    let is_ext_ref = |n: &[u8]| tag_local_eq(n, b"f") || tag_local_eq(n, b"sqref");
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out: Vec<(String, String)> = Vec::new();
    // (kind, sqref, formula bodies, dataValidation `type`, dataValidation `operator`)
    let mut cfdv: Option<(String, String, Vec<String>, String, String)> = None;
    let mut ext_depth = 0u32;
    let mut ext_refs: Vec<String> = Vec::new();
    let mut cap = Cap::None;
    let mut cap_is_formula2 = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let n = e.name();
                if tag_local_eq(n.as_ref(), b"extLst") {
                    ext_depth += 1;
                } else if ext_depth == 0 && is_cfdv(n.as_ref()) {
                    let kind = String::from_utf8_lossy(local_of(n.as_ref())).into_owned();
                    let sqref = canonical_sqref(&attr_by_local(&e, b"sqref").unwrap_or_default());
                    let dv_type = attr_by_local(&e, b"type").unwrap_or_default();
                    let operator = attr_by_local(&e, b"operator").unwrap_or_default();
                    cfdv = Some((kind, sqref, Vec::new(), dv_type, operator));
                } else if ext_depth == 0 && cfdv.is_some() && is_legacy_f(n.as_ref()) {
                    cap = Cap::Legacy;
                    cap_is_formula2 = tag_local_eq(n.as_ref(), b"formula2");
                    raw.clear();
                } else if ext_depth > 0 && is_ext_ref(n.as_ref()) {
                    cap = Cap::Ext;
                    raw.clear();
                }
            }
            Ok(Event::Empty(e)) if ext_depth == 0 && is_cfdv(e.name().as_ref()) => {
                let kind = String::from_utf8_lossy(local_of(e.name().as_ref())).into_owned();
                let sqref = canonical_sqref(&attr_by_local(&e, b"sqref").unwrap_or_default());
                out.push((kind, format!("sqref={sqref}")));
            }
            Ok(Event::End(e)) => {
                let n = e.name();
                if cap != Cap::None && (is_legacy_f(n.as_ref()) || is_ext_ref(n.as_ref())) {
                    let logical = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                    // A conditional-formatting / data-validation formula body may carry an INERT
                    // leading `=` (`=Lists!$A$1:$A$3`); Excel and LibreOffice both accept and
                    // normalize it away, so a foreign editor dropping it is a faithful, value-
                    // identical edit. Strip a single leading `=` so its presence/absence does not
                    // flip the key.
                    let logical = logical
                        .strip_prefix('=')
                        .map(str::to_string)
                        .unwrap_or(logical);
                    // Redundant sheet-name quoting (`'Data'!A1` vs `Data!A1`) is semantically
                    // inert; canonicalize it so a faithful re-serialization is not refused.
                    let logical = canonicalize_sheet_quotes(&logical);
                    match cap {
                        Cap::Legacy => {
                            if let Some((_, _, fs, dv_type, operator)) = cfdv.as_mut() {
                                // `formula2` is a VALUE input ONLY for the between/notBetween
                                // operators (the default when `operator` is absent). For a
                                // type="list" dropdown OR any SCALAR operator (greaterThan,
                                // lessThan, equal, …) it is inert — Excel ignores it — so a foreign
                                // editor writing `<formula2>0</formula2>` there (LibreOffice does on
                                // every non-between DV) is a faithful, value-preserving edit. Skip.
                                let formula2_inert = dv_type == "list"
                                    || (!operator.is_empty()
                                        && operator != "between"
                                        && operator != "notBetween");
                                if !(cap_is_formula2 && formula2_inert) {
                                    fs.push(logical);
                                }
                            }
                        }
                        Cap::Ext => ext_refs.push(logical),
                        Cap::None => {}
                    }
                    cap = Cap::None;
                    cap_is_formula2 = false;
                    raw.clear();
                }
                if ext_depth == 0 && is_cfdv(n.as_ref()) {
                    if let Some((kind, sqref, fs, _dv_type, _operator)) = cfdv.take() {
                        out.push((kind, format!("sqref={sqref}|{}", fs.join("|"))));
                    }
                }
                if tag_local_eq(n.as_ref(), b"extLst") {
                    ext_depth = ext_depth.saturating_sub(1);
                    if ext_depth == 0 && !ext_refs.is_empty() {
                        ext_refs.sort();
                        out.push(("extLst".into(), ext_refs.join("|")));
                        ext_refs.clear();
                    }
                }
            }
            Ok(Event::Text(t)) if cap != Cap::None => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if cap != Cap::None => push_ref_raw(&mut raw, &r),
            Ok(Event::Eof) => break,
            // Unparseable: emit a sentinel so expected/edited can still differ meaningfully.
            Err(_) => {
                out.push(("parse_error".into(), String::new()));
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// Separator joining an element's `key=value` attribute entries in `element_attr_semantics`. A
/// control character (ASCII Unit Separator) that XML 1.0 FORBIDS inside an attribute value — so a
/// per-key consumer can split on it to recover each attribute WHOLE, even when a value contains
/// spaces (a sheet name `Data 2024`, a filter threshold `North America`). A plain space separator
/// let `split_whitespace` truncate such a value mid-attribute, colliding two distinct references.
pub(crate) const ATTR_SEP: &str = "\u{1f}";

/// For each element whose LOCAL name is in `wanted`, its sorted attributes as a stable
/// string (entries joined by [`ATTR_SEP`]), paired with the local name; sorted. Certify compares
/// this between its transform and a foreign edit for verbatim-preserved elements the cell diff
/// never sees — e.g. `<sheetProtection>`/`<protectedRange>`/`<workbookProtection>` (stripping or
/// weakening a password control is a SECURITY change). Attribute values are entity-normalized and
/// the attribute order is normalized, so a cosmetic re-serialization does not false-refuse.
pub(crate) fn element_attr_semantics(xml: &[u8], wanted: &[&[u8]]) -> Vec<(String, String)> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if wanted.iter().any(|w| tag_local_eq(e.name().as_ref(), w)) =>
            {
                let local = String::from_utf8_lossy(local_of(e.name().as_ref())).into_owned();
                let mut attrs: Vec<String> = e
                    .attributes()
                    .flatten()
                    .map(|a| {
                        let k = String::from_utf8_lossy(local_of(a.key.as_ref())).into_owned();
                        let v = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map(|c| c.into_owned())
                            .unwrap_or_default();
                        format!("{k}={v}")
                    })
                    .collect();
                attrs.sort();
                out.push((local, attrs.join(ATTR_SEP)));
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                out.push(("parse_error".into(), String::new()));
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// The logical text of every element whose LOCAL name is in `locals`, sorted. Certify uses
/// this to compare a chart part's `<f>` data-range references (which the transform shifts)
/// and a drawing part's `<col>`/`<row>` cell-anchor coordinates against its transform,
/// instead of refusing every workbook that contains a chart or image.
pub(crate) fn element_text_semantics(xml: &[u8], locals: &[&[u8]]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut cap = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if locals.iter().any(|l| tag_local_eq(e.name().as_ref(), l)) => {
                cap = true;
                raw.clear();
            }
            Ok(Event::End(e))
                if cap && locals.iter().any(|l| tag_local_eq(e.name().as_ref(), l)) =>
            {
                out.push(logical_formula(&raw).unwrap_or_else(|| raw.clone()));
                cap = false;
                raw.clear();
            }
            Ok(Event::Text(t)) if cap => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if cap => push_ref_raw(&mut raw, &r),
            // A CDATA-wrapped body (`<x:FmlaMacro><![CDATA[Macro]]></x:FmlaMacro>`, which Excel
            // emits for legacy VML form-control bindings) carries its content LITERALLY. Without
            // this arm the body extracted as empty, so two DISTINCT bindings collapsed to the same
            // key and a re-pointed macro/link certified. CDATA is never entity-encoded — append raw.
            Ok(Event::CData(c)) if cap => raw.push_str(&String::from_utf8_lossy(c.as_ref())),
            Ok(Event::Eof) => break,
            Err(_) => {
                out.push("parse_error".into());
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// Every ISO-8601 date VALUE cell (`<c r="H1" t="d"><v>2020-01-01T00:00:00</v></c>`, ECMA-376
/// §18.18.11 ST_CellType `d`) as (A1-ref, trimmed `<v>` text). ironcalc's importer does NOT
/// implement this cell type — it discards the value and loads a constant NIMPL error — so certify's
/// engine snapshot is blind to a change of the stored date; certify compares these at the byte
/// level instead. Namespace-aware (matches `<c>`/`<v>` and the `t`/`r` attributes by local name);
/// an empty `<c t="d"/>` yields an empty value.
pub(crate) fn date_typed_value_cells(xml: &[u8]) -> Vec<(String, String)> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut in_d_cell = false;
    let mut cur_ref = String::new();
    let mut in_v = false;
    let mut val = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                if attr_by_local(&e, b"t").as_deref() == Some("d") {
                    in_d_cell = true;
                    cur_ref = attr_by_local(&e, b"r").unwrap_or_default();
                    val.clear();
                } else {
                    in_d_cell = false;
                }
            }
            Ok(Event::Empty(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                if attr_by_local(&e, b"t").as_deref() == Some("d") {
                    out.push((attr_by_local(&e, b"r").unwrap_or_default(), String::new()));
                }
            }
            Ok(Event::Start(e)) if in_d_cell && tag_local_eq(e.name().as_ref(), b"v") => {
                in_v = true;
                val.clear();
            }
            Ok(Event::Text(t)) if in_v => val.push_str(&String::from_utf8_lossy(t.as_ref())),
            Ok(Event::End(e)) if in_v && tag_local_eq(e.name().as_ref(), b"v") => in_v = false,
            Ok(Event::End(e)) if in_d_cell && tag_local_eq(e.name().as_ref(), b"c") => {
                out.push((cur_ref.clone(), val.trim().to_string()));
                in_d_cell = false;
            }
            Ok(Event::Eof) => break,
            // Unparseable -> sentinel so expected != edited forces a fail-closed refusal.
            Err(_) => {
                out.push(("__parse_error__".into(), String::new()));
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// Cell refs (`<c r>`) whose `<f>` body carries a range-intersection — Excel's space operator
/// (`A1:A10 A3:A5`), at any paren depth (inside `SUM(…)` too). IronCalc cannot evaluate an
/// intersection: it collapses the second operand (SUM form -> #ERROR!) or returns a wrong scalar
/// (bare form), so the cache oracle must NOT vouch these cells (they are excluded, fail-closed).
/// Detected from the RAW `<f>` because the engine's own reparse drops the operator. Namespace-aware.
pub(crate) fn cells_with_range_intersection(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut cur_ref: Option<String> = None;
    let mut in_f = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cur_ref = attr_by_local(&e, b"r");
            }
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = true;
                raw.clear();
            }
            Ok(Event::Text(t)) if in_f => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if in_f => push_ref_raw(&mut raw, &r),
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = false;
                let body = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                if has_range_intersection_any_depth(&body) {
                    if let Some(r) = cur_ref.clone() {
                        out.push(r);
                    }
                }
                raw.clear();
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cur_ref = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// True if any `<c>` cell carries TWO OR MORE `<v>` value children — a schema violation (CT_Cell
/// permits at most one `<v>`). Excel and LibreOffice (lenient readers) take the LAST `<v>`, but
/// ironcalc's importer misreads such a cell as empty/error, so certify's engine snapshot is blind
/// to a value injected as a second `<v>`. certify refuses a workbook carrying one (fail-closed).
/// Namespace-aware; an unparseable sheet returns true (fail-closed).
pub(crate) fn cell_has_repeated_value(xml: &[u8]) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut in_c = false;
    let mut v_count = 0u32;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                in_c = true;
                v_count = 0;
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                in_c = false;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if in_c && tag_local_eq(e.name().as_ref(), b"v") =>
            {
                v_count += 1;
                if v_count >= 2 {
                    return true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return true, // unparseable -> fail closed
            _ => {}
        }
        buf.clear();
    }
    false
}

/// Map every FORMULA cell (`<c>` with an `<f>`) that carries a PRESENT, non-empty stored
/// cache (`<v>…</v>`) to its stored value text, keyed by the cell's `r` reference.
///
/// A formula cell with no `<v>` or an empty `<v/>` is EXCLUDED: Excel must recompute a
/// formula cell that has no stored result, so an absent cache is always safe (and is exactly
/// what a cache-dropping tool like openpyxl — and xlq's own shifted-cell blanking — leaves).
/// certify compares these maps between xlq's proven transform and a foreign edit: a present
/// cache the transform did not vouch is a value Excel could display verbatim without
/// recomputing, so it must be accounted for.
pub(crate) fn formula_cache_map(xml: &[u8]) -> std::collections::BTreeMap<String, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = std::collections::BTreeMap::new();
    let mut cell_ref: Option<String> = None;
    let mut cell_type = String::from("n");
    let mut has_f = false;
    let mut cap_v = false;
    let mut v_text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = attr_by_local(&e, b"r");
                // The cell TYPE (`t`): absent/"n" = number, "str" = formula string result,
                // "b" = boolean, "e" = error. Part of the cache signature so a number→text
                // retype of the same digit string (`<v>55</v>` n vs str) is caught.
                cell_type = attr_by_local(&e, b"t").unwrap_or_else(|| "n".into());
                has_f = false;
                v_text.clear();
            }
            // An `<f>` (Start for a normal formula, Empty for a shared-formula dependent
            // `<f t="shared" si="0"/>`) marks the cell as a formula cell.
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                has_f = true;
            }
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"v") => {
                cap_v = true;
                v_text.clear();
            }
            // Accumulate the `<v>` cache RAW (escaped): under quick-xml an entity reference
            // (`&amp;`/`&lt;`/`&gt;`) arrives as a separate GeneralRef event, so a Text-only capture
            // would DROP it (`Sales &amp; Costs` -> `Sales  Costs`) and never match the engine's true
            // unescaped value — a spurious refusal of a faithful cache. Reassembled + unescaped at
            // `</c>`, exactly as the other body readers do.
            Ok(Event::Text(t)) if cap_v => push_text_raw(&mut v_text, &t),
            Ok(Event::GeneralRef(r)) if cap_v => push_ref_raw(&mut v_text, &r),
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"v") => {
                cap_v = false;
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                let v_val = logical_formula(&v_text).unwrap_or_else(|| v_text.clone());
                if has_f && !v_val.trim().is_empty() {
                    if let Some(r) = cell_ref.take() {
                        // A STRING result's leading/trailing whitespace is SIGNIFICANT — a
                        // `=A1&" "` label or fixed-width padding — and the engine oracle
                        // (`cell_value_sig`) does not trim it, so trimming here refused a faithful
                        // padded-string cache (round-56 defect 4). Preserve str: verbatim; a
                        // numeric/bool/error value carries no significant surrounding whitespace, so
                        // trim those (f64::parse rejects surrounding whitespace anyway).
                        let stored = if cell_type == "str" {
                            v_val.as_str()
                        } else {
                            v_val.trim()
                        };
                        out.insert(r, format!("{cell_type}:{stored}"));
                    }
                }
                cell_ref = None;
                cell_type = String::from("n");
                has_f = false;
                v_text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The `r` (row number) of every worksheet row marked hidden (`<row hidden="1">`), sorted.
/// A manually hidden row is a VALUE input to `SUBTOTAL(101–111)` / hidden-ignoring
/// `AGGREGATE` (they exclude it from the aggregate), so certify compares this set on any
/// sheet where such a function is present (see [`hidden_row_exclusion_present`]).
pub(crate) fn hidden_rows(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"row") =>
            {
                let hidden = attr_by_local(&e, b"hidden")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                if hidden {
                    if let Some(r) = attr_by_local(&e, b"r") {
                        out.push(r);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// True if any stored `<f>` on the sheet uses a function whose result EXCLUDES manually
/// hidden rows: `SUBTOTAL` with a 101–111 function code, or `AGGREGATE` with an option that
/// ignores hidden rows (1, 3, 5, 7). For such a sheet a manual `<row hidden>` is a value
/// INPUT and certify compares the hidden-row set; on any other sheet a hidden row is pure
/// display state. The code parse is CONSERVATIVE — an unparseable code counts as excluding,
/// so a novel serialization fails toward comparing (never toward a silent miss).
pub(crate) fn hidden_row_exclusion_present(xml: &[u8]) -> bool {
    element_text_semantics(xml, &[b"f"])
        .iter()
        .any(|body| formula_excludes_hidden_rows(body))
}

fn leading_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn formula_excludes_hidden_rows(f: &str) -> bool {
    let up = f.to_ascii_uppercase();
    // SUBTOTAL(n, …): excludes manually hidden rows iff n >= 101.
    let mut from = 0;
    while let Some(p) = up[from..].find("SUBTOTAL") {
        let s = from + p;
        from = s + "SUBTOTAL".len();
        let Some(args) = up[s + "SUBTOTAL".len()..].trim_start().strip_prefix('(') else {
            continue;
        };
        match leading_int(args) {
            Some(n) if n < 101 => {} // 1..=11: excludes only filter-hidden, not manual
            _ => return true,        // 101..=111, or an unparseable code -> conservative
        }
    }
    // AGGREGATE(fn, options, …): ignores hidden rows iff options is 1, 3, 5, or 7.
    from = 0;
    while let Some(p) = up[from..].find("AGGREGATE") {
        let s = from + p;
        from = s + "AGGREGATE".len();
        let Some(args) = up[s + "AGGREGATE".len()..].trim_start().strip_prefix('(') else {
            continue;
        };
        let opt = args.find(',').and_then(|c| leading_int(&args[c + 1..]));
        match opt {
            Some(0 | 2 | 4 | 6) => {} // does not ignore hidden rows
            _ => return true,         // 1/3/5/7, or unparseable -> conservative
        }
    }
    false
}

/// Per formula cell, the VALUE-AFFECTING `<f>` TYPE attribute the engine normalizes away on
/// load — `t="array"` (a legacy CSE array: on non-dynamic-array Excel a plain
/// `=SUM(A1:A3*A1:A3)` implicit-intersects to a scalar, while `{=SUM(...)}` computes the full
/// 14; and a wider `ref` materializes spilled cells) and `t="dataTable"` — keyed by the cell's
/// `r`, with the element's `ref` (array extent / table output). IronCalc strips these, so the
/// loaded-model cell diff is blind to a foreign edit that ADDS/removes the flag or widens its
/// extent; certify compares this map per cell.
pub(crate) fn array_formula_cells(xml: &[u8]) -> std::collections::BTreeMap<String, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = std::collections::BTreeMap::new();
    let mut cell_ref: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = attr_by_local(&e, b"r");
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                if let Some(t) = attr_by_local(&e, b"t") {
                    if (t == "array" || t == "dataTable") && cell_ref.is_some() {
                        let rf = attr_by_local(&e, b"ref").unwrap_or_default();
                        // For a what-if data table the `r1`/`r2` INPUT cells (the cells each trial
                        // value is substituted into) and the `dt2D`/`dtr` axis flags DETERMINE the
                        // whole tabulated result column. xlq's transform shifts r1/r2, so comparing
                        // them catches a foreign RE-POINT (r1=A2->A9) that recomputes the table
                        // differently in Excel; ironcalc strips the dataTable on load, so the cell
                        // diff is blind to it.
                        let sig = if t == "dataTable" {
                            let g = |k: &[u8]| attr_by_local(&e, k).unwrap_or_default();
                            format!(
                                "dataTable:{rf};dt2D={};dtr={};r1={};r2={}",
                                g(b"dt2D"),
                                g(b"dtr"),
                                g(b"r1"),
                                g(b"r2")
                            )
                        } else {
                            format!("{t}:{rf}")
                        };
                        out.insert(cell_ref.clone().unwrap(), sig);
                    }
                }
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Every `_xlfn.`/`_xlfn._xlws.`-prefixed function token in one stored `<f>` body, sorted.
/// The OOXML persisted format REQUIRES this prefix for post-2007 functions (CONCAT, XLOOKUP,
/// TEXTJOIN, …); a consumer strips it on load and re-adds it on export. certify's cell diff
/// compares the loaded (stripped) form, so a foreign edit that DROPS the prefix — which makes
/// Excel render `#NAME?` — is invisible to it.
fn xlfn_tokens_in(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(p) = rest.find("_xlfn.") {
        let after = &rest[p..];
        let end = after[6..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .map(|e| 6 + e)
            .unwrap_or(after.len());
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out.sort();
    out
}

/// The POSITIONS of the IMPLICIT-INTERSECTION `@` operators in one stored `<f>` body, as
/// their character index in the `@`-STRIPPED body (so the operators do not offset each other).
/// `@` is the dynamic-array implicit-intersection operator: `@A1:A10` coerces a range to the
/// single intersecting cell (a scalar), whereas the bare `A1:A10` SPILLS the whole range — a
/// different computed value AND footprint. A consumer normalizes `@` away on load (IronCalc
/// does), so certify's cell diff — over the loaded, normalized form — is blind to it.
///
/// Positions, not a bare count, so a WITHIN-cell relocation of `@` between operands
/// (`@A1:A3-A1:A3` → `A1:A3-@A1:A3`, same count but a different spill) is caught. A `@` inside
/// a `[...]` structured (table) reference (`Table1[@Col]`) is a column specifier, NOT the
/// intersection operator, so bracket-interior `@` is excluded; `@`/`[` inside a quoted string
/// literal or quoted sheet name are likewise ignored.
fn implicit_at_positions(body: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut stripped_len = 0usize; // chars emitted so far excluding `@`
    let mut bracket_depth: u32 = 0;
    let mut in_dquote = false;
    let mut in_squote = false;
    for c in body.chars() {
        if in_dquote {
            if c == '"' {
                in_dquote = false;
            }
            stripped_len += 1;
            continue;
        }
        if in_squote {
            if c == '\'' {
                in_squote = false;
            }
            stripped_len += 1;
            continue;
        }
        match c {
            '"' => in_dquote = true,
            '\'' => in_squote = true,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '@' if bracket_depth == 0 => {
                positions.push(stripped_len); // index into the @-stripped body
                continue; // do NOT count the `@` toward stripped_len
            }
            _ => {}
        }
        stripped_len += 1;
    }
    positions
}

/// Collapse every run of whitespace to a single space and trim — a canonical form that is
/// invariant under a foreign tool's benign re-spacing while preserving a SIGNIFICANT space.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out.trim().to_string()
}

/// True if `body` contains Excel's range-INTERSECTION operator: a whitespace run at bracket-depth
/// 0, outside string/quoted-name literals, flanked by OPERAND characters (a reference/number/name
/// ends on the left, one begins on the right — NOT an operator, and not a `name (` function call).
/// ironcalc mis-normalizes a top-level intersection `A1:A10 A4:A4` (the `=Revenue January` idiom)
/// to `@A1:A10`, DROPPING the second operand, so the loaded-model diff is blind to a change of
/// that operand's VALUE — a false-certify. certify signs the raw body when this is present.
fn has_top_level_intersection(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let mut depth: i32 = 0;
    let mut in_dq = false;
    let mut in_sq = false;
    let is_operand_end =
        |c: char| c.is_alphanumeric() || matches!(c, ')' | '$' | '!' | '}' | '_' | '.' | '#');
    // A right operand may be PARENTHESIZED (`A1:A10 (A4:A4)` — grouping is a valid reference
    // operand), so `(` starts an operand. ironcalc collapses the whole intersection to its first
    // operand regardless of the parens, dropping the second, so the raw body must be signed or a
    // change to the parenthesized operand's VALUE certifies. (A false positive on a space-before-
    // paren `name (args)` only adds the raw-body signature — at worst a rare over-refusal on a
    // space renormalization, never a false-certify.)
    let is_operand_start =
        |c: char| c.is_alphanumeric() || matches!(c, '$' | '\'' | '[' | '_' | '(');
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_dq {
            if c == '"' {
                in_dq = false;
            }
            i += 1;
            continue;
        }
        if in_sq {
            if c == '\'' {
                in_sq = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => in_dq = true,
            '\'' => in_sq = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if c.is_whitespace() && depth == 0 => {
                let prev = chars[..i]
                    .iter()
                    .rev()
                    .find(|c| !c.is_whitespace())
                    .copied();
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let next = chars.get(j).copied();
                if let (Some(p), Some(n)) = (prev, next) {
                    if is_operand_end(p) && is_operand_start(n) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// True if `body` contains a range-INTERSECTION operator at ANY paren depth (`SUM(A1:A10 A3:A5)` as
/// well as the bare top-level `A1:A10 A3:A5`): a whitespace run OUTSIDE string/quoted-name literals
/// and OUTSIDE a `[...]` structured-ref bracket (where a column name legitimately holds spaces,
/// e.g. `Table[Amount Due]`), flanked by operand characters. IronCalc cannot evaluate an
/// intersection anywhere (it errors or returns a wrong scalar), so certify excludes such a cell
/// from the cache oracle. (Distinct from `has_top_level_intersection`, which is depth-0 only and
/// feeds the hidden-token signature.)
fn has_range_intersection_any_depth(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let mut bracket_depth: i32 = 0;
    let mut in_dq = false;
    let mut in_sq = false;
    let is_operand_end =
        |c: char| c.is_alphanumeric() || matches!(c, ')' | '$' | '!' | '}' | '_' | '.' | '#');
    let is_operand_start =
        |c: char| c.is_alphanumeric() || matches!(c, '$' | '\'' | '[' | '_' | '(');
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_dq {
            if c == '"' {
                in_dq = false;
            }
            i += 1;
            continue;
        }
        if in_sq {
            if c == '\'' {
                in_sq = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => in_dq = true,
            '\'' => in_sq = true,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = (bracket_depth - 1).max(0),
            _ if c.is_whitespace() && bracket_depth == 0 => {
                let prev = chars[..i]
                    .iter()
                    .rev()
                    .find(|c| !c.is_whitespace())
                    .copied();
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let next = chars.get(j).copied();
                if let (Some(p), Some(n)) = (prev, next) {
                    if is_operand_end(p) && is_operand_start(n) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// True if the formula body contains a unary `+`/`-` operator run that IronCalc DROPS while Excel
/// treats as a numeric COERCION. Excel's unary `+`/`-` coerces its operand to a number
/// (`--A1`/`+A1` turn TRUE->1, numeric-looking text "5"->5 — the canonical `--` coercion idiom);
/// IronCalc's parser folds a run of leading signs into ONE `sign` and ignores `+` entirely
/// (parser/mod.rs:585), so a run whose net sign is POSITIVE (an even number of `-`, or a `+` with an
/// even number of `-`) is parsed AND evaluated as the un-coerced operand — `--A1` == `A1` == TRUE,
/// not 1. certify's loaded-model diff is therefore blind to a foreign edit that adds/removes such a
/// coercion; sign the raw body so the byte-level comparison catches it. A single unary `-` (`-A1`,
/// net NEGATIVE) survives as an `UnaryKind::Minus`, so it is caught by the ordinary formula diff and
/// is NOT flagged (avoids gratuitous over-refusal). String literals are skipped.
fn has_unary_coercion(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let mut in_dq = false;
    let mut in_sq = false;
    // A `+`/`-` following an operand END is a BINARY operator; otherwise it is unary (coercion).
    let is_operand_end =
        |c: char| c.is_alphanumeric() || matches!(c, ')' | '$' | '!' | '}' | '_' | '.' | '#' | '%');
    let mut prev_operand_end = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_dq {
            if c == '"' {
                in_dq = false;
                prev_operand_end = true; // a string literal is an operand
            }
            i += 1;
            continue;
        }
        if in_sq {
            if c == '\'' {
                in_sq = false;
                prev_operand_end = true; // a quoted sheet name is (part of) an operand
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_dq = true;
                i += 1;
            }
            '\'' => {
                in_sq = true;
                i += 1;
            }
            _ if c.is_whitespace() => i += 1, // whitespace does not change operand state
            '+' | '-' if !prev_operand_end => {
                // Start of a UNARY operator run — consume the whole run and count the minuses.
                let mut minuses = 0;
                while i < chars.len() {
                    match chars[i] {
                        '-' => {
                            minuses += 1;
                            i += 1;
                        }
                        '+' => i += 1,
                        c if c.is_whitespace() => i += 1,
                        _ => break,
                    }
                }
                // Even minus count (incl. a bare `+` run) => net POSITIVE => IronCalc drops the whole
                // run (coercion lost). Odd => a single surviving `-` (preserved, diff-visible).
                if minuses % 2 == 0 {
                    return true;
                }
                prev_operand_end = false; // the run is followed by its operand
            }
            _ => {
                prev_operand_end = is_operand_end(c);
                i += 1;
            }
        }
    }
    false
}

/// Per formula cell (a `<c r>` carrying an `<f>`), the tokens IronCalc NORMALIZES AWAY on
/// load — which the loaded-model cell diff therefore cannot see — keyed by the cell's `r`
/// reference. The signature is the implicit-intersection `@` positions, the sorted `_xlfn.`
/// function tokens, and — when the body carries a top-level range-INTERSECTION (which ironcalc
/// collapses, dropping an operand) — the whitespace-canonicalized raw body. Only cells with at
/// least one such token are included. certify compares this map POSITIONALLY (per cell), so it
/// catches not just a DROP/ADD of `@`/`_xlfn.` but a same-sheet RELOCATION between cells (`@`
/// moved C1→C5) and a value-changing edit of an intersection operand — each a value change the
/// loaded-model diff would miss.
pub(crate) fn formula_hidden_tokens(xml: &[u8]) -> std::collections::BTreeMap<String, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = std::collections::BTreeMap::new();
    let mut cell_ref: Option<String> = None;
    let mut in_f = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = attr_by_local(&e, b"r");
            }
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = true;
                raw.clear();
            }
            Ok(Event::Text(t)) if in_f => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if in_f => push_ref_raw(&mut raw, &r),
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = false;
                let body = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                let at = implicit_at_positions(&body);
                let xlfn = xlfn_tokens_in(&body);
                // The isect/coerce signatures sign the RAW body, but must apply the SAME
                // value-neutral canonicalizations the cell diff and every other raw-body comparator
                // use, or a purely cosmetic re-encoding (a foreign editor unquoting a safe sheet name
                // `'Data'!A1`->`Data!A1`, folding `TRUE()`->`TRUE`, or dropping a `Data!#REF!`
                // qualifier) over-refuses (round-59 defect 2). These folds never change an operand's
                // VALUE, so a genuine intersection-operand / coercion change still differs.
                let canon = crate::diff::canonicalize_ref_errors(
                    &crate::diff::normalize_bool_literals(&canonicalize_sheet_quotes(&body)),
                );
                // A top-level range-intersection is signed by its canonical raw body, because
                // ironcalc drops its 2nd operand and the loaded diff cannot see an operand change.
                let isect = if has_top_level_intersection(&body) {
                    collapse_ws(&canon)
                } else {
                    String::new()
                };
                // A unary `+`/`-` coercion (`--A1`/`+A1`) IronCalc folds away is invisible to the
                // loaded diff — sign the canonical body so an add/remove of the coercion is caught.
                let coerce = if has_unary_coercion(&body) {
                    collapse_ws(&canon)
                } else {
                    String::new()
                };
                if let Some(r) = cell_ref.clone() {
                    if !at.is_empty() || !xlfn.is_empty() || !isect.is_empty() || !coerce.is_empty()
                    {
                        out.insert(
                            r,
                            format!("@{at:?};{};isect={isect};coerce={coerce}", xlfn.join(",")),
                        );
                    }
                }
                raw.clear();
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The A1 references of formula cells whose `<f>` body calls a VOLATILE function (`NOW`/`TODAY`/
/// `RAND`/`OFFSET`/`INDIRECT`/…). Excel recomputes a volatile cell on every load, so its stored
/// cache can never surface a stale value; certify ignores such a cell's cache rather than
/// disabling its cache oracle workbook-wide because one volatile function is present somewhere.
pub(crate) fn volatile_formula_cells(xml: &[u8]) -> std::collections::BTreeSet<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = std::collections::BTreeSet::new();
    let mut cell_ref: Option<String> = None;
    let mut in_f = false;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = attr_by_local(&e, b"r");
            }
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = true;
                raw.clear();
            }
            Ok(Event::Text(t)) if in_f => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if in_f => push_ref_raw(&mut raw, &r),
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"f") => {
                in_f = false;
                let body = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                if let Some(r) = cell_ref.clone() {
                    if crate::census::is_volatile_formula(&body) {
                        out.insert(r);
                    }
                }
                raw.clear();
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"c") => {
                cell_ref = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// The value/reference-affecting semantics of every Excel Table in a table part: the table's
/// `name`/`displayName`/`ref` (its extent — structured references resolve through it), each
/// column's `name` (a structured-reference target) and `totalsRowFunction` (a value input),
/// and any `calculatedColumnFormula`/`totalsRowFormula` body. certify compares these — the
/// transform never shifts a table (it refuses an edit that would move one), so a faithful edit
/// leaves them unchanged and a mangle differs — while tolerating a foreign tool's cosmetic
/// re-serialization (style / id / header-count attrs are excluded). Values are captured whole,
/// so a name or label containing spaces is compared correctly.
pub(crate) fn table_semantics(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut in_f = false;
    let mut raw = String::new();
    // The ORDINAL position of each `<tableColumn>` — the engine resolves a structured reference
    // `Table1[Amount]` by the column's POSITION in the tableColumns list, so a reorder (names
    // unchanged) remaps the reference. Position-keying the column signature makes a reorder differ.
    let mut col_idx = 0usize;
    let is_tbl_f = |n: &[u8]| {
        tag_local_eq(n, b"calculatedColumnFormula") || tag_local_eq(n, b"totalsRowFormula")
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"table") =>
            {
                for key in [
                    b"name".as_slice(),
                    b"displayName".as_slice(),
                    b"ref".as_slice(),
                ] {
                    if let Some(v) = attr_by_local(&e, key) {
                        out.push(format!("table.{}={}", String::from_utf8_lossy(key), v));
                    }
                }
                // The table's DATA-BODY extent: a structured reference `Table1[Col]` (`[#Data]`)
                // resolves to rows [ref.top + headerRowCount .. ref.bottom - totalsRowCount], so a
                // tamper of either count silently re-aggregates every structured-reference formula
                // (headerRowCount default 1, totalsRowCount default 0) — normalized to the default so
                // a foreign tool writing it explicitly does not over-refuse (round-57 defect 2).
                out.push(format!(
                    "table.headerRowCount={}",
                    attr_by_local(&e, b"headerRowCount").unwrap_or_else(|| "1".into())
                ));
                out.push(format!(
                    "table.totalsRowCount={}",
                    attr_by_local(&e, b"totalsRowCount").unwrap_or_else(|| "0".into())
                ));
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"tableColumn") =>
            {
                for key in [b"name".as_slice(), b"totalsRowFunction".as_slice()] {
                    if let Some(v) = attr_by_local(&e, key) {
                        out.push(format!(
                            "col[{col_idx}].{}={}",
                            String::from_utf8_lossy(key),
                            v
                        ));
                    }
                }
                col_idx += 1;
            }
            Ok(Event::Start(e)) if is_tbl_f(e.name().as_ref()) => {
                in_f = true;
                raw.clear();
            }
            Ok(Event::Text(t)) if in_f => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if in_f => push_ref_raw(&mut raw, &r),
            Ok(Event::End(e)) if in_f && is_tbl_f(e.name().as_ref()) => {
                out.push(format!(
                    "f={}",
                    logical_formula(&raw).unwrap_or_else(|| raw.clone())
                ));
                in_f = false;
                raw.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// Every form-control / OLE / web-publish data-binding target on a sheet: the value of a
/// `linkedCell` / `fmlaLink` / `listFillRange` / `sourceRef` attribute (the cell a control
/// reads/writes), sorted. certify compares these so a foreign edit that RE-POINTS a control's
/// binding (a value/behavior change the cell diff never sees) is caught.
pub(crate) fn control_binding_attrs(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                for key in [
                    b"linkedCell".as_slice(),
                    b"fmlaLink".as_slice(),
                    b"listFillRange".as_slice(),
                    b"fmlaRange".as_slice(),
                    b"sourceRef".as_slice(),
                    b"link".as_slice(),
                    // Option-button-GROUP cell link and edit-box (textbox) cell link — genuine
                    // CT_FormControlPr cell references (the modern mirror of VML FmlaGroup/FmlaTxbx,
                    // compared below). A foreign RE-POINT of either writes/reads a different cell.
                    b"fmlaGroup".as_slice(),
                    b"fmlaTxbx".as_slice(),
                ] {
                    if let Some(v) = attr_by_local(&e, key) {
                        out.push(format!("{}={}", String::from_utf8_lossy(key), v));
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

/// True if a FOREIGN worksheet carries a reference to the edited sheet in a body the
/// foreign-sheet shift path does NOT rewrite, and that this edit would move. The shift
/// (`shift_text_in_element` with tag `f`) matches by LOCAL name, so it rewrites plain `<f>`
/// cell-formula text (shared formulas are expanded to plain `<f>` first) AND `<xm:f>` — the
/// x14/sparkline extLst formula element, whose local name is also `f`. It does NOT touch:
///   - a `<formula>`/`<formula1>`/`<formula2>` body (legacy conditional formatting / data
///     validation),
///   - an ARRAY `<f>` (`t="array"`) — `shift_text_in_element` skips these, so an array
///     formula's cross-reference is left stale.
///
/// Each such body is reassembled across Text + `GeneralRef` (resolving XML entities) and
/// tested with the σ oracle `formula_would_shift`, NOT a substring match — so a reference
/// qualified to the edited sheet via a case variant, a quoted/apostrophe name, an
/// entity-encoded qualifier, or a 3D span whose first endpoint is the edited sheet
/// (`Sheet1:Sheet3!`) is caught. Fail-closed on parse error.
/// True if a FOREIGN worksheet carries a `ref`/`sqref` ATTRIBUTE (on any element — e.g. a
/// dataConsolidate `<dataRef ref="Sheet1!..">`) that is QUALIFIED to the edited sheet and
/// that this edit would shift. The transform rewrites ref/sqref only on the edited sheet's
/// own `has_ref_attr` elements, so such a cross-sheet attribute is otherwise left stale.
/// Unqualified refs (local to the foreign sheet) are never flagged — the σ oracle's phantom
/// host means only a reference explicitly naming the edited sheet counts.
fn foreign_sheet_ref_attr_crosses(xml: &[u8], edit: &StructuralEdit) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                // ref/sqref (dataRef/…) plus a form-control / OLE / web-publish data binding
                // whose value names a cell — `linkedCell`/`fmlaLink`/`listFillRange`/`sourceRef`.
                // A foreign-sheet binding QUALIFIED to the edited sheet (`Sheet1!$A$5`) would be
                // left stale (the foreign shift path never rewrites it); the σ oracle's null
                // context sheet means an UNQUALIFIED binding to the control's own sheet is
                // correctly not flagged.
                // A `<dataRef ref="$A$1:$A$10" sheet="Data"/>` (CT_DataRef) names its source sheet
                // in a SEPARATE `sheet` attribute, with a BARE `ref` — the σ oracle evaluated against
                // the phantom host never sees the qualifier, so a dataRef targeting the edited sheet
                // was committed STALE (round-59 defect 3). When a `sheet` sibling attribute is
                // present, qualify a bare ref/sqref with it (mirroring rewrite_pivot's worksheetSource
                // handling) so the σ oracle recognizes the cross-reference.
                let sheet_attr = attr_by_local(&e, b"sheet");
                for key in [
                    b"ref".as_slice(),
                    b"sqref".as_slice(),
                    b"linkedCell".as_slice(),
                    b"fmlaLink".as_slice(),
                    b"listFillRange".as_slice(),
                    b"fmlaRange".as_slice(),
                    b"sourceRef".as_slice(),
                    b"link".as_slice(),
                    // Option-button-group / edit-box cell links (mirror of VML FmlaGroup/FmlaTxbx).
                    b"fmlaGroup".as_slice(),
                    b"fmlaTxbx".as_slice(),
                ] {
                    if let Some(v) = attr_by_local(&e, key) {
                        let is_ref = key == b"ref" || key == b"sqref";
                        // ref/sqref may be space-separated; test each token via the oracle. Use the
                        // QUOTE-AWARE tokenizer (round-52) so a space-containing quoted sheet
                        // qualifier (`'My Sheet'!$A$8`) stays ONE token — a raw split_whitespace tore
                        // it into `'My` + `Sheet'!$A$8`, neither of which the σ oracle recognizes, so
                        // a foreign binding to the edited sheet was committed STALE (silent
                        // mis-binding). The shift path already fixed this; the DETECTION path had
                        // drifted.
                        let crosses = split_sqref_tokens(&v).into_iter().any(|tok| {
                            if is_ref && !tok.contains('!') {
                                if let Some(s) = &sheet_attr {
                                    // A bare ref scoped to the sibling `sheet` — qualify it so the
                                    // oracle can tell whether the named sheet is the edited one.
                                    let q = format!("'{}'!{}", s.replace('\'', "''"), tok);
                                    return formula_would_shift(&q, edit);
                                }
                            }
                            formula_would_shift(tok, edit)
                        });
                        if crosses {
                            return true;
                        }
                    }
                }
            }
            Ok(Event::Eof) => return false,
            Err(_) => return true, // unparseable -> fail closed
            _ => {}
        }
        buf.clear();
    }
}

fn foreign_sheet_cross_ref_unshifted(xml: &[u8], edit: &StructuralEdit) -> bool {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    // Track when we are inside a body the foreign shift does NOT rewrite.
    let mut capture_depth = 0u32;
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let is_f = tag_local_eq(name.as_ref(), b"f");
                // A plain/shared `<f>` IS shifted (shared via expansion), so is `<xm:f>` (the
                // x14/sparkline extLst formula — same local name `f`), and so now are the legacy
                // CF/DV formula tags (`<formula>`/`<formula1>`/`<formula2>`, shifted in the foreign
                // path). The ONLY formula body the shift still skips is an ARRAY `<f>`.
                let unshifted_body = is_f && is_array_f(&e);
                if unshifted_body {
                    capture_depth += 1;
                    raw.clear();
                }
            }
            Ok(Event::End(_)) => {
                if capture_depth > 0 {
                    capture_depth -= 1;
                    let body = logical_formula(&raw).unwrap_or_else(|| raw.clone());
                    if formula_would_shift(&body, edit) {
                        return true;
                    }
                    raw.clear();
                }
            }
            Ok(Event::Text(t)) if capture_depth > 0 => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if capture_depth > 0 => push_ref_raw(&mut raw, &r),
            Ok(Event::Eof) => return false,
            Err(_) => return true,
            _ => {}
        }
        buf.clear();
    }
}

/// True if this table part's OWN formula (`calculatedColumnFormula`/`totalsRowFormula`) carries
/// a reference to the EDITED sheet that THIS edit would MOVE. We never rewrite a table part, so
/// such a formula would be left stale — refuse. But a purely STRUCTURED calculated column
/// (`[@Price]*[@Qty]`) or a totals row (`SUBTOTAL(109,Tbl[Amount])`) is table-local — it names
/// no sheet coordinate, so the σ oracle (phantom host) leaves it unchanged and it is allowed.
/// This stops over-refusing the ubiquitous computed-column table on an unrelated sheet.
/// Namespace-aware and fail-closed: a formula element the parser cannot read surfaces a
/// `parse_error` sentinel body, treated as crossing.
fn table_formula_crosses_edited(xml: &[u8], edit: &StructuralEdit) -> bool {
    element_text_semantics(xml, &[b"calculatedColumnFormula", b"totalsRowFormula"])
        .iter()
        .any(|f| f == "parse_error" || formula_would_shift(f, edit))
}

/// True if a cell-ref-shaped defined name is a real mis-shift hazard for THIS edit. Two
/// conditions must both hold (else the name is safe and refusing it is spurious):
///   1. The name, read as a cell reference, would actually MOVE under the edit — e.g. `Q1`
///      (row 1) is untouched by an insert at row 3, so no mis-tokenization could change it.
///   2. The name is USED (as text) in a formula the edit SHIFTS — the edited sheet's cells, a
///      chart data ref, or another defined name's body. A name used only in a FOREIGN sheet's
///      formula is never seen by the shift tokenizer. Substring match is sound (never misses a
///      real use); it may over-refuse on a rare coincidental substring, which is fail-closed.
fn defined_name_collision_risk(
    input: &[u8],
    name: &str,
    edit: &StructuralEdit,
    edited_part: &str,
) -> bool {
    // (1) aliased coordinate actually moves.
    if refshift::shift_formula(name, &edit.sheet, edit).0 == name {
        return false;
    }
    // (2) mentioned in a shifted formula body.
    let mentions = |xml: &[u8]| {
        element_text_semantics(
            xml,
            &[b"f", b"definedName", b"formula", b"formula1", b"formula2"],
        )
        .iter()
        .any(|f| f.contains(name))
    };
    // Edited sheet and workbook (defined-name bodies) are always in the shift path.
    if crate::ooxml::read_part(input, edited_part)
        .map(|x| mentions(&x))
        .unwrap_or(true)
    {
        return true;
    }
    if crate::ooxml::read_part(input, "xl/workbook.xml")
        .map(|x| mentions(&x))
        .unwrap_or(true)
    {
        return true;
    }
    // Chart data references are shifted too.
    if let Ok(parts) = archive_names(input) {
        for p in &parts {
            if p.starts_with("xl/charts/")
                && p.ends_with(".xml")
                && crate::ooxml::read_part(input, p)
                    .map(|x| mentions(&x))
                    .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// Does the sheet's OWN xml declare structured tables (`<tableParts>`)? This is the
/// authoritative, relationship-independent signal that a table's extent lives in this
/// sheet's coordinates. Consulted so a sheet whose `.rels` part is missing or
/// unreadable cannot slip a table past the scan (fail closed: unreadable => assume
/// tables). Namespace-aware.
fn sheet_declares_tables(input: &[u8], sheet_part: &str) -> bool {
    match crate::ooxml::read_part(input, sheet_part) {
        Ok(b) => xml_has_local_element(&b, &[b"tableParts"], true),
        Err(_) => true,
    }
}

/// True if this edit would MOVE the extent of the table in `table_part` — i.e. shifting the
/// table's `<table ref>` (sheet-local, on the edited sheet) under σ changes or consumes it.
/// We never rewrite a table part, so an affected extent must be refused; an edit strictly
/// below/right of the table leaves `ref` unchanged and is safe. An unreadable table part, or
/// one with no `ref`, is treated as affected (fail closed).
fn table_extent_affected(input: &[u8], table_part: &str, edit: &StructuralEdit) -> bool {
    let Ok(bytes) = crate::ooxml::read_part(input, table_part) else {
        return true;
    };
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if tag_local_eq(e.name().as_ref(), b"table") =>
            {
                let Some(r) = attr_by_local(&e, b"ref") else {
                    return true;
                };
                let (nv, _n, consumed, _all) = shift_sqref(&r, &edit.sheet, edit);
                return nv != r || consumed > 0;
            }
            Ok(Event::Eof) | Err(_) => return true,
            _ => {}
        }
        buf.clear();
    }
}

/// The `name` / `displayName` of every structured table in the package. A structured
/// reference in a formula is spelled `<name>[…]`, so these are the tokens the shift
/// algebra must not mangle.
fn table_display_names(input: &[u8], table_parts: &BTreeSet<String>) -> Vec<String> {
    let mut names = Vec::new();
    for t in table_parts {
        let bytes = match crate::ooxml::read_part(input, t) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let mut reader = Reader::from_reader(bytes.as_slice());
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if tag_local_eq(e.name().as_ref(), b"table") =>
                {
                    for key in [b"name".as_ref(), b"displayName".as_ref()] {
                        if let Some(v) = attr_by_local(&e, key) {
                            if !v.is_empty() {
                                names.push(v);
                            }
                        }
                    }
                    break;
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    names
}

/// True if any formula body in `xml` uses a structured reference to one of `names`
/// (`Name[…]`). The reference-shift algebra tokenizes the specifier inside `[…]` and
/// can mangle a column name that looks like an A1 ref (e.g. `Table1[Q4]` -> `[Q5]`),
/// so a workbook that uses structured references is refused. Namespace-aware over the
/// formula elements; an unparseable formula part fails closed (-> refuse).
fn part_uses_structured_ref(
    xml: &[u8],
    names: &[String],
    sheet: &str,
    edit: &StructuralEdit,
) -> bool {
    if names.is_empty() {
        return false;
    }
    let needles: Vec<String> = names
        .iter()
        .map(|n| format!("{}[", n.to_lowercase()))
        .collect();
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut depth = 0u32;
    // Accumulate the RAW (still-escaped) body across Text + GeneralRef exactly as the
    // shift path does, then resolve entities — otherwise `Table1&#91;Q4]` (a `[`
    // written as a numeric char-ref -> a GeneralRef event) would drop the bracket and
    // evade the `Name[` scan.
    let mut raw = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if is_formula_tag(e.name().as_ref()) => depth += 1,
            Ok(Event::End(e)) if is_formula_tag(e.name().as_ref()) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    // Fail closed: an entity we cannot resolve could hide a structured
                    // reference — refuse.
                    let logical = match logical_formula(&raw) {
                        Some(s) => s,
                        None => return true,
                    };
                    let hay = logical.to_lowercase();
                    if needles.iter().any(|p| hay.contains(p.as_str())) {
                        // AFFECT-based (not presence): the `[…]` specifier can only be mangled if
                        // the shift actually REWRITES this formula. If σ leaves it byte-identical —
                        // the edit moves nothing it references (e.g. an insert far below the table
                        // and the formula) — the structured reference is copied verbatim and is
                        // safe. Only refuse when σ would change it (a real shift that could also
                        // touch the specifier). This unblocks the ubiquitous `=SUM(Table[Col])`
                        // idiom for edits that move nothing.
                        if refshift::shift_formula(&logical, sheet, edit).0 != logical {
                            return true;
                        }
                    }
                    raw.clear();
                }
            }
            Ok(Event::Text(t)) if depth > 0 => push_text_raw(&mut raw, &t),
            Ok(Event::GeneralRef(r)) if depth > 0 => push_ref_raw(&mut raw, &r),
            Ok(Event::Eof) => return false,
            Err(_) => return true,
            _ => {}
        }
        buf.clear();
    }
}

/// Every table part in the package: the conventional `xl/tables/*.xml` path UNIONED
/// with the targets of EVERY table relationship declared anywhere in the package. A
/// crafted file can put the sheet and/or the table part at a non-conventional path;
/// scanning all `.rels` means the formula scan below can never miss one.
fn all_table_parts(input: &[u8], names: &[String]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = names
        .iter()
        .filter(|n| n.starts_with("xl/tables/") && n.ends_with(".xml"))
        .cloned()
        .collect();
    for n in names {
        if n.contains("/_rels/") && n.ends_with(".rels") {
            for t in table_targets_in_rels(input, n) {
                out.insert(t);
            }
        }
    }
    out
}

fn scan_extra_residuals(
    names: &[String],
    input: &[u8],
    edit: &StructuralEdit,
    edited_part: &str,
    sheet_names: &[String],
    report: &mut StructuralReport,
) {
    // (0a) NON-STANDARD WORKSHEET PATHS. The main shift loop keys foreign-sheet
    // handling on the path pattern `xl/worksheets/sheet*.xml`; a worksheet part at
    // any other path (fully legal OOXML — the part is resolved via workbook.xml.rels)
    // is copied byte-for-byte and its cross-sheet references left stale, and the
    // formula scans below (which use the same pattern) skip it. Rather than shift by
    // path (a change to the certified engine), refuse a workbook with any worksheet at
    // a non-conventional path. Real producers always use `sheetN.xml`.
    if let Ok(sheets) = crate::ooxml::all_sheets(input) {
        for (name, part) in &sheets {
            let conventional = part
                .strip_prefix("xl/worksheets/sheet")
                .and_then(|s| s.strip_suffix(".xml"))
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
            // A CHARTSHEET / DIALOGSHEET is listed in <sheets> like a worksheet but carries
            // no cell grid — its chart data references live in xl/charts/*.xml, which the
            // chart path already shifts. So a non-`xl/worksheets/` path for one of these is
            // expected, not a hidden worksheet; do not refuse it. (Macrosheets can carry XLM
            // formula cells, so they stay fail-closed.)
            let non_grid_sheet =
                part.starts_with("xl/chartsheets/") || part.starts_with("xl/dialogsheets/");
            if !conventional && !non_grid_sheet {
                report.residuals.push(Residual {
                    part: part.clone(),
                    reason: "nonstandard_sheet_path".into(),
                    detail: format!(
                        "worksheet '{name}' is at a non-conventional part path ('{part}'); the \
                         shift engine keys on xl/worksheets/sheetN.xml — edit refused (fail-closed)"
                    ),
                });
            }
        }
    }

    // (0b) COORDINATE-BEARING ATTACHMENTS ON THE EDITED SHEET (fail-closed whitelist).
    // Drawings/images, cell comments (+ legacy VML), pivot tables, form/OLE controls,
    // timelines, slicers … live in the edited sheet's coordinates but are copied
    // byte-for-byte, never shifted, so they detach after the edit. Only external-URL
    // hyperlinks and printer settings are coordinate-free-safe (tables are handled by
    // the dedicated guard below). Anything else — including a future, unrecognized
    // attachment type — refuses.
    if let Some(bad) = edited_sheet_bad_attachment(input, edited_part, edit) {
        report.residuals.push(Residual {
            part: edited_part.to_string(),
            reason: "unshiftable_sheet_attachment".into(),
            detail: format!(
                "the edited sheet has a '{bad}' attachment whose coordinates we do not shift \
                 (drawings/comments/pivots/controls are copied verbatim) — edit refused (fail-closed)"
            ),
        });
    }

    // (0c) EXTERNAL LINKS are NOT refused (round-58 defect 5). A cross-workbook reference is spelled
    // `[n]Sheet!ref`, and `shift_ref` returns Shift::Unchanged for any `[`-prefixed reference (proven
    // across the plain / mixed-with-local / range / defined-name / quoted forms) — so shift_formula
    // shifts only the LOCAL portion and leaves every external ref intact — while the cached grid in
    // `xl/externalLinks/*` is keyed by the EXTERNAL workbook's coordinates (never the edited sheet's)
    // and is copied verbatim like any unrelated part. An intra-workbook row/column edit therefore
    // never touches an external reference, so a pure coordinate shift IS coordinate-expressible and
    // faithful; the old presence-refusal declined a provably-equivalent edit (an over-refusal, not a
    // sound fail-safe).

    // (a) Structured tables carry an extent (`ref` / `autoFilter`) we do not rewrite.
    // But a table is endangered by THIS edit only if either:
    //   - it is attached to the EDITED sheet (its extent lives in that sheet's
    //     coordinates and would have to shift), or
    //   - it carries its own formula (`calculatedColumnFormula` / `totalsRowFormula`),
    //     which may hold a CROSS-SHEET reference; we never rewrite table parts, so we
    //     cannot prove such a formula is unaffected — refuse conservatively.
    // A plain table on an unrelated sheet is genuinely untouched by a row/column edit
    // elsewhere (its coordinates are sheet-local, and the part is copied byte-for-byte).
    // Refusing those was a FALSE refusal that made xlq decline workbooks it handles
    // correctly.
    let edited_tables = tables_attached_to(input, edited_part);
    // Declared-but-unreadable: the edited sheet's xml has <tableParts> but its rels did not
    // resolve a table part, so we cannot read the extent to prove it unaffected — fail closed.
    if edited_tables.is_empty() && sheet_declares_tables(input, edited_part) {
        report.residuals.push(Residual {
            part: edited_part.to_string(),
            reason: "table_unsupported".into(),
            detail: "the edited sheet declares a structured table whose extent we cannot read \
                     to prove unaffected; edit refused (fail-closed)"
                .into(),
        });
    }
    let table_parts = all_table_parts(input, names);
    for t in &table_parts {
        // An edited-sheet table whose EXTENT this edit MOVES (an insert/delete at or before its
        // rows/cols) must be refused — we do not rewrite a table part's `<ref>`. An edit
        // strictly below/right of the table leaves its extent correct, so it is allowed (a
        // Table with a summary block below it is a very common, faithfully-handled layout).
        if edited_tables.contains(t) && table_extent_affected(input, t, edit) {
            report.residuals.push(Residual {
                part: t.clone(),
                reason: "table_unsupported".into(),
                detail: "a structured table on the edited sheet has an extent this edit moves, \
                         which we do not rewrite; edit refused (fail-closed)"
                    .into(),
            });
            continue;
        }
        // A table's OWN formula (calculated column / totals row) that REFERENCES the edited
        // sheet at a moved cell is not rewritten — refuse. A table-local structured formula
        // (`[@Price]*[@Qty]`, `SUBTOTAL(109,Tbl[Amount])`) references no sheet coordinate and is
        // safe, even on the edited sheet. A table part we cannot read is one we cannot clear.
        let refuse_formula = match crate::ooxml::read_part(input, t) {
            Ok(bytes) => table_formula_crosses_edited(&bytes, edit),
            Err(_) => true,
        };
        if refuse_formula {
            report.residuals.push(Residual {
                part: t.clone(),
                reason: "table_formula_unsupported".into(),
                detail: "a structured table's own formula (calculated column / totals row) \
                         references the edited sheet at a cell this edit moves, and is not \
                         rewritten; edit refused (fail-closed)"
                    .into(),
            });
        }
    }
    // (c) STRUCTURED REFERENCES (`Table1[Col]`) in any formula: the shift algebra
    // tokenizes the specifier inside `[…]` and can mangle a column name that looks
    // like an A1 ref (`Table1[Q4]` -> `[Q5]`), silently breaking the reference. The
    // old refuse-any-table rule masked this; now that a plain table on an unrelated
    // sheet is allowed, we must refuse a workbook whose formulas actually use a
    // structured reference. Scanned across all formula-bearing parts below.
    let table_names = table_display_names(input, &table_parts);
    // DEFINED-NAME ALIASING: a defined name spelled like a grid-valid cell (e.g.
    // `FY2021` = col FY, row 2021) is indistinguishable from a reference to the
    // shift tokenizer, so a formula using it would be silently mis-shifted AND the
    // resulting file would still equal xlq's own (wrong) transform — the one place
    // certified⇒correct could be false on a real workbook. Decidable from the
    // names table the file already carries: detect it and REFUSE (fail closed).
    if let Ok(bytes) = crate::ooxml::read_part(input, "xl/workbook.xml") {
        for (name, _scope, _refers) in defined_names(&bytes) {
            if refshift::looks_like_cell_ref(&name)
                && defined_name_collision_risk(input, &name, edit, edited_part)
            {
                report.residuals.push(Residual {
                    part: "xl/workbook.xml".into(),
                    reason: "defined_name_ref_collision".into(),
                    detail: format!(
                        "defined name '{name}' is spelled like a cell reference this edit moves, \
                         and is used in a formula the edit shifts — its uses cannot be \
                         distinguished from cell refs; edit refused (fail-closed)"
                    ),
                });
            }
        }
    }
    // scan formula text across sheets, charts, workbook for unverifiable 3D spans
    let scan_parts: Vec<&String> = names
        .iter()
        .filter(|n| {
            (n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
                || (n.starts_with("xl/charts/") && n.ends_with(".xml"))
                || *n == "xl/workbook.xml"
        })
        .collect();
    for n in scan_parts {
        if let Ok(bytes) = crate::ooxml::read_part(input, n) {
            // Scan only FORMULA bodies, NOT the raw part text: an inline-string cell
            // (`<t>Enter totals in A1:A5!</t>`) or a cached value can legitimately contain an
            // `X:Y!` substring, which a whole-text scan misreads as a 3D interior-tab span or a
            // non-ASCII qualifier. A live reference can appear only inside a formula element.
            let formula_texts = element_text_semantics(
                &bytes,
                &[b"f", b"formula", b"formula1", b"formula2", b"definedName"],
            );
            // NON-ASCII edited-sheet name: the reference tokenizer only starts a candidate
            // at an ASCII letter/$/digit, so an UNQUOTED non-ASCII sheet qualifier (集計!A11)
            // is never parsed and the σ-oracle silently leaves such a cross-reference stale.
            // We cannot shift it — but the CELL part after the `!` is ASCII and parseable, so
            // we refuse only when THIS edit would actually move that cell (an affect check).
            // An edit far from the referenced cell (row 50 insert vs a row-11 reference) shifts
            // nothing and must not refuse — presence alone blocked every edit on a CJK/Cyrillic
            // sheet. (A quoted '集計'! reference is handled by the tokenizer's quoted path.)
            if !edit.sheet.is_ascii()
                && formula_texts
                    .iter()
                    .any(|f| non_ascii_qualifier_affected(f, &edit.sheet, edit))
            {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "non_ascii_sheet_qualifier".into(),
                    detail: "a reference qualifies the edited sheet by an unquoted non-ASCII \
                             name (which the tokenizer cannot parse) at a cell THIS edit moves — \
                             edit refused (fail-closed)"
                        .into(),
                });
            }
            if formula_texts
                .iter()
                .any(|f| refshift::has_unverifiable_3d_span(f, sheet_names, edit))
            {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "threeD_span_unverifiable".into(),
                    detail:
                        "a multi-sheet 3D span covering the edited sheet has a coordinate this \
                             edit moves, which cannot be shifted uniformly across the span — edit \
                             refused (fail-closed)"
                            .into(),
                });
            }
            if has_cdata_formula_body(&bytes) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "cdata_formula_body".into(),
                    detail: "a formula body is wrapped in CDATA, which the reference-shift \
                             path does not reassemble — edit refused (fail-closed)"
                        .into(),
                });
            }
            // Only formulas the shift algebra REWRITES can have their `[…]` specifier
            // mangled: the edited sheet's own cells, chart `<c:f>` data ranges, and workbook
            // defined names. A structured reference on a FOREIGN worksheet is copied verbatim
            // (never shifted), so it cannot be mangled — refusing it blocked ordinary
            // table-driven workbooks for a safe edit on an unrelated sheet.
            let shifted_part =
                n == edited_part || n.starts_with("xl/charts/") || n == "xl/workbook.xml";
            if shifted_part && part_uses_structured_ref(&bytes, &table_names, &edit.sheet, edit) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "structured_reference_unsupported".into(),
                    detail: "a formula the edit shifts uses a structured table reference \
                             (Table[Column]); the shift algebra can mangle the specifier inside \
                             [] — edit refused (fail-closed)"
                        .into(),
                });
            }
            // (Internal hyperlink `location`s are SHIFTED, not refused — see
            // shift_hyperlink_locations in the per-sheet rewrite above.)
            // EXTENSION-LIST value content on the EDITED sheet: x14 conditional formatting
            // (<xm:f>/<xm:sqref>) and sparklines carry edited-sheet coordinates the base
            // shift never processes. AFFECT-based: refuse ONLY when this edit would actually
            // move one of those coordinates — a data bar / color scale / sparkline that the
            // edit does not touch is unaffected, and Excel writes those on nearly every real
            // workbook, so a presence-refuse would reject almost every legitimate edit.
            if n == edited_part && sheet_extlst_affected(&bytes, &edit.sheet, edit) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "extension_construct_unsupported".into(),
                    detail: "the edited sheet has an extension-list construct (x14 conditional \
                             formatting or sparklines) whose reference this edit would move but \
                             the base shift does not rewrite — edit refused (fail-closed)"
                        .into(),
                });
            }
            // EDITED-SHEET BODY constructs the shift engine copies verbatim (it shifts
            // ref/sqref only for the has_ref_attr set and `r` only on cells): a
            // <protectedRange sqref> (security), <inputCells r> (scenario write target),
            // <dataRef ref>, … would be left stale.
            if n == edited_part {
                if let Some(elem) = edited_sheet_body_unshifted_ref(&bytes, &edit.sheet, edit) {
                    report.residuals.push(Residual {
                        part: n.clone(),
                        reason: "unshiftable_body_reference".into(),
                        detail: format!(
                            "the edited sheet carries a <{elem}> whose cell reference the shift \
                             engine does not rewrite; it would be left stale — edit refused \
                             (fail-closed)"
                        ),
                    });
                }
                // GRID OVERFLOW: an insert that would push a populated row/column past the grid
                // edge. The row/cell RELOCATION path (shift_line/shift_cell_tag) does not clamp
                // — unlike the reference-shift path — so without this it emits an out-of-grid
                // `<row r="1048577">` and orphans a datum out of a SUM (a silent value change).
                // Excel itself refuses this ("cannot shift nonblank cells off the worksheet").
                if insert_overflows_grid(&bytes, edit) {
                    report.residuals.push(Residual {
                        part: n.clone(),
                        reason: "grid_overflow".into(),
                        detail: "an insert would push a populated row/column past the grid edge \
                                 (row 1048576 / column XFD); Excel refuses this (data loss) — \
                                 edit refused (fail-closed)"
                            .into(),
                    });
                }
            }
            // FOREIGN-SHEET references to the edited sheet in a construct the foreign
            // shift does not rewrite (only <f> is shifted there): a conditional-format
            // or data-validation formula, or an extLst formula, that names the edited
            // sheet is left stale. (On the edited sheet these ARE shifted.)
            if n != edited_part
                && n.starts_with("xl/worksheets/")
                && foreign_sheet_cross_ref_unshifted(&bytes, edit)
            {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "cross_sheet_reference_unsupported".into(),
                    detail:
                        "a foreign sheet references the edited sheet from a conditional-format \
                             / data-validation / extension formula the shift does not rewrite — \
                             edit refused (fail-closed)"
                            .into(),
                });
            }
            // FOREIGN-SHEET ref/sqref ATTRIBUTE qualified to the edited sheet (e.g. a
            // dataConsolidate <dataRef ref="Sheet1!..">): the transform shifts ref/sqref only
            // on the edited sheet's own has_ref_attr elements, so this is left stale.
            if n != edited_part
                && n.starts_with("xl/worksheets/")
                && foreign_sheet_ref_attr_crosses(&bytes, edit)
            {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "cross_sheet_reference_unsupported".into(),
                    detail: "a foreign sheet has a ref/sqref attribute qualified to the edited \
                             sheet (e.g. a consolidation dataRef) that is not shifted — edit \
                             refused (fail-closed)"
                        .into(),
                });
            }
        }
    }
    // MODERN FORM CONTROL bindings (`xl/ctrlProps/*`): a `<formControlPr fmlaLink=…>` (or
    // linkedCell/fmlaRange/sourceRef) qualified to the edited sheet is copied verbatim — a
    // ctrlProps part is NOT a worksheet, so the worksheet cross-ref scans above skip it. certify
    // DOES compare ctrlProps bindings, so leaving one stale both silently mis-binds the control
    // AND inverts certify (it would refuse the faithful edit and certify xlq's stale one). Refuse
    // it — fail-closed and consistent with the inline `<controlPr>` case.
    for n in names.iter().filter(|n| {
        let low = n.to_ascii_lowercase();
        low.starts_with("xl/ctrlprops/") && low.ends_with(".xml")
    }) {
        if let Ok(bytes) = crate::ooxml::read_part(input, n) {
            if foreign_sheet_ref_attr_crosses(&bytes, edit) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "cross_sheet_reference_unsupported".into(),
                    detail: "a modern form control (xl/ctrlProps) has a data binding qualified to \
                             the edited sheet that is not shifted — edit refused (fail-closed)"
                        .into(),
                });
            }
        }
    }
    // LEGACY VML FORM CONTROL bindings (`xl/drawings/*.vml`): a form control's cell binding lives
    // in ELEMENT TEXT (`<x:FmlaLink>Sheet1!$A$8</x:FmlaLink>`, `<x:FmlaRange>`/`<x:FmlaTxbx>`/
    // `<x:FmlaGroup>`), not an attribute, and .vml is not a worksheet — so both the worksheet
    // cross-ref scans and the ctrlProps (attribute) scan skip it. A foreign-sheet control bound to
    // the edited sheet would be committed STALE (the control re-binds to the wrong, now-shifted
    // cell), and because certify DOES compare VML FmlaLink it would then invert (refuse the
    // faithful edit, certify xlq's stale one). Refuse — fail-closed, symmetric with ctrlProps.
    for n in names
        .iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".vml"))
    {
        if let Ok(bytes) = crate::ooxml::read_part(input, n) {
            if vml_binding_crosses_edited(&bytes, edit) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "cross_sheet_reference_unsupported".into(),
                    detail: "a legacy VML form control has a cell binding (FmlaLink/FmlaRange/…) \
                             qualified to the edited sheet that is not shifted — edit refused \
                             (fail-closed)"
                        .into(),
                });
            }
        }
    }
}

/// True if any legacy VML form-control cell binding (`<x:FmlaLink>`/`<x:FmlaRange>`/`<x:FmlaTxbx>`/
/// `<x:FmlaGroup>` element TEXT) names a cell this edit MOVES, evaluated with `host` scoping
/// UNqualified refs. Pass the phantom host (`\0`) for a FOREIGN sheet's VML (only a ref explicitly
/// qualified to the edited sheet counts); pass the EDITED sheet name for the edited sheet's own VML
/// (so a local, unqualified `$A$8` counts too). (`FmlaMacro` is a macro NAME, not a cell ref.)
fn vml_binding_affected_on_host(xml: &[u8], edit: &StructuralEdit, host: &str) -> bool {
    element_text_semantics(xml, &[b"FmlaLink", b"FmlaRange", b"FmlaTxbx", b"FmlaGroup"])
        .iter()
        .any(|t| {
            let (shifted, _n) = refshift::shift_formula(t, host, edit);
            shifted != *t
        })
}

/// Foreign-sheet variant: a VML binding CROSSES to the edited sheet (phantom host, so only an
/// explicitly-qualified `Sheet1!…` ref counts — an unqualified binding is local to the control's
/// own foreign sheet and unaffected by an edit elsewhere).
fn vml_binding_crosses_edited(xml: &[u8], edit: &StructuralEdit) -> bool {
    vml_binding_affected_on_host(xml, edit, "\u{0}")
}

/// True if this edit MOVES the anchor cell of any legacy VML shape (a comment note box or a form
/// control): `<x:Row>`/`<x:Column>` (0-based single cell) or the 8-number `<x:Anchor>` (indices
/// 2,6 = top/bottom row; 0,4 = left/right col). The VML is copied verbatim, so a moved anchor
/// leaves the note/control displaced onto the wrong cell.
fn vml_anchor_affected(xml: &[u8], edit: &StructuralEdit) -> bool {
    let row_axis = edit.axis == Axis::Row;
    let single: &[u8] = if row_axis { b"Row" } else { b"Column" };
    let moved = |idx0: u32| {
        let one_based = idx0 + 1;
        shift_line(one_based, edit) != Some(one_based)
    };
    for t in element_text_semantics(xml, &[single]) {
        if let Ok(i0) = t.trim().parse::<u32>() {
            if moved(i0) {
                return true;
            }
        }
    }
    for t in element_text_semantics(xml, &[b"Anchor"]) {
        let nums: Vec<u32> = t
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        let idxs: &[usize] = if row_axis { &[2, 6] } else { &[0, 4] };
        if idxs.iter().any(|&i| nums.get(i).is_some_and(|&v| moved(v))) {
            return true;
        }
    }
    false
}

/// True if this edit MOVES the anchor cell of any legacy note (`<comment ref>`) or threaded
/// comment (`<threadedComment ref>`), which a verbatim copy would leave anchored to the wrong cell.
fn comment_refs_affected(xml: &[u8], edit: &StructuralEdit) -> bool {
    for (_, attrs) in element_attr_semantics(xml, &[b"comment", b"threadedComment"]) {
        if let Some(r) = attrs.split(ATTR_SEP).find_map(|kv| kv.strip_prefix("ref=")) {
            if let Some((col, row)) = parse_cell_rc(r) {
                let line = if edit.axis == Axis::Row { row } else { col };
                if shift_line(line, edit) != Some(line) {
                    return true;
                }
            }
        }
    }
    false
}

/// True if THIS edit would move a PivotTable's render `<location ref>` (its display extent on this
/// sheet), so the pivot cannot be committed with a stale render position. A pivot whose extent lies
/// entirely outside the edit's shift band is unaffected — faithful to copy verbatim. Fail-closed
/// (returns true) if the part is malformed, has no location, or carries an unparseable ref.
fn pivot_location_affected(xml: &[u8], edit: &StructuralEdit) -> bool {
    let mut saw_location = false;
    for (tag, attrs) in element_attr_semantics(xml, &[b"location"]) {
        if tag == "parse_error" {
            return true;
        }
        saw_location = true;
        let Some(r) = attrs.split(ATTR_SEP).find_map(|kv| kv.strip_prefix("ref=")) else {
            return true; // a location with no ref -> fail closed
        };
        for endpoint in r.split(':') {
            let Some((col, row)) = parse_cell_rc(endpoint) else {
                return true; // unparseable endpoint -> fail closed
            };
            let line = if edit.axis == Axis::Row { row } else { col };
            if shift_line(line, edit) != Some(line) {
                return true;
            }
        }
    }
    // A pivotTable part always has a <location>; its absence means we cannot prove it is unaffected.
    !saw_location
}

/// True if any shiftable formula body (`<f>`, `<formula*>`, or `<definedName>`) in
/// this part is wrapped in CDATA. The shift paths reassemble a formula only from
/// `Event::Text` + entity (`Event::GeneralRef`) events; a CDATA body arrives as
/// `Event::CData`, is NOT reassembled, and would be committed UNSHIFTED with no
/// residual — a silent-wrong output. We detect it up front and refuse. CDATA in a
/// formula body does not occur in workbooks real tools produce, so refusing it is
/// the correct fail-closed choice (never silently wrong on untrusted input).
fn has_cdata_formula_body(xml: &[u8]) -> bool {
    let is_formula = |name: &[u8]| is_formula_tag(name) || tag_local_eq(name, b"definedName");
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut depth = 0u32;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if is_formula(e.name().as_ref()) => depth += 1,
            Ok(Event::End(e)) if is_formula(e.name().as_ref()) => {
                depth = depth.saturating_sub(1);
            }
            Ok(Event::CData(_)) if depth > 0 => return true,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    false
}

/// (name, scope, refers-to) for every `<definedName>` in workbook.xml, sorted. `scope` is
/// the `localSheetId` (empty string for a workbook-global name) — Excel resolves a name by
/// its scope, so a foreign edit that RE-SCOPES a name (or swaps two same-named names'
/// scopes) is a semantic change certify must catch; comparing (name, refers-to) alone
/// missed it. Namespace-prefix-insensitive (matches a prefixed `<x:definedName>` too) and
/// entity-resolving on the `name` attribute and the refers-to body — mirroring the shifter,
/// which rewrites defined names via `tag_local_eq`. The old raw-substring scan was blind to
/// a prefixed `<x:definedName>`: it hid such a name from the collision check AND (in
/// certify) let a stale prefixed defined name compare equal to xlq's shifted one.
pub(crate) fn defined_names(workbook_xml: &[u8]) -> Vec<(String, String, String)> {
    let mut reader = Reader::from_reader(workbook_xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut cur_name: Option<String> = None;
    let mut cur_scope = String::new();
    let mut body = String::new();
    // The name's SCOPE plus its value/security-affecting attributes: `localSheetId`, then a
    // `|function`/`|vbProcedure`/`|hidden` suffix for each set flag. `function`/`vbProcedure`
    // reclassify a name from a data-range reference into a VBA UDF/macro binding (a computed-
    // value + macro-execution change); `hidden` conceals it. A no-flag name keeps just its
    // localSheetId, so the common case is unchanged.
    let scope_of = |e: &BytesStart| {
        let mut sig = attr_by_local(e, b"localSheetId").unwrap_or_default();
        for k in [
            b"function".as_slice(),
            b"vbProcedure".as_slice(),
            b"hidden".as_slice(),
        ] {
            if attr_by_local(e, k)
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            {
                sig.push('|');
                sig.push_str(&String::from_utf8_lossy(k));
            }
        }
        sig
    };
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), b"definedName") => {
                cur_name = Some(attr_by_local(&e, b"name").unwrap_or_default());
                cur_scope = scope_of(&e);
                body.clear();
            }
            Ok(Event::Empty(e)) if tag_local_eq(e.name().as_ref(), b"definedName") => {
                // Self-closing: a name with an empty refers-to body.
                out.push((
                    attr_by_local(&e, b"name").unwrap_or_default(),
                    scope_of(&e),
                    String::new(),
                ));
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), b"definedName") => {
                if let Some(name) = cur_name.take() {
                    let refers = logical_formula(&body).unwrap_or_else(|| body.clone());
                    out.push((name, std::mem::take(&mut cur_scope), refers));
                }
                body.clear();
            }
            Ok(Event::Text(t)) if cur_name.is_some() => push_text_raw(&mut body, &t),
            Ok(Event::GeneralRef(r)) if cur_name.is_some() => push_ref_raw(&mut body, &r),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// raw attribute-value surgery (keeps sibling bytes identical)
// ---------------------------------------------------------------------------

/// Replace the value of attribute `key` in a tag's inner bytes, preserving the
/// quote char and every other byte. Returns the new inner bytes, or the input
/// unchanged if the attribute isn't found.
fn replace_attr_value(inner: &[u8], key: &[u8], new_val: &str) -> Vec<u8> {
    let s = inner;
    let mut i = 0;
    while i + key.len() < s.len() {
        let at_boundary = i == 0 || s[i - 1].is_ascii_whitespace();
        if at_boundary && s[i..].starts_with(key) && s.get(i + key.len()) == Some(&b'=') {
            let qpos = i + key.len() + 1;
            if let Some(&q) = s.get(qpos) {
                if q == b'"' || q == b'\'' {
                    if let Some(rel) = s[qpos + 1..].iter().position(|&b| b == q) {
                        let end = qpos + 1 + rel;
                        let mut out = Vec::with_capacity(s.len());
                        out.extend_from_slice(&s[..qpos + 1]);
                        out.extend_from_slice(xml_attr_escape(new_val).as_bytes());
                        out.extend_from_slice(&s[end..]);
                        return out;
                    }
                }
            }
        }
        i += 1;
    }
    s.to_vec()
}

fn xml_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Minimal XML text-content escaping — only the characters that MUST be escaped
/// in element text (`&`, `<`, `>`). Crucially leaves `'` and `"` literal, so a
/// shifted formula like `'Data'!$A$6` keeps its apostrophes exactly as Excel
/// wrote them (quick-xml's default writer would emit `&apos;`, breaking the
/// minimal-patch invariant on sheet-qualified references).
fn text_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// --- Formula-body reassembly across quick-xml >=0.38 Text + GeneralRef events ---
//
// Since quick-xml 0.38 an entity reference (`&gt;` `&amp;` `&#60;` …) inside
// element text is no longer part of `Event::Text` — it is emitted as a separate
// `Event::GeneralRef`. A formula like `IF(A5>0,A5&"x")` therefore arrives as
// Text("IF(A5") + GeneralRef("gt") + Text("0,A5") + GeneralRef("amp") + Text(...).
// Each `<f>` body must be REASSEMBLED across all its Text+GeneralRef events, then
// shifted ONCE — shifting the fragments independently would silently corrupt
// exactly the formulas this tool promises never to corrupt. We accumulate the
// original (still-escaped) bytes so an unchanged formula is written back
// byte-identically, and derive the logical text via a single `unescape`.

/// Append a formula-body `Event::Text` fragment to the raw (escaped) accumulator.
fn push_text_raw(acc: &mut String, t: &BytesText) {
    acc.push_str(&t.decode().unwrap_or_default());
}

/// Append a formula-body `Event::GeneralRef` (an entity like `gt`/`#60`) to the
/// raw accumulator, reconstructing the exact `&name;` bytes it came from.
fn push_ref_raw(acc: &mut String, r: &BytesRef) {
    acc.push('&');
    acc.push_str(&r.decode().unwrap_or_default());
    acc.push(';');
}

/// Resolve a reassembled raw formula body (with XML entities) to its logical
/// text for the reference-shift algebra. Returns `None` when it carries an
/// entity outside the XML predefined set / char-refs (fail-closed: the caller
/// writes the raw bytes back verbatim rather than mis-shift).
fn logical_formula(raw: &str) -> Option<String> {
    quick_xml::escape::unescape(raw)
        .ok()
        .map(|c| c.into_owned())
}

/// A sheet name is safe to write UNQUOTED iff it is a plain identifier: a non-empty run of
/// `[A-Za-z0-9_.]` that does not begin with a digit. Names needing quotes (spaces,
/// punctuation, an embedded apostrophe, a leading digit) return false and keep their quotes,
/// so canonicalization can never merge two DISTINCT sheet names.
fn sheet_name_safe_unquoted(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Normalize REDUNDANT sheet-name quoting in a formula / refers-to body so two encodings of the
/// SAME reference key identically. A sheet name that needs no quoting may be written quoted
/// (`'Data'!$A$1`, as openpyxl emits for the `_xlnm._FilterDatabase` autofilter name) or
/// unquoted (`Data!$A$1`, as Excel/LibreOffice emit) — semantically identical, so comparing the
/// raw bodies spuriously refuses a faithful edit. Only a quoted token that (a) is immediately
/// followed by `!` or `:` (a sheet qualifier, never a string literal — those use `"`) and (b)
/// holds a plain identifier is unquoted; everything else is copied verbatim, so no two distinct
/// references collide (a `#REF!` swap, a different sheet, an apostrophe-bearing name all differ).
pub(crate) fn canonicalize_sheet_quotes(f: &str) -> String {
    let mut out = String::with_capacity(f.len());
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                // Double-quoted string literal: copy verbatim, honoring `""` escapes.
                out.push('"');
                while let Some(d) = chars.next() {
                    out.push(d);
                    if d == '"' {
                        if chars.peek() == Some(&'"') {
                            out.push('"');
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
            }
            '\'' => {
                // Single-quoted sheet qualifier: capture the inner name (honoring `''` escapes).
                let mut inner = String::new();
                let mut closed = false;
                while let Some(d) = chars.next() {
                    if d == '\'' {
                        if chars.peek() == Some(&'\'') {
                            inner.push('\'');
                            chars.next();
                            continue;
                        }
                        closed = true;
                        break;
                    }
                    inner.push(d);
                }
                let is_qualifier = matches!(chars.peek(), Some('!') | Some(':'));
                if closed && is_qualifier && sheet_name_safe_unquoted(&inner) {
                    out.push_str(&inner);
                } else {
                    // Not a redundant quote — reconstruct the token exactly.
                    out.push('\'');
                    for ch in inner.chars() {
                        out.push(ch);
                        if ch == '\'' {
                            out.push('\'');
                        }
                    }
                    if closed {
                        out.push('\'');
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Build a BytesStart from raw inner bytes (name + attributes).
fn tag_from_inner(inner: Vec<u8>, name_len: usize) -> BytesStart<'static> {
    BytesStart::from_content(String::from_utf8_lossy(&inner).into_owned(), name_len)
}

/// Apply a set of attribute-value replacements to a tag by raw surgery.
fn set_attrs(e: &BytesStart, repl: &[(&[u8], String)]) -> BytesStart<'static> {
    if repl.is_empty() {
        return e.to_owned();
    }
    let mut inner = e.as_ref().to_vec();
    for (k, v) in repl {
        inner = replace_attr_value(&inner, k, v);
    }
    tag_from_inner(inner, e.name().as_ref().len())
}

// ---------------------------------------------------------------------------
// edited sheet
// ---------------------------------------------------------------------------

fn rewrite_edited_sheet(
    src: &[u8],
    edit: &StructuralEdit,
    part_name: &str,
    report: &mut StructuralReport,
) -> Result<Vec<u8>> {
    // Move REORDERS rows (σ is non-monotonic), so the STREAMING insert/delete
    // renumber below cannot be used — it assumes rows stay in ascending order.
    // The buffered path collects, relabels, and re-emits rows sorted by σ.
    if edit.op == Op::Move {
        return rewrite_edited_sheet_move(src, edit, part_name, report);
    }
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let sheet = edit.sheet.clone();
    let row_axis = edit.axis == Axis::Row;
    let col_axis = edit.axis == Axis::Col;
    let mut inserted = false;
    let mut in_f = false;
    let mut f_residual = false;
    // Reassembled formula body across quick-xml Text + GeneralRef events; the
    // shift/writeback happens once, at the closing </f> (see push_text_raw).
    let mut f_raw = String::new();
    // When the current `<f>` is an ARRAY, its `ref` extent — the affect decision is DEFERRED to
    // `</f>` (it needs the reassembled body). The array `<f>`/body are copied verbatim.
    let mut f_array_ref: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,

            Event::Start(e) if row_axis && e.name().as_ref() == b"row" => {
                maybe_inject(&mut writer, &e, edit, &mut inserted, report)?;
                if delete_skip(&e, edit) {
                    reader.read_to_end(e.name())?;
                    report.rows_deleted = edit.count;
                    buf.clear();
                    continue;
                }
                writer.write_event(Event::Start(shift_row_tag(&e, edit)))?;
            }
            Event::Empty(e) if row_axis && e.name().as_ref() == b"row" => {
                maybe_inject(&mut writer, &e, edit, &mut inserted, report)?;
                if delete_skip(&e, edit) {
                    report.rows_deleted = edit.count;
                    buf.clear();
                    continue;
                }
                writer.write_event(Event::Empty(shift_row_tag(&e, edit)))?;
            }

            // Column definitions (`<cols><col min max width/hidden/style>…</cols>`) on a
            // COLUMN-axis edit: each `<col>`'s min/max are column indices in the sheet's
            // coordinates and must shift with the columns they format, or a stale `<col>`
            // would hide/style the wrong column. A `<col>` whose whole range is deleted is
            // dropped. The container is buffered so that if EVERY `<col>` is dropped we omit
            // `<cols>` entirely — an empty `<cols></cols>` is schema-invalid.
            Event::Start(e) if col_axis && tag_local_eq(e.name().as_ref(), b"cols") => {
                let cols_start = e.into_owned();
                let mut survivors: Vec<BytesStart<'static>> = Vec::new();
                loop {
                    match reader.read_event_into(&mut buf)? {
                        Event::Empty(c) | Event::Start(c)
                            if tag_local_eq(c.name().as_ref(), b"col") =>
                        {
                            if let Some(tag) = shift_col_tag(&c, edit) {
                                survivors.push(tag);
                            }
                        }
                        Event::End(c) if tag_local_eq(c.name().as_ref(), b"cols") => break,
                        Event::Eof => break,
                        _ => {} // stray whitespace/comment inside <cols>
                    }
                }
                if !survivors.is_empty() {
                    writer.write_event(Event::Start(cols_start))?;
                    for s in survivors {
                        writer.write_event(Event::Empty(s))?;
                    }
                    writer.write_event(Event::End(quick_xml::events::BytesEnd::new("cols")))?;
                }
                buf.clear();
                continue;
            }

            // On a COLUMN delete, DROP a <c> whose column falls inside the deleted band:
            // the coordinate shift alone leaves the deleted column's content stale, and an
            // interior delete would emit duplicate coordinates (two `r="B1"`) = invalid
            // OOXML. This is the column analogue of the row-delete `delete_skip`.
            Event::Start(e) if col_axis && edit.op == Op::Delete && cell_col_deleted(&e, edit) => {
                reader.read_to_end(e.name())?;
                buf.clear();
                continue;
            }
            Event::Empty(e) if col_axis && edit.op == Op::Delete && cell_col_deleted(&e, edit) => {
                buf.clear();
                continue;
            }

            // A mergeCell / dataValidation / conditionalFormatting / … whose whole range a
            // delete consumes is DROPPED — shifting it to an empty `ref=""`/`sqref=""` is
            // malformed OOXML (Excel repair).
            Event::Start(e) if ref_fully_consumed(&e, &sheet, edit) => {
                reader.read_to_end(e.name())?;
                buf.clear();
                continue;
            }
            Event::Empty(e) if ref_fully_consumed(&e, &sheet, edit) => {
                buf.clear();
                continue;
            }

            Event::Start(e) => {
                if is_formula_tag(e.name().as_ref()) {
                    in_f = true;
                    f_raw.clear();
                    f_array_ref = None;
                    f_residual = false;
                    if is_array_f(&e) {
                        // AFFECT-BASED: defer the array refusal to </f> (needs the body). The `<f>`
                        // and body are copied verbatim, so f_residual stays false (body IS captured).
                        f_array_ref = Some(attr_by_local(&e, b"ref").unwrap_or_default());
                    } else if let Some(reason) = detect_residual(&e) {
                        // shared-formula (should be pre-expanded; refuse if one survives).
                        f_residual = true;
                        report.residuals.push(Residual {
                            part: part_name.into(),
                            reason: reason.into(),
                            detail: "shared formula present; refused (sound over-approximation)"
                                .into(),
                        });
                    }
                }
                writer.write_event(Event::Start(transform_tag(&e, &sheet, edit, report)))?;
            }
            Event::Empty(e) => {
                if tag_local_eq(e.name().as_ref(), b"f") {
                    if is_array_f(&e) {
                        // An array STUB (no body): affect-check its ref extent only.
                        let ref_extent = attr_by_local(&e, b"ref").unwrap_or_default();
                        if array_formula_affected(&ref_extent, "", &sheet, edit) {
                            report.residuals.push(Residual {
                                part: part_name.into(),
                                reason: "array_formula_present".into(),
                                detail: "an array formula whose extent the edit would MOVE is not \
                                         shifted — edit refused (fail-closed)"
                                    .into(),
                            });
                        }
                    } else if let Some(reason) = detect_residual(&e) {
                        report.residuals.push(Residual {
                            part: part_name.into(),
                            reason: reason.into(),
                            detail: "shared-formula dependent stub".into(),
                        });
                    }
                }
                writer.write_event(Event::Empty(transform_tag(&e, &sheet, edit, report)))?;
            }
            Event::End(e) => {
                if is_formula_tag(e.name().as_ref()) {
                    if let Some(ref_extent) = f_array_ref.take() {
                        // ARRAY formula: copied VERBATIM (never shifted). Refuse only if the edit
                        // MOVES its extent or a cell its body references (affect-based).
                        let raw = std::mem::take(&mut f_raw);
                        if array_formula_affected(&ref_extent, &raw, &sheet, edit) {
                            report.residuals.push(Residual {
                                part: part_name.to_string(),
                                reason: "array_formula_present".into(),
                                detail:
                                    "an array/dynamic-array formula whose extent or a cell its \
                                         body references the edit would MOVE is not shifted — edit \
                                         refused (fail-closed)"
                                        .into(),
                            });
                        }
                        writer.write_event(Event::Text(BytesText::from_escaped(raw)))?;
                    } else if in_f && !f_residual {
                        // The whole <f> body has now been reassembled; shift once.
                        let raw = std::mem::take(&mut f_raw);
                        match logical_formula(&raw) {
                            Some(logical)
                                if !refshift::has_unquoted_non_ascii_qualifier(&logical) =>
                            {
                                let before_ref = logical.matches("#REF!").count();
                                let (nf, n) = refshift::shift_formula(&logical, &sheet, edit);
                                report.refs_shifted += n;
                                // Only #REF! this edit NEWLY introduced — a formula that already
                                // carried a dangling #REF! (from an earlier deletion) must not
                                // inflate the reported error count.
                                report.ref_errors +=
                                    nf.matches("#REF!").count().saturating_sub(before_ref) as u32;
                                if nf == logical {
                                    // unchanged: preserve the ORIGINAL bytes exactly
                                    // (do not let the writer re-escape e.g. ' -> &apos;)
                                    writer
                                        .write_event(Event::Text(BytesText::from_escaped(raw)))?;
                                } else {
                                    writer.write_event(Event::Text(BytesText::from_escaped(
                                        text_escape(&nf),
                                    )))?;
                                }
                            }
                            Some(logical)
                                if sheet.is_ascii()
                                    && refshift::neutralize_non_ascii_quals(&logical)
                                        .is_some_and(|resid| {
                                            refshift::shift_formula(&resid, &sheet, edit).0 == resid
                                        }) =>
                            {
                                // The edited sheet is ASCII, so every non-ASCII qualifier names a
                                // DIFFERENT sheet the edit cannot move; with those refs neutralized
                                // the remaining edited-sheet references do not shift. Nothing moves
                                // -> the body is verbatim-correct (affect-based, not presence-based).
                                writer.write_event(Event::Text(BytesText::from_escaped(raw)))?;
                            }
                            Some(_) => {
                                // FAIL-CLOSED: an unquoted non-ASCII qualifier that the affect
                                // check could NOT clear (an edited-sheet reference would shift, or
                                // a non-ASCII 3D span may enclose the edited sheet) — refuse rather
                                // than mis-shift, and write the body back verbatim.
                                report.residuals.push(Residual {
                                    part: part_name.to_string(),
                                    reason: "non_ascii_sheet_qualifier".into(),
                                    detail: "a formula carries an UNQUOTED non-ASCII sheet qualifier \
                                             the edit would move (or an unverifiable non-ASCII 3D span), \
                                             which the reference tokenizer cannot safely shift — edit \
                                             refused (fail-closed)".into(),
                                });
                                writer.write_event(Event::Text(BytesText::from_escaped(raw)))?;
                            }
                            None => {
                                // An entity outside the predefined/char-ref set: do not
                                // shift, write the body back verbatim (fail-closed).
                                writer.write_event(Event::Text(BytesText::from_escaped(raw)))?;
                            }
                        }
                    }
                    in_f = false;
                    f_residual = false;
                }
                writer.write_event(Event::End(e.into_owned()))?;
            }
            Event::Text(t) if in_f && !f_residual => {
                push_text_raw(&mut f_raw, &t);
            }
            Event::GeneralRef(r) if in_f && !f_residual => {
                push_ref_raw(&mut f_raw, &r);
            }
            other => {
                writer.write_event(other.into_owned())?;
            }
        }
        buf.clear();
    }

    let mut out = writer.into_inner().into_inner();
    if row_axis && edit.op == Op::Insert && !inserted {
        out = inject_blanks_at_end(&out, edit)?;
        report.rows_inserted = edit.count;
    }
    // A delete that consumes EVERY child of `<mergeCells>`/`<dataValidations>`/`<hyperlinks>`
    // drops the children but leaves the parent container empty — schema-invalid (CT_MergeCells /
    // CT_DataValidations / CT_Hyperlinks all declare their child with minOccurs=1), which Excel
    // opens with a repair prompt. Omit an emptied container (the `<cols>` path already does this for
    // its children). `<hyperlinks` cannot collide with the child `<hyperlink ` — it is the longer
    // name and omit_empty_container requires a name boundary.
    // `<ignoredErrors>` (child `<ignoredError>` minOccurs=1) and `<sortState>` (child
    // `<sortCondition>` minOccurs=1) can also be emptied when a delete consumes their only child's
    // ref via ref_fully_consumed (both are has_ref_attr elements) — an empty container is
    // schema-invalid the same way, so omit it too (round-56 defect 8).
    for c in [
        "mergeCells",
        "dataValidations",
        "hyperlinks",
        "ignoredErrors",
        "sortState",
    ] {
        out = omit_empty_container(out, c);
    }
    // A delete that drops SOME (not all) children leaves the container's `count` attribute stale
    // (`<mergeCells count="2">` with one child) — ISO/IEC 29500 defines @count as the child count,
    // so the file is internally inconsistent (Excel: "Repaired Records" prompt). Recompute it to the
    // surviving child count. (`<hyperlinks>` has no count attribute, so it is not in this list.)
    for (container, child) in [
        ("mergeCells", "mergeCell"),
        ("dataValidations", "dataValidation"),
    ] {
        out = fix_container_count(out, container, child);
    }
    Ok(out)
}

/// Count of direct `child`-local-named elements inside the first `container`-local-named element,
/// or None if the container is absent. Namespace-prefix aware (so `<x:mergeCell>` counts).
fn container_child_count(xml: &[u8], container: &[u8], child: &[u8]) -> Option<u32> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    let mut inside = false;
    let mut present = false;
    let mut count = 0u32;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), container) => {
                inside = true;
                present = true;
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), container) => inside = false,
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if inside && tag_local_eq(e.name().as_ref(), child) =>
            {
                count += 1;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    present.then_some(count)
}

/// Rewrite a `<container … count="N" …>` start tag's `count` to the ACTUAL surviving child count.
/// A targeted value splice (no re-serialization) so an unchanged container stays byte-identical; a
/// container without a `count` attribute (valid — it is optional) is left untouched.
fn fix_container_count(xml: Vec<u8>, container: &str, child: &str) -> Vec<u8> {
    let Some(actual) = container_child_count(&xml, container.as_bytes(), child.as_bytes()) else {
        return xml;
    };
    let Ok(s) = std::str::from_utf8(&xml) else {
        return xml;
    };
    let open = format!("<{container}");
    let Some(start) = s.find(&open) else {
        return xml;
    };
    // Name boundary: the next char must not continue the element name (`<mergeCells` vs a
    // hypothetical `<mergeCellsX`), so the child `<mergeCell ` is never mistaken for the container.
    if s[start + open.len()..]
        .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return xml;
    }
    let Some(gt_rel) = s[start..].find('>') else {
        return xml;
    };
    let tag = &s[start..start + gt_rel];
    let Some(cpos) = tag.find("count=\"") else {
        return xml; // no count attribute -> no mismatch possible
    };
    let val_start = start + cpos + "count=\"".len();
    let Some(val_len) = s[val_start..].find('"') else {
        return xml;
    };
    if s[val_start..val_start + val_len] == *actual.to_string() {
        return xml; // already correct -> byte-identical
    }
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..val_start]);
    out.push_str(&actual.to_string());
    out.push_str(&s[val_start + val_len..]);
    out.into_bytes()
}

/// Remove an EMPTY `<container …></container>` (start tag immediately followed, modulo
/// whitespace, by its end tag) from a worksheet part. A worksheet has at most one `<mergeCells>`
/// / `<dataValidations>`, so the first match suffices. Non-empty containers, a self-closing tag,
/// or invalid UTF-8 are returned unchanged.
fn omit_empty_container(xml: Vec<u8>, container: &str) -> Vec<u8> {
    let Ok(s) = std::str::from_utf8(&xml) else {
        return xml;
    };
    let open = format!("<{container}");
    let Some(start) = s.find(&open) else {
        return xml;
    };
    // `<mergeCells` must not match the child `<mergeCell ` — require a name boundary next.
    let after = start + open.len();
    if !s[after..]
        .chars()
        .next()
        .is_some_and(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '>' || c == '/')
    {
        return xml;
    }
    let Some(gt) = s[start..].find('>').map(|i| start + i) else {
        return xml;
    };
    if s.as_bytes().get(gt.wrapping_sub(1)) == Some(&b'/') {
        return xml; // self-closing (already empty-and-valid, nothing to drop)
    }
    let close = format!("</{container}>");
    let trimmed = s[gt + 1..].trim_start();
    if let Some(after_close) = trimmed.strip_prefix(&close) {
        // `after_close` is a suffix of `s`, so its byte offset is `s.len() - after_close.len()`.
        let close_end = s.len() - after_close.len();
        let mut result = String::with_capacity(start + after_close.len());
        result.push_str(&s[..start]);
        result.push_str(&s[close_end..]);
        return result.into_bytes();
    }
    xml
}

/// Buffered rewrite for `Op::Move`. Row σ REORDERS rows, so we cannot stream in
/// document order like insert/delete. We: (1) copy every non-row byte verbatim,
/// (2) for each `<row>` relabel its `r` and every child `<c>`'s `r` to σ(row) and
/// shift every formula's references (via `shift_formula` → `shift_index` → σ),
/// buffering the transformed row keyed by its NEW row number, and (3) at
/// `</sheetData>` re-emit the buffered rows SORTED ascending by that new number.
/// A range that reorders under σ (a straddle) surfaces as a new `#REF!` and is
/// recorded as a `move_straddles_range` residual so the edit is refused.
fn rewrite_edited_sheet_move(
    src: &[u8],
    edit: &StructuralEdit,
    part_name: &str,
    report: &mut StructuralReport,
) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut main = Writer::new(Cursor::new(Vec::new()));
    let sheet = edit.sheet.clone();
    let mut buf = Vec::new();

    // buffered rows: (new_row_number, serialized transformed row bytes)
    let mut rows: Vec<(u32, Vec<u8>)> = Vec::new();
    // Some(writer) while inside a <row>…</row>; events route here, not to `main`.
    let mut row_buf: Option<(u32, Writer<Cursor<Vec<u8>>>)> = None;
    let mut in_sheetdata = false;
    let mut in_f = false;
    let mut f_residual = false;
    let mut straddle_flagged = false;
    // Reassembled formula body across Text + GeneralRef; shifted once at </f>.
    let mut f_raw = String::new();

    loop {
        let ev = reader.read_event_into(&mut buf)?;
        match ev {
            Event::Eof => break,

            // ---- <row> boundaries: buffer instead of streaming ----
            Event::Start(e) if e.name().as_ref() == b"row" => {
                let old_r =
                    attr_u32(&e, b"r").ok_or_else(|| anyhow!("move: <row> without r attribute"))?;
                let new_r = refshift::move_row_sigma(old_r, edit.at, edit.count, edit.dest);
                let mut w = Writer::new(Cursor::new(Vec::new()));
                w.write_event(Event::Start(shift_row_tag(&e, edit)))?;
                row_buf = Some((new_r, w));
            }
            Event::Empty(e) if e.name().as_ref() == b"row" => {
                let old_r =
                    attr_u32(&e, b"r").ok_or_else(|| anyhow!("move: <row> without r attribute"))?;
                let new_r = refshift::move_row_sigma(old_r, edit.at, edit.count, edit.dest);
                let mut w = Writer::new(Cursor::new(Vec::new()));
                w.write_event(Event::Empty(shift_row_tag(&e, edit)))?;
                rows.push((new_r, w.into_inner().into_inner()));
            }
            Event::End(e) if e.name().as_ref() == b"row" => {
                if let Some((key, mut w)) = row_buf.take() {
                    w.write_event(Event::End(e.into_owned()))?;
                    rows.push((key, w.into_inner().into_inner()));
                } else {
                    main.write_event(Event::End(e.into_owned()))?;
                }
            }

            // ---- <sheetData> boundaries: flush sorted rows at the close ----
            Event::Start(e) if e.name().as_ref() == b"sheetData" => {
                in_sheetdata = true;
                main.write_event(Event::Start(e.into_owned()))?;
            }
            Event::Empty(e) if e.name().as_ref() == b"sheetData" => {
                main.write_event(Event::Empty(e.into_owned()))?;
            }
            Event::End(e) if e.name().as_ref() == b"sheetData" => {
                in_sheetdata = false;
                rows.sort_by_key(|(k, _)| *k);
                for (_, bytes) in &rows {
                    main.get_mut()
                        .write_all(bytes)
                        .map_err(|e| anyhow!("flush row: {e}"))?;
                }
                rows.clear();
                main.write_event(Event::End(e.into_owned()))?;
            }

            // ---- formula-bearing element: TEXT carries A1 refs ----
            Event::Start(e) if is_formula_tag(e.name().as_ref()) => {
                in_f = true;
                f_residual = detect_residual(&e).is_some();
                f_raw.clear();
                if f_residual {
                    report.residuals.push(Residual {
                        part: part_name.into(),
                        reason: detect_residual(&e).unwrap().into(),
                        detail: "shared/array formula present; refused (sound over-approximation)"
                            .into(),
                    });
                }
                match row_buf.as_mut() {
                    Some((_, w)) => w.write_event(Event::Start(e.into_owned()))?,
                    None => main.write_event(Event::Start(e.into_owned()))?,
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"f" => {
                if let Some(reason) = detect_residual(&e) {
                    report.residuals.push(Residual {
                        part: part_name.into(),
                        reason: reason.into(),
                        detail: "shared-formula dependent stub".into(),
                    });
                }
                match row_buf.as_mut() {
                    Some((_, w)) => w.write_event(Event::Empty(e.into_owned()))?,
                    None => main.write_event(Event::Empty(e.into_owned()))?,
                }
            }
            Event::End(e) if is_formula_tag(e.name().as_ref()) => {
                if in_f && !f_residual {
                    // Whole <f> body reassembled across Text + GeneralRef; shift once.
                    let raw = std::mem::take(&mut f_raw);
                    let out_ev = match logical_formula(&raw) {
                        Some(logical) if !refshift::has_unquoted_non_ascii_qualifier(&logical) => {
                            let before_ref = logical.matches("#REF!").count();
                            let (nf, n) = refshift::shift_formula(&logical, &sheet, edit);
                            let new_ref = nf.matches("#REF!").count().saturating_sub(before_ref);
                            report.refs_shifted += n;
                            report.ref_errors += new_ref as u32;
                            if new_ref > 0 && !straddle_flagged {
                                straddle_flagged = true;
                                report.residuals.push(Residual {
                                    part: part_name.into(),
                                    reason: "move_straddles_range".into(),
                                    detail: "a range reference reorders under the move \
                                             (σ(head) > σ(tail)); it cannot be expressed as a shifted \
                                             rectangle — edit refused (fail-closed)"
                                        .into(),
                                });
                            }
                            if nf == logical {
                                Event::Text(BytesText::from_escaped(raw))
                            } else {
                                Event::Text(BytesText::from_escaped(text_escape(&nf)))
                            }
                        }
                        Some(logical)
                            if sheet.is_ascii()
                                && refshift::neutralize_non_ascii_quals(&logical).is_some_and(
                                    |resid| {
                                        refshift::shift_formula(&resid, &sheet, edit).0 == resid
                                    },
                                ) =>
                        {
                            // The edited sheet is ASCII, so every non-ASCII qualifier names a
                            // DIFFERENT sheet the move cannot touch; with those refs neutralized the
                            // remaining edited-sheet references do not shift. Nothing moves -> the
                            // body is verbatim-correct (affect-based, not presence-based).
                            Event::Text(BytesText::from_escaped(raw))
                        }
                        Some(_) => {
                            // FAIL-CLOSED: an unquoted non-ASCII qualifier the affect check could
                            // NOT clear (an edited-sheet reference would shift, or a non-ASCII 3D
                            // span may enclose the edited sheet) — refuse rather than mis-shift.
                            report.residuals.push(Residual {
                                part: part_name.to_string(),
                                reason: "non_ascii_sheet_qualifier".into(),
                                detail: "a formula carries an UNQUOTED non-ASCII sheet qualifier \
                                         the move would shift (or an unverifiable non-ASCII 3D span), \
                                         which the reference tokenizer cannot safely shift — edit \
                                         refused (fail-closed)"
                                    .into(),
                            });
                            Event::Text(BytesText::from_escaped(raw))
                        }
                        None => Event::Text(BytesText::from_escaped(raw)),
                    };
                    match row_buf.as_mut() {
                        Some((_, w)) => w.write_event(out_ev)?,
                        None => main.write_event(out_ev)?,
                    }
                }
                in_f = false;
                f_residual = false;
                match row_buf.as_mut() {
                    Some((_, w)) => w.write_event(Event::End(e.into_owned()))?,
                    None => main.write_event(Event::End(e.into_owned()))?,
                }
            }
            Event::Text(t) if in_f && !f_residual => {
                push_text_raw(&mut f_raw, &t);
            }
            Event::GeneralRef(r) if in_f && !f_residual => {
                push_ref_raw(&mut f_raw, &r);
            }

            // ---- any other start/empty element: attribute σ-shift ----
            Event::Start(e) => {
                let tag = transform_tag_move(&e, &sheet, edit, report);
                match row_buf.as_mut() {
                    Some((_, w)) => w.write_event(Event::Start(tag))?,
                    None => main.write_event(Event::Start(tag))?,
                }
            }
            Event::Empty(e) => {
                let tag = transform_tag_move(&e, &sheet, edit, report);
                match row_buf.as_mut() {
                    Some((_, w)) => w.write_event(Event::Empty(tag))?,
                    None => main.write_event(Event::Empty(tag))?,
                }
            }

            // drop insignificant whitespace that sits between rows in sheetData
            Event::Text(t) if in_sheetdata && row_buf.is_none() => {
                let _ = t;
            }

            other => match row_buf.as_mut() {
                Some((_, w)) => w.write_event(other.into_owned())?,
                None => main.write_event(other.into_owned())?,
            },
        }
        buf.clear();
    }

    Ok(main.into_inner().into_inner())
}

/// Non-row attribute transform for `Op::Move`. Cells and CONTENT-FOLLOWING references
/// (mergeCell/hyperlink ref, CF/DV sqref, ignoredError sqref, autoFilter/sortState/sortCondition
/// ref) relocate with their rows via σ — a move whose block crosses the region rigidly shifts it,
/// and a straddling region trips the move_straddles_range net. `dimension` (advisory, Excel
/// recomputes) and view state (selection/pane/brk) are non-semantic and left byte-identical.
fn transform_tag_move(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    // Match by LOCAL name so a namespace-PREFIXED element (`<x:c>`/`<x:f>` bound to the main
    // SpreadsheetML namespace, legal OOXML) is shifted too — a prefixed data-table `<x:f>` was
    // left with stale r1/r2 input cells (silent value corruption).
    match local_of(e.name().as_ref()) {
        b"c" => shift_cell_tag(e, edit),
        b"f" if is_datatable_f(e) => shift_datatable_attrs(e, sheet, edit, report),
        // `ignoredError sqref` names SPECIFIC cells (error-checking suppression), not an extent or
        // view-state, so it must relocate with its cells under a MOVE exactly as it does under
        // insert/delete. Omitting it here (round-55 defect 4) left the suppression on the wrong cell
        // — silently, since the fail-closed body scan skips it (it is in has_ref_attr). shift_sqref
        // uses move_row_sigma for Op::Move, so a straddling range trips the move_straddles_range net.
        // `autoFilter`/`sortState`/`sortCondition` are NOT invariant under a move: a move whose block
        // CROSSES the filter/sort region rigidly relocates its data rows, so the region's ref/sqref
        // must follow via σ (round-57 defect 4). shift_sqref uses move_row_sigma; a straddling region
        // returns Shift::Ref -> ref_errors > 0 -> the move_straddles_range net refuses (fail-closed).
        b"mergeCell"
        | b"hyperlink"
        | b"conditionalFormatting"
        | b"dataValidation"
        | b"ignoredError"
        | b"autoFilter"
        | b"sortState"
        | b"sortCondition" => shift_ref_attrs(e, sheet, edit, report),
        _ => e.to_owned(),
    }
}

/// Attribute-only transform for a non-row tag: cells and ref-bearing elements.
fn transform_tag(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    // Match by LOCAL name (namespace-prefix aware) — see transform_tag_move.
    match local_of(e.name().as_ref()) {
        b"c" => shift_cell_tag(e, edit),
        b"f" if is_datatable_f(e) => shift_datatable_attrs(e, sheet, edit, report),
        n if has_ref_attr(n) => shift_ref_attrs(e, sheet, edit, report),
        _ => e.to_owned(),
    }
}

fn maybe_inject(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    e: &BytesStart,
    edit: &StructuralEdit,
    inserted: &mut bool,
    report: &mut StructuralReport,
) -> Result<bool> {
    if edit.op == Op::Insert && !*inserted {
        if let Some(r) = attr_u32(e, b"r") {
            if r >= edit.at {
                for i in 0..edit.count {
                    let tag = BytesStart::from_content(format!("row r=\"{}\"", edit.at + i), 3);
                    writer.write_event(Event::Empty(tag))?;
                }
                *inserted = true;
                report.rows_inserted = edit.count;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn delete_skip(e: &BytesStart, edit: &StructuralEdit) -> bool {
    edit.op == Op::Delete
        && attr_u32(e, b"r").is_some_and(|r| r >= edit.at && r < edit.at + edit.count)
}

fn inject_blanks_at_end(out: &[u8], edit: &StructuralEdit) -> Result<Vec<u8>> {
    let needle = b"</sheetData>";
    if let Some(pos) = out.windows(needle.len()).position(|w| w == needle) {
        let mut blanks = String::new();
        for i in 0..edit.count {
            blanks.push_str(&format!("<row r=\"{}\"/>", edit.at + i));
        }
        let mut v = Vec::with_capacity(out.len() + blanks.len());
        v.extend_from_slice(&out[..pos]);
        v.extend_from_slice(blanks.as_bytes());
        v.extend_from_slice(&out[pos..]);
        Ok(v)
    } else {
        Ok(out.to_vec())
    }
}

fn shift_row_tag(e: &BytesStart, edit: &StructuralEdit) -> BytesStart<'static> {
    let mut repl: Vec<(&[u8], String)> = Vec::new();
    if let Some(rn) = attr_u32(e, b"r") {
        if let Some(nr) = shift_line(rn, edit) {
            if nr != rn {
                repl.push((b"r", nr.to_string()));
            }
        }
    }
    set_attrs(e, &repl)
}

/// Shift a `<col>`'s inclusive column-index range `[min, max]` under a COLUMN-axis edit.
/// Returns the new `(min, max)`, or `None` if the whole range falls inside a deletion (the
/// `<col>` element is then dropped). Insert extends a straddling range to cover the inserted
/// columns (Excel's inherit-left behavior); delete clamps each endpoint to the surviving
/// coordinate space. Move is rows-only, so it never reaches a `<col>`; treated as identity.
fn shift_col_range(min: u32, max: u32, edit: &StructuralEdit) -> Option<(u32, u32)> {
    const MAX_COL: u32 = 16384; // XFD, the last column that exists
    match edit.op {
        Op::Insert => {
            let m = if min >= edit.at {
                min + edit.count
            } else {
                min
            };
            let x = if max >= edit.at {
                max + edit.count
            } else {
                max
            };
            // Columns pushed past the sheet's last column no longer exist: drop a range
            // whose start overflows, clamp a range whose end overflows.
            if m > MAX_COL {
                None
            } else {
                Some((m, x.min(MAX_COL)))
            }
        }
        Op::Delete => {
            let del_lo = edit.at;
            let del_hi = edit.at + edit.count - 1; // inclusive
            let clamp = |c: u32| -> Option<u32> {
                if c < del_lo {
                    Some(c)
                } else if c > del_hi {
                    Some(c - edit.count)
                } else {
                    None // inside the deleted span
                }
            };
            match (clamp(min), clamp(max)) {
                (Some(m), Some(x)) => Some((m, x)),
                // min deleted, max survives above: surviving range starts at the first
                // column after the deletion, renumbered to del_lo.
                (None, Some(x)) => Some((del_lo, x)),
                // max deleted, min survives below: surviving range ends just before it.
                (Some(m), None) => Some((m, del_lo.saturating_sub(1))),
                // both endpoints inside the deleted span: the whole <col> is gone.
                (None, None) => None,
            }
        }
        Op::Move => Some((min, max)),
    }
}

/// Rewrite a `<col>`'s `min`/`max` under a column-axis edit. `None` = drop (range deleted).
fn shift_col_tag(e: &BytesStart, edit: &StructuralEdit) -> Option<BytesStart<'static>> {
    let min = attr_u32(e, b"min");
    let max = attr_u32(e, b"max");
    let (Some(min), Some(max)) = (min, max) else {
        // Malformed <col> (missing min/max): leave verbatim rather than guess.
        return Some(e.to_owned());
    };
    let (nm, nx) = shift_col_range(min, max, edit)?;
    let mut repl: Vec<(&[u8], String)> = Vec::new();
    if nm != min {
        repl.push((b"min", nm.to_string()));
    }
    if nx != max {
        repl.push((b"max", nx.to_string()));
    }
    Some(set_attrs(e, &repl))
}

fn shift_cell_tag(e: &BytesStart, edit: &StructuralEdit) -> BytesStart<'static> {
    let mut repl: Vec<(&[u8], String)> = Vec::new();
    if let Some(a) = e.attributes().flatten().find(|a| a.key.as_ref() == b"r") {
        let val = String::from_utf8_lossy(&a.value).into_owned();
        if let Shift::Shifted(ns) = refshift::shift_ref(&val, &edit.sheet, edit) {
            repl.push((b"r", ns));
        }
    }
    set_attrs(e, &repl)
}

/// Formula-bearing element local names whose TEXT carries A1 references:
/// `<f>` (cell), `<formula>` (cfRule), `<formula1>`/`<formula2>` (dataValidation).
fn is_formula_tag(name: &[u8]) -> bool {
    let local = match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    };
    matches!(local, b"f" | b"formula" | b"formula1" | b"formula2")
}

fn has_ref_attr(name: &[u8]) -> bool {
    matches!(
        name,
        b"mergeCell"
            | b"hyperlink"
            | b"conditionalFormatting"
            | b"dataValidation"
            | b"dimension"
            | b"selection"
            | b"pane"
            | b"autoFilter"
            | b"sortState"
            | b"sortCondition"
            | b"brk"
            | b"ignoredError"
    )
}

/// Shift the `location` (in-workbook target) of every `<hyperlink>` on a sheet. A hyperlink's
/// `ref` (the cell it sits on) is shifted by `shift_ref_attrs`, but its `location` points at a
/// DESTINATION cell/range that a row/column edit also moves — the ubiquitous table-of-contents
/// link `location="Data!A15"` must follow to `Data!A16` after an insert. The σ oracle with
/// `host` as the context sheet shifts it exactly: an unqualified location on the edited sheet,
/// or one qualified to the edited sheet, moves; a location targeting another sheet is
/// untouched (host makes an unqualified foreign-sheet location local, so it is not moved).
/// Returns (bytes, shifted, ref_errors); a target a delete CONSUMES becomes `#REF!`, mirroring
/// the cell-formula path and Excel. When nothing shifts, the ORIGINAL bytes are returned
/// verbatim so a no-op does not re-serialize the part (which would spuriously mark it touched).
/// The 1-based column index of a reference's FIRST cell (`A1:D13` -> 1, `$C$2` -> 3).
fn first_col_of_ref(reference: &str) -> Option<u32> {
    let first = reference
        .split(':')
        .next()
        .unwrap_or(reference)
        .replace('$', "");
    parse_cell_rc(&first).map(|(col, _row)| col)
}

/// Translate a single `<filterColumn>`'s `colId` for a column edit. `colId` is a 0-based offset
/// from the autoFilter range's first column, so its ABSOLUTE column = `old_first + colId`. Returns
/// `None` to DROP the filterColumn (its column was deleted / fell outside the shifted range), or
/// `Some((tag, changed))` with the (possibly rewritten) element.
fn translate_filter_column(
    e: &BytesStart,
    old_first: u32,
    new_first: u32,
    edit: &StructuralEdit,
) -> Option<(BytesStart<'static>, bool)> {
    let Some(col_id) = attr_u32(e, b"colId") else {
        return Some((e.to_owned(), false)); // no colId to translate
    };
    let old_abs = old_first + col_id;
    match refshift::shift_index(old_abs, edit) {
        None => None, // filtered column deleted -> drop
        Some(new_abs) if new_abs >= new_first => {
            let new_col_id = new_abs - new_first;
            if new_col_id == col_id {
                Some((e.to_owned(), false))
            } else {
                Some((
                    set_attrs(e, &[(b"colId".as_slice(), new_col_id.to_string())]),
                    true,
                ))
            }
        }
        Some(_) => None, // shifted before the range's new first column -> drop (out of range)
    }
}

/// Re-target every `<filterColumn colId>` under a sheet `<autoFilter>` when a COLUMN insert/delete
/// moves the filtered column. `colId` is a 0-based offset from the autoFilter range's first column
/// that the streaming rewrite (which shifts only the parent `ref`) leaves stale — so after the edit
/// the filter predicate points at the wrong (blank/shifted) column, silently changing which rows a
/// SUBTOTAL/AGGREGATE over the range hides. Runs BEFORE the `ref` is shifted and derives the new
/// first column with the SAME `shift_sqref` the ref shift uses, so the two stay consistent. No-op
/// for row/move edits (`colId` is a column offset; a move leaves the autoFilter extent invariant).
fn shift_autofilter_columns(
    xml: &[u8],
    sheet: &str,
    edit: &StructuralEdit,
) -> Result<(Vec<u8>, u32, u32)> {
    if edit.axis != Axis::Col || !matches!(edit.op, Op::Insert | Op::Delete) {
        return Ok((xml.to_vec(), 0, 0));
    }
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    // (old_first_col, new_first_col) of the enclosing autoFilter, present only when both parse.
    let mut cur: Option<(u32, u32)> = None;
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Start(e) if local_of(e.name().as_ref()) == b"autoFilter" => {
                // Derive (old first col, new first col) with the SAME shift_sqref the ref shift
                // uses, so colId translation stays consistent with the (later) ref shift.
                cur = attr_by_local(&e, b"ref").and_then(|reference| {
                    let old = first_col_of_ref(&reference)?;
                    let new_ref = shift_sqref(&reference, sheet, edit).0;
                    let new = first_col_of_ref(&new_ref)?;
                    Some((old, new))
                });
                writer.write_event(Event::Start(e.into_owned()))?;
            }
            Event::End(e) if local_of(e.name().as_ref()) == b"autoFilter" => {
                cur = None;
                writer.write_event(Event::End(e.into_owned()))?;
            }
            Event::Start(e) if local_of(e.name().as_ref()) == b"filterColumn" && cur.is_some() => {
                let (of, nf) = cur.expect("checked is_some");
                match translate_filter_column(&e, of, nf, edit) {
                    Some((tag, changed)) => {
                        shifted += changed as u32;
                        writer.write_event(Event::Start(tag))?;
                    }
                    None => {
                        // Drop the whole filterColumn subtree (its filters/criteria go with it).
                        errs += skip_element(&mut reader, b"filterColumn")?;
                        shifted += 1;
                    }
                }
            }
            Event::Empty(e) if local_of(e.name().as_ref()) == b"filterColumn" && cur.is_some() => {
                let (of, nf) = cur.expect("checked is_some");
                match translate_filter_column(&e, of, nf, edit) {
                    Some((tag, changed)) => {
                        shifted += changed as u32;
                        writer.write_event(Event::Empty(tag))?;
                    }
                    None => shifted += 1, // dropped, no children
                }
            }
            Event::Eof => break,
            other => writer.write_event(other.into_owned())?,
        }
        buf.clear();
    }
    Ok((writer.into_inner().into_inner(), shifted, errs))
}

/// Consume events until the End that closes the currently-open element with local name `local`
/// (which has already been read). Returns 0 (no error signalled; a dropped element is a faithful
/// removal, not a #REF!).
fn skip_element(reader: &mut Reader<&[u8]>, local: &[u8]) -> Result<u32> {
    let mut depth = 1i32;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?
        {
            Event::Start(e) if local_of(e.name().as_ref()) == local => depth += 1,
            Event::End(e) if local_of(e.name().as_ref()) == local => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(0)
}

fn shift_hyperlink_locations(
    xml: &[u8],
    host: &str,
    edit: &StructuralEdit,
) -> Result<(Vec<u8>, u32, u32)> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e) if tag_local_eq(e.name().as_ref(), b"hyperlink") => {
                let (t, n, r) = shift_hyperlink_tag(&e, host, edit);
                shifted += n;
                errs += r;
                writer
                    .write_event(Event::Start(t))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Empty(e) if tag_local_eq(e.name().as_ref(), b"hyperlink") => {
                let (t, n, r) = shift_hyperlink_tag(&e, host, edit);
                shifted += n;
                errs += r;
                writer
                    .write_event(Event::Empty(t))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            other => writer
                .write_event(other)
                .map_err(|e| anyhow!("xml write: {e}"))?,
        }
        buf.clear();
    }
    if shifted == 0 {
        return Ok((xml.to_vec(), 0, 0));
    }
    Ok((writer.into_inner().into_inner(), shifted, errs))
}

fn shift_hyperlink_tag(
    e: &BytesStart,
    host: &str,
    edit: &StructuralEdit,
) -> (BytesStart<'static>, u32, u32) {
    let Some(loc) = attr_by_local(e, b"location") else {
        return (e.to_owned(), 0, 0);
    };
    let (nl, _n) = refshift::shift_formula(&loc, host, edit);
    if nl == loc {
        return (e.to_owned(), 0, 0);
    }
    let errs = nl.matches("#REF!").count() as u32;
    (set_attrs(e, &[(b"location", nl)]), 1, errs)
}

/// A what-if data table cell formula: `<f t="dataTable" ref="C2:C5" r1="A1" r2="B1"/>`.
/// Unlike an ordinary `<f>`, it carries LIVE coordinates in ATTRIBUTES — `ref` (the output
/// array extent), `r1` (the column input cell), `r2` (the row input cell) — none in the body.
fn is_datatable_f(e: &BytesStart) -> bool {
    local_of(e.name().as_ref()) == b"f"
        && e.attributes()
            .flatten()
            .any(|a| local_of(a.key.as_ref()) == b"t" && a.value.as_ref() == b"dataTable")
}

/// Shift a data-table `<f>`'s `ref`/`r1`/`r2` cell references. Left unshifted (the `<f>` body
/// path only rewrites formula TEXT, and the edited-body scan skips formula tags), the input
/// cell would read a blank inserted row and the declared output extent would no longer match
/// the body cells — a silent value corruption.
fn shift_datatable_attrs(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    const KEYS: &[&[u8]] = &[b"ref", b"r1", b"r2"];
    let mut repl: Vec<(&[u8], String)> = Vec::new();
    for a in e.attributes().flatten() {
        let key = a.key.as_ref();
        if let Some(&sk) = KEYS.iter().find(|k| **k == key) {
            let val = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .unwrap_or_default()
                .into_owned();
            let (nv, n, c, _all) = shift_sqref(&val, sheet, edit);
            report.refs_shifted += n;
            report.ref_errors += c;
            // A what-if data table carries its input cells (r1/r2) and output extent (ref) as LIVE
            // coordinates. When a DELETE consumes one (c > 0, or the value collapses to empty), it
            // cannot be expressed as a coordinate shift — writing `r1=""` is a malformed ST_CellRef
            // Excel treats as corrupt (round-56 defects 5/9). Fail closed: emit a residual so
            // restructure REFUSES, rather than committing an empty/invalid reference.
            if c > 0 || (!val.is_empty() && nv.trim().is_empty()) {
                report.residuals.push(Residual {
                    part: format!("worksheet ({sheet})"),
                    reason: "datatable_input_consumed".into(),
                    detail: "a what-if data table's input cell / output extent (ref/r1/r2) is \
                             consumed by the delete — not coordinate-shiftable, edit refused \
                             (fail-closed)"
                        .into(),
                });
                // The edit is refused; leave the ORIGINAL attribute value rather than splice in a
                // malformed empty ST_CellRef.
            } else if nv != val {
                repl.push((sk, nv));
            }
        }
    }
    set_attrs(e, &repl)
}

fn shift_ref_attrs(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    let name = e.name().as_ref().to_vec();
    // Matched by the FULL element name (namespace-prefix SENSITIVE): a prefixed `<x:mergeCell>`
    // could be a spreadsheetML element (same namespace, just prefixed — should shift) OR a foreign
    // element that merely shares the local name (should NOT have its attribute rewritten). Telling
    // them apart needs namespace resolution (the prefix's xmlns binding), which this scan does not
    // do — so it stays conservative (a prefixed element is left to the pre-flight refusal).
    let ref_attrs: &[&[u8]] = match name.as_slice() {
        b"mergeCell" | b"hyperlink" | b"dimension" | b"autoFilter" | b"sortState"
        | b"sortCondition" => &[b"ref"],
        b"conditionalFormatting" | b"dataValidation" | b"selection" | b"ignoredError" => {
            &[b"sqref"]
        }
        b"pane" => &[b"topLeftCell"],
        _ => &[],
    };
    let mut repl: Vec<(&[u8], String)> = Vec::new();
    for a in e.attributes().flatten() {
        let key = a.key.as_ref();
        if let Some(&sk) = ref_attrs.iter().find(|k| **k == key) {
            let val = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .unwrap_or_default()
                .into_owned();
            let (nv, n, c, _all) = shift_sqref(&val, sheet, edit);
            report.refs_shifted += n;
            report.ref_errors += c;
            if nv != val {
                repl.push((sk, nv));
            }
        }
    }
    set_attrs(e, &repl)
}

/// Shift a space-separated sqref/ref value; drop consumed rectangles.
/// Split a `sqref`/binding value into its space-separated reference tokens WITHOUT splitting inside
/// a quoted sheet name (`'My Sheet'!$A$5` is ONE token). A raw `split_whitespace` broke a quoted
/// qualifier into `'My` + `Sheet'!$A$5`, neither of which shift_ref recognizes — so a binding
/// qualified to the edited sheet by a spaced, quoted name was silently left stale (would_shift
/// returned false, defeating the refusal). Handles the `''` escape inside a quoted name.
fn split_sqref_tokens(value: &str) -> Vec<&str> {
    let bytes = value.as_bytes();
    let mut tokens = Vec::new();
    let mut in_quote = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                if in_quote && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2; // '' — an escaped quote inside the name
                    continue;
                }
                in_quote = !in_quote;
            }
            b' ' | b'\t' | b'\n' | b'\r' if !in_quote => {
                if start < i {
                    tokens.push(&value[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < value.len() {
        tokens.push(&value[start..]);
    }
    tokens
}

fn shift_sqref(value: &str, sheet: &str, edit: &StructuralEdit) -> (String, u32, u32, bool) {
    let mut parts = Vec::new();
    let (mut shifted, mut consumed) = (0u32, 0u32);
    let tokens = split_sqref_tokens(value);
    let total = tokens.len();
    for r in tokens {
        match refshift::shift_ref(r, sheet, edit) {
            Shift::Unchanged => parts.push(r.to_string()),
            Shift::Shifted(ns) => {
                shifted += 1;
                parts.push(ns);
            }
            Shift::Ref => consumed += 1,
        }
    }
    (
        parts.join(" "),
        shifted,
        consumed,
        parts.is_empty() && total > 0,
    )
}

fn detect_residual(e: &BytesStart) -> Option<&'static str> {
    let t = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"t")
        .map(|a| a.value.as_ref().to_vec());
    match t.as_deref() {
        Some(b"array") => Some("array_formula_present"),
        Some(b"shared") => Some("shared_formula_present"),
        _ => None,
    }
}

/// True if this edit MOVES an array formula's extent (`<f t="array" ref=…>`) or a cell its body
/// references. The array's `<f>` (ref extent) and body are copied VERBATIM (never shifted), so an
/// affected array must be refused; when nothing it touches moves, it is unaffected and the edit
/// commits — the affect-based walkback of the presence-based refusal that rejected EVERY
/// dynamic-array (FILTER/UNIQUE/SORT/SEQUENCE) workbook, since Excel persists all such spills as
/// `<f t="array" ref=…>`. `body_raw` is the reassembled raw `<f>` body (empty for a stub).
fn array_formula_affected(
    ref_extent: &str,
    body_raw: &str,
    sheet: &str,
    edit: &StructuralEdit,
) -> bool {
    let ref_moved = !ref_extent.is_empty() && shift_sqref(ref_extent, sheet, edit).0 != ref_extent;
    let body_moved = if body_raw.is_empty() {
        false
    } else {
        match logical_formula(body_raw) {
            None => true, // unparseable body -> fail closed
            Some(l) if refshift::has_unquoted_non_ascii_qualifier(&l) => {
                // A non-ASCII sheet qualifier on an ASCII edited sheet necessarily names a DIFFERENT
                // sheet the edit cannot move, so — mirroring the main `<f>` path (rewrite_edited_sheet)
                // and scan_extra_residuals's `!edit.sheet.is_ascii()` gate — neutralize those refs and
                // flag only if the remaining (edited-sheet) portion shifts. A non-ASCII 3D span that
                // could enclose the edited sheet (neutralize -> None) fails closed, as does a non-ASCII
                // EDITED sheet (the qualifier could be an unparseable self-reference).
                if sheet.is_ascii() {
                    match refshift::neutralize_non_ascii_quals(&l) {
                        Some(resid) => refshift::shift_formula(&resid, sheet, edit).0 != resid,
                        None => true,
                    }
                } else {
                    true
                }
            }
            Some(l) => refshift::shift_formula(&l, sheet, edit).0 != l,
        }
    };
    ref_moved || body_moved
}

fn attr_u32(e: &BytesStart, key: &[u8]) -> Option<u32> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
}

/// True if a formula qualifies the (non-ASCII-named) edited sheet at a cell/range THIS edit
/// would MOVE. The `{sheet}!` qualifier is non-ASCII (the tokenizer cannot parse it to shift
/// the whole reference), but the ASCII cell part after the `!` IS parseable, so we extract it
/// and ask the σ oracle whether the edit shifts it. Handles both a direct `{sheet}!A11` and a
/// 3D span `{sheet}:Other!A11` (the ref follows the span's `!`). An edit that moves nothing it
/// references is not refused.
fn non_ascii_qualifier_affected(formula: &str, sheet: &str, edit: &StructuralEdit) -> bool {
    // The ASCII cell/range ref immediately following a qualifier's `!` shifts under the edit.
    let ref_after_shifts = |after: &str| -> bool {
        let refstr: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '$' || *c == ':')
            .collect();
        if refstr.is_empty() {
            return false;
        }
        // The ref is on the edited sheet; shift it as such and see if it changes / overflows.
        refshift::shift_formula(&refstr, sheet, edit).0 != refstr
    };
    // Direct `{sheet}!REF`.
    for (i, _) in formula.match_indices(&format!("{sheet}!")) {
        let after = &formula[i + sheet.len() + 1..];
        if ref_after_shifts(after) {
            return true;
        }
    }
    // 3D span `{sheet}:Other!REF` — the ref follows the span's own `!`.
    for (i, _) in formula.match_indices(&format!("{sheet}:")) {
        let rest = &formula[i + sheet.len() + 1..];
        if let Some(bang) = rest.find('!') {
            if ref_after_shifts(&rest[bang + 1..]) {
                return true;
            }
        }
    }
    false
}

/// True if an INSERT edit would push a populated `<row>` (row axis) or a cell's column (column
/// axis) PAST the grid edge (row 1048576 / column XFD=16384). The row/cell relocation path does
/// not clamp — `shift_line` returns `pos + count` with no bound and `shift_cell_tag` silently
/// drops the `Shift::Ref` that the reference-shift path correctly returns for an overflow — so
/// the two paths disagree at the boundary, emitting an out-of-grid coordinate and orphaning a
/// datum from a range. Detected up front so the edit fails closed (Excel refuses it too).
fn insert_overflows_grid(xml: &[u8], edit: &StructuralEdit) -> bool {
    // A MOVE relocates rows via move_row_sigma with NO upper clamp (unlike the reference-shift
    // path), so a row whose σ-image exceeds the grid edge is emitted as an off-grid `<row r>` —
    // schema-invalid, and IronCalc's reopen accepts a cell-less formatted row (it bounds cell refs
    // but parses `<row r>` unbounded), so without this guard the corrupt file COMMITS (round-57
    // defect 5). Refuse if any PRESENT row's move image runs past the last row; the destination
    // closed-form catches a move-down of the block itself.
    if edit.op == Op::Move {
        let max = refshift::grid_max(Axis::Row);
        if edit.dest > max.saturating_add(1) {
            return true;
        }
        let mut reader = Reader::from_reader(xml);
        reader.config_mut().expand_empty_elements = false;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e))
                    if tag_local_eq(e.name().as_ref(), b"row") =>
                {
                    if let Some(r) = attr_u32(&e, b"r") {
                        if refshift::move_row_sigma(r, edit.at, edit.count, edit.dest) > max {
                            return true;
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        return false;
    }
    if edit.op != Op::Insert {
        return false;
    }
    // The inserted blank rows/cols themselves occupy [at, at+count-1]; if that range runs past the
    // grid edge the inserter would emit an off-grid `<row r>`/cell coordinate (schema-invalid, and
    // Excel refuses the edit). Guard this UP FRONT — the per-coordinate scan below only catches an
    // EXISTING populated datum shifted off-grid, not the blank rows emitted into an empty region.
    if edit.at.saturating_add(edit.count).saturating_sub(1) > refshift::grid_max(edit.axis) {
        return true;
    }
    let row_axis = edit.axis == Axis::Row;
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().expand_empty_elements = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if row_axis => {
                if tag_local_eq(e.name().as_ref(), b"row") {
                    if let Some(r) = attr_u32(&e, b"r") {
                        if r >= edit.at && r + edit.count > refshift::grid_max(Axis::Row) {
                            return true;
                        }
                    }
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if tag_local_eq(e.name().as_ref(), b"c") {
                    if let Some(rref) = attr_by_local(&e, b"r") {
                        let letters: String = rref
                            .chars()
                            .take_while(|c| c.is_ascii_alphabetic())
                            .collect();
                        if let Some(col) = refshift::col_to_num(&letters) {
                            if col >= edit.at && col + edit.count > refshift::grid_max(Axis::Col) {
                                return true;
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    false
}

fn shift_line(pos: u32, edit: &StructuralEdit) -> Option<u32> {
    match edit.op {
        Op::Insert => Some(if pos >= edit.at {
            pos + edit.count
        } else {
            pos
        }),
        Op::Delete => {
            if pos < edit.at {
                Some(pos)
            } else if pos >= edit.at + edit.count {
                Some(pos - edit.count)
            } else {
                None
            }
        }
        Op::Move => Some(refshift::move_row_sigma(
            pos, edit.at, edit.count, edit.dest,
        )),
    }
}

// ---------------------------------------------------------------------------
// foreign parts
// ---------------------------------------------------------------------------

fn rewrite_pivot(src: &[u8], edit: &StructuralEdit) -> Result<(Vec<u8>, u32, u32, bool)> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    // Fail-closed: rewrite_pivot shifts ONLY a <worksheetSource>. Any OTHER pivot element
    // that references the edited sheet (a `sheet` attr equal to it — e.g. a consolidation
    // `<rangeSet ref sheet>`) carries a grid coordinate we do not shift, so we flag it and
    // the caller refuses rather than committing it stale.
    let mut unhandled_edited_ref = false;
    // Shift the `ref`/`sheet` attributes of a <worksheetSource> in place. Emitted
    // in the SAME event shape it arrived in (Empty stays self-closing; Start stays
    // a Start whose children the loop copies through) — the previous code read and
    // discarded the following event and forced an Empty, silently dropping a
    // sibling element and unbalancing the pivot XML.
    let shift_source =
        |e: &BytesStart, shifted: &mut u32, errs: &mut u32| -> Vec<(&'static [u8], String)> {
            let sheet_attr = e
                .attributes()
                .flatten()
                .find(|a| a.key.as_ref() == b"sheet")
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                .unwrap_or_default();
            let mut repl: Vec<(&[u8], String)> = Vec::new();
            if sheet_attr.eq_ignore_ascii_case(&edit.sheet) {
                if let Some(a) = e.attributes().flatten().find(|a| a.key.as_ref() == b"ref") {
                    let val = String::from_utf8_lossy(&a.value).into_owned();
                    let (nv, n, c, _) = shift_sqref(&val, &edit.sheet, edit);
                    *shifted += n;
                    *errs += c;
                    if nv != val {
                        repl.push((b"ref", nv));
                    }
                }
            }
            repl
        };
    loop {
        // Fail closed: a mid-stream parse error must NOT commit a truncated part.
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("pivot xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Empty(e) if e.name().as_ref() == b"worksheetSource" => {
                let repl = shift_source(&e, &mut shifted, &mut errs);
                writer
                    .write_event(Event::Empty(set_attrs(&e, &repl)))
                    .map_err(|e| anyhow!("pivot write: {e}"))?;
            }
            Event::Start(e) if e.name().as_ref() == b"worksheetSource" => {
                let repl = shift_source(&e, &mut shifted, &mut errs);
                writer
                    .write_event(Event::Start(set_attrs(&e, &repl)))
                    .map_err(|e| anyhow!("pivot write: {e}"))?;
            }
            other => {
                // A non-worksheetSource element (e.g. a consolidation `<rangeSet ref sheet>`)
                // naming the edited sheet carries a grid `ref` we do not shift -> refuse.
                if let Event::Start(e) | Event::Empty(e) = &other {
                    if e.attributes().flatten().any(|a| {
                        a.key.as_ref() == b"sheet"
                            && String::from_utf8_lossy(&a.value).eq_ignore_ascii_case(&edit.sheet)
                    }) {
                        unhandled_edited_ref = true;
                    }
                }
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("pivot write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((
        writer.into_inner().into_inner(),
        shifted,
        errs,
        unhandled_edited_ref,
    ))
}

/// For every <TAG>text</TAG> (namespace-insensitive local match), run
/// shift_formula on the text. `host` scopes unqualified refs.
fn shift_text_in_element(
    src: &[u8],
    tag: &[u8],
    edit: &StructuralEdit,
    host: &str,
) -> Result<(Vec<u8>, u32, u32, bool)> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut in_tag = false;
    let mut residual = false;
    let mut qualifier_risk = false;
    // Reassembled element body across Text + GeneralRef; shifted once at </TAG>.
    let mut f_raw = String::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        // Fail closed: a mid-stream parse or write error must NOT commit a
        // truncated part (chart / definedName / foreign sheet) as a "success".
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e) if tag_local_eq(e.name().as_ref(), tag) => {
                in_tag = true;
                residual = detect_residual(&e).is_some();
                f_raw.clear();
                writer
                    .write_event(Event::Start(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::End(e) if tag_local_eq(e.name().as_ref(), tag) => {
                if in_tag && !residual {
                    // Whole element body reassembled across Text + GeneralRef; shift once.
                    let raw = std::mem::take(&mut f_raw);
                    let out_ev = match logical_formula(&raw) {
                        Some(logical) if !refshift::has_unquoted_non_ascii_qualifier(&logical) => {
                            let before_ref = logical.matches("#REF!").count();
                            let (nf, n) = refshift::shift_formula(&logical, host, edit);
                            shifted += n;
                            // Count only NEWLY introduced #REF! (a genuine straddle/overflow),
                            // subtracting any pre-existing #REF! the formula already carried — a
                            // dangling reference left by an earlier column/name deletion is not a
                            // fault of THIS edit and must not inflate ref_errors (which, for a
                            // move, would spuriously trip the straddle net).
                            errs += nf.matches("#REF!").count().saturating_sub(before_ref) as u32;
                            if nf == logical {
                                Event::Text(BytesText::from_escaped(raw))
                            } else {
                                Event::Text(BytesText::from_escaped(text_escape(&nf)))
                            }
                        }
                        Some(_) => {
                            // FAIL-CLOSED: unquoted non-ASCII qualifier — flag, do not shift.
                            qualifier_risk = true;
                            Event::Text(BytesText::from_escaped(raw))
                        }
                        None => Event::Text(BytesText::from_escaped(raw)),
                    };
                    writer
                        .write_event(out_ev)
                        .map_err(|e| anyhow!("xml write: {e}"))?;
                }
                in_tag = false;
                residual = false;
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Text(t) if in_tag && !residual => {
                push_text_raw(&mut f_raw, &t);
            }
            Event::GeneralRef(r) if in_tag && !residual => {
                push_ref_raw(&mut f_raw, &r);
            }
            other => {
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((
        writer.into_inner().into_inner(),
        shifted,
        errs,
        qualifier_risk,
    ))
}

/// Shift the refers-to body of every `<definedName>` in workbook.xml, scoping unqualified
/// references to the name's OWN sheet. A worksheet-scoped name (`localSheetId="N"`) resolves
/// its unqualified references against the Nth sheet (0-based, workbook order), so a scoped
/// name like `$A$8` on the edited sheet is shifted — the generic path used host="" for every
/// name, which never matched the edited sheet and left scoped unqualified names stale. A
/// global name (no localSheetId) uses no host; its refers-to must be qualified.
fn shift_defined_names(
    src: &[u8],
    edit: &StructuralEdit,
    sheet_names: &[String],
) -> Result<(Vec<u8>, u32, u32, bool)> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut in_dn = false;
    let mut host = String::new();
    let mut qualifier_risk = false;
    let mut f_raw = String::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e) if tag_local_eq(e.name().as_ref(), b"definedName") => {
                in_dn = true;
                host = attr_by_local(&e, b"localSheetId")
                    .and_then(|s| s.parse::<usize>().ok())
                    .and_then(|i| sheet_names.get(i).cloned())
                    .unwrap_or_default();
                f_raw.clear();
                writer
                    .write_event(Event::Start(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::End(e) if tag_local_eq(e.name().as_ref(), b"definedName") => {
                if in_dn {
                    let raw = std::mem::take(&mut f_raw);
                    let out_ev = match logical_formula(&raw) {
                        Some(logical) if !refshift::has_unquoted_non_ascii_qualifier(&logical) => {
                            let before_ref = logical.matches("#REF!").count();
                            let (nf, n) = refshift::shift_formula(&logical, &host, edit);
                            shifted += n;
                            // Only NEWLY introduced #REF! counts — a defined name that already
                            // held a dangling #REF! (a common leftover from an earlier column/
                            // name deletion) is not this edit's fault and must not inflate
                            // ref_errors (which would spuriously trip the move straddle net).
                            errs += nf.matches("#REF!").count().saturating_sub(before_ref) as u32;
                            if nf == logical {
                                Event::Text(BytesText::from_escaped(raw))
                            } else {
                                Event::Text(BytesText::from_escaped(text_escape(&nf)))
                            }
                        }
                        // A non-ASCII sheet qualifier: shift_formula would mis-parse it, so the
                        // body is left unshifted. Refuse ONLY when a shift is actually needed:
                        // when the edited sheet is NON-ASCII its references can only be spelled
                        // with that name, so the affect check covers them exactly; when it is
                        // ASCII, a co-located ASCII edited-sheet reference could need a shift we
                        // cannot safely apply through the non-ASCII body, so stay conservative.
                        Some(logical) => {
                            if edit.sheet.is_ascii()
                                || non_ascii_qualifier_affected(&logical, &edit.sheet, edit)
                            {
                                qualifier_risk = true;
                            }
                            Event::Text(BytesText::from_escaped(raw))
                        }
                        None => Event::Text(BytesText::from_escaped(raw)),
                    };
                    writer
                        .write_event(out_ev)
                        .map_err(|e| anyhow!("xml write: {e}"))?;
                }
                in_dn = false;
                host.clear();
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Text(t) if in_dn => push_text_raw(&mut f_raw, &t),
            Event::GeneralRef(r) if in_dn => push_ref_raw(&mut f_raw, &r),
            other => {
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("xml write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((
        writer.into_inner().into_inner(),
        shifted,
        errs,
        qualifier_risk,
    ))
}

/// The local part of a (possibly namespace-prefixed) XML name: `x:table` -> `table`.
pub(crate) fn local_of(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

fn tag_local_eq(name: &[u8], local: &[u8]) -> bool {
    local_of(name) == local
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refshift::{Axis, Op, StructuralEdit};
    use std::io::Read; // File::read_to_end/read_to_string in fixture helpers below

    fn edit(sheet: &str, axis: Axis, op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit {
            axis,
            at,
            count,
            op,
            sheet: sheet.into(),
            dest: 0,
        }
    }
    /// A minimal worksheet xml carrying the given trailing element(s) after
    /// </sheetData> (e.g. a <tableParts> block) — for the table-detection tests.
    fn sd_worksheet(trailer: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/>{trailer}</worksheet>"#
        )
    }

    fn move_edit(sheet: &str, at: u32, count: u32, dest: u32) -> StructuralEdit {
        StructuralEdit {
            axis: Axis::Row,
            at,
            count,
            op: Op::Move,
            sheet: sheet.into(),
            dest,
        }
    }

    #[test]
    fn replace_attr_preserves_siblings() {
        let inner = br#"c r="A5" s="3" t="n""#;
        let out = replace_attr_value(inner, b"r", "A6");
        assert_eq!(&out, br#"c r="A6" s="3" t="n""#);
    }

    #[test]
    fn foreign_sheet_shifts_only_edited_sheet_refs() {
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><f>Sheet1!A5+B10</f></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);
        let (out, n, _r, _q) = shift_text_in_element(xml, b"f", &e, "Sheet2").unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Sheet1!A6+B10"), "got: {s}");
        assert_eq!(n, 1);
    }

    #[test]
    fn pivot_shifts_ref_and_preserves_following_sibling() {
        // Regression: rewrite_pivot previously read+discarded the event AFTER a
        // self-closing <worksheetSource/>, silently dropping the next sibling and
        // unbalancing the pivot XML. The sibling (<cacheFields/>) must survive and
        // the ref must shift under an insert on the source sheet.
        let xml = br#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource ref="A1:B5" sheet="S"/></cacheSource><cacheFields count="2"/></pivotCacheDefinition>"#;
        let e = edit("S", Axis::Row, Op::Insert, 2, 1);
        let (out, n, r, _unhandled) = rewrite_pivot(xml, &e).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("<cacheFields"), "following sibling dropped: {s}");
        assert!(s.contains(r#"ref="A1:B6""#), "ref not shifted: {s}");
        assert_eq!((n, r), (1, 0)); // one range shifted, no #REF!
                                    // and the output is well-formed (round-trips through the reader)
        let mut rd = Reader::from_reader(out.as_slice());
        let mut b = Vec::new();
        loop {
            if rd
                .read_event_into(&mut b)
                .expect("malformed pivot XML produced")
                == Event::Eof
            {
                break;
            }
        }
    }

    #[test]
    fn external_link_workbook_is_not_presence_refused() {
        // REGRESSION (round-58 defect 5): an intra-workbook edit never touches an external `[n]...`
        // reference (shift_ref leaves `[`-prefixed refs unchanged) or the external-link cache (keyed
        // by the EXTERNAL workbook's coordinates), so a workbook with an external link must NOT be
        // presence-refused — the edit is a faithful pure coordinate shift.
        use std::io::{Read, Write};
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
                let mut s = String::new();
                f.read_to_string(&mut s).unwrap();
                let s = s.replace(
                    "</sheetData>",
                    r#"<row r="10"><c r="A10"><f>[1]Sheet1!A1+A5</f><v>99</v></c></row></sheetData>"#,
                );
                out.write_all(s.as_bytes()).unwrap();
            } else {
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                out.write_all(&b).unwrap();
            }
        }
        out.start_file("xl/externalLinks/externalLink1.xml", opts)
            .unwrap();
        out.write_all(br#"<externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><externalBook><sheetDataSet><sheetData sheetId="0"><row r="1"><cell r="A1"><v>99</v></cell></row></sheetData></sheetDataSet></externalBook></externalLink>"#).unwrap();
        let bytes = out.finish().unwrap().into_inner();
        let (result, report) =
            structural_edit(&bytes, &edit("Sheet1", Axis::Row, Op::Insert, 2, 1)).unwrap();
        assert!(
            report.residuals.is_empty(),
            "an external-link workbook must not be refused: {:?}",
            report.residuals
        );
        let mut z = zip::ZipArchive::new(Cursor::new(result.as_slice())).unwrap();
        let mut sf = z.by_name("xl/worksheets/sheet1.xml").unwrap();
        let mut s = String::new();
        sf.read_to_string(&mut s).unwrap();
        assert!(
            s.contains("[1]Sheet1!A1+A6"),
            "external ref preserved while local ref shifts: {s}"
        );
    }

    #[test]
    fn real_pivot_workbook_stays_wellformed_after_structural_edit() {
        // End-to-end regression on the committed pivot+chart fixture: a structural
        // edit must leave every pivot/chart part WELL-FORMED (the event-swallow bug
        // produced unbalanced XML) and the whole workbook must reload.
        const PIVOT: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/t1/pivot-chart.xlsx"
        );
        let input = std::fs::read(PIVOT).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        let (out, _report) = structural_edit(&input, &e).unwrap();
        // every pivot/chart part in the output parses as well-formed XML
        let mut z = zip::ZipArchive::new(Cursor::new(out.as_slice())).unwrap();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let name = f.name().to_string();
            if name.starts_with("xl/pivotCache")
                || name.starts_with("xl/pivotTables")
                || name.starts_with("xl/charts/")
            {
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                let mut rd = Reader::from_reader(b.as_slice());
                let mut buf = Vec::new();
                loop {
                    if rd
                        .read_event_into(&mut buf)
                        .unwrap_or_else(|err| panic!("{name} is not well-formed: {err}"))
                        == Event::Eof
                    {
                        break;
                    }
                    buf.clear();
                }
            }
        }
        // and the workbook still loads in the engine
        let tmp = unique_tmp("pivotwb");
        std::fs::write(&tmp, &out).unwrap();
        assert!(ironcalc::import::load_from_xlsx(&tmp, "en", "UTC", "en").is_ok());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn pivot_start_form_keeps_children() {
        // Non-self-closing <worksheetSource>...</worksheetSource>: the Start form
        // must stay a Start (children copied through), not be forced to Empty.
        let xml = br#"<worksheetSource ref="A1:B5" sheet="S"><child/></worksheetSource>"#;
        let e = edit("S", Axis::Row, Op::Insert, 2, 1);
        let (out, _n, _r, _u) = rewrite_pivot(xml, &e).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("<child/>"), "child dropped: {s}");
        assert!(s.contains("</worksheetSource>"), "not closed: {s}");
        assert!(s.contains(r#"ref="A1:B6""#), "ref not shifted: {s}");
    }

    #[test]
    fn malformed_secondary_xml_fails_closed_not_truncated() {
        // A mid-stream parse error must propagate (Err), never return a silently
        // truncated part. Feed unbalanced XML to both rewriters.
        let bad = b"<definedName>Sheet1!A1</definedName><oops attr='unclosed";
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        assert!(shift_text_in_element(bad, b"definedName", &e, "").is_err());
        let badpivot = b"<worksheetSource ref='A1' sheet='S'/><oops attr='unclosed";
        assert!(rewrite_pivot(badpivot, &e).is_err());
    }

    #[test]
    fn chart_ref_shifts() {
        let xml = br#"<c:chart><c:f>Sheet1!$A$1:$A$10</c:f></c:chart>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 2);
        let (out, n, _r, _q) = shift_text_in_element(xml, b"f", &e, "").unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Sheet1!$A$1:$A$12"), "got: {s}");
        assert_eq!(n, 1);
    }

    #[test]
    fn defined_name_shifts() {
        let xml = br#"<definedNames><definedName name="Data">Sheet1!$A$1:$A$10</definedName></definedNames>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (out, n, _r, _q) = shift_text_in_element(xml, b"definedName", &e, "").unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Sheet1!$A$1:$A$11"), "got: {s}");
        assert_eq!(n, 1);
    }

    #[test]
    fn edited_sheet_insert_shifts_rows_cells_formulas() {
        let xml = br#"<worksheet><dimension ref="A1:B10"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="10"><c r="A10"><f>SUM(A1:A9)</f></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "xl/worksheets/sheet1.xml", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        // row 10 -> 11, cell A10 -> A11, SUM(A1:A9) straddles insert at 5 -> A1:A10
        assert!(s.contains(r#"<row r="11">"#), "row bump: {s}");
        assert!(s.contains(r#"<c r="A11">"#), "cell bump: {s}");
        assert!(s.contains("SUM(A1:A10)"), "formula grow: {s}");
        // blank row injected at 5
        assert!(s.contains(r#"<row r="5"/>"#), "blank inject: {s}");
        // row 1 untouched
        assert!(s.contains(r#"<row r="1">"#), "row1 kept: {s}");
        assert_eq!(report.rows_inserted, 1);
    }

    #[test]
    fn edited_sheet_delete_row_and_ref() {
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><f>A5</f></c></row><row r="5"><c r="A5"><v>9</v></c></row><row r="6"><c r="A6"><v>7</v></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Delete, 5, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        // original row 5 (value 9) is gone entirely
        assert!(!s.contains(r#"<v>9</v>"#), "deleted row content gone: {s}");
        // A5 in the formula was consumed → #REF!
        assert!(s.contains(r#"<f>#REF!</f>"#), "ref err: {s}");
        // old row 6 (value 7) shifts up into row 5 / cell A5
        assert!(s.contains(r#"<row r="5">"#), "old row6 -> 5: {s}");
        assert!(
            s.contains(r#"<c r="A5">"#) && s.contains(r#"<v>7</v>"#),
            "A6 -> A5: {s}"
        );
        assert_eq!(report.rows_deleted, 1);
    }

    /// Committed fixtures, resolved relative to the crate (machine-independent).
    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/structural/");

    fn unique_tmp(tag: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("xlq-st-{tag}-{}-{n}.xlsx", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn end_to_end_insert_row_recomputes_and_shifts_all_refs() {
        let input = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (out, report) = structural_edit(&input, &e).unwrap();

        // proof-carrying: reopen in IronCalc and check recompute-equivalence
        let path = unique_tmp("e2e-ins");
        std::fs::write(&path, &out).unwrap();
        let mut m = ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en").unwrap();
        m.evaluate();
        // A11 SUM moved to A12, still = 55 (blank row 5 contributes 0)
        assert_eq!(
            m.get_formatted_cell_value(0, 12, 1).unwrap(),
            "55",
            "SUM recompute"
        );
        // A13 (=A5*2) moved to A14, A5 shifted to A6 (value 5) => 10
        assert_eq!(
            m.get_formatted_cell_value(0, 14, 1).unwrap(),
            "10",
            "A5*2 recompute"
        );
        // Sheet2!B1 = Sheet1!A11 -> Sheet1!A12 = 55
        assert_eq!(
            m.get_formatted_cell_value(1, 1, 2).unwrap(),
            "55",
            "cross-sheet recompute"
        );
        std::fs::remove_file(&path).ok();

        // formula-shift correctness in the output XML
        let sheet1 = read_zip_part(&out, "xl/worksheets/sheet1.xml");
        assert!(
            sheet1.contains("SUM(A1:A11)"),
            "SUM grew: {}",
            &sheet1[..sheet1.len().min(400)]
        );
        assert!(sheet1.contains("A6*2"), "A5*2 -> A6*2");
        assert!(sheet1.contains("$A$9"), "$A$8 -> $A$9");
        assert!(sheet1.contains(r#"<row r="5"/>"#), "blank row injected");
        let sheet2 = read_zip_part(&out, "xl/worksheets/sheet2.xml");
        assert!(sheet2.contains("Sheet1!A12"), "cross-sheet ref shifted");
        assert!(sheet2.contains("Sheet2!A1+5"), "self-sheet ref unchanged");
        let wb = read_zip_part(&out, "xl/workbook.xml");
        assert!(wb.contains("Sheet1!$A$12"), "defined name shifted: {wb}");

        assert!(report.residuals.is_empty(), "no residuals expected");
        assert!(
            report.refs_shifted >= 4,
            "shifted {} refs",
            report.refs_shifted
        );
    }

    #[test]
    fn minimal_patch_only_coordinate_bytes_change() {
        // The invariant: parts with no reference to the edited sheet are
        // byte-identical; changed parts differ only where σ fired.
        let input = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (out, _r) = structural_edit(&input, &e).unwrap();
        let before = zip_parts(&input);
        let after = zip_parts(&out);
        // styles/theme/sharedStrings must be byte-identical (no ref to edited rows)
        for p in [
            "xl/styles.xml",
            "xl/theme/theme1.xml",
            "xl/sharedStrings.xml",
        ] {
            if let (Some(b), Some(a)) = (before.get(p), after.get(p)) {
                assert_eq!(b, a, "part {p} must be byte-identical");
            }
        }
        // calcChain is dropped (rebuildable), never present in output
        assert!(!after.contains_key("xl/calcChain.xml"), "calcChain dropped");
    }

    #[test]
    fn volatile_dependencies_is_dropped_like_calcchain() {
        // The volatile/RTD dependency cache carries <tr r> cell coords that would go stale after a
        // shift; restructure drops it (as it does calcChain) so no stale coordinate is committed —
        // Excel rebuilds it on open. (certify allowlists it for a foreign edit that keeps it.)
        let input = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        let with_vd = inject_part(
            &input,
            "xl/volatileDependencies.xml",
            br#"<volTypes xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><volType type="volatile"><main first="1"><tp t="n"><tr r="A1" s="0"/></tp></main></volType></volTypes>"#,
        );
        assert!(
            zip_parts(&with_vd).contains_key("xl/volatileDependencies.xml"),
            "fixture must contain the part"
        );
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (out, _r) = structural_edit(&with_vd, &e).unwrap();
        assert!(
            !zip_parts(&out).contains_key("xl/volatileDependencies.xml"),
            "volatileDependencies must be dropped"
        );
    }

    fn inject_part(bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
        let mut zin = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut out));
            let opts = zip::write::SimpleFileOptions::default();
            for i in 0..zin.len() {
                let mut f = zin.by_index(i).unwrap();
                let n = f.name().to_string();
                let mut data = Vec::new();
                f.read_to_end(&mut data).unwrap();
                zw.start_file(n, opts).unwrap();
                std::io::Write::write_all(&mut zw, &data).unwrap();
            }
            zw.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zw, content).unwrap();
            zw.finish().unwrap();
        }
        out
    }

    fn read_zip_part(bytes: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut f = z.by_name(name).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        s
    }
    fn zip_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut m = BTreeMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            if f.is_file() {
                let name = f.name().to_string();
                let mut v = Vec::new();
                f.read_to_end(&mut v).unwrap();
                m.insert(name, v);
            }
        }
        m
    }

    fn rezip(parts: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut z = zip::ZipWriter::new(Cursor::new(&mut out));
            let opts = zip::write::SimpleFileOptions::default();
            for (n, c) in parts {
                z.start_file(n, opts).unwrap();
                std::io::Write::write_all(&mut z, c).unwrap();
            }
            z.finish().unwrap();
        }
        out
    }

    #[test]
    fn foreign_sheet_drawing_textlink_is_shifted_end_to_end() {
        // REGRESSION (round-49 defect 1/4): a drawing on a FOREIGN sheet (Sheet2) carrying a linked
        // shape `textlink="Sheet1!$A$8"` must SHIFT when the edited sheet (Sheet1) moves that cell —
        // else the shape silently mirrors a different cell (value corruption), and certify's own
        // baseline (built from this transform) is equally stale so it spuriously REFUSES the
        // faithful edit. structural_edit must now shift the drawing and report it in parts_touched.
        let base = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        let mut parts = zip_parts(&base);
        let s2 = String::from_utf8(parts["xl/worksheets/sheet2.xml"].clone())
            .unwrap()
            .replace(
                "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
                "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
                 xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
            )
            .replace("</worksheet>", "<drawing r:id=\"rId1\"/></worksheet>");
        parts.insert("xl/worksheets/sheet2.xml".into(), s2.into_bytes());
        parts.insert(
            "xl/worksheets/_rels/sheet2.xml.rels".into(),
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#.to_vec(),
        );
        // A qualified textlink (Sheet1!$A$8) AND an unqualified `<xdr:f>` (A8, local to Sheet2's
        // host — must NOT shift, since Sheet2 row 8 is untouched by an edit on Sheet1).
        parts.insert(
            "xl/drawings/drawing1.xml".into(),
            br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><xdr:twoCellAnchor><xdr:from><xdr:col>3</xdr:col><xdr:row>0</xdr:row></xdr:from><xdr:to><xdr:col>5</xdr:col><xdr:row>2</xdr:row></xdr:to><xdr:sp macro="" textlink="Sheet1!$A$8"><xdr:spPr/></xdr:sp><xdr:graphicFrame><xdr:f>A8</xdr:f></xdr:graphicFrame></xdr:twoCellAnchor></xdr:wsDr>"#.to_vec(),
        );
        let input = rezip(&parts);
        let (out, report) =
            structural_edit(&input, &edit("Sheet1", Axis::Row, Op::Insert, 1, 3)).unwrap();
        assert!(
            report.residuals.is_empty(),
            "faithful drawing shift must not refuse: {:?}",
            report.residuals
        );
        let drawing = read_zip_part(&out, "xl/drawings/drawing1.xml");
        // qualified cross-sheet textlink follows the moved cell A8 -> A11
        assert!(
            drawing.contains(r#"textlink="Sheet1!$A$11""#),
            "cross-sheet textlink must shift A8->A11: {drawing}"
        );
        // unqualified `<xdr:f>` is local to Sheet2 (the host) -> Sheet2 row 8 unaffected -> unchanged
        assert!(
            drawing.contains("<xdr:f>A8</xdr:f>"),
            "an unqualified drawing ref local to the foreign host must NOT shift: {drawing}"
        );
        assert!(
            report
                .parts_touched
                .iter()
                .any(|p| p == "xl/drawings/drawing1.xml"),
            "a shifted drawing must be reported in parts_touched: {:?}",
            report.parts_touched
        );
    }

    #[test]
    fn date_typed_value_cells_extracts_iso_dates() {
        // The t="d" scanner backs certify's byte-level date_value_mismatch guard (round-49 defect 3).
        let xml = br#"<worksheet xmlns="urn:x"><sheetData><row r="1"><c r="A1" t="n"><v>5</v></c><c r="H1" t="d"><v>2020-01-01T00:00:00</v></c></row><row r="2"><c r="H2" t="d"><v>1999-12-31T00:00:00</v></c></row></sheetData></worksheet>"#;
        assert_eq!(
            date_typed_value_cells(xml),
            vec![
                ("H1".to_string(), "2020-01-01T00:00:00".to_string()),
                ("H2".to_string(), "1999-12-31T00:00:00".to_string()),
            ]
        );
        // namespace-PREFIXED date cell (bound to main ns) is caught too — a prefix must not evade it.
        let px = br#"<x:worksheet xmlns:x="urn:x"><x:sheetData><x:c r="B2" t="d"><x:v>2000-06-15T00:00:00</x:v></x:c></x:sheetData></x:worksheet>"#;
        assert_eq!(
            date_typed_value_cells(px),
            vec![("B2".to_string(), "2000-06-15T00:00:00".to_string())]
        );
        // an ordinary (non-date) cell whose VALUE happens to look like a date is NOT captured.
        let nd = br#"<worksheet><sheetData><row><c r="A1"><v>2020-01-01T00:00:00</v></c></row></sheetData></worksheet>"#;
        assert!(date_typed_value_cells(nd).is_empty());
    }

    #[test]
    fn cf_and_dv_formula_bodies_shift() {
        // conditional-formatting rule body + data-validation formula must shift,
        // not just their sqref (the confirmed half-shift bug).
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"/></row></sheetData><conditionalFormatting sqref="B5:B20"><cfRule type="expression"><formula>$A5&gt;0</formula></cfRule></conditionalFormatting><dataValidation sqref="C5:C20"><formula1>$D5</formula1></dataValidation></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains(r#"sqref="B6:B21""#), "CF sqref shifts: {s}");
        assert!(s.contains("$A6"), "CF formula body shifts ($A5->$A6): {s}");
        assert!(s.contains(r#"sqref="C6:C21""#), "DV sqref shifts");
        assert!(s.contains("$D6"), "DV formula1 body shifts ($D5->$D6): {s}");
    }

    #[test]
    fn main_formula_entities_reassemble_and_shift() {
        // REGRESSION for the quick-xml >=0.38 entity split: a <f> body carrying
        // multiple XML entities (>, &, <>) is delivered as Text + GeneralRef
        // fragments; it must be reassembled and shifted as ONE formula, never
        // per-fragment (which would silently corrupt it). Insert at row 3 shifts
        // the row-5 refs (A5->A6, B5->B6) and re-escapes byte-exact.
        let xml = br#"<worksheet><sheetData><row r="5"><c r="A5"><f>IF(A5&gt;0,A5&amp;"x",B5&lt;&gt;0)</f><v>1</v></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"<f>IF(A6&gt;0,A6&amp;"x",B6&lt;&gt;0)</f>"#),
            "multi-entity formula reassembled, shifted, re-escaped exactly: {s}"
        );
        assert_eq!(report.ref_errors, 0, "no #REF!: {s}");
    }

    #[test]
    fn unchanged_entity_formula_is_byte_identical() {
        // The no-op path must write the ORIGINAL entity-bearing bytes back
        // verbatim (from_escaped(raw)), not re-normalize them. Insert far below
        // the referenced rows leaves the formula unchanged.
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><f>IF(A1&gt;0,A1&amp;"y",B1&lt;&gt;0)</f></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 50, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"<f>IF(A1&gt;0,A1&amp;"y",B1&lt;&gt;0)</f>"#),
            "unchanged entity formula preserved byte-exact: {s}"
        );
        assert_eq!(report.refs_shifted, 0, "nothing shifted below the edit");
    }

    #[test]
    fn strip_formula_caches_drops_only_formula_results() {
        // REGRESSION (round-32): a structural edit changes computed values, so xlq must
        // invalidate every formula cache (Excel/openpyxl recompute a cache-less formula) —
        // committing the stale `<v>` was a silent value corruption. Formula `<v>` is dropped,
        // the `<f>` is kept, and a LITERAL cell's `<v>` survives.
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>SUM(A1:A1)</f><v>55</v></c><c r="C1" t="str"><f>A1&amp;"x"</f><v>1x</v></c><c r="D1"><f>A1</f><v /></c><c r="E1"><f t="shared" si="0"/><v>9</v></c></row></sheetData></worksheet>"#;
        let out = strip_formula_caches(xml);
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"<c r="A1"><v>1</v></c>"#),
            "literal value kept: {s}"
        );
        assert!(
            s.contains(r#"<c r="B1"><f>SUM(A1:A1)</f></c>"#),
            "B1 formula kept, cache dropped: {s}"
        );
        assert!(
            s.contains(r#"<f>A1&amp;"x"</f>"#) && !s.contains("1x"),
            "C1 string-result cache dropped, formula kept: {s}"
        );
        assert!(
            !s.contains("<v>9</v>"),
            "shared-dependent formula cache dropped: {s}"
        );
        assert!(
            !s.contains("</f><v>"),
            "no populated formula cache remains: {s}"
        );
        // A sheet with no cached formula result is byte-identical.
        let plain = br#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#;
        assert_eq!(strip_formula_caches(plain), plain.to_vec());
    }

    #[test]
    fn shared_master_with_entity_expands_correctly() {
        // The shared-master body (A2>0) carries an entity; it must be captured
        // whole across Text+GeneralRef so dependents expand to A3>0, A4>0.
        let xml = br#"<worksheet><sheetData><row r="2"><c r="B2"><f t="shared" ref="B2:B4" si="0">A2&gt;0</f></c></row><row r="3"><c r="B3"><f t="shared" si="0"/></c></row><row r="4"><c r="B4"><f t="shared" si="0"/></c></row></sheetData></worksheet>"#;
        let out = expand_shared_in_sheet(xml).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("A2&gt;0"), "master body preserved: {s}");
        assert!(s.contains("A3&gt;0"), "dependent B3 -> A3>0: {s}");
        assert!(s.contains("A4&gt;0"), "dependent B4 -> A4>0: {s}");
    }

    #[test]
    fn move_path_entity_formula_shifts() {
        // The move path must also reassemble entity-bearing formula bodies.
        let xml = br#"<worksheet><dimension ref="A1:C8"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><row r="3"><c r="A3"><v>3</v></c></row><row r="4"><c r="A4"><v>4</v></c></row><row r="5"><c r="A5"><v>5</v></c></row><row r="6"><c r="A6"><v>6</v></c><c r="C6"><f>IF(A6&gt;0,A6,B6)</f><v>12</v></c></row><row r="7"><c r="A7"><v>7</v></c></row><row r="8"><c r="A8"><v>8</v></c></row></sheetData></worksheet>"#;
        let e = move_edit("Sheet1", 6, 1, 3);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "xl/worksheets/sheet1.xml", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"<c r="C3"><f>IF(A3&gt;0,A3,B3)</f>"#),
            "move-path entity formula reassembled and shifted (row 6 -> 3): {s}"
        );
        assert!(
            report.residuals.is_empty(),
            "no residuals: {:?}",
            report.residuals
        );
    }

    #[test]
    fn table_part_forces_residual() {
        // a workbook containing a table part must be REFUSED (we don't shift
        // table extents), never silently corrupted.
        let input = std::fs::read(format!("{FIX}table.xlsx")).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);
        let (_out, report) = structural_edit(&input, &e).unwrap();
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "table_unsupported"),
            "table must force a residual"
        );
    }

    #[test]
    fn table_on_an_unrelated_sheet_does_not_block_the_edit() {
        // REGRESSION (false refusal): pivot-chart.xlsx has a structured Table on the
        // sheet "Table" (xl/worksheets/sheet5.xml). Editing a DIFFERENT sheet cannot
        // move that table's extent — its coordinates are sheet-local and the part is
        // copied byte-for-byte — so refusing the edit was wrong. It must now proceed.
        let input = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/t1/pivot-chart.xlsx"
        ))
        .unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (_out, report) = structural_edit(&input, &e).unwrap();
        // The TABLE (on sheet5) must not force a residual — that was the false refusal.
        assert!(
            !report
                .residuals
                .iter()
                .any(|r| r.reason.starts_with("table_")),
            "a table on an unrelated sheet must not force a residual: {:?}",
            report.residuals
        );
        // (Sheet1 of this fixture separately owns a comment + drawing, so the edit is
        // still refused for `unshiftable_sheet_attachment` — a CORRECT refusal that the
        // crude table rule used to mask; see the guard tests below.)
    }

    #[test]
    fn comment_on_the_edited_sheet_is_refused() {
        // REGRESSION (round-2 review): a comment anchored to the edited sheet is not
        // shifted, so the note would detach; the fail-closed attachment whitelist
        // refuses it. pivot-chart's Sheet1 owns a comment (and a drawing).
        let input = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/t1/pivot-chart.xlsx"
        ))
        .unwrap();
        assert!(edited_sheet_bad_attachment(
            &input,
            "xl/worksheets/sheet1.xml",
            &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
        )
        .is_some());
        let (_o, report) =
            structural_edit(&input, &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)).unwrap();
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "unshiftable_sheet_attachment"),
            "an unshiftable attachment on the edited sheet must force a residual: {:?}",
            report.residuals
        );
        // A sheet with NO rels part has no attachments -> not flagged (refs.xlsx Sheet1).
        let plain = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        assert!(edited_sheet_bad_attachment(
            &plain,
            "xl/worksheets/sheet1.xml",
            &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
        )
        .is_none());
    }

    #[test]
    fn comment_and_vml_attachments_are_affect_based() {
        // REGRESSION (round-33): comments/notes and legacy VML shapes were PRESENCE-refused,
        // rejecting almost every real annotated workbook. They are now AFFECT-based, exactly like
        // drawing anchors — refuse only when the edit MOVES the anchored cell or a bound cell.
        let ins1 = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let ins100 = edit("Sheet1", Axis::Row, Op::Insert, 100, 1);
        let ins5 = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);

        // A note anchored at A2 MOVES under an insert at row 1, but not one far below at row 100.
        let cmts = br#"<comments><commentList><comment ref="A2"><text><t>x</t></text></comment></commentList></comments>"#;
        assert!(comment_refs_affected(cmts, &ins1));
        assert!(!comment_refs_affected(cmts, &ins100));

        // A VML note anchor <x:Row>1</x:Row> (0-based -> row 2) moves under insert@1, not @100.
        let vml_note = br#"<xml xmlns:x="u"><x:ClientData ObjectType="Note"><x:Row>1</x:Row><x:Column>0</x:Column></x:ClientData></xml>"#;
        assert!(vml_anchor_affected(vml_note, &ins1));
        assert!(!vml_anchor_affected(vml_note, &ins100));

        // A LOCAL unqualified control binding ($A$8, edited-sheet host) moves under insert@5 —
        // this is why the edited-sheet VML must be checked with the edited sheet as host (the
        // phantom-host foreign scan would miss it, opening a silent-wrong hole).
        let vml_bind = br#"<xml xmlns:x="u"><x:ClientData ObjectType="Checkbox"><x:FmlaLink>$A$8</x:FmlaLink></x:ClientData></xml>"#;
        assert!(vml_binding_affected_on_host(vml_bind, &ins5, "Sheet1"));
        assert!(!vml_binding_crosses_edited(vml_bind, &ins5)); // phantom host: local ref not "crossing"

        // The FOREIGN-sheet scan flags a binding explicitly qualified to the edited sheet.
        let vml_x = br#"<xml xmlns:x="u"><x:ClientData ObjectType="Checkbox"><x:FmlaLink>Sheet1!$A$8</x:FmlaLink></x:ClientData></xml>"#;
        assert!(vml_binding_crosses_edited(vml_x, &ins5));
        assert!(!vml_binding_crosses_edited(vml_note, &ins5)); // no binding element -> not flagged
    }

    #[test]
    fn pivot_attachment_is_affect_based_not_presence_refused() {
        // REGRESSION (round-54 defect 6, over-refusal): a PivotTable on the edited sheet was
        // presence-refused unconditionally. Now it is affect-based (like comments/drawings): refused
        // only when the edit moves its render <location ref>; a pivot outside the edit's band commits.
        let pt = |loc: &str| {
            format!(r#"<pivotTableDefinition xmlns="urn:x"><location ref="{loc}"/></pivotTableDefinition>"#).into_bytes()
        };
        // insert 1 row at 200: a pivot rendered at A100:F120 (entirely above) is UNAFFECTED.
        let e = edit("Sheet1", Axis::Row, Op::Insert, 200, 1);
        assert!(
            !pivot_location_affected(&pt("A100:F120"), &e),
            "pivot above the edit is unaffected"
        );
        // the SAME pivot when the insert is ABOVE it (row 50) DOES move -> affected (fail-closed).
        let e2 = edit("Sheet1", Axis::Row, Op::Insert, 50, 1);
        assert!(
            pivot_location_affected(&pt("A100:F120"), &e2),
            "pivot below the insert moves -> affected"
        );
        // a straddling insert (inside the extent) grows it -> affected.
        assert!(pivot_location_affected(
            &pt("A100:F120"),
            &edit("Sheet1", Axis::Row, Op::Insert, 110, 1)
        ));
        // a malformed / location-less part fails closed.
        assert!(pivot_location_affected(
            b"<pivotTableDefinition xmlns=\"urn:x\"/>",
            &e
        ));

        // End-to-end through edited_sheet_bad_attachment: a /pivotTable rel to an unaffected pivot is
        // NOT flagged; an affected one IS.
        let with_pivot = |loc: &str| {
            let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("xl/worksheets/_rels/sheet1.xml.rels", opts)
                .unwrap();
            z.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>"#).unwrap();
            z.start_file("xl/pivotTables/pivotTable1.xml", opts)
                .unwrap();
            z.write_all(&pt(loc)).unwrap();
            z.finish().unwrap().into_inner()
        };
        use std::io::Write;
        assert!(
            edited_sheet_bad_attachment(&with_pivot("A100:F120"), "xl/worksheets/sheet1.xml", &e)
                .is_none(),
            "an unaffected pivot must not be presence-refused"
        );
        assert_eq!(
            edited_sheet_bad_attachment(&with_pivot("A100:F120"), "xl/worksheets/sheet1.xml", &e2)
                .as_deref(),
            Some("pivotTable"),
            "an affected pivot is still refused (fail-closed)"
        );
    }

    #[test]
    fn edited_sheet_attachment_whitelist_is_prefix_insensitive_and_fails_closed() {
        use std::io::Write;
        // Build a tiny zip carrying only one sheet's .rels part.
        let with_rels = |rels: &[u8]| {
            let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("xl/worksheets/_rels/sheet1.xml.rels", opts)
                .unwrap();
            z.write_all(rels).unwrap();
            z.finish().unwrap().into_inner()
        };
        let rel = |ty: &str| {
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="{ty}" Target="../x"/></Relationships>"#
            ).into_bytes()
        };
        let base = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        // SAFE types produce no residual. `/image` (round-53 defect 5) is the coordinate-free
        // sheet-background picture at the worksheet level — an over-refusal until it was whitelisted.
        for ty in [
            format!("{base}/hyperlink"),
            format!("{base}/printerSettings"),
            format!("{base}/table"),
            format!("{base}/image"),
        ] {
            let z = with_rels(&rel(&ty));
            assert!(
                edited_sheet_bad_attachment(
                    &z,
                    "xl/worksheets/sheet1.xml",
                    &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
                )
                .is_none(),
                "safe attachment {ty} must not be flagged"
            );
        }
        // A DRAWING is now SHIFTED by the drawing dispatch (refs + display anchor), not refused
        // (round-50 defect 2) — it is no longer a bad attachment.
        let z = with_rels(&rel(&format!("{base}/drawing")));
        assert!(
            edited_sheet_bad_attachment(
                &z,
                "xl/worksheets/sheet1.xml",
                &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
            )
            .is_none(),
            "a drawing is shifted, not flagged"
        );
        // A comment WHOSE ANCHOR THE EDIT MOVES is still flagged by its label (affect-based).
        let z = with_rels(&rel(&format!("{base}/comments")));
        assert_eq!(
            edited_sheet_bad_attachment(
                &z,
                "xl/worksheets/sheet1.xml",
                &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
            )
            .as_deref(),
            Some("comment")
        );
        // An UNKNOWN (future) attachment type refuses by default — the fail-closed whitelist.
        let z = with_rels(&rel("http://example.com/some/future/thing"));
        assert!(edited_sheet_bad_attachment(
            &z,
            "xl/worksheets/sheet1.xml",
            &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
        )
        .is_some());
        // A present-but-MALFORMED rels part fails CLOSED (cannot enumerate -> refuse).
        let z = with_rels(b"<Relationships><not well formed");
        assert_eq!(
            edited_sheet_bad_attachment(
                &z,
                "xl/worksheets/sheet1.xml",
                &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
            )
            .as_deref(),
            Some("unparseable_rels")
        );
    }

    #[test]
    fn drawing_anchor_is_shifted_not_refused() {
        // REGRESSION (round-50 defect 2): a drawing anchored AFTER the edited range is a DISPLAY
        // position only (certify does not compare it), so the edit must not be REFUSED — the anchor
        // is SHIFTED so the chart/image relocates with the rows/cols the edit moves. xdr anchors
        // are 0-based.
        let ins = |x: &[u8], e: &StructuralEdit| -> String {
            String::from_utf8(shift_drawing_anchor(x, e).unwrap()).unwrap()
        };
        // a twoCellAnchor at rows 5..30; insert 2 rows at 3 -> both anchor rows shift +2 (7, 32).
        let two = br#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>5</xdr:row></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:row>30</xdr:row></xdr:to></xdr:twoCellAnchor></xdr:wsDr>"#;
        let out = ins(two, &edit("S", Axis::Row, Op::Insert, 3, 2));
        assert!(out.contains("<xdr:row>7</xdr:row>"), "from row 5->7: {out}");
        assert!(
            out.contains("<xdr:row>32</xdr:row>"),
            "to row 30->32: {out}"
        );
        // an anchor ABOVE the edit is untouched (no false shift): row 0 anchor, insert at 25.
        let above = br#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>0</xdr:row></xdr:from><xdr:ext cx="1" cy="1"/></xdr:oneCellAnchor></xdr:wsDr>"#;
        assert!(
            ins(above, &edit("S", Axis::Row, Op::Insert, 25, 1)).contains("<xdr:row>0</xdr:row>")
        );
        // a COLUMN edit shifts the <col>, not the <row>.
        let out_c = ins(two, &edit("S", Axis::Col, Op::Insert, 1, 1));
        assert!(
            out_c.contains("<xdr:col>1</xdr:col>"),
            "from col 0->1: {out_c}"
        );
        assert!(
            out_c.contains("<xdr:col>4</xdr:col>"),
            "to col 3->4: {out_c}"
        );
        assert!(
            out_c.contains("<xdr:row>5</xdr:row>"),
            "row untouched on a col edit: {out_c}"
        );
        // the edited-sheet dashboard layout no longer refuses.
        let with_rels = |draw: &[u8]| -> Vec<u8> {
            let base = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
            let mut parts = zip_parts(&base);
            let s1 = String::from_utf8(parts["xl/worksheets/sheet1.xml"].clone())
                .unwrap()
                .replace(
                    "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
                    "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
                )
                .replace("</worksheet>", "<drawing r:id=\"rId1\"/></worksheet>");
            parts.insert("xl/worksheets/sheet1.xml".into(), s1.into_bytes());
            parts.insert(
                "xl/worksheets/_rels/sheet1.xml.rels".into(),
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#.to_vec(),
            );
            parts.insert("xl/drawings/drawing1.xml".into(), draw.to_vec());
            rezip(&parts)
        };
        let wb = with_rels(two);
        let (_o, report) =
            structural_edit(&wb, &edit("Sheet1", Axis::Row, Op::Insert, 8, 1)).unwrap();
        assert!(
            !report
                .residuals
                .iter()
                .any(|r| r.reason == "unshiftable_sheet_attachment"),
            "a chart anchored after the edit must NOT refuse: {:?}",
            report.residuals
        );
    }

    #[test]
    fn drawing_textlink_and_xdrf_are_shifted() {
        // A linked shape's `textlink` (and a graphic-frame `<xdr:f>`) is a live cell reference that
        // mirrors a cell's value. An edit that MOVES the referenced cell must SHIFT it (Excel does),
        // scoping unqualified refs to the drawing's owning sheet. `host` is that owning sheet.
        let ins5 = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let tl = |v: &str| {
            format!(
                r#"<xdr:wsDr xmlns:xdr="u"><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>0</xdr:row></xdr:from><xdr:to><xdr:col>1</xdr:col><xdr:row>1</xdr:row></xdr:to><xdr:sp macro="" textlink="{v}"><xdr:spPr/></xdr:sp></xdr:twoCellAnchor></xdr:wsDr>"#
            )
        };
        let shifted = |x: &str| -> String {
            let (out, _n, _e, q) = shift_drawing_refs(x.as_bytes(), "Sheet1", &ins5).unwrap();
            assert!(!q);
            String::from_utf8(out).unwrap()
        };
        // qualified ref to a MOVED cell (A8, row >= 5) -> shifts to A9
        assert!(shifted(&tl("Sheet1!$A$8")).contains(r#"textlink="Sheet1!$A$9""#));
        // ref ABOVE the edit (A2) -> unchanged (no false shift)
        assert!(shifted(&tl("Sheet1!$A$2")).contains(r#"textlink="Sheet1!$A$2""#));
        // ref to ANOTHER sheet -> unchanged by an edit on Sheet1
        assert!(shifted(&tl("Sheet2!$A$8")).contains(r#"textlink="Sheet2!$A$8""#));
        // UNQUALIFIED ref ($A$8) is local to the owning (host) sheet -> shifts to $A$9
        assert!(shifted(&tl("$A$8")).contains(r#"textlink="$A$9""#));
        // a graphic-frame formula referencing a moved cell -> `<xdr:f>` body shifts to A9
        let gf = r#"<xdr:wsDr xmlns:xdr="u"><xdr:graphicFrame><xdr:f>Sheet1!$A$8</xdr:f></xdr:graphicFrame></xdr:wsDr>"#;
        assert!(shifted(gf).contains("<xdr:f>Sheet1!$A$9</xdr:f>"));

        // REGRESSION (round-56 defect 7): a textlink to a NON-ASCII-named sheet on an ASCII host is
        // copied verbatim (NOT refused) — the edit on Sheet1 cannot move a cell on 集計.
        let (out, _n, _e, q) =
            shift_drawing_refs(tl("集計!$A$8").as_bytes(), "Sheet1", &ins5).unwrap();
        assert!(
            !q,
            "non-ASCII textlink to another sheet must not raise qualifier_risk"
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains(r#"textlink="集計!$A$8""#));
    }

    #[test]
    fn extlst_affected_is_affect_based_not_presence_based() {
        let x14 = br#"<worksheet><extLst><ext><x14:conditionalFormatting xmlns:x14="urn:x14"><x14:cfRule><xm:f>$A$5&gt;0</xm:f></x14:cfRule><xm:sqref>A5:A9</xm:sqref></x14:conditionalFormatting></ext></extLst></worksheet>"#;
        // an edit that MOVES the xm:sqref range (insert at row 1) -> affected -> refuse
        assert!(sheet_extlst_affected(
            x14,
            "Sheet1",
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1)
        ));
        // an edit far BELOW the range (insert at row 50) does NOT move it -> NOT affected
        // (a data bar Excel writes on every workbook must not refuse an unrelated edit).
        assert!(!sheet_extlst_affected(
            x14,
            "Sheet1",
            &edit("Sheet1", Axis::Row, Op::Insert, 50, 1)
        ));
        // an <extLst> WITHOUT an xm:sqref is never affected.
        assert!(!sheet_extlst_affected(
            br#"<worksheet><extLst><ext uri="{x}"><foo:bar xmlns:foo="urn:foo"><foo:color rgb="FF0000"/></foo:bar></ext></extLst></worksheet>"#,
            "Sheet1",
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1)
        ));
        // an sqref OUTSIDE any extLst (ordinary sheet content) is not considered.
        assert!(!sheet_extlst_affected(
            br#"<worksheet><conditionalFormatting sqref="A5:A9"><cfRule/></conditionalFormatting></worksheet>"#,
            "Sheet1",
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1)
        ));
    }

    #[test]
    fn foreign_cf_dv_formula_cross_ref_is_shifted_not_refused() {
        // REGRESSION (round-51 defect 4): a foreign CF <formula> / DV <formula1> naming the edited
        // sheet is now SHIFTED (host = its own sheet), so foreign_sheet_cross_ref_unshifted does NOT
        // flag it. (A 3D-span / non-ASCII CF formula stays refused — via the separate
        // threeD_span_unverifiable / non_ascii_qualifier residuals, not this scan.)
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let hit = |xml: &[u8]| foreign_sheet_cross_ref_unshifted(xml, &e);

        let cf = br#"<worksheet><conditionalFormatting sqref="D1"><cfRule><formula>Sheet1!$A$11&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#;
        assert!(!hit(cf));
        let (out, n, _r, _q) = shift_text_in_element(cf, b"formula", &e, "Sheet2").unwrap();
        assert!(n >= 1 && String::from_utf8_lossy(&out).contains("Sheet1!$A$12"));
        let dv = br#"<worksheet><dataValidation sqref="E1"><formula1>Sheet1!$B$1</formula1></dataValidation></worksheet>"#;
        assert!(!hit(dv));
        let (out, n, _r, _q) = shift_text_in_element(dv, b"formula1", &e, "Sheet2").unwrap();
        assert!(n >= 1 && String::from_utf8_lossy(&out).contains("Sheet1!$B$2"));

        // ARRAY <f> — shift_text_in_element SKIPS these, so a cross-ref is left stale -> still flagged.
        assert!(hit(br#"<worksheet><sheetData><row r="1"><c r="A1"><f t="array" ref="A1:B2">SUM(Sheet1!A1)</f></c></row></sheetData></worksheet>"#));
        // A plain cell <f> IS shifted -> not a residual hazard.
        assert!(!hit(br#"<worksheet><sheetData><row r="1"><c r="A1"><f>Sheet1!A11</f></c></row></sheetData></worksheet>"#));
        // A CF body naming a DIFFERENT sheet -> nothing to shift, not flagged.
        assert!(!hit(br#"<worksheet><conditionalFormatting sqref="D1"><cfRule><formula>Sheet2!$A$1&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#));
    }

    #[test]
    fn foreign_shared_dependent_crossing_boundary_is_detected() {
        // Sheet2 shared master B1 = Sheet1!A1 (relative cell part); dependent B6 therefore
        // means Sheet1!A6. Inserting at Sheet1 row 5 does NOT move the master's A1 (row 1)
        // but DOES move the dependent's A6 -> A7. Gating on the master body alone would miss
        // it, so we expand first and gate over the explicit dependents.
        let sheet = br#"<worksheet><sheetData><row r="1"><c r="B1"><f t="shared" ref="B1:B10" si="9">Sheet1!A1</f></c></row><row r="6"><c r="B6"><f t="shared" si="9"/></c></row></sheetData></worksheet>"#;
        let expanded = expand_shared_in_sheet(sheet).unwrap();
        let s = String::from_utf8_lossy(&expanded);
        assert!(
            s.contains("Sheet1!A6"),
            "expand must offset the dependent to Sheet1!A6: {s}"
        );
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        assert!(
            foreign_sheet_needs_shift(&expanded, &e),
            "the dependent Sheet1!A6 crosses the insert at row 5 and must be detected"
        );
        // The master alone (row 1) would NOT trip the gate — proving the regression path.
        let master_only = br#"<worksheet><sheetData><row r="1"><c r="B1"><f>Sheet1!A1</f></c></row></sheetData></worksheet>"#;
        assert!(!foreign_sheet_needs_shift(master_only, &e));
    }

    #[test]
    fn element_text_semantics_captures_cdata_body() {
        // REGRESSION (round-42): a CDATA-wrapped binding body (Excel emits this for legacy VML
        // form-control FmlaMacro/FmlaLink) must be extracted, not dropped to "" — else two
        // DISTINCT macro bindings collapse to the same key and a re-point certifies.
        let xml = br#"<xml xmlns:x="urn:x"><x:ClientData><x:FmlaMacro><![CDATA[EvilMacro]]></x:FmlaMacro></x:ClientData></xml>"#;
        assert_eq!(
            element_text_semantics(xml, &[b"FmlaMacro"]),
            vec!["EvilMacro".to_string()]
        );
        // Distinct macros must yield distinct keys.
        let safe =
            br#"<xml xmlns:x="urn:x"><x:FmlaMacro><![CDATA[SafeMacro]]></x:FmlaMacro></xml>"#;
        assert_ne!(
            element_text_semantics(xml, &[b"FmlaMacro"]),
            element_text_semantics(safe, &[b"FmlaMacro"])
        );
    }

    #[test]
    fn formula_hidden_tokens_are_per_cell() {
        let m = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>@A1:A10</f></c><c r="D1"><f>_xlfn.CONCAT(A1,A2)+_xlfn._xlws.FILTER(B1:B5,C1)</f></c><c r="E1"><f>SUM(A1:A9)</f></c></row></sheetData></worksheet>"#,
        );
        // `@` cell keyed by ref (signature is the `@` POSITION list); a plain formula excluded.
        assert_eq!(
            m.get("C1").map(String::as_str),
            Some("@[0];;isect=;coerce=")
        );
        assert_eq!(
            m.get("D1").map(String::as_str),
            Some("@[];_xlfn.CONCAT,_xlfn._xlws.FILTER;isect=;coerce=")
        );
        assert_eq!(m.get("E1"), None);
        assert_eq!(m.len(), 2);
        // A top-level range-INTERSECTION (round-34): ironcalc drops the 2nd operand, so the raw
        // body is signed. Two intersections differing only in the 2nd operand differ here.
        let isa = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>A1:A10 A4:A4</f></c></row></sheetData></worksheet>"#,
        );
        let isb = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>A1:A10 A7:A7</f></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(
            isa.get("C1").map(String::as_str),
            Some("@[];;isect=A1:A10 A4:A4;coerce=")
        );
        assert_ne!(
            isa.get("C1"),
            isb.get("C1"),
            "intersection operand change must differ"
        );
        // REGRESSION (round-42): a PARENTHESIZED right operand `A1:A10 (A4:A4)` is still a
        // top-level intersection ironcalc collapses to `@A1:A10`; it must be signed or a mangle of
        // the parenthesized operand's VALUE certifies (the '(' was excluded from is_operand_start).
        let pa = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>A1:A10 (A4:A4)</f></c></row></sheetData></worksheet>"#,
        );
        let pb = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>A1:A10 (A7:A7)</f></c></row></sheetData></worksheet>"#,
        );
        assert!(
            pa.contains_key("C1"),
            "parenthesized intersection must be signed: {pa:?}"
        );
        assert_ne!(
            pa.get("C1"),
            pb.get("C1"),
            "parenthesized-operand change must differ"
        );
        // REGRESSION (round-59 defect 2): the isect/coerce signature must apply the SAME
        // value-neutral canonicalizations as the cell diff, so a cosmetic re-encoding (sheet-quote
        // fold / TRUE()->TRUE) of a coercion or intersection formula is NOT over-refused.
        let coerce_quoted = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>SUMPRODUCT(--('Data'!A1:A10&gt;5))</f></c></row></sheetData></worksheet>"#,
        );
        let coerce_bare = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>SUMPRODUCT(--(Data!A1:A10&gt;5))</f></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(
            coerce_quoted.get("C1"),
            coerce_bare.get("C1"),
            "a sheet-quote fold of a coercion body must not over-refuse: {coerce_quoted:?} vs {coerce_bare:?}"
        );
        // But a genuine coercion-operand change still differs.
        let coerce_changed = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>SUMPRODUCT(--(Data!A1:A10&gt;9))</f></c></row></sheetData></worksheet>"#,
        );
        assert_ne!(coerce_bare.get("C1"), coerce_changed.get("C1"));
        // But a plain arithmetic formula with spaces around an operator is NOT an intersection.
        let arith = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f>A1 + A2</f></c></row></sheetData></worksheet>"#,
        );
        assert!(
            arith.is_empty(),
            "operator-spacing is not an intersection: {arith:?}"
        );
        // A `@` inside a `[@Col]` structured ref or a string/quoted-name is NOT counted.
        let bracket = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="C5"><f>Table1[@Amount]*"a@b"&amp;'x@y'!A1</f></c></row></sheetData></worksheet>"#,
        );
        assert!(bracket.is_empty());
        // WITHIN-cell relocation of `@` between operands (round-30): same count, DIFFERENT
        // position -> different signature.
        let a = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="E1"><f>@A1:A3-A1:A3</f></c></row></sheetData></worksheet>"#,
        );
        let b = formula_hidden_tokens(
            br#"<worksheet><sheetData><row r="1"><c r="E1"><f>A1:A3-@A1:A3</f></c></row></sheetData></worksheet>"#,
        );
        assert_ne!(
            a.get("E1"),
            b.get("E1"),
            "within-cell @ move changes signature"
        );
    }

    #[test]
    fn hidden_row_exclusion_and_hidden_rows() {
        let has = |f: &str| {
            hidden_row_exclusion_present(
                format!(r#"<worksheet><sheetData><row r="1"><c r="A1"><f>{f}</f></c></row></sheetData></worksheet>"#)
                    .as_bytes(),
            )
        };
        // SUBTOTAL 101-111 and hidden-ignoring AGGREGATE options exclude manual hidden rows.
        assert!(has("SUBTOTAL(109,A2:A11)"));
        assert!(has("_xlfn.AGGREGATE(9,5,A2:A11)"));
        assert!(has("AGGREGATE(9,7,A2:A11)"));
        // SUBTOTAL 1-11 and non-hidden-ignoring AGGREGATE options do NOT.
        assert!(!has("SUBTOTAL(9,A2:A11)"));
        assert!(!has("AGGREGATE(9,6,A2:A11)"));
        assert!(!has("SUM(A2:A11)"));
        // an unparseable code is conservative -> counts as excluding.
        assert!(has("SUBTOTAL(foo,A2:A11)"));

        // hidden_rows collects only rows flagged hidden (1/true), sorted.
        let rows = hidden_rows(br#"<worksheet><sheetData><row r="1"/><row r="6" hidden="1"><c r="A6"/></row><row r="8" hidden="true"/><row r="9" hidden="0"/></sheetData></worksheet>"#);
        assert_eq!(rows, vec!["6".to_string(), "8".to_string()]);
    }

    #[test]
    fn formula_cache_map_keeps_present_drops_empty() {
        let m = formula_cache_map(
            br#"<worksheet><sheetData><row r="1"><c r="A1" t="n"><v>1</v></c><c r="B1"><f>SUM(A1:A1)</f><v>1</v></c></row><row r="2"><c r="B2"><f>A1</f><v /></c><c r="B3"><f>A1</f></c></row></sheetData></worksheet>"#,
        );
        // B1: formula cell with a present cache -> kept (signature is `type:value`, default
        // type "n"). A1: a data cell (no <f>) -> excluded. B2: empty <v/> -> excluded (Excel
        // recomputes). B3: no <v> at all -> excluded.
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("B1").map(String::as_str), Some("n:1"));
        // A shared-formula dependent (`<f .../>` empty element) with a present cache is a
        // formula cell too.
        let s = formula_cache_map(
            br#"<worksheet><sheetData><row r="1"><c r="C1"><f t="shared" si="0"/><v>7</v></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(s.get("C1").map(String::as_str), Some("n:7"));
        // A number->text RETYPE (same <v> digits) yields a DIFFERENT signature (round-26).
        let text = formula_cache_map(
            br#"<worksheet><sheetData><row r="1"><c r="C1" t="str"><f>x</f><v>7</v></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(text.get("C1").map(String::as_str), Some("str:7"));
        // REGRESSION (round-53 defect 3): a cached STRING result containing an XML entity
        // (`&amp;`/`&lt;`/`&gt;`) must be reassembled + UNESCAPED to the engine's true value, not
        // have the entity dropped (`Sales &amp; Costs` -> `Sales  Costs`), which spuriously refused a
        // faithful cache. The signature is the unescaped `str:Sales & Costs`.
        let ent = formula_cache_map(
            br#"<worksheet><sheetData><row r="1"><c r="C1" t="str"><f>A&amp;B</f><v>Sales &amp; Costs</v></c><c r="D1" t="str"><f>x</f><v>a &lt; b</v></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(ent.get("C1").map(String::as_str), Some("str:Sales & Costs"));
        assert_eq!(ent.get("D1").map(String::as_str), Some("str:a < b"));
        // REGRESSION (round-56 defect 4): a STRING result's leading/trailing whitespace is
        // SIGNIFICANT (a `=A1&" "` label); it must NOT be trimmed (the engine oracle does not trim
        // it), else a faithful padded-string cache is refused. A numeric cache stays trimmed.
        let padded = formula_cache_map(
            br#"<worksheet><sheetData><row r="1"><c r="C1" t="str"><f>x</f><v xml:space="preserve"> x </v></c><c r="C2" t="n"><f>y</f><v> 5 </v></c></row></sheetData></worksheet>"#,
        );
        assert_eq!(padded.get("C1").map(String::as_str), Some("str: x "));
        assert_eq!(padded.get("C2").map(String::as_str), Some("n:5"));
    }

    #[test]
    fn autofilter_filter_column_colid_is_retargeted() {
        // REGRESSION (round-53 defect 4, silent-wrong): `<filterColumn colId>` is a 0-based offset
        // from the autoFilter range's first column. When a COLUMN edit moves the filtered column it
        // must be re-targeted (or dropped if its column is deleted); left stale it points the filter
        // predicate at the wrong column, changing which rows SUBTOTAL/AGGREGATE hides.
        let af = br#"<worksheet><sheetData/><autoFilter ref="A1:D13"><filterColumn colId="2"><filters><filter val="8"/></filters></filterColumn></autoFilter></worksheet>"#;
        let colid = |xml: &[u8]| -> Option<u32> {
            let s = String::from_utf8_lossy(xml);
            s.split("colId=\"")
                .nth(1)
                .and_then(|t| t.split('"').next())
                .and_then(|v| v.parse().ok())
        };
        // insert a column at 3 (the filtered column C): C moves to D, offset 2 -> 3.
        let (out, n, _) =
            shift_autofilter_columns(af, "Sheet1", &edit("Sheet1", Axis::Col, Op::Insert, 3, 1))
                .unwrap();
        assert_eq!(
            colid(&out),
            Some(3),
            "colId re-targeted after column insert"
        );
        assert_eq!(n, 1);
        // delete column B: C(3)->B(2), first col stays A -> offset 1.
        let (out, _, _) =
            shift_autofilter_columns(af, "Sheet1", &edit("Sheet1", Axis::Col, Op::Delete, 2, 1))
                .unwrap();
        assert_eq!(
            colid(&out),
            Some(1),
            "colId re-targeted after column delete"
        );
        // delete the FILTERED column C itself: the whole <filterColumn> (with its <filters>) is dropped.
        let (out, _, _) =
            shift_autofilter_columns(af, "Sheet1", &edit("Sheet1", Axis::Col, Op::Delete, 3, 1))
                .unwrap();
        assert!(
            !String::from_utf8_lossy(&out).contains("filterColumn"),
            "deleted filtered column drops the filterColumn: {}",
            String::from_utf8_lossy(&out)
        );
        assert!(
            !String::from_utf8_lossy(&out).contains("<filter "),
            "the filter criteria go with it"
        );
        // a ROW edit does not touch colId (it is a column offset) — no-op, byte-identical.
        let (out, n, _) =
            shift_autofilter_columns(af, "Sheet1", &edit("Sheet1", Axis::Row, Op::Insert, 2, 1))
                .unwrap();
        assert_eq!(
            out,
            af.to_vec(),
            "a row edit leaves the autoFilter untouched"
        );
        assert_eq!(n, 0);
    }

    #[test]
    fn prefixed_worksheet_grid_is_detected() {
        // REGRESSION (round-53 defect 8, HIGH silent-wrong): a worksheet that binds the
        // spreadsheetML namespace to a PREFIX must be detected so the edit refuses (fail-closed)
        // rather than shift cells while leaving rows stale. An ordinary (default-namespace) sheet —
        // the universal case — must NOT be flagged (no over-refusal).
        assert!(edited_sheet_has_prefixed_grid(
            br#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>"#
        ));
        assert!(!edited_sheet_has_prefixed_grid(
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#
        ));
        // A prefix on an UNRELATED element (a foreign extension) does not trip the grid detector.
        assert!(!edited_sheet_has_prefixed_grid(
            br#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><ext:extLst xmlns:ext="urn:x"><ext:ext/></ext:extLst></worksheet>"#
        ));
        // REGRESSION (round-57 defect 6): the x14 `<xm:f>` formula element (Office excel/2006/main
        // namespace, prefix `xm`) shares the local name `f` but is NOT a spreadsheetML grid element —
        // it must NOT trip a false prefixed-worksheet refusal (it appears in every sparkline / x14
        // dropdown / x14 CF rule on an ordinary unprefixed sheet).
        assert!(!edited_sheet_has_prefixed_grid(
            br#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><extLst><ext xmlns:x14="urn:x14"><x14:dataValidations xmlns:xm="urn:xm"><x14:dataValidation><x14:formula1><xm:f>Sheet2!$A$1</xm:f></x14:formula1><xm:sqref>A5</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst></worksheet>"#
        ));
    }

    #[test]
    fn row_without_r_attribute_is_detected() {
        // REGRESSION (round-59): a <row> without the OOXML-optional `r` attribute breaks the
        // r-keyed insert/delete surgery (no-op delete + duplicate cell coordinates), so it must be
        // refused up front. Rows that all carry `r` are fine.
        assert!(edited_sheet_has_row_without_r(
            br#"<worksheet><sheetData><row><c r="A1"><v>10</v></c></row><row r="2"><c r="A2"/></row></sheetData></worksheet>"#
        ));
        assert!(!edited_sheet_has_row_without_r(
            br#"<worksheet><sheetData><row r="1"><c r="A1"/></row><row r="2"><c r="A2"/></row></sheetData></worksheet>"#
        ));
        // An empty sheetData (data-free workbook) has no offending row.
        assert!(!edited_sheet_has_row_without_r(
            br#"<worksheet><sheetData/></worksheet>"#
        ));
        // End-to-end: a row-axis delete on such a sheet must produce the residual, not a corrupt file.
        let xml = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c r="A1"><v>10</v></c></row><row><c r="A2"><v>20</v></c></row><row><c r="A3"><v>30</v></c></row></sheetData></worksheet>"#;
        // (edited_sheet_has_row_without_r is what the structural_edit pre-flight consults.)
        assert!(edited_sheet_has_row_without_r(xml));
    }

    #[test]
    fn dropped_part_relationship_and_override_are_pruned() {
        // REGRESSION (round-53 defect 6, HIGH invalid-output): dropping calcChain.xml /
        // volatileDependencies.xml must also prune their workbook-rels <Relationship> and
        // [Content_Types] <Override>, else the package has a dangling relationship / orphaned
        // override (OPC-invalid — strict readers reject it, Excel flags the file for repair).
        let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId5" Type="x/calcChain" Target="calcChain.xml"/><Relationship Id="rId6" Type="x/volatileDependencies" Target="volatileDependencies.xml"/><Relationship Id="rId2" Type="x/styles" Target="styles.xml"/></Relationships>"#;
        let pruned = String::from_utf8(prune_relationships_to_dropped(rels)).unwrap();
        assert!(
            !pruned.contains("calcChain.xml"),
            "calcChain rel pruned: {pruned}"
        );
        assert!(
            !pruned.contains("volatileDependencies.xml"),
            "volatileDependencies rel pruned: {pruned}"
        );
        assert!(
            pruned.contains("worksheets/sheet1.xml"),
            "worksheet rel kept"
        );
        assert!(pruned.contains("styles.xml"), "styles rel kept");

        let ct = br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="a"/><Override PartName="/xl/calcChain.xml" ContentType="b"/><Override PartName="/xl/styles.xml" ContentType="c"/></Types>"#;
        let pruned_ct = String::from_utf8(prune_content_type_overrides_of_dropped(ct)).unwrap();
        assert!(
            !pruned_ct.contains("/xl/calcChain.xml"),
            "calcChain override pruned: {pruned_ct}"
        );
        assert!(
            pruned_ct.contains("/xl/workbook.xml"),
            "workbook override kept"
        );
        assert!(pruned_ct.contains("/xl/styles.xml"), "styles override kept");

        // A workbook with NEITHER part is not gated in (no needless re-serialization).
        assert!(!mentions_dropped_part(
            br#"<Relationships><Relationship Target="styles.xml"/></Relationships>"#
        ));
        assert!(mentions_dropped_part(rels));
    }

    #[test]
    fn insert_overflows_grid_detects_boundary() {
        // A data row at the last grid row: inserting above it would push it to 1048577 (off
        // grid) — refuse. An insert not reaching the last row is fine.
        let last_row = br#"<worksheet><sheetData><row r="1048576"><c r="A1048576"><v>5</v></c></row></sheetData></worksheet>"#;
        assert!(insert_overflows_grid(
            last_row,
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1)
        ));
        let mid_row = br#"<worksheet><sheetData><row r="10"><c r="A10"><v>5</v></c></row></sheetData></worksheet>"#;
        assert!(!insert_overflows_grid(
            mid_row,
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1)
        ));
        // REGRESSION (round-48): the INSERTED BLANK ROWS themselves must not run past the grid, even
        // with NO populated row near the boundary (the per-coordinate scan misses those blanks).
        let empty = br#"<worksheet><sheetData/></worksheet>"#;
        assert!(
            insert_overflows_grid(empty, &edit("Sheet1", Axis::Row, Op::Insert, 1048575, 3)),
            "blanks at 1048575..1048577 run off-grid"
        );
        assert!(insert_overflows_grid(
            empty,
            &edit("Sheet1", Axis::Row, Op::Insert, 2000000, 5)
        ));
        assert!(!insert_overflows_grid(
            empty,
            &edit("Sheet1", Axis::Row, Op::Insert, 5, 1)
        ));
        // Column axis: a cell at XFD pushed past column 16384 by an insert-cols.
        let xfd = br#"<worksheet><sheetData><row r="1"><c r="XFD1"><v>5</v></c></row></sheetData></worksheet>"#;
        assert!(insert_overflows_grid(
            xfd,
            &edit("Sheet1", Axis::Col, Op::Insert, 1, 1)
        ));
        // A delete never overflows.
        assert!(!insert_overflows_grid(
            last_row,
            &edit("Sheet1", Axis::Row, Op::Delete, 1, 1)
        ));
        // REGRESSION (round-57 defect 5): a MOVE that relocates a (formatted, cell-less) present row
        // past the grid edge must be refused too — the reopen gate accepts an off-grid cell-less
        // `<row r>`. Move row 20 -> dest 1048578 maps it to 1048577 (> 1048576).
        let fmt_row = br#"<worksheet><sheetData><row r="20" ht="30" customHeight="1"/></sheetData></worksheet>"#;
        assert!(insert_overflows_grid(
            fmt_row,
            &move_edit("Sheet1", 20, 1, 1048578)
        ));
        // A move that stays on the grid does not overflow.
        assert!(!insert_overflows_grid(
            fmt_row,
            &move_edit("Sheet1", 20, 1, 5)
        ));
    }

    #[test]
    fn non_ascii_qualifier_affect_check() {
        let sheet = "集計";
        // insert at row 1 moves A11 -> a reference 集計!A11 IS affected.
        let e1 = edit(sheet, Axis::Row, Op::Insert, 1, 1);
        assert!(non_ascii_qualifier_affected("集計!A11+1", sheet, &e1));
        // insert at row 50 does NOT move A11 -> NOT affected (the over-refusal fix).
        let e50 = edit(sheet, Axis::Row, Op::Insert, 50, 1);
        assert!(!non_ascii_qualifier_affected("集計!$A$11+1", sheet, &e50));
        // a reference to a DIFFERENT sheet is never affected.
        assert!(!non_ascii_qualifier_affected("Другой!A11", sheet, &e1));
        // a 3D span whose first endpoint is the edited sheet, at a moved cell -> affected.
        assert!(non_ascii_qualifier_affected(
            "SUM(集計:Sheet3!A11)",
            sheet,
            &e1
        ));
    }

    /// On an ASCII-named EDITED sheet a formula whose only non-ASCII qualifier names a
    /// DIFFERENT sheet moves nothing, so the write path must keep the body verbatim
    /// rather than presence-refuse it — but must still refuse when a co-located
    /// edited-sheet reference shifts or a non-ASCII 3D span may enclose the edited sheet.
    #[test]
    fn non_ascii_qualifier_edited_sheet_ascii_affect_based() {
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);

        // (a) OVER-REFUSAL FIX: `集計!A5` names a non-edited sheet -> verbatim, no residual.
        let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><f>集計!A5</f></c></row></sheetData></worksheet>"#;
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml.as_bytes(), &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            report.residuals.is_empty(),
            "no residual: {:?}",
            report.residuals
        );
        assert!(s.contains("<f>集計!A5</f>"), "body verbatim: {s}");

        // (b) SILENT-WRONG GUARD: `集計!A5+A5` — the bare A5 is an edited-sheet ref that the
        //     insert at row 3 shifts to A6 -> must REFUSE (never write a stale bare ref).
        let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><f>集計!A5+A5</f></c></row></sheetData></worksheet>"#;
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml.as_bytes(), &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "non_ascii_sheet_qualifier"),
            "refused: {:?}",
            report.residuals
        );
        assert!(
            s.contains("集計!A5+A5"),
            "body left verbatim on refusal: {s}"
        );

        // (c) LATENT MIS-SHIFT PREVENTED: `A1計!B5` — the ASCII `A1` prefix would tempt a naive
        //     tokenizer to shift it; the whole qualified ref names a non-edited sheet -> verbatim.
        let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><f>A1計!B5</f></c></row></sheetData></worksheet>"#;
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml.as_bytes(), &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            report.residuals.is_empty(),
            "no residual: {:?}",
            report.residuals
        );
        assert!(s.contains("<f>A1計!B5</f>"), "prefix not mis-shifted: {s}");

        // (d) NON-ASCII 3D SPAN: may enclose the edited sheet as an interior tab -> REFUSE.
        let xml = r#"<worksheet><sheetData><row r="1"><c r="B1"><f>SUM(集計:売上!A5)</f></c></row></sheetData></worksheet>"#;
        let mut report = StructuralReport::default();
        let _ = rewrite_edited_sheet(xml.as_bytes(), &e, "s", &mut report).unwrap();
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "non_ascii_sheet_qualifier"),
            "3D span refused: {:?}",
            report.residuals
        );
    }

    #[test]
    fn datatable_f_attrs_are_shifted() {
        // <f t="dataTable" ref/r1/r2> carries live coordinates in ATTRIBUTES; an insert must
        // shift them or the table reads the wrong input cell and declares the wrong extent.
        let xml = br#"<worksheet><sheetData><row r="2"><c r="C2"><f t="dataTable" ref="C2:C5" dt2D="0" dtr="0" r1="A1" ca="1"/><v>1</v></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            report.residuals.is_empty(),
            "no residual: {:?}",
            report.residuals
        );
        assert!(s.contains(r#"ref="C3:C6""#), "output extent shifted: {s}");
        assert!(s.contains(r#"r1="A2""#), "input cell shifted: {s}");

        // REGRESSION (round-56 defects 5/9): a DELETE that CONSUMES an input cell must fail closed
        // (a data table with r1="" is malformed OOXML), not silently commit an empty attribute.
        let dt = br#"<worksheet><sheetData><row r="1"><c r="A1"><v>2</v></c><c r="B1"><v>3</v></c></row><row r="2"><c r="C2"><f t="dataTable" ref="C2:C5" dt2D="0" dtr="0" r1="B1" r2="A1" ca="1"/><v>6</v></c></row></sheetData></worksheet>"#;
        let del = edit("Sheet1", Axis::Row, Op::Delete, 1, 1);
        let mut dreport = StructuralReport::default();
        let dout = rewrite_edited_sheet(dt, &del, "s", &mut dreport).unwrap();
        assert!(
            dreport
                .residuals
                .iter()
                .any(|r| r.reason == "datatable_input_consumed"),
            "consuming a data-table input cell must refuse: {:?}",
            dreport.residuals
        );
        assert!(
            !String::from_utf8_lossy(&dout).contains(r#"r1="""#),
            "never emit an empty r1"
        );
    }

    #[test]
    fn foreign_extlst_xmf_is_shifted_not_refused() {
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        // A foreign sheet's x14/sparkline extLst <xm:f> qualified to the edited sheet is NOT
        // an unshifted residual: shift_text_in_element(b"f") rewrites it (local name `f`).
        let spark = br#"<worksheet xmlns:xm="urn:xm"><sheetData/><extLst><ext><x14:sparklineGroups xmlns:x14="urn:x14"><x14:sparklineGroup><x14:sparklines><x14:sparkline><xm:f>Sheet1!A1:A10</xm:f><xm:sqref>C1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext></extLst></worksheet>"#;
        assert!(!foreign_sheet_cross_ref_unshifted(spark, &e));
        // And the shift path actually rewrites it: Sheet1!A1:A10 -> Sheet1!A2:A11.
        let (out, n, _r, _q) = shift_text_in_element(spark, b"f", &e, "Sheet2").unwrap();
        assert!(n >= 1);
        assert!(String::from_utf8_lossy(&out).contains("Sheet1!A2:A11"));
        // A LEGACY CF <formula> qualified to the edited sheet is now SHIFTED (round-51), so it is
        // NOT flagged, and the foreign shift rewrites Sheet1!A1 -> Sheet1!A2.
        let legacy = br#"<worksheet><sheetData/><conditionalFormatting><cfRule><formula>Sheet1!A1</formula></cfRule></conditionalFormatting></worksheet>"#;
        assert!(!foreign_sheet_cross_ref_unshifted(legacy, &e));
        let (lout, ln, _r, _q) = shift_text_in_element(legacy, b"formula", &e, "Sheet2").unwrap();
        assert!(ln >= 1);
        assert!(String::from_utf8_lossy(&lout).contains("Sheet1!A2"));
        // An ARRAY <f> is still not shifted -> still flagged.
        let arr = br#"<worksheet><sheetData><row r="1"><c r="C1"><f t="array" ref="C1:C1">Sheet1!A1</f></c></row></sheetData></worksheet>"#;
        assert!(foreign_sheet_cross_ref_unshifted(arr, &e));
    }

    #[test]
    fn whitespace_padded_range_colon_is_a_range() {
        // REGRESSION (round-51 defect 1): a range whose `:` is padded by NON-space whitespace
        // (newline from Excel Alt+Enter, or a tab) must be recognized as a range and delete-clamped,
        // not split into two independent cells (which left the tail at #REF!).
        let del = edit("S", Axis::Row, Op::Delete, 8, 3);
        for f in [
            "SUM(A1\n:A10)",
            "SUM(A1 :\tA10)",
            "SUM(A1\r\n:\r\nA10)",
            "SUM(A1:A10)",
        ] {
            let (out, _n) = refshift::shift_formula(f, "S", &del);
            assert!(
                out.contains("A1:A7") && !out.contains("#REF!"),
                "`{f}` must clamp to A1:A7, got `{out}`"
            );
        }
    }

    #[test]
    fn repeated_value_cell_is_detected() {
        // REGRESSION (round-51 defect 2): a `<c>` with two `<v>` children is malformed; certify
        // refuses a workbook carrying one (the engine misreads it, real readers take the last <v>).
        let bad = br#"<worksheet><sheetData><row r="1"><c r="A1" t="n"><v>0</v><v>999</v></c></row></sheetData></worksheet>"#;
        assert!(cell_has_repeated_value(bad));
        let ok = br#"<worksheet><sheetData><row r="1"><c r="A1"><f>SUM(B1:B2)</f><v>5</v></c></row></sheetData></worksheet>"#;
        assert!(!cell_has_repeated_value(ok));
    }

    #[test]
    fn range_intersection_cells_are_detected() {
        // REGRESSION (round-51 defect 7): a cell whose `<f>` has a top-level range-intersection is
        // detected from the RAW XML (the engine drops the operator on reparse).
        let xml = br#"<worksheet><sheetData><row r="1"><c r="B1"><f>SUM(A1:A10 A3:A5)</f><v>12</v></c><c r="B2"><f>SUM(A1:A10)</f><v>55</v></c></row></sheetData></worksheet>"#;
        assert_eq!(cells_with_range_intersection(xml), vec!["B1".to_string()]);
    }

    #[test]
    fn foreign_sheet_control_binding_to_edited_sheet_is_flagged() {
        // A control on a FOREIGN sheet bound to the edited sheet (linkedCell="Sheet1!$A$5")
        // must be flagged when the edit moves that cell; an unqualified binding (to the
        // control's own sheet) must not be.
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let qualified = br#"<worksheet><sheetData/><controls><control><controlPr linkedCell="Sheet1!$A$5" fmlaLink="Sheet1!$A$5"/></control></controls></worksheet>"#;
        assert!(foreign_sheet_ref_attr_crosses(qualified, &e));
        let unqualified = br#"<worksheet><sheetData/><controls><control><controlPr linkedCell="$A$5"/></control></controls></worksheet>"#;
        assert!(!foreign_sheet_ref_attr_crosses(unqualified, &e));
        // REGRESSION (round-37): the option-button-GROUP link (fmlaGroup) and edit-box link
        // (fmlaTxbx) — modern CT_FormControlPr cell references — must ALSO cross (they were
        // omitted, so restructure committed a stale group/textbox binding).
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<formControlPr fmlaGroup="Sheet1!$A$5"/>"#,
            &e
        ));
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<formControlPr fmlaTxbx="Sheet1!$A$5"/>"#,
            &e
        ));
        // REGRESSION (round-54 defect 5): a binding qualified to the edited sheet by a QUOTED,
        // space-containing sheet name (`'My Sheet'!$A$8`) must ALSO cross — split_whitespace tore it
        // into two invalid tokens, so a foreign control was left STALE. The quote-aware tokenizer
        // keeps it whole. (An unrelated quoted sheet name does not cross — no over-refusal.)
        let e2 = edit("My Sheet", Axis::Row, Op::Insert, 1, 1);
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<formControlPr fmlaLink="'My Sheet'!$A$8"/>"#,
            &e2
        ));
        assert!(!foreign_sheet_ref_attr_crosses(
            br#"<formControlPr fmlaLink="'Other Sheet'!$A$8"/>"#,
            &e2
        ));
        // And control_binding_attrs (certify's compare surface) captures them, so a re-point is
        // caught rather than false-certified.
        let g1 = control_binding_attrs(br#"<formControlPr fmlaGroup="Sheet2!$B$1"/>"#);
        let g2 = control_binding_attrs(br#"<formControlPr fmlaGroup="Sheet2!$Z$9"/>"#);
        assert!(
            !g1.is_empty() && g1 != g2,
            "fmlaGroup re-point must change the binding key"
        );
        let t1 = control_binding_attrs(br#"<formControlPr fmlaTxbx="Sheet2!$B$1"/>"#);
        assert!(!t1.is_empty(), "fmlaTxbx must be captured");
    }

    #[test]
    fn ignored_error_sqref_is_shifted_not_refused() {
        // <ignoredError sqref> (green-triangle suppression) is a benign, ubiquitous construct
        // whose coordinate the shift engine now tracks: an insert shifts it (no residual),
        // rather than refusing the whole edit as an unshiftable body reference.
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"/></row></sheetData><ignoredErrors><ignoredError sqref="A5:A9" numberStoredAsText="1"/></ignoredErrors></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            report.residuals.is_empty(),
            "no residual: {:?}",
            report.residuals
        );
        // inserting a row at 2 pushes A5:A9 down to A6:A10.
        assert!(s.contains(r#"sqref="A6:A10""#), "sqref shifted: {s}");

        // REGRESSION (round-56 defect 8): a delete consuming the ONLY <ignoredError> drops the child
        // but must NOT leave an empty <ignoredErrors></ignoredErrors> (CT_IgnoredErrors child
        // minOccurs=1 -> Excel repair prompt).
        let del_xml = br#"<worksheet><sheetData><row r="1"><c r="A1"/></row></sheetData><ignoredErrors><ignoredError sqref="A5:A6" numberStoredAsText="1"/></ignoredErrors></worksheet>"#;
        let del = edit("Sheet1", Axis::Row, Op::Delete, 5, 2);
        let mut dreport = StructuralReport::default();
        let dout = rewrite_edited_sheet(del_xml, &del, "s", &mut dreport).unwrap();
        let ds = String::from_utf8_lossy(&dout);
        assert!(
            !ds.contains("<ignoredError"),
            "the consumed ignoredError is dropped: {ds}"
        );
        assert!(
            !ds.contains("<ignoredErrors"),
            "empty ignoredErrors container omitted: {ds}"
        );

        // REGRESSION (round-55 defect 4): under a MOVE the sqref must ALSO follow its cells (it was
        // omitted from transform_tag_move and committed stale). Move row 3 -> before row 8 relocates
        // A3 to A7 (move_row_sigma(3,3,1,8)=7), so ignoredError sqref="A3" -> "A7".
        let mv_xml = br#"<worksheet><sheetData><row r="3"><c r="A3"><v>3</v></c></row><row r="8"><c r="A8"><v>8</v></c></row></sheetData><ignoredErrors><ignoredError sqref="A3" numberStoredAsText="1"/></ignoredErrors></worksheet>"#;
        let mv = move_edit("Sheet1", 3, 1, 8);
        let mut mreport = StructuralReport::default();
        let mout = rewrite_edited_sheet(mv_xml, &mv, "s", &mut mreport).unwrap();
        let ms = String::from_utf8_lossy(&mout);
        assert!(
            mreport.residuals.is_empty(),
            "no residual under move: {:?}",
            mreport.residuals
        );
        assert!(
            ms.contains(r#"sqref="A7""#) && !ms.contains(r#"sqref="A3""#),
            "ignoredError sqref follows the move: {ms}"
        );
    }

    #[test]
    fn delete_drops_a_fully_consumed_mergecell() {
        // deleting rows 5-6 fully consumes mergeCell A5:B6 -> the element is DROPPED (not
        // emitted with a malformed ref="") AND the now-empty <mergeCells>/<dataValidations>
        // container is OMITTED (schema-invalid otherwise — round-27).
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"/></row></sheetData><mergeCells count="1"><mergeCell ref="A5:B6"/></mergeCells><dataValidations count="1"><dataValidation type="whole" sqref="A5:A6"><formula1>1</formula1></dataValidation></dataValidations></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Delete, 5, 2);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(!s.contains("ref=\"\""), "no empty ref: {s}");
        assert!(!s.contains("<mergeCell "), "the merge is dropped: {s}");
        assert!(
            !s.contains("<mergeCells"),
            "empty mergeCells container omitted: {s}"
        );
        assert!(
            !s.contains("<dataValidations"),
            "empty dataValidations container omitted: {s}"
        );
        // A container that KEEPS a survivor is not omitted — AND its `count` is recomputed to the
        // surviving child count (round-55 defect 5): dropping A5:B6 leaves only A1:B1, so count 2->1.
        let keep = br#"<worksheet><sheetData/><mergeCells count="2"><mergeCell ref="A5:B6"/><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#;
        let out2 = rewrite_edited_sheet(keep, &e, "s", &mut report).unwrap();
        let s2 = String::from_utf8_lossy(&out2);
        assert!(s2.contains("<mergeCells"), "non-empty container kept");
        assert!(
            s2.contains(r#"count="1""#) && !s2.contains(r#"count="2""#),
            "count recomputed to the surviving child count: {s2}"
        );
        // A container whose children all SURVIVE keeps its count unchanged (byte-identical splice).
        let e2 = edit("Sheet1", Axis::Row, Op::Insert, 50, 1);
        let unchanged = br#"<worksheet><sheetData/><mergeCells count="2"><mergeCell ref="A1:B1"/><mergeCell ref="A3:B3"/></mergeCells></worksheet>"#;
        let out3 = rewrite_edited_sheet(unchanged, &e2, "s", &mut report).unwrap();
        assert!(
            String::from_utf8_lossy(&out3).contains(r#"count="2""#),
            "count stays correct when no child is dropped"
        );
    }

    #[test]
    fn delete_drops_a_fully_consumed_hyperlink_and_omits_empty_container() {
        // REGRESSION (round-54 defect 7, invalid-output): deleting the only cell a <hyperlink> sits
        // on drops the <hyperlink> but must NOT leave an empty <hyperlinks></hyperlinks> — that
        // violates CT_Hyperlinks (child minOccurs=1) and Excel opens it with a repair prompt.
        let xml = br#"<worksheet xmlns:r="urn:r"><sheetData><row r="1"><c r="A1"/></row></sheetData><hyperlinks><hyperlink ref="A5" r:id="rId1"/></hyperlinks></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Delete, 5, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            !s.contains("<hyperlink "),
            "the consumed hyperlink is dropped: {s}"
        );
        assert!(
            !s.contains("<hyperlinks"),
            "empty hyperlinks container omitted: {s}"
        );
        // A hyperlink on a surviving cell keeps its container.
        let keep = br#"<worksheet xmlns:r="urn:r"><sheetData/><hyperlinks><hyperlink ref="A1" r:id="rId1"/></hyperlinks></worksheet>"#;
        let out2 = rewrite_edited_sheet(keep, &e, "s", &mut report).unwrap();
        assert!(
            String::from_utf8_lossy(&out2).contains("<hyperlinks"),
            "container with a surviving hyperlink kept"
        );
    }

    #[test]
    fn defined_name_vba_flags_are_in_the_signature() {
        // REGRESSION (round-27): function/vbProcedure/hidden reclassify a name into a VBA
        // binding (a value/security change); they must be part of certify's compared signature.
        let plain = defined_names(
            br#"<workbook><definedName name="Total">Sheet1!$A$1</definedName></workbook>"#,
        );
        let vba = defined_names(
            br#"<workbook><definedName name="Total" function="1" vbProcedure="1" hidden="1">Sheet1!$A$1</definedName></workbook>"#,
        );
        assert_ne!(
            plain, vba,
            "the VBA-flag rebinding must change the signature"
        );
        // the plain name's scope is unchanged (no localSheetId, no flags).
        assert_eq!(plain[0].1, "");
    }

    #[test]
    fn delete_cols_drops_deleted_cell_content() {
        // A1=10 B1=20 C1=30 D1=40; delete cols 2-3 (B,C). B/C content must be DROPPED (not
        // left stale), and D1 shifted to B1 — no duplicate coordinates (invalid OOXML).
        let xml = br#"<worksheet><sheetData><row r="1"><c r="A1"><v>10</v></c><c r="B1"><v>20</v></c><c r="C1"><v>30</v></c><c r="D1"><v>40</v></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Col, Op::Delete, 2, 2);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        // exactly two cells survive: A1 (10) and B1 (was D1 = 40).
        let cells: Vec<&str> = s.matches("<c r=\"").map(|_| "").collect();
        assert_eq!(cells.len(), 2, "two cells must survive: {s}");
        assert!(s.contains(r#"<c r="A1"><v>10</v>"#), "A1 kept: {s}");
        assert!(
            s.contains(r#"<c r="B1"><v>40</v>"#),
            "D1(40) shifted to B1: {s}"
        );
        assert!(
            !s.contains("<v>20</v>") && !s.contains("<v>30</v>"),
            "B/C dropped: {s}"
        );
    }

    #[test]
    fn cols_shift_on_column_edits_and_empty_container_is_dropped() {
        // insert-cols shifts min/max (3 -> 4)
        let e = edit("Sheet1", Axis::Col, Op::Insert, 1, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(
            br#"<worksheet><cols><col min="3" max="3" hidden="1"/></cols><sheetData/></worksheet>"#,
            &e,
            "s",
            &mut report,
        )
        .unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"min="4""#) && s.contains(r#"max="4""#),
            "col min/max must shift 3->4: {s}"
        );
        // delete-cols removing the only col must OMIT the (now-empty, schema-invalid) <cols>
        let e = edit("Sheet1", Axis::Col, Op::Delete, 3, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(
            br#"<worksheet><cols><col min="3" max="3" hidden="1"/></cols><sheetData><row r="1"><c r="A1"/></row></sheetData></worksheet>"#,
            &e,
            "s",
            &mut report,
        )
        .unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            !s.contains("<cols"),
            "empty <cols> must be omitted entirely: {s}"
        );
        // A row-axis edit must leave <cols> untouched (columns don't move on a row edit).
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(
            br#"<worksheet><cols><col min="3" max="3" hidden="1"/></cols><sheetData/></worksheet>"#,
            &e,
            "s",
            &mut report,
        )
        .unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#"min="3""#),
            "row edit must not touch <col>: {s}"
        );
    }

    #[test]
    fn scoped_defined_name_with_unqualified_body_shifts() {
        let names = vec!["Sheet1".to_string(), "Sheet2".to_string()];
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        // A Sheet1-scoped name (localSheetId=0) with an UNqualified body resolves against
        // Sheet1, so it shifts; a global name's qualified body also shifts.
        let (out, _n, _r, _q) = shift_defined_names(
            br#"<workbook><definedNames><definedName name="L" localSheetId="0">$A$8</definedName><definedName name="G">Sheet1!$A$8</definedName></definedNames></workbook>"#,
            &e,
            &names,
        )
        .unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(">$A$9<"),
            "scoped unqualified body $A$8->$A$9: {s}"
        );
        assert!(
            s.contains("Sheet1!$A$9"),
            "global qualified body shifts: {s}"
        );
        // A name scoped to a DIFFERENT sheet must NOT shift its unqualified body.
        let (out2, _n, _r, _q) = shift_defined_names(
            br#"<workbook><definedNames><definedName name="L2" localSheetId="1">$A$8</definedName></definedNames></workbook>"#,
            &e,
            &names,
        )
        .unwrap();
        assert!(
            String::from_utf8_lossy(&out2).contains(">$A$8<"),
            "Sheet2-scoped name is unaffected by a Sheet1 edit"
        );
    }

    #[test]
    fn sheet_ref_construct_semantics_extracts_cf_dv_and_extlst() {
        // legacy CF: sqref attr + formula body (logical, entity-resolved)
        assert_eq!(
            sheet_ref_construct_semantics(br#"<worksheet><conditionalFormatting sqref="A1:A10"><cfRule><formula>$A1&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#),
            vec![("conditionalFormatting".to_string(), "sqref=A1:A10|$A1>0".to_string())]
        );
        // legacy DV: sqref + formula1
        assert_eq!(
            sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:B5"><formula1>Sheet2!$A$1:$A$3</formula1></dataValidation></worksheet>"#),
            vec![("dataValidation".to_string(), "sqref=B1:B5|Sheet2!$A$1:$A$3".to_string())]
        );
        // x14 extLst: xm:sqref + xm:f collected (sorted) under "extLst"
        assert_eq!(
            sheet_ref_construct_semantics(br#"<worksheet><extLst><ext><x14:conditionalFormatting><x14:cfRule><xm:f>$D$1&gt;0</xm:f></x14:cfRule><xm:sqref>D1:D5</xm:sqref></x14:conditionalFormatting></ext></extLst></worksheet>"#),
            vec![("extLst".to_string(), "$D$1>0|D1:D5".to_string())]
        );
        // a plain sheet (cell formulas only) yields nothing.
        assert!(sheet_ref_construct_semantics(br#"<worksheet><sheetData><row r="1"><c r="A1"><f>SUM(A1:A9)</f></c></row></sheetData></worksheet>"#).is_empty());
        // REGRESSION (round-34): for a type="list" DV, the inert `formula2` (which LibreOffice
        // writes) is IGNORED — Excel uses formula2 only with between/notBetween, inapplicable to a
        // list — so its presence/absence must NOT change the key (a spurious refusal otherwise).
        let with_f2 = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B3:B8"><formula1>"Yes,No"</formula1><formula2>0</formula2></dataValidation></worksheet>"#);
        let without = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B3:B8"><formula1>"Yes,No"</formula1></dataValidation></worksheet>"#);
        assert_eq!(
            with_f2, without,
            "list-DV inert formula2 must not affect the key"
        );
        // But for a NON-list type (whole/between), formula2 IS a real bound and is kept.
        let between = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="whole" operator="between" sqref="B3:B8"><formula1>1</formula1><formula2>10</formula2></dataValidation></worksheet>"#);
        assert!(
            between[0].1.contains("10"),
            "non-list formula2 must be kept: {between:?}"
        );
        // REGRESSION (round-46): for a SCALAR operator (greaterThan/lessThan/…), formula2 is inert
        // too (Excel uses it only for between/notBetween) — LibreOffice emits `<formula2>0</formula2>`
        // on every non-between DV, so it must not change the key.
        let gt_f2 = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="whole" operator="greaterThan" sqref="A2:A10"><formula1>0</formula1><formula2>0</formula2></dataValidation></worksheet>"#);
        let gt = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="whole" operator="greaterThan" sqref="A2:A10"><formula1>0</formula1></dataValidation></worksheet>"#);
        assert_eq!(
            gt_f2, gt,
            "scalar-operator inert formula2 must not affect the key"
        );
        // An ABSENT operator defaults to between, so formula2 there IS kept.
        let dflt = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="whole" sqref="A2:A10"><formula1>1</formula1><formula2>9</formula2></dataValidation></worksheet>"#);
        assert!(
            dflt[0].1.contains('9'),
            "default-operator formula2 must be kept: {dflt:?}"
        );
        // REGRESSION (round-35): a foreign editor coalescing ADJACENT sqref ranges over the SAME
        // cells (`B1:B11 C1:C11` -> `B1:C11`) is a lossless serialization normalization — the key
        // must be identical so a faithful edit is not spuriously refused.
        let split = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:B11 C1:C11"><formula1>"a,b"</formula1></dataValidation></worksheet>"#);
        let coalesced = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:C11"><formula1>"a,b"</formula1></dataValidation></worksheet>"#);
        assert_eq!(
            split, coalesced,
            "adjacent-range coalescing must not change the key"
        );
        // A DIFFERENT cell set still differs (the canonicalization must not over-merge).
        let bigger = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:D11"><formula1>"a,b"</formula1></dataValidation></worksheet>"#);
        assert_ne!(split, bigger, "a genuinely larger sqref must differ");
        // REGRESSION (round-38): a CF/DV formula body may carry an INERT leading `=`
        // (`=Lists!$A$1:$A$3`) that Excel/LibreOffice normalize away; a foreign editor dropping
        // it is value-identical, so its presence/absence must NOT change the key.
        let with_eq = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:B5"><formula1>=Sheet2!$A$1:$A$3</formula1></dataValidation></worksheet>"#);
        let without_eq = sheet_ref_construct_semantics(br#"<worksheet><dataValidation type="list" sqref="B1:B5"><formula1>Sheet2!$A$1:$A$3</formula1></dataValidation></worksheet>"#);
        assert_eq!(
            with_eq, without_eq,
            "inert leading = must not change the DV key"
        );
        // The same for a legacy CF expression body.
        let cf_eq = sheet_ref_construct_semantics(br#"<worksheet><conditionalFormatting sqref="A1:A10"><cfRule type="expression"><formula>=$A1&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#);
        let cf_noeq = sheet_ref_construct_semantics(br#"<worksheet><conditionalFormatting sqref="A1:A10"><cfRule type="expression"><formula>$A1&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#);
        assert_eq!(cf_eq, cf_noeq, "inert leading = must not change the CF key");
    }

    #[test]
    fn canonicalize_sheet_quotes_drops_only_redundant_quotes() {
        // REGRESSION (round-40): openpyxl writes the autofilter _FilterDatabase name QUOTED
        // (`'Data'!…`) while Excel/LibreOffice write it unquoted — a faithful edit was refused.
        assert_eq!(
            canonicalize_sheet_quotes("'Data'!$A$1:$B$10"),
            "Data!$A$1:$B$10"
        );
        assert_eq!(
            canonicalize_sheet_quotes("SUM('Data'!A1,'Sheet2'!B2)"),
            "SUM(Data!A1,Sheet2!B2)"
        );
        // A 3D span: both endpoints unquoted.
        assert_eq!(canonicalize_sheet_quotes("'S1':'S2'!A1"), "S1:S2!A1");
        // Names that NEED quotes keep them — no two distinct names may collide.
        assert_eq!(canonicalize_sheet_quotes("'My Sheet'!A1"), "'My Sheet'!A1");
        assert_eq!(canonicalize_sheet_quotes("'2020'!A1"), "'2020'!A1");
        assert_eq!(canonicalize_sheet_quotes("'O''Brien'!A1"), "'O''Brien'!A1");
        // A DOUBLE-quoted string literal containing an apostrophe is untouched; the real
        // qualifier after it is still normalized.
        assert_eq!(
            canonicalize_sheet_quotes(r#"IF(A1="it's",'Data'!B1,0)"#),
            r#"IF(A1="it's",Data!B1,0)"#
        );
        // No quotes -> identity.
        assert_eq!(canonicalize_sheet_quotes("Data!A1+B2"), "Data!A1+B2");
    }

    #[test]
    fn canonical_sqref_coalesces_adjacent_ranges() {
        // A single range is its own canonical form (existing keys unchanged).
        assert_eq!(canonical_sqref("A1:A10"), "A1:A10");
        // Two adjacent ranges whose union is a rectangle -> that rectangle.
        assert_eq!(canonical_sqref("B1:B11 C1:C11"), "B1:C11");
        assert_eq!(canonical_sqref("C1:C11 B1:B11"), "B1:C11"); // order-independent
                                                                // A non-rectangular union is stable but not merged (an L-shape).
        assert_eq!(
            canonical_sqref("A1:A2 B1:B1"),
            canonical_sqref("B1:B1 A1:A2")
        );
        assert_ne!(canonical_sqref("B1:B11 C1:C11"), canonical_sqref("B1:D11"));
    }

    #[test]
    fn sqref_tokeniser_keeps_quoted_space_names_whole() {
        // REGRESSION (round-52 defect 1): sqref/ref-shaped values were split on raw whitespace, so a
        // quoted sheet name containing a space (`'My Sheet'!$A$5`) was torn into `'My` and
        // `Sheet'!$A$5` — neither a valid ref — and `would_shift` returned false, silently committing
        // a STALE binding after an insert. A quote-aware tokeniser keeps it whole.
        assert_eq!(
            split_sqref_tokens("'My Sheet'!$A$5 'My Sheet'!$B$9"),
            vec!["'My Sheet'!$A$5", "'My Sheet'!$B$9"]
        );
        // An escaped inner quote ('') does not close the quote.
        assert_eq!(
            split_sqref_tokens("'It''s A'!$A$1 B2"),
            vec!["'It''s A'!$A$1", "B2"]
        );
        // Plain unqualified ranges are unaffected.
        assert_eq!(
            split_sqref_tokens("A1:A5 B2 C3:D4"),
            vec!["A1:A5", "B2", "C3:D4"]
        );

        // End-to-end through the shift: a `'My Sheet'!$A$5` binding on sheet "My Sheet" DOES shift
        // when a row is inserted above it (previously it stayed put).
        let e = edit("My Sheet", Axis::Row, Op::Insert, 3, 1);
        let (nv, _n, _c, _all) = shift_sqref("'My Sheet'!$A$5", "My Sheet", &e);
        assert_eq!(nv, "'My Sheet'!$A$6");
    }

    #[test]
    fn volatile_formula_cells_detects_volatile() {
        // REGRESSION (round-34): certify skips a volatile cell's cache (Excel recomputes it) rather
        // than disabling its oracle workbook-wide. The per-cell detector finds the volatile ones.
        let v = volatile_formula_cells(br#"<worksheet><sheetData><row r="1"><c r="A1"><f>TODAY()</f><v>1</v></c><c r="B1"><f>SUM(A2:A9)</f><v>2</v></c><c r="C1"><f>OFFSET(A1,1,0)</f><v>3</v></c></row></sheetData></worksheet>"#);
        assert!(v.contains("A1"), "TODAY is volatile");
        assert!(v.contains("C1"), "OFFSET is volatile");
        assert!(!v.contains("B1"), "SUM is not volatile");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn foreign_ref_attr_crosses_detects_cross_sheet_dataref() {
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        // a foreign dataConsolidate dataRef qualified to the edited sheet -> crosses
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="Sheet1!$A$1:$A$10" sheet="Sheet1"/></dataRefs></dataConsolidate></worksheet>"#,
            &e
        ));
        // REGRESSION (round-59 defect 3): the SCHEMA-CONFORMANT CT_DataRef form has a BARE `ref` and
        // names the source sheet in a separate `sheet` attribute — this must ALSO cross (it was
        // committed stale because the phantom-host oracle never saw the qualifier).
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="$A$1:$A$10" sheet="Sheet1"/></dataRefs></dataConsolidate></worksheet>"#,
            &e
        ));
        // a bare dataRef whose `sheet` is a DIFFERENT (space-named) sheet does not cross.
        let e2 = edit("My Data", Axis::Row, Op::Insert, 2, 1);
        assert!(foreign_sheet_ref_attr_crosses(
            br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="$A$1:$A$10" sheet="My Data"/></dataRefs></dataConsolidate></worksheet>"#,
            &e2
        ));
        assert!(!foreign_sheet_ref_attr_crosses(
            br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="$A$1:$A$10" sheet="Other"/></dataRefs></dataConsolidate></worksheet>"#,
            &e2
        ));
        // an UNqualified ref (local to the foreign sheet, no `sheet` attr) does NOT cross.
        assert!(!foreign_sheet_ref_attr_crosses(
            br#"<worksheet><mergeCells><mergeCell ref="A1:B2"/></mergeCells></worksheet>"#,
            &e
        ));
        // a ref qualified to a DIFFERENT sheet does not cross
        assert!(!foreign_sheet_ref_attr_crosses(
            br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="Sheet3!$A$1" sheet="Sheet3"/></dataRefs></dataConsolidate></worksheet>"#,
            &e
        ));
    }

    #[test]
    fn rewrite_pivot_fails_closed_on_edited_sheet_consolidation_source() {
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        // a consolidation rangeSet naming the edited sheet -> unhandled (refuse)
        let (_o, _n, _r, unhandled) = rewrite_pivot(
            br#"<pivotCacheDefinition><cacheSource type="consolidation"><consolidation><rangeSets><rangeSet ref="A1:C10" sheet="Sheet1"/></rangeSets></consolidation></cacheSource></pivotCacheDefinition>"#,
            &e,
        )
        .unwrap();
        assert!(
            unhandled,
            "a consolidation rangeSet on the edited sheet must be flagged"
        );
        // a consolidation rangeSet on a DIFFERENT sheet is unaffected -> not flagged
        let (_o, _n, _r, u2) = rewrite_pivot(
            br#"<pivotCacheDefinition><cacheSource type="consolidation"><consolidation><rangeSets><rangeSet ref="A1:C10" sheet="Other"/></rangeSets></consolidation></cacheSource></pivotCacheDefinition>"#,
            &e,
        )
        .unwrap();
        assert!(!u2);
    }

    #[test]
    fn shift_col_range_clamps_at_the_last_column() {
        let ins = |at, count| edit("S", Axis::Col, Op::Insert, at, count);
        // a range pushed entirely past XFD (16384) is dropped
        assert_eq!(shift_col_range(16380, 16384, &ins(1, 5)), None);
        // a range whose end overflows is clamped to 16384
        assert_eq!(
            shift_col_range(16380, 16382, &ins(1, 3)),
            Some((16383, 16384))
        );
    }

    #[test]
    fn shift_col_range_handles_insert_and_delete_clamp() {
        let ins = |at, count| edit("S", Axis::Col, Op::Insert, at, count);
        let del = |at, count| edit("S", Axis::Col, Op::Delete, at, count);
        // Insert at 3, count 1:
        assert_eq!(shift_col_range(1, 2, &ins(3, 1)), Some((1, 2))); // before -> unchanged
        assert_eq!(shift_col_range(3, 5, &ins(3, 1)), Some((4, 6))); // at/after -> +1
        assert_eq!(shift_col_range(2, 4, &ins(3, 1)), Some((2, 5))); // straddle -> extend right
                                                                     // Delete columns [3,4] (at=3, count=2):
        assert_eq!(shift_col_range(1, 2, &del(3, 2)), Some((1, 2))); // entirely before
        assert_eq!(shift_col_range(6, 8, &del(3, 2)), Some((4, 6))); // entirely after -> -2
        assert_eq!(shift_col_range(3, 4, &del(3, 2)), None); // entirely deleted -> drop
        assert_eq!(shift_col_range(1, 4, &del(3, 2)), Some((1, 2))); // right part deleted -> clamp to at-1
        assert_eq!(shift_col_range(3, 6, &del(3, 2)), Some((3, 4))); // left part deleted -> [at, max-count]
        assert_eq!(shift_col_range(1, 6, &del(3, 2)), Some((1, 4))); // spans deletion -> max-count
    }

    #[test]
    fn defined_names_is_namespace_and_entity_aware() {
        // Unprefixed still works. Tuple is (name, scope, refers-to); scope empty = global.
        assert_eq!(
            defined_names(br#"<workbook><definedNames><definedName name="A">Sheet1!$A$1</definedName></definedNames></workbook>"#),
            vec![("A".to_string(), String::new(), "Sheet1!$A$1".to_string())]
        );
        // REGRESSION: a namespace-prefixed <x:definedName> (which the shifter DOES rewrite)
        // must be seen — the old substring scan was blind to it, a false-certification hole.
        assert_eq!(
            defined_names(br#"<x:workbook xmlns:x="urn:x"><x:definedNames><x:definedName name="Anchor">Sheet1!$A$2</x:definedName></x:definedNames></x:workbook>"#),
            vec![("Anchor".to_string(), String::new(), "Sheet1!$A$2".to_string())]
        );
        // Entities in the refers-to body are resolved (so an entity-encoded target compares
        // equal to its plain spelling instead of masking a stale reference).
        assert_eq!(
            defined_names(
                br#"<workbook><definedName name="X">Sheet1&#33;$A$1</definedName></workbook>"#
            ),
            vec![("X".to_string(), String::new(), "Sheet1!$A$1".to_string())]
        );
        // localSheetId (scope) is captured, so a re-scoped name does not compare equal.
        assert_eq!(
            defined_names(br#"<workbook><definedName name="A" localSheetId="1">Sheet1!$A$1</definedName></workbook>"#),
            vec![("A".to_string(), "1".to_string(), "Sheet1!$A$1".to_string())]
        );
    }

    #[test]
    fn edited_sheet_body_unshifted_ref_is_an_affect_based_whitelist() {
        // insert at row 1 moves every row >= 1, so all the ranges below ARE affected.
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        let f = |xml: &[u8]| edited_sheet_body_unshifted_ref(xml, "Sheet1", &e);
        // Coordinate-bearing body constructs the engine copies verbatim -> flagged.
        assert_eq!(
            f(br#"<worksheet><protectedRanges><protectedRange sqref="A5:A9" name="x"/></protectedRanges></worksheet>"#).as_deref(),
            Some("protectedRange")
        );
        assert_eq!(
            f(br#"<worksheet><scenarios><scenario name="s"><inputCells r="A8" val="1"/></scenario></scenarios></worksheet>"#).as_deref(),
            Some("inputCells")
        );
        // `$`-anchored and range/qualified `r` values must not evade the cell-shape test.
        assert_eq!(
            f(br#"<worksheet><scenarios><scenario name="s"><inputCells r="$A$8" val="1"/></scenario></scenarios></worksheet>"#).as_deref(),
            Some("inputCells")
        );
        assert_eq!(
            f(br#"<worksheet><cellWatches><cellWatch r="A8:B9"/></cellWatches></worksheet>"#)
                .as_deref(),
            Some("cellWatch")
        );
        assert_eq!(
            f(br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="A5:A9"/></dataRefs></dataConsolidate></worksheet>"#).as_deref(),
            Some("dataRef")
        );
        // REGRESSION (round-60 defect 6, over-refusal): a <dataRef> whose BARE ref is scoped to a
        // sibling `sheet` naming a DIFFERENT, unaffected sheet is genuinely not moved by a Sheet1
        // edit — it must NOT be flagged. A `sheet` naming the edited sheet keeps the refusal.
        assert_eq!(
            f(br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="A5:A9" sheet="Other"/></dataRefs></dataConsolidate></worksheet>"#),
            None
        );
        assert_eq!(
            f(br#"<worksheet><dataConsolidate><dataRefs><dataRef ref="A5:A9" sheet="Sheet1"/></dataRefs></dataConsolidate></worksheet>"#).as_deref(),
            Some("dataRef")
        );
        // <ignoredError sqref> (green-triangle suppression, e.g. number-stored-as-text) is
        // now SHIFTED like other sqref-bearing elements, not refused — a benign, ubiquitous
        // construct whose coordinate the shift engine tracks. So it is NOT flagged here.
        assert_eq!(
            f(br#"<worksheet><ignoredErrors><ignoredError sqref="A5:A9" numberStoredAsText="1"/></ignoredErrors></worksheet>"#),
            None
        );
        // Form-control data binding (linkedCell/fmlaLink) is flagged.
        assert_eq!(
            f(br#"<worksheet><controls><control><controlPr linkedCell="A8" fmlaLink="A9"/></control></controls></worksheet>"#).as_deref(),
            Some("controlPr")
        );
        // REGRESSION (round-44): the modern CT_FormControlPr links fmlaGroup (option-button-group
        // result cell) and fmlaTxbx (edit-box bound cell) are ALSO flagged — they were left stale.
        assert_eq!(
            f(br#"<worksheet><controls><control><formControlPr fmlaGroup="$A$8" fmlaTxbx="$A$5"/></control></controls></worksheet>"#).as_deref(),
            Some("formControlPr")
        );
        // A list/combo-box SOURCE range (fmlaRange, sibling of fmlaLink) is flagged too.
        assert_eq!(
            f(br#"<worksheet><controls><control><controlPr fmlaRange="A1:A8"/></control></controls></worksheet>"#).as_deref(),
            Some("controlPr")
        );
        // An <oleObject link="..."> linked-cell source is flagged (guarded by would_shift).
        assert_eq!(
            f(br#"<worksheet><oleObjects><oleObject progId="X" link="Sheet1!$A$8"/></oleObjects></worksheet>"#).as_deref(),
            Some("oleObject")
        );
        // A web-publish source range (sourceRef, ST_Ref in a non-ref/sqref/r attr) is flagged.
        assert_eq!(
            f(br#"<worksheet><webPublishItems><webPublishItem sourceType="range" sourceRef="A8:A10"/></webPublishItems></worksheet>"#).as_deref(),
            Some("webPublishItem")
        );
        // A namespace-prefixed CF the engine does NOT recognize is flagged.
        assert_eq!(
            f(br#"<worksheet><x:conditionalFormatting xmlns:x="urn:x" sqref="A5:A9"/></worksheet>"#).as_deref(),
            Some("conditionalFormatting")
        );

        // AFFECT-based: the SAME protectedRange is NOT flagged when the edit is far away
        // (insert at row 50 does not move A5:A9) — no spurious over-refusal.
        let far = edit("Sheet1", Axis::Row, Op::Insert, 50, 1);
        assert!(edited_sheet_body_unshifted_ref(br#"<worksheet><protectedRanges><protectedRange sqref="A5:A9" name="x"/></protectedRanges></worksheet>"#, "Sheet1", &far).is_none());

        // Handled / benign constructs must NOT be flagged.
        // sortState/sortCondition are shifted (in has_ref_attr).
        assert!(f(br#"<worksheet><autoFilter ref="A1:A9"><sortState ref="A2:A9"><sortCondition ref="A2:A9"/></sortState></autoFilter></worksheet>"#).is_none());
        // Cells/rows (row/cell path), formula bodies (formula path):
        assert!(f(br#"<worksheet><sheetData><row r="1"><c r="A1"><f>SUM(A1:A9)</f><v>1</v></c></row></sheetData></worksheet>"#).is_none());
        // has_ref_attr elements whose ref/sqref IS shifted:
        assert!(f(br#"<worksheet><mergeCells><mergeCell ref="A1:B2"/></mergeCells><conditionalFormatting sqref="C1:C9"><cfRule/></conditionalFormatting><dataValidations><dataValidation sqref="D1:D9"/></dataValidations><autoFilter ref="A1:D9"/><dimension ref="A1:D20"/></worksheet>"#).is_none());
        // Pure view-state carries no ref/sqref/r:
        assert!(f(br#"<worksheet><sheetViews><sheetView topLeftCell="B2"><selection activeCell="C3" sqref="C3"/><pane topLeftCell="B2"/></sheetView></sheetViews></worksheet>"#).is_none());
    }

    #[test]
    fn hyperlink_location_is_shifted_not_refused() {
        // A hyperlink's `location` (its in-workbook destination) is SHIFTED by the edit — a
        // TOC/index link into a moved cell follows it — rather than refusing the edit.
        let hl = |loc: &str| {
            format!(
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><hyperlinks><hyperlink ref="A1" location="{loc}"/></hyperlinks></worksheet>"#
            )
        };
        let loc_of = |out: &[u8]| {
            let s = String::from_utf8_lossy(out).into_owned();
            let i = s.find("location=\"").unwrap() + "location=\"".len();
            s[i..i + s[i..].find('"').unwrap()].to_string()
        };
        let e = edit("Sheet1", Axis::Row, Op::Insert, 1, 1);
        // On the edited sheet (host=Sheet1): an unqualified local location shifts A11 -> A12.
        let (out, n, _r) = shift_hyperlink_locations(hl("A11").as_bytes(), "Sheet1", &e).unwrap();
        assert_eq!(n, 1);
        assert_eq!(loc_of(&out), "A12");
        // A location qualified to the edited sheet shifts, from ANY host sheet (host=Notes).
        let (out, n, _r) =
            shift_hyperlink_locations(hl("Sheet1!A11").as_bytes(), "Notes", &e).unwrap();
        assert_eq!(n, 1);
        assert_eq!(loc_of(&out), "Sheet1!A12");
        // A location targeting a DIFFERENT sheet is untouched.
        let (out, n, _r) =
            shift_hyperlink_locations(hl("Other!A11").as_bytes(), "Sheet1", &e).unwrap();
        assert_eq!(n, 0);
        assert_eq!(loc_of(&out), "Other!A11");
        // An unqualified location on a NON-edited (foreign) sheet is local -> not shifted.
        let (_out, n, _r) = shift_hyperlink_locations(hl("A11").as_bytes(), "Notes", &e).unwrap();
        assert_eq!(n, 0);
        // A link to a cell ABOVE an insert at row 5 does not move -> no shift.
        let e5 = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (_out, n, _r) =
            shift_hyperlink_locations(hl("Sheet1!A1").as_bytes(), "Notes", &e5).unwrap();
        assert_eq!(n, 0);
        // An external (URL) hyperlink has no `location` -> untouched.
        let (_out, n, _r) = shift_hyperlink_locations(
            br#"<worksheet><hyperlinks><hyperlink ref="A1" r:id="rId1"/></hyperlinks></worksheet>"#,
            "Sheet1",
            &e,
        )
        .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn table_formula_crossing_edited_sheet_is_refused_affect_based() {
        let e = |sh: &str, at: u32| edit(sh, Axis::Row, Op::Insert, at, 1);
        // A calculatedColumnFormula referencing the EDITED sheet at a MOVED cell -> refuse.
        let cc = br#"<table ref="A1:B2"><tableColumns><tableColumn name="x"><calculatedColumnFormula>Sheet1!A5*2</calculatedColumnFormula></tableColumn></tableColumns></table>"#;
        assert!(table_formula_crosses_edited(cc, &e("Sheet1", 1)));
        // ...but NOT when the edit is on a DIFFERENT sheet (Sheet1!A5 unaffected).
        assert!(!table_formula_crosses_edited(cc, &e("Other", 1)));
        // ...nor when the edit moves nothing it references (insert below A5).
        assert!(!table_formula_crosses_edited(cc, &e("Sheet1", 50)));
        // A purely STRUCTURED calculated column / totals row is table-local -> allowed (the
        // round-25 over-refusal fix), even on the edited sheet.
        assert!(!table_formula_crosses_edited(
            br#"<table ref="A1:C5"><tableColumns><tableColumn name="Total"><calculatedColumnFormula>Cat[[#This Row],[Price]]*Cat[[#This Row],[Qty]]</calculatedColumnFormula></tableColumn></tableColumns></table>"#,
            &e("Sheet1", 1)
        ));
        assert!(!table_formula_crosses_edited(
            br#"<table ref="A1:B2"><tableColumn name="t"><totalsRowFormula>SUBTOTAL(109,T[Amount])</totalsRowFormula></tableColumn></table>"#,
            &e("Sheet1", 1)
        ));
        // A NAMESPACE-PREFIXED crossing formula is caught (matched by local name).
        assert!(table_formula_crosses_edited(
            br#"<x:table xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:tableColumn name="x"><x:calculatedColumnFormula>Sheet2!A2*2</x:calculatedColumnFormula></x:tableColumn></x:table>"#,
            &e("Sheet2", 1)
        ));
        // A plain table (no own formula) -> allowed.
        assert!(!table_formula_crosses_edited(
            br#"<table ref="A1:D4" name="Table1"><autoFilter ref="A1:D4"/><tableColumns count="1"><tableColumn id="1" name="Cars"/></tableColumns></table>"#,
            &e("Sheet1", 1)
        ));
        // An unparseable table part fails CLOSED.
        assert!(table_formula_crosses_edited(
            b"<table <<< not xml",
            &e("Sheet1", 1)
        ));
    }

    #[test]
    fn namespace_prefixed_tableparts_are_detected() {
        // REGRESSION (adversarial review): sheet_declares_tables used a substring scan
        // that <x:tableParts> evaded, letting a table on the EDITED sheet be missed.
        let plain = sd_worksheet(r#"<tableParts count="1"><tablePart r:id="rId1"/></tableParts>"#);
        let prefixed = sd_worksheet(
            r#"<x:tableParts xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1"><x:tablePart r:id="rId1"/></x:tableParts>"#,
        );
        assert!(xml_has_local_element(
            plain.as_bytes(),
            &[b"tableParts"],
            true
        ));
        assert!(xml_has_local_element(
            prefixed.as_bytes(),
            &[b"tableParts"],
            true
        ));
        assert!(!xml_has_local_element(
            sd_worksheet("").as_bytes(),
            &[b"tableParts"],
            true
        ));
    }

    #[test]
    fn structured_reference_in_a_formula_is_detected() {
        // AFFECT-based (round-44): the `[…]` specifier can only be mangled if σ REWRITES the
        // formula. A structured-ref formula the edit does not move is copied verbatim and is safe.
        let names = vec!["Table1".to_string(), "Sales".to_string()];
        let moves = edit("Sheet1", Axis::Row, Op::Insert, 1, 1); // shifts every row >= 1
        let below = edit("Sheet1", Axis::Row, Op::Insert, 5000, 1); // moves nothing referenced
                                                                    // SILENT-WRONG PROTECTION: a CELL-REF-SHAPED specifier (`Table1[Q4]`, Q4 looks like an A1
                                                                    // ref) IS mangled by σ (verified: -> `Table1[Q5]`) when the edit shifts that area -> the
                                                                    // change is detected -> refuse.
        assert!(part_uses_structured_ref(
            br#"<x><c><f>SUM(Table1[Q4])</f></c></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        // case-insensitive (Excel normalizes table-name case)
        assert!(part_uses_structured_ref(
            br#"<x><f>SUM(table1[Q4])</f></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        // char-ref evasion: `[` written as &#91; (a GeneralRef) must still be resolved
        assert!(part_uses_structured_ref(
            br#"<x><f>SUM(Table1&#91;Q4])</f></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        // A co-located REGULAR ref the edit shifts also forces a σ rewrite -> refuse (conservative).
        assert!(part_uses_structured_ref(
            br#"<x><f>SUM(Table1[Amount])+A1</f></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        // OVER-REFUSAL FIX: the ubiquitous `=SUM(Table[Col])` idiom, with an edit far below that
        // moves nothing, is NO LONGER refused (σ leaves it byte-identical).
        assert!(!part_uses_structured_ref(
            br#"<x><c><f>SUM(Table1[Amount])</f></c></x>"#,
            &names,
            "Sheet1",
            &below
        ));
        // a formula with no structured ref, or referencing an unknown name, is fine
        assert!(!part_uses_structured_ref(
            br#"<x><f>SUM(A1:A9)</f></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        assert!(!part_uses_structured_ref(
            br#"<x><f>Other[Col]</f></x>"#,
            &names,
            "Sheet1",
            &moves
        ));
        // no tables in the workbook => nothing to scan
        assert!(!part_uses_structured_ref(
            br#"<x><f>Table1[Q4]</f></x>"#,
            &[],
            "Sheet1",
            &moves
        ));
    }

    #[test]
    fn prefixed_relationship_element_still_resolves_a_table() {
        // REGRESSION: table_targets_in_rels parsed by splitting on "<Relationship",
        // which a prefixed <pr:Relationship> evaded. Namespace-aware now.
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("xlq-rels-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // Build a tiny zip with just the rels part we want to read.
        let zip_path = dir.join("wb.xlsx");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("xl/worksheets/_rels/sheet1.xml.rels", opts)
                .unwrap();
            z.write_all(br#"<?xml version="1.0"?><pr:Relationships xmlns:pr="http://schemas.openxmlformats.org/package/2006/relationships"><pr:Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></pr:Relationships>"#).unwrap();
            z.finish().unwrap();
        }
        let bytes = std::fs::read(&zip_path).unwrap();
        assert_eq!(
            tables_attached_to(&bytes, "xl/worksheets/sheet1.xml"),
            vec!["xl/tables/table1.xml".to_string()],
            "a prefixed <pr:Relationship> table rel must still resolve"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rel_target_resolution_handles_parent_segments() {
        assert_eq!(
            resolve_rel_target("xl/worksheets", "../tables/table1.xml").unwrap(),
            "xl/tables/table1.xml"
        );
        assert_eq!(
            resolve_rel_target("xl/worksheets", "/xl/tables/t.xml").unwrap(),
            "xl/tables/t.xml"
        );
        assert_eq!(
            resolve_rel_target("xl", "tables/t.xml").unwrap(),
            "xl/tables/t.xml"
        );
    }

    #[test]
    fn sheet_declares_tables_is_the_authoritative_signal() {
        // Fail-open guard: the scan must not depend on the .rels part being readable.
        // The edited sheet's OWN xml (<tableParts>) is authoritative, so a crafted file
        // that drops/corrupts sheetN.xml.rels cannot hide its table from the scan.
        let tbl = std::fs::read(format!("{FIX}table.xlsx")).unwrap();
        assert!(
            sheet_declares_tables(&tbl, "xl/worksheets/sheet1.xml"),
            "table.xlsx sheet1 declares <tableParts>"
        );
        let pivot = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/t1/pivot-chart.xlsx"
        ))
        .unwrap();
        assert!(
            !sheet_declares_tables(&pivot, "xl/worksheets/sheet1.xml"),
            "pivot Sheet1 owns no table"
        );
        assert!(
            sheet_declares_tables(&pivot, "xl/worksheets/sheet5.xml"),
            "pivot sheet5 owns the table"
        );
        // An unreadable/absent sheet part fails CLOSED (assume tables).
        assert!(sheet_declares_tables(&pivot, "xl/worksheets/nope.xml"));
    }

    #[test]
    fn tables_attached_to_finds_only_the_owning_sheets_tables() {
        let input = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/t1/pivot-chart.xlsx"
        ))
        .unwrap();
        // The table lives on sheet5, not sheet1.
        assert!(tables_attached_to(&input, "xl/worksheets/sheet1.xml").is_empty());
        assert_eq!(
            tables_attached_to(&input, "xl/worksheets/sheet5.xml"),
            vec!["xl/tables/table1.xml".to_string()]
        );
    }

    #[test]
    fn detects_cdata_wrapped_formula_body() {
        // REGRESSION: a CDATA-wrapped formula body arrives as Event::CData, is not
        // reassembled by the shift path, and would otherwise commit UNSHIFTED with
        // no residual (silent-wrong). It must be detected so the edit is refused
        // up front (same residual->refuse wiring as table_part_forces_residual).
        assert!(has_cdata_formula_body(
            br#"<worksheet><sheetData><row r="5"><c r="A5"><f><![CDATA[SUM(A1:A5)]]></f></c></row></sheetData></worksheet>"#
        ));
        assert!(has_cdata_formula_body(
            br#"<x><formula><![CDATA[$A$1>0]]></formula></x>"#
        ));
        assert!(has_cdata_formula_body(
            br#"<workbook><definedName name="n"><![CDATA[Sheet1!$A$1]]></definedName></workbook>"#
        ));
        // Plain (escaped) formula bodies and CDATA OUTSIDE a formula are fine.
        assert!(!has_cdata_formula_body(br#"<x><f>SUM(A1:A5)</f></x>"#));
        assert!(!has_cdata_formula_body(br#"<x><f>IF(A1&gt;0,1,2)</f></x>"#));
        assert!(!has_cdata_formula_body(
            br#"<x><is><t><![CDATA[a literal string]]></t></is></x>"#
        ));
    }

    #[test]
    fn three_d_span_affect_and_order_aware() {
        use crate::refshift::has_unverifiable_3d_span as u3d;
        let order: Vec<String> = ["Sheet1", "Sheet2", "Sheet3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // insert at row 1 moves A5; insert at row 50 does not.
        let ins1 = |sh: &str| edit(sh, Axis::Row, Op::Insert, 1, 1);
        let ins50 = |sh: &str| edit(sh, Axis::Row, Op::Insert, 50, 1);
        // Interior tab (Sheet2), endpoint tabs (Sheet1/Sheet3): all WITHIN the span and A5
        // moves -> unverifiable.
        assert!(u3d("=SUM(Sheet1:Sheet3!A5)", &order, &ins1("Sheet2")));
        assert!(u3d("=SUM(Sheet1:Sheet3!A5)", &order, &ins1("Sheet1")));
        assert!(u3d("=SUM(Sheet1:Sheet3!A5)", &order, &ins1("Sheet3")));
        // OUTSIDE the span (edit a sheet after Sheet2 for a Sheet1:Sheet2 span) -> safe
        // (the round-24 over-refusal fix).
        assert!(!u3d("=SUM(Sheet1:Sheet2!A1)", &order, &ins1("Sheet3")));
        // WITHIN the span but the edit moves NOTHING the span references (A5 above a row-50
        // insert) -> safe.
        assert!(!u3d("=SUM(Sheet1:Sheet3!A5)", &order, &ins50("Sheet1")));
        // A SELF-span is a normal reference -> safe.
        assert!(!u3d("=SUM(Sheet1:Sheet1!A5)", &order, &ins1("Sheet1")));
        assert!(!u3d("=A5+B10", &order, &ins1("Sheet2")));
        // A string literal with a colon-bang must not false-positive.
        assert!(!u3d(
            r#"=IF(A1,"Sheet1:Sheet3!x","")"#,
            &order,
            &ins1("Sheet2")
        ));
        // A PARTIAL/mixed-quoted span must be recognized (round-24 silent-wrong).
        assert!(u3d("=SUM(Sheet1:'Sheet2'!A1)", &order, &ins1("Sheet1")));
        assert!(u3d("=SUM('Sheet1':Sheet2!A1)", &order, &ins1("Sheet2")));
        // A quoted span with special-char endpoint names (round-12): order includes them.
        let qorder: Vec<String> = ["A-Sheet", "Mid", "B-Sheet"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(u3d("=SUM('A-Sheet:B-Sheet'!A5)", &qorder, &ins1("Mid")));
        assert!(u3d("=SUM('A-Sheet:B-Sheet'!A5)", &qorder, &ins1("A-Sheet")));
    }

    #[test]
    fn shared_formula_expands_to_explicit() {
        // master B2 body A2 (relative, same row); dependents B3 (A3), B4 (A4)
        let xml = br#"<worksheet><sheetData><row r="2"><c r="B2"><f t="shared" ref="B2:B4" si="0">A2*2</f></c></row><row r="3"><c r="B3"><f t="shared" si="0"/></c></row><row r="4"><c r="B4"><f t="shared" si="0"/></c></row></sheetData></worksheet>"#;
        let out = expand_shared_in_sheet(xml).unwrap();
        let s = String::from_utf8_lossy(&out);
        // master keeps its body as a plain <f>; dependents get explicit offsets
        assert!(s.contains("<f>A2*2</f>"), "master expanded: {s}");
        assert!(s.contains("<f>A3*2</f>"), "B3 dependent expanded: {s}");
        assert!(s.contains("<f>A4*2</f>"), "B4 dependent expanded: {s}");
        assert!(!s.contains("t=\"shared\""), "no shared stubs remain: {s}");
    }

    #[test]
    fn shared_formula_no_longer_refused_end_to_end() {
        // a workbook whose only formulas are shared must now be SAFELY editable
        // (expanded + shifted), not refused.
        let input = std::fs::read(format!("{FIX}shared.xlsx")).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);
        let (out, report) = structural_edit(&input, &e).unwrap();
        assert!(
            report.residuals.is_empty(),
            "shared formulas should be expanded, not refused: {:?}",
            report.residuals
        );
        // and the output recomputes in the engine
        let p = unique_tmp("shared-e2e");
        std::fs::write(&p, &out).unwrap();
        let mut m = ironcalc::import::load_from_xlsx(&p, "en", "UTC", "en").unwrap();
        m.evaluate();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn datatable_signature_includes_input_cells() {
        // REGRESSION (round-44): a what-if data table's r1/r2 INPUT cells determine the whole
        // tabulated result column; certify must compare them so a re-point (r1=A2->A9) differs.
        let dt = |r1: &str| {
            format!(
                r#"<worksheet><sheetData><row r="3"><c r="E3"><f t="dataTable" ref="E3:E5" dt2D="0" dtr="0" r1="{r1}" ca="1"/><v>10</v></c></row></sheetData></worksheet>"#
            )
        };
        let a = array_formula_cells(dt("A2").as_bytes());
        let b = array_formula_cells(dt("A9").as_bytes());
        assert!(
            a.get("E3").is_some_and(|s| s.contains("r1=A2")),
            "r1 signed: {a:?}"
        );
        assert_ne!(
            a.get("E3"),
            b.get("E3"),
            "a re-pointed input cell must differ"
        );
        // An identical table keys identically (no over-refusal).
        assert_eq!(a, array_formula_cells(dt("A2").as_bytes()));
    }

    #[test]
    fn array_formula_still_refused() {
        // arrays are NOT expanded (Excel forbids splitting) — must still refuse.
        let xml = br#"<worksheet><sheetData><row r="2"><c r="B2"><f t="array" ref="B2:B10">A2:A10*2</f></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let mut report = StructuralReport::default();
        let _ = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "array_formula_present"),
            "array must still be refused: {:?}",
            report.residuals
        );
    }

    #[test]
    fn array_formula_is_affect_based() {
        // REGRESSION (round-34): an array/dynamic-array formula is refused only when the edit MOVES
        // its extent or a cell its body references — NOT on mere presence (which rejected every
        // FILTER/UNIQUE/SORT workbook, since Excel persists all spills as <f t="array" ref=...>).
        let xml = br#"<worksheet><sheetData><row r="3"><c r="C3"><f t="array" ref="C3:C3">A1*2</f><v>2</v></c></row></sheetData></worksheet>"#;
        // Insert far BELOW (row 100): moves neither C3:C3 nor A1 -> commits, array byte-preserved.
        let mut r1 = StructuralReport::default();
        let out = rewrite_edited_sheet(
            xml,
            &edit("Sheet1", Axis::Row, Op::Insert, 100, 1),
            "s",
            &mut r1,
        )
        .unwrap();
        assert!(
            r1.residuals.is_empty(),
            "unaffected array must commit: {:?}",
            r1.residuals
        );
        assert!(
            String::from_utf8_lossy(&out).contains(r#"<f t="array" ref="C3:C3">A1*2</f>"#),
            "unaffected array preserved verbatim"
        );
        // Insert-cols far right (col 100): unaffected -> commits.
        let mut r2 = StructuralReport::default();
        let _ = rewrite_edited_sheet(
            xml,
            &edit("Sheet1", Axis::Col, Op::Insert, 100, 1),
            "s",
            &mut r2,
        )
        .unwrap();
        assert!(
            r2.residuals.is_empty(),
            "col-far array must commit: {:?}",
            r2.residuals
        );
        // Insert ABOVE (row 2): moves C3 and its ref extent -> refused.
        let mut r3 = StructuralReport::default();
        let _ = rewrite_edited_sheet(
            xml,
            &edit("Sheet1", Axis::Row, Op::Insert, 2, 1),
            "s",
            &mut r3,
        )
        .unwrap();
        assert!(
            r3.residuals
                .iter()
                .any(|r| r.reason == "array_formula_present"),
            "array whose extent the edit moves must refuse: {:?}",
            r3.residuals
        );
        // Insert at row 1: moves the body ref A1 (and the extent) -> refused.
        let mut r4 = StructuralReport::default();
        let _ = rewrite_edited_sheet(
            xml,
            &edit("Sheet1", Axis::Row, Op::Insert, 1, 1),
            "s",
            &mut r4,
        )
        .unwrap();
        assert!(
            r4.residuals
                .iter()
                .any(|r| r.reason == "array_formula_present"),
            "array whose body ref the edit moves must refuse: {:?}",
            r4.residuals
        );
    }

    #[test]
    fn array_formula_non_ascii_qualifier_is_affect_based() {
        // REGRESSION (round-56 defect 6): an array body referencing a NON-ASCII-named sheet must NOT
        // be refused on an ASCII edited sheet when the edit moves nothing it references — the
        // non-ASCII qualifier necessarily names a DIFFERENT sheet the edit cannot move.
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1); // Sheet1 (ASCII), D1 at row 1 unmoved
        assert!(
            !array_formula_affected("D1:D1", "SUM(集計!A1:A10)", "Sheet1", &e),
            "array to a non-ASCII sheet on an ASCII edited sheet, ref unmoved -> NOT affected"
        );
        // But if the edit WOULD move an edited-sheet ref in the same body, it is still affected.
        assert!(
            array_formula_affected("D1:D1", "SUM(集計!A1:A10)+A5", "Sheet1", &e),
            "an edited-sheet ref the insert moves (A5) -> affected"
        );
        // And on a NON-ASCII edited sheet the qualifier could be a self-reference -> conservative.
        let e2 = edit("集計", Axis::Row, Op::Insert, 5, 1);
        assert!(
            array_formula_affected("D1:D1", "SUM(集計!A1:A10)", "集計", &e2),
            "non-ASCII edited sheet -> conservative refuse"
        );
    }

    /// Collect row `r` numbers in the ORDER they appear in the serialized sheet.
    fn row_order(xml: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = xml;
        while let Some(p) = rest.find("<row ") {
            rest = &rest[p + 5..];
            if let Some(rp) = rest.find("r=\"") {
                let after = &rest[rp + 3..];
                if let Some(end) = after.find('"') {
                    if let Ok(n) = after[..end].parse::<u32>() {
                        out.push(n);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn move_rows_reorders_relabels_and_shifts_formula() {
        // rows 1..8; a value in A6 and a formula =A6*2 at C6. Move row 6 (a=6,n=1)
        // to before row 3 (dest=3, move up). σ: 6→3, 3→4, 4→5, 5→6, others fixed.
        let xml = br#"<worksheet><dimension ref="A1:C8"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><row r="3"><c r="A3"><v>3</v></c></row><row r="4"><c r="A4"><v>4</v></c></row><row r="5"><c r="A5"><v>5</v></c></row><row r="6"><c r="A6"><v>6</v></c><c r="C6"><f>A6*2</f><v>12</v></c></row><row r="7"><c r="A7"><v>7</v></c></row><row r="8"><c r="A8"><v>8</v></c></row></sheetData></worksheet>"#;
        let e = move_edit("Sheet1", 6, 1, 3);
        let mut report = StructuralReport::default();
        let out = rewrite_edited_sheet(xml, &e, "xl/worksheets/sheet1.xml", &mut report).unwrap();
        let s = String::from_utf8_lossy(&out);
        // physical rows are re-emitted ASCENDING by new row number
        assert_eq!(
            row_order(&s),
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            "rows ascending: {s}"
        );
        // old row 6 (value 6, formula) landed at row 3, cells relabeled, ref followed
        assert!(
            s.contains(r#"<row r="3"><c r="A3"><v>6</v></c><c r="C3"><f>A3*2</f>"#),
            "moved row content + shifted ref: {s}"
        );
        // old row 3 (value 3) shifted down into row 4
        assert!(
            s.contains(r#"<row r="4"><c r="A4"><v>3</v></c></row>"#),
            "gap row shifted: {s}"
        );
        // old row 5 (value 5) → row 6
        assert!(
            s.contains(r#"<row r="6"><c r="A6"><v>5</v></c></row>"#),
            "gap row shifted: {s}"
        );
        // dimension (extent) is invariant under a permutation — left byte-identical
        assert!(
            s.contains(r#"<dimension ref="A1:C8"/>"#),
            "dimension unchanged: {s}"
        );
        assert!(
            report.residuals.is_empty(),
            "no residuals: {:?}",
            report.residuals
        );
        assert_eq!(report.ref_errors, 0, "no #REF! for a clean move");
    }

    #[test]
    fn move_rows_straddling_range_forces_residual() {
        // a range SUM(A4:A6) reorders under moving row 6 before row 3
        // (σ(4)=5, σ(6)=3 → 5>3): must be refused as move_straddles_range.
        let xml = br#"<worksheet><sheetData><row r="3"><c r="A3"><v>3</v></c></row><row r="6"><c r="A6"><f>SUM(A4:A6)</f></c></row></sheetData></worksheet>"#;
        let e = move_edit("Sheet1", 6, 1, 3);
        let mut report = StructuralReport::default();
        let _ = rewrite_edited_sheet(xml, &e, "xl/worksheets/sheet1.xml", &mut report).unwrap();
        assert!(
            report
                .residuals
                .iter()
                .any(|r| r.reason == "move_straddles_range"),
            "straddle must be refused: {:?}",
            report.residuals
        );
    }

    #[test]
    fn preexisting_ref_error_not_counted_by_aux_shift_helpers() {
        // REGRESSION (round-40): a dangling #REF! already present in a defined name / chart /
        // cross-sheet formula (a common leftover from an earlier column or name deletion) must
        // NOT count toward ref_errors. The raw count did, so a move-rows on any such workbook
        // spuriously tripped the move_straddles_range net even though the move touched nothing.
        let e = move_edit("Sheet1", 5, 1, 2);
        // (a) defined names: a Broken=#REF!+1 name the move never touches.
        let wbxml = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><definedNames><definedName name="Broken">#REF!+1</definedName><definedName name="Total">Sheet1!$A$100</definedName></definedNames></workbook>"#;
        let (_o, _s, errs, _q) = shift_defined_names(wbxml, &e, &["Sheet1".to_string()]).unwrap();
        assert_eq!(
            errs, 0,
            "pre-existing #REF! in a defined name must not count"
        );
        // (b) foreign element body (chart series / cross-sheet <f>): same rule.
        let el = br#"<chartSpace><f>#REF!+Sheet1!A100</f></chartSpace>"#;
        let (_o, _s, errs, _q) = shift_text_in_element(el, b"f", &e, "Sheet2").unwrap();
        assert_eq!(
            errs, 0,
            "pre-existing #REF! in a foreign <f> must not count"
        );
        // NON-VACUITY: an edit that GENUINELY breaks a reference still counts (a delete that
        // consumes the target). Delete the row A100 sits on -> the ref becomes a NEW #REF!.
        let del = edit("Sheet1", Axis::Row, Op::Delete, 100, 1);
        let clean = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><definedNames><definedName name="Total">Sheet1!$A$100</definedName></definedNames></workbook>"#;
        let (_o, _s, errs, _q) = shift_defined_names(clean, &del, &["Sheet1".to_string()]).unwrap();
        assert_eq!(errs, 1, "a newly-broken reference must still count");
    }

    #[test]
    fn move_cols_rejected_defensively() {
        // Move is row-only; a column Move must error rather than mis-transform.
        let input = std::fs::read(format!("{FIX}refs.xlsx")).unwrap();
        let e = StructuralEdit {
            axis: Axis::Col,
            at: 2,
            count: 1,
            op: Op::Move,
            sheet: "Sheet1".into(),
            dest: 5,
        };
        assert!(
            structural_edit(&input, &e).is_err(),
            "col move must be rejected"
        );
    }
}
