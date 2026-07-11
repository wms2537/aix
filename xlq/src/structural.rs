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
use std::collections::BTreeMap;
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
    scan_extra_residuals(&archive_names(input)?, input, edit, &mut report);

    // One decompression budget across the whole workbook. The declared
    // uncompressed size is attacker-controlled; read_entry_capped bounds BOTH the
    // reservation (defeats the over-allocation attack) AND the actual decompressed
    // length (defeats the real bomb — the old .min(8<<20) clamped only the former,
    // so read_to_end still expanded the whole entry unbounded).
    let mut budget = crate::ooxml::total_cap();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| anyhow!("zip entry: {e}"))?;
        let name = file.name().to_string();
        if name == "xl/calcChain.xml" {
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
        let before = if name == edited_part { Vec::new() } else { bytes.clone() };
        if name == edited_part {
            // Materialize shared formulas so σ shifts them uniformly, then run
            // the row/cell coordinate + formula surgery on the explicit sheet.
            let expanded = expand_shared_in_sheet(&bytes)?;
            bytes = rewrite_edited_sheet(&expanded, edit, &name, &mut report)?;
        } else if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            // Only touch a foreign sheet if it cross-references the edited sheet.
            // Sheets that do not are byte-identical — and a shared formula there
            // must NOT trigger a spurious refusal.
            if references_sheet(&bytes, &edit.sheet) {
                let host = part_sheet.get(&name).cloned().unwrap_or_default();
                let expanded = expand_shared_in_sheet(&bytes)?;
                let (out, n, r, qrisk) = shift_text_in_element(&expanded, b"f", edit, &host)?;
                bytes = out;
                report.refs_shifted += n;
                report.ref_errors += r;
                if qrisk {
                    report.residuals.push(Residual {
                        part: name.clone(),
                        reason: "non_ascii_sheet_qualifier".into(),
                        detail: "unquoted non-ASCII sheet qualifier in a cross-sheet formula \
                                 — edit refused (fail-closed)".into(),
                    });
                }
            }
        } else if name == "xl/workbook.xml" {
            let (out, n, r, qrisk) = shift_text_in_element(&bytes, b"definedName", edit, "")?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
            if qrisk {
                report.residuals.push(Residual {
                    part: name.clone(),
                    reason: "non_ascii_sheet_qualifier".into(),
                    detail: "unquoted non-ASCII sheet qualifier in a defined name — edit \
                             refused (fail-closed)".into(),
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
                             refused (fail-closed)".into(),
                });
            }
        } else if (name.starts_with("xl/pivotCache/") || name.starts_with("xl/pivotTables/"))
            && name.ends_with(".xml")
        {
            let (out, n, r) = rewrite_pivot(&bytes, edit)?;
            bytes = out;
            report.refs_shifted += n;
            report.ref_errors += r;
        }
        let touched = name != edited_part && bytes != before;
        if touched {
            report.parts_touched.push(name.clone());
        }

        writer
            .start_file(&name, base_opts)
            .map_err(|e| anyhow!("start {name}: {e}"))?;
        writer.write_all(&bytes).map_err(|e| anyhow!("write {name}: {e}"))?;
    }
    // Move straddle safety net: under Move a #REF! can ONLY arise from a range
    // that reorders across the move boundary (σ is a total bijection on single
    // cells, so no single cell errors). Any ref error therefore means a straddle
    // the coordinate shift cannot express — refuse (fail-closed) even if it lived
    // in a cross-sheet formula / chart / defined name / pivot the per-sheet scan
    // above did not already flag.
    if edit.op == Op::Move
        && report.ref_errors > 0
        && !report.residuals.iter().any(|r| r.reason == "move_straddles_range")
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

/// Materialize shared-formula groups into explicit per-cell formulas so σ can
/// shift them uniformly (what Excel/LibreOffice do around a structural edit).
/// Pass 1 collects each master's (position, body) by shared index; pass 2
/// rewrites the master to a plain `<f>` and every dependent stub to its explicit
/// formula (master body translated by the dependent's offset). Array formulas
/// are NOT expanded (Excel forbids splitting them) — they remain and are refused
/// upstream. Returns the input unchanged if the sheet has no shared formulas.
fn expand_shared_in_sheet(src: &[u8]) -> Result<Vec<u8>> {
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
            match reader.read_event_into(&mut buf).map_err(|e| anyhow!("shared-formula xml: {e}"))? {
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

/// Does this part reference `sheet` by a qualified reference (`Sheet!` or
/// `'Sheet'!`)? Used to decide whether a foreign sheet needs σ at all.
fn references_sheet(bytes: &[u8], sheet: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains(&format!("{}!", sheet)) || text.contains(&format!("'{}'!", sheet))
}

fn cell_pos(e: &BytesStart) -> Option<(u32, u32)> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"r")
        .and_then(|a| parse_cell_rc(&String::from_utf8_lossy(&a.value)))
}
fn is_shared_f(e: &BytesStart) -> bool {
    e.attributes()
        .flatten()
        .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"shared")
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
fn scan_extra_residuals(names: &[String], input: &[u8], edit: &StructuralEdit, report: &mut StructuralReport) {
    for n in names {
        if n.starts_with("xl/tables/") && n.ends_with(".xml") {
            report.residuals.push(Residual {
                part: n.clone(),
                reason: "table_unsupported".into(),
                detail: "structured table extent/refs are not shifted; edit refused".into(),
            });
        }
    }
    // DEFINED-NAME ALIASING: a defined name spelled like a grid-valid cell (e.g.
    // `FY2021` = col FY, row 2021) is indistinguishable from a reference to the
    // shift tokenizer, so a formula using it would be silently mis-shifted AND the
    // resulting file would still equal xlq's own (wrong) transform — the one place
    // certified⇒correct could be false on a real workbook. Decidable from the
    // names table the file already carries: detect it and REFUSE (fail closed).
    if let Ok(bytes) = crate::ooxml::read_part(input, "xl/workbook.xml") {
        let text = String::from_utf8_lossy(&bytes);
        for name in defined_name_names(&text) {
            if refshift::looks_like_cell_ref(&name) {
                report.residuals.push(Residual {
                    part: "xl/workbook.xml".into(),
                    reason: "defined_name_ref_collision".into(),
                    detail: format!(
                        "defined name '{}' is spelled like a cell reference; its uses \
                         cannot be safely distinguished from cell refs — edit refused",
                        name
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
            let text = String::from_utf8_lossy(&bytes);
            if refshift::has_unverifiable_3d_span(&text, &edit.sheet) {
                report.residuals.push(Residual {
                    part: n.clone(),
                    reason: "threeD_span_unverifiable".into(),
                    detail: "a 3D span not anchored on the edited sheet may cover it as an interior tab".into(),
                });
            }
        }
    }
}

/// Extract the `name` attribute of every `<definedName ...>` in workbook.xml.
fn defined_name_names(workbook_xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = workbook_xml;
    while let Some(p) = rest.find("<definedName") {
        rest = &rest[p..];
        let tag_end = match rest.find('>') {
            Some(e) => e,
            None => break,
        };
        let tag = &rest[..tag_end];
        if let Some(np) = tag.find("name=") {
            let after = &tag[np + 5..];
            if let Some(q) = after.chars().next() {
                if q == '"' || q == '\'' {
                    if let Some(end) = after[1..].find(q) {
                        out.push(after[1..1 + end].to_string());
                    }
                }
            }
        }
        rest = &rest[tag_end..];
    }
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
    while i + key.len() + 1 <= s.len() {
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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// Minimal XML text-content escaping — only the characters that MUST be escaped
/// in element text (`&`, `<`, `>`). Crucially leaves `'` and `"` literal, so a
/// shifted formula like `'Data'!$A$6` keeps its apostrophes exactly as Excel
/// wrote them (quick-xml's default writer would emit `&apos;`, breaking the
/// minimal-patch invariant on sheet-qualified references).
fn text_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
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
    quick_xml::escape::unescape(raw).ok().map(|c| c.into_owned())
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
    let mut inserted = false;
    let mut in_f = false;
    let mut f_residual = false;
    // Reassembled formula body across quick-xml Text + GeneralRef events; the
    // shift/writeback happens once, at the closing </f> (see push_text_raw).
    let mut f_raw = String::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,

            Event::Start(e) if row_axis && e.name().as_ref() == b"row" => {
                if maybe_inject(&mut writer, &e, edit, &mut inserted, report)? {}
                if delete_skip(&e, edit) {
                    reader.read_to_end(e.name())?;
                    report.rows_deleted = edit.count;
                    buf.clear();
                    continue;
                }
                writer.write_event(Event::Start(shift_row_tag(&e, edit)))?;
            }
            Event::Empty(e) if row_axis && e.name().as_ref() == b"row" => {
                if maybe_inject(&mut writer, &e, edit, &mut inserted, report)? {}
                if delete_skip(&e, edit) {
                    report.rows_deleted = edit.count;
                    buf.clear();
                    continue;
                }
                writer.write_event(Event::Empty(shift_row_tag(&e, edit)))?;
            }

            Event::Start(e) => {
                if is_formula_tag(e.name().as_ref()) {
                    in_f = true;
                    f_residual = detect_residual(&e).is_some();
                    f_raw.clear();
                    if f_residual {
                        report.residuals.push(Residual {
                            part: part_name.into(),
                            reason: detect_residual(&e).unwrap().into(),
                            detail: "shared/array formula present; refused (sound over-approximation)".into(),
                        });
                    }
                }
                writer.write_event(Event::Start(transform_tag(&e, &sheet, edit, report)))?;
            }
            Event::Empty(e) => {
                if e.name().as_ref() == b"f" {
                    if let Some(reason) = detect_residual(&e) {
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
                    if in_f && !f_residual {
                        // The whole <f> body has now been reassembled; shift once.
                        let raw = std::mem::take(&mut f_raw);
                        match logical_formula(&raw) {
                            Some(logical)
                                if !refshift::has_unquoted_non_ascii_qualifier(&logical) =>
                            {
                                let (nf, n) = refshift::shift_formula(&logical, &sheet, edit);
                                report.refs_shifted += n;
                                report.ref_errors += nf.matches("#REF!").count() as u32;
                                if nf == logical {
                                    // unchanged: preserve the ORIGINAL bytes exactly
                                    // (do not let the writer re-escape e.g. ' -> &apos;)
                                    writer.write_event(Event::Text(BytesText::from_escaped(raw)))?;
                                } else {
                                    writer.write_event(Event::Text(BytesText::from_escaped(
                                        text_escape(&nf),
                                    )))?;
                                }
                            }
                            Some(_) => {
                                // FAIL-CLOSED: unquoted non-ASCII sheet qualifiers are
                                // outside the tokenizer's ASCII grammar — refuse rather
                                // than mis-shift, and write the body back verbatim.
                                report.residuals.push(Residual {
                                    part: part_name.to_string(),
                                    reason: "non_ascii_sheet_qualifier".into(),
                                    detail: "a formula carries an UNQUOTED non-ASCII sheet qualifier, \
                                             which the reference tokenizer cannot parse — edit refused \
                                             (fail-closed)".into(),
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
    Ok(out)
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
                    main.get_mut().write_all(bytes).map_err(|e| anyhow!("flush row: {e}"))?;
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
                        Some(logical)
                            if !refshift::has_unquoted_non_ascii_qualifier(&logical) =>
                        {
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
                        Some(_) => {
                            // FAIL-CLOSED: same non-ASCII-qualifier guard as insert/delete.
                            report.residuals.push(Residual {
                                part: part_name.to_string(),
                                reason: "non_ascii_sheet_qualifier".into(),
                                detail: "a formula carries an UNQUOTED non-ASCII sheet qualifier, \
                                         which the reference tokenizer cannot parse — edit refused \
                                         (fail-closed)".into(),
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

/// Non-row attribute transform for `Op::Move`. Cells and CONTENT-FOLLOWING
/// references (mergeCell/hyperlink ref, conditional-formatting/data-validation
/// sqref) relocate with their rows via σ. `dimension`/`autoFilter` describe an
/// EXTENT (invariant under an intra-sheet permutation) and view state
/// (selection/pane/brk) is non-semantic — both are left byte-identical, so the
/// move never spuriously shrinks a used-range or #REF!s a page break.
fn transform_tag_move(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    match e.name().as_ref() {
        b"c" => shift_cell_tag(e, edit),
        b"mergeCell" | b"hyperlink" | b"conditionalFormatting" | b"dataValidation" => {
            shift_ref_attrs(e, sheet, edit, report)
        }
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
    match e.name().as_ref() {
        b"c" => shift_cell_tag(e, edit),
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
        && attr_u32(e, b"r").map_or(false, |r| r >= edit.at && r < edit.at + edit.count)
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
            | b"brk"
    )
}

fn shift_ref_attrs(
    e: &BytesStart,
    sheet: &str,
    edit: &StructuralEdit,
    report: &mut StructuralReport,
) -> BytesStart<'static> {
    let name = e.name().as_ref().to_vec();
    let ref_attrs: &[&[u8]] = match name.as_slice() {
        b"mergeCell" | b"hyperlink" | b"dimension" | b"autoFilter" => &[b"ref"],
        b"conditionalFormatting" | b"dataValidation" | b"selection" => &[b"sqref"],
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
fn shift_sqref(value: &str, sheet: &str, edit: &StructuralEdit) -> (String, u32, u32, bool) {
    let mut parts = Vec::new();
    let (mut shifted, mut consumed) = (0u32, 0u32);
    let total = value.split_whitespace().count();
    for r in value.split_whitespace() {
        match refshift::shift_ref(r, sheet, edit) {
            Shift::Unchanged => parts.push(r.to_string()),
            Shift::Shifted(ns) => {
                shifted += 1;
                parts.push(ns);
            }
            Shift::Ref => consumed += 1,
        }
    }
    (parts.join(" "), shifted, consumed, parts.is_empty() && total > 0)
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

fn attr_u32(e: &BytesStart, key: &[u8]) -> Option<u32> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
}

fn shift_line(pos: u32, edit: &StructuralEdit) -> Option<u32> {
    match edit.op {
        Op::Insert => Some(if pos >= edit.at { pos + edit.count } else { pos }),
        Op::Delete => {
            if pos < edit.at {
                Some(pos)
            } else if pos >= edit.at + edit.count {
                Some(pos - edit.count)
            } else {
                None
            }
        }
        Op::Move => Some(refshift::move_row_sigma(pos, edit.at, edit.count, edit.dest)),
    }
}

// ---------------------------------------------------------------------------
// foreign parts
// ---------------------------------------------------------------------------

fn rewrite_pivot(src: &[u8], edit: &StructuralEdit) -> Result<(Vec<u8>, u32, u32)> {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    // Shift the `ref`/`sheet` attributes of a <worksheetSource> in place. Emitted
    // in the SAME event shape it arrived in (Empty stays self-closing; Start stays
    // a Start whose children the loop copies through) — the previous code read and
    // discarded the following event and forced an Empty, silently dropping a
    // sibling element and unbalancing the pivot XML.
    let shift_source = |e: &BytesStart,
                        shifted: &mut u32,
                        errs: &mut u32|
     -> Vec<(&'static [u8], String)> {
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
        let ev = reader.read_event_into(&mut buf).map_err(|e| anyhow!("pivot xml: {e}"))?;
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
                writer
                    .write_event(other.into_owned())
                    .map_err(|e| anyhow!("pivot write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((writer.into_inner().into_inner(), shifted, errs))
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
        let ev = reader.read_event_into(&mut buf).map_err(|e| anyhow!("xml: {e}"))?;
        match ev {
            Event::Eof => break,
            Event::Start(e) if tag_local_eq(e.name().as_ref(), tag) => {
                in_tag = true;
                residual = detect_residual(&e).is_some();
                f_raw.clear();
                writer.write_event(Event::Start(e.into_owned())).map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::End(e) if tag_local_eq(e.name().as_ref(), tag) => {
                if in_tag && !residual {
                    // Whole element body reassembled across Text + GeneralRef; shift once.
                    let raw = std::mem::take(&mut f_raw);
                    let out_ev = match logical_formula(&raw) {
                        Some(logical)
                            if !refshift::has_unquoted_non_ascii_qualifier(&logical) =>
                        {
                            let (nf, n) = refshift::shift_formula(&logical, host, edit);
                            shifted += n;
                            errs += nf.matches("#REF!").count() as u32;
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
                    writer.write_event(out_ev).map_err(|e| anyhow!("xml write: {e}"))?;
                }
                in_tag = false;
                residual = false;
                writer.write_event(Event::End(e.into_owned())).map_err(|e| anyhow!("xml write: {e}"))?;
            }
            Event::Text(t) if in_tag && !residual => {
                push_text_raw(&mut f_raw, &t);
            }
            Event::GeneralRef(r) if in_tag && !residual => {
                push_ref_raw(&mut f_raw, &r);
            }
            other => {
                writer.write_event(other.into_owned()).map_err(|e| anyhow!("xml write: {e}"))?;
            }
        }
        buf.clear();
    }
    Ok((writer.into_inner().into_inner(), shifted, errs, qualifier_risk))
}

fn tag_local_eq(name: &[u8], local: &[u8]) -> bool {
    let n = match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    };
    n == local
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refshift::{Axis, Op, StructuralEdit};
    use std::io::Read; // File::read_to_end/read_to_string in fixture helpers below

    fn edit(sheet: &str, axis: Axis, op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit { axis, at, count, op, sheet: sheet.into(), dest: 0 }
    }
    fn move_edit(sheet: &str, at: u32, count: u32, dest: u32) -> StructuralEdit {
        StructuralEdit { axis: Axis::Row, at, count, op: Op::Move, sheet: sheet.into(), dest }
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
        let (out, n, r) = rewrite_pivot(xml, &e).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("<cacheFields"), "following sibling dropped: {s}");
        assert!(s.contains(r#"ref="A1:B6""#), "ref not shifted: {s}");
        assert_eq!((n, r), (1, 0)); // one range shifted, no #REF!
        // and the output is well-formed (round-trips through the reader)
        let mut rd = Reader::from_reader(out.as_slice());
        let mut b = Vec::new();
        loop {
            match rd.read_event_into(&mut b).expect("malformed pivot XML produced") {
                Event::Eof => break,
                _ => {}
            }
        }
    }

    #[test]
    fn real_pivot_workbook_stays_wellformed_after_structural_edit() {
        // End-to-end regression on the committed pivot+chart fixture: a structural
        // edit must leave every pivot/chart part WELL-FORMED (the event-swallow bug
        // produced unbalanced XML) and the whole workbook must reload.
        const PIVOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/t1/pivot-chart.xlsx");
        let input = std::fs::read(PIVOT).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 2, 1);
        let (out, _report) = structural_edit(&input, &e).unwrap();
        // every pivot/chart part in the output parses as well-formed XML
        let mut z = zip::ZipArchive::new(Cursor::new(out.as_slice())).unwrap();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let name = f.name().to_string();
            if name.starts_with("xl/pivotCache") || name.starts_with("xl/pivotTables")
                || name.starts_with("xl/charts/")
            {
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                let mut rd = Reader::from_reader(b.as_slice());
                let mut buf = Vec::new();
                loop {
                    match rd.read_event_into(&mut buf)
                        .unwrap_or_else(|err| panic!("{name} is not well-formed: {err}"))
                    {
                        Event::Eof => break,
                        _ => {}
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
        let (out, _n, _r) = rewrite_pivot(xml, &e).unwrap();
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
        assert!(s.contains(r#"<c r="A5">"#) && s.contains(r#"<v>7</v>"#), "A6 -> A5: {s}");
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
        assert_eq!(m.get_formatted_cell_value(0, 12, 1).unwrap(), "55", "SUM recompute");
        // A13 (=A5*2) moved to A14, A5 shifted to A6 (value 5) => 10
        assert_eq!(m.get_formatted_cell_value(0, 14, 1).unwrap(), "10", "A5*2 recompute");
        // Sheet2!B1 = Sheet1!A11 -> Sheet1!A12 = 55
        assert_eq!(m.get_formatted_cell_value(1, 1, 2).unwrap(), "55", "cross-sheet recompute");
        std::fs::remove_file(&path).ok();

        // formula-shift correctness in the output XML
        let sheet1 = read_zip_part(&out, "xl/worksheets/sheet1.xml");
        assert!(sheet1.contains("SUM(A1:A11)"), "SUM grew: {}", &sheet1[..sheet1.len().min(400)]);
        assert!(sheet1.contains("A6*2"), "A5*2 -> A6*2");
        assert!(sheet1.contains("$A$9"), "$A$8 -> $A$9");
        assert!(sheet1.contains(r#"<row r="5"/>"#), "blank row injected");
        let sheet2 = read_zip_part(&out, "xl/worksheets/sheet2.xml");
        assert!(sheet2.contains("Sheet1!A12"), "cross-sheet ref shifted");
        assert!(sheet2.contains("Sheet2!A1+5"), "self-sheet ref unchanged");
        let wb = read_zip_part(&out, "xl/workbook.xml");
        assert!(wb.contains("Sheet1!$A$12"), "defined name shifted: {wb}");

        assert!(report.residuals.is_empty(), "no residuals expected");
        assert!(report.refs_shifted >= 4, "shifted {} refs", report.refs_shifted);
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
        for p in ["xl/styles.xml", "xl/theme/theme1.xml", "xl/sharedStrings.xml"] {
            if let (Some(b), Some(a)) = (before.get(p), after.get(p)) {
                assert_eq!(b, a, "part {p} must be byte-identical");
            }
        }
        // calcChain is dropped (rebuildable), never present in output
        assert!(!after.contains_key("xl/calcChain.xml"), "calcChain dropped");
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
        assert!(report.residuals.is_empty(), "no residuals: {:?}", report.residuals);
    }

    #[test]
    fn table_part_forces_residual() {
        // a workbook containing a table part must be REFUSED (we don't shift
        // table extents), never silently corrupted.
        let input = std::fs::read(format!("{FIX}table.xlsx")).unwrap();
        let e = edit("Sheet1", Axis::Row, Op::Insert, 3, 1);
        let (_out, report) = structural_edit(&input, &e).unwrap();
        assert!(
            report.residuals.iter().any(|r| r.reason == "table_unsupported"),
            "table must force a residual"
        );
    }

    #[test]
    fn threeD_interior_span_forces_residual() {
        assert!(crate::refshift::has_unverifiable_3d_span("=SUM(Sheet1:Sheet3!A5)", "Sheet2"));
        assert!(!crate::refshift::has_unverifiable_3d_span("=SUM(Sheet1:Sheet3!A5)", "Sheet1"));
        assert!(!crate::refshift::has_unverifiable_3d_span("=A5+B10", "Sheet2"));
        // string literal with a colon-bang must not false-positive
        assert!(!crate::refshift::has_unverifiable_3d_span(r#"=IF(A1,"Sheet1:Sheet3!x","")"#, "Sheet2"));
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
    fn array_formula_still_refused() {
        // arrays are NOT expanded (Excel forbids splitting) — must still refuse.
        let xml = br#"<worksheet><sheetData><row r="2"><c r="B2"><f t="array" ref="B2:B10">A2:A10*2</f></c></row></sheetData></worksheet>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let mut report = StructuralReport::default();
        let _ = rewrite_edited_sheet(xml, &e, "s", &mut report).unwrap();
        assert!(
            report.residuals.iter().any(|r| r.reason == "array_formula_present"),
            "array must still be refused: {:?}",
            report.residuals
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
        assert_eq!(row_order(&s), vec![1, 2, 3, 4, 5, 6, 7, 8], "rows ascending: {s}");
        // old row 6 (value 6, formula) landed at row 3, cells relabeled, ref followed
        assert!(s.contains(r#"<row r="3"><c r="A3"><v>6</v></c><c r="C3"><f>A3*2</f>"#),
            "moved row content + shifted ref: {s}");
        // old row 3 (value 3) shifted down into row 4
        assert!(s.contains(r#"<row r="4"><c r="A4"><v>3</v></c></row>"#), "gap row shifted: {s}");
        // old row 5 (value 5) → row 6
        assert!(s.contains(r#"<row r="6"><c r="A6"><v>5</v></c></row>"#), "gap row shifted: {s}");
        // dimension (extent) is invariant under a permutation — left byte-identical
        assert!(s.contains(r#"<dimension ref="A1:C8"/>"#), "dimension unchanged: {s}");
        assert!(report.residuals.is_empty(), "no residuals: {:?}", report.residuals);
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
            report.residuals.iter().any(|r| r.reason == "move_straddles_range"),
            "straddle must be refused: {:?}",
            report.residuals
        );
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
        assert!(structural_edit(&input, &e).is_err(), "col move must be rejected");
    }
}
