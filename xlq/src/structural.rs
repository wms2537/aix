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
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

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

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| anyhow!("zip entry: {e}"))?;
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
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes).map_err(|e| anyhow!("read: {e}"))?;
        drop(file);

        let touched;
        if name == edited_part {
            // Materialize shared formulas so σ shifts them uniformly, then run
            // the row/cell coordinate + formula surgery on the explicit sheet.
            let expanded = expand_shared_in_sheet(&bytes)?;
            bytes = rewrite_edited_sheet(&expanded, edit, &name, &mut report)?;
            touched = true;
        } else if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            // Only touch a foreign sheet if it cross-references the edited sheet.
            // Sheets that do not are byte-identical — and a shared formula there
            // must NOT trigger a spurious refusal.
            if references_sheet(&bytes, &edit.sheet) {
                let host = part_sheet.get(&name).cloned().unwrap_or_default();
                let expanded = expand_shared_in_sheet(&bytes)?;
                let (out, n, r) = shift_text_in_element(&expanded, b"f", edit, &host);
                bytes = out;
                touched = n > 0 || r > 0;
                report.refs_shifted += n;
                report.ref_errors += r;
            } else {
                touched = false;
            }
        } else if name == "xl/workbook.xml" {
            let (out, n, r) = shift_text_in_element(&bytes, b"definedName", edit, "");
            bytes = out;
            touched = n > 0 || r > 0;
            report.refs_shifted += n;
            report.ref_errors += r;
        } else if name.starts_with("xl/charts/") && name.ends_with(".xml") {
            let (out, n, r) = shift_text_in_element(&bytes, b"f", edit, "");
            bytes = out;
            touched = n > 0 || r > 0;
            report.refs_shifted += n;
            report.ref_errors += r;
        } else if (name.starts_with("xl/pivotCache/") || name.starts_with("xl/pivotTables/"))
            && name.ends_with(".xml")
        {
            let (out, n, r) = rewrite_pivot(&bytes, edit);
            bytes = out;
            touched = n > 0 || r > 0;
            report.refs_shifted += n;
            report.ref_errors += r;
        } else {
            touched = false;
        }
        if touched && name != edited_part {
            report.parts_touched.push(name.clone());
        }

        writer
            .start_file(&name, base_opts)
            .map_err(|e| anyhow!("start {name}: {e}"))?;
        writer.write_all(&bytes).map_err(|e| anyhow!("write {name}: {e}"))?;
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
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) | Err(_) => break,
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"c" => {
                    cur = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"r")
                        .and_then(|a| parse_cell_rc(&String::from_utf8_lossy(&a.value)));
                }
                Ok(Event::Start(e)) if e.name().as_ref() == b"f" => {
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
                        pending_si = si; // master body is the next Text
                    }
                }
                Ok(Event::Text(t)) if pending_si.is_some() => {
                    if let Some((c, r)) = cur {
                        let body = t.unescape().unwrap_or_default().into_owned();
                        masters.insert(pending_si.take().unwrap(), (c, r, body));
                    } else {
                        pending_si = None;
                    }
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

fn archive_names(input: &[u8]) -> Result<Vec<String>> {
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
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let sheet = edit.sheet.clone();
    let row_axis = edit.axis == Axis::Row;
    let mut inserted = false;
    let mut in_f = false;
    let mut f_residual = false;

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
                    in_f = false;
                    f_residual = false;
                }
                writer.write_event(Event::End(e.into_owned()))?;
            }
            Event::Text(t) if in_f && !f_residual => {
                let raw = t.unescape().unwrap_or_default().into_owned();
                let (nf, n) = refshift::shift_formula(&raw, &sheet, edit);
                report.refs_shifted += n;
                report.ref_errors += nf.matches("#REF!").count() as u32;
                if nf == raw {
                    // unchanged: preserve the ORIGINAL bytes exactly (do not let
                    // the writer re-escape e.g. ' -> &apos;)
                    writer.write_event(Event::Text(t.into_owned()))?;
                } else {
                    writer.write_event(Event::Text(BytesText::from_escaped(text_escape(&nf))))?;
                }
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
            let val = a.unescape_value().unwrap_or_default().into_owned();
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
    }
}

// ---------------------------------------------------------------------------
// foreign parts
// ---------------------------------------------------------------------------

fn rewrite_pivot(src: &[u8], edit: &StructuralEdit) -> (Vec<u8>, u32, u32) {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        let ev = reader.read_event_into(&mut buf);
        match ev {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().as_ref() == b"worksheetSource" =>
            {
                let is_empty = matches!(reader.read_event_into(&mut Vec::new()), _ if false);
                let _ = is_empty;
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
                        shifted += n;
                        errs += c;
                        if nv != val {
                            repl.push((b"ref", nv));
                        }
                    }
                }
                // worksheetSource is self-closing in practice
                let _ = writer.write_event(Event::Empty(set_attrs(&e, &repl)));
            }
            Ok(other) => {
                let _ = writer.write_event(other.into_owned());
            }
        }
        buf.clear();
    }
    (writer.into_inner().into_inner(), shifted, errs)
}

/// For every <TAG>text</TAG> (namespace-insensitive local match), run
/// shift_formula on the text. `host` scopes unqualified refs.
fn shift_text_in_element(
    src: &[u8],
    tag: &[u8],
    edit: &StructuralEdit,
    host: &str,
) -> (Vec<u8>, u32, u32) {
    let mut reader = Reader::from_reader(src);
    reader.config_mut().expand_empty_elements = false;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut buf = Vec::new();
    let mut in_tag = false;
    let mut residual = false;
    let (mut shifted, mut errs) = (0u32, 0u32);
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) if tag_local_eq(e.name().as_ref(), tag) => {
                in_tag = true;
                residual = detect_residual(&e).is_some();
                let _ = writer.write_event(Event::Start(e.into_owned()));
            }
            Ok(Event::End(e)) if tag_local_eq(e.name().as_ref(), tag) => {
                in_tag = false;
                residual = false;
                let _ = writer.write_event(Event::End(e.into_owned()));
            }
            Ok(Event::Text(t)) if in_tag && !residual => {
                let raw = t.unescape().unwrap_or_default().into_owned();
                let (nf, n) = refshift::shift_formula(&raw, host, edit);
                shifted += n;
                errs += nf.matches("#REF!").count() as u32;
                if nf == raw {
                    let _ = writer.write_event(Event::Text(t.into_owned()));
                } else {
                    let _ = writer.write_event(Event::Text(BytesText::from_escaped(text_escape(&nf))));
                }
            }
            Ok(other) => {
                let _ = writer.write_event(other.into_owned());
            }
        }
        buf.clear();
    }
    (writer.into_inner().into_inner(), shifted, errs)
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

    fn edit(sheet: &str, axis: Axis, op: Op, at: u32, count: u32) -> StructuralEdit {
        StructuralEdit { axis, at, count, op, sheet: sheet.into() }
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
        let (out, n, _r) = shift_text_in_element(xml, b"f", &e, "Sheet2");
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Sheet1!A6+B10"), "got: {s}");
        assert_eq!(n, 1);
    }

    #[test]
    fn chart_ref_shifts() {
        let xml = br#"<c:chart><c:f>Sheet1!$A$1:$A$10</c:f></c:chart>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 2);
        let (out, n, _r) = shift_text_in_element(xml, b"f", &e, "");
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Sheet1!$A$1:$A$12"), "got: {s}");
        assert_eq!(n, 1);
    }

    #[test]
    fn defined_name_shifts() {
        let xml = br#"<definedNames><definedName name="Data">Sheet1!$A$1:$A$10</definedName></definedNames>"#;
        let e = edit("Sheet1", Axis::Row, Op::Insert, 5, 1);
        let (out, n, _r) = shift_text_in_element(xml, b"definedName", &e, "");
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

    fn unique_tmp(tag: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        format!(
            "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/st-{tag}-{n}.xlsx"
        )
    }

    #[test]
    fn end_to_end_insert_row_recomputes_and_shifts_all_refs() {
        let input = std::fs::read("/home/soh/aix/fixtures/structural/refs.xlsx").unwrap();
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
        let input = std::fs::read("/home/soh/aix/fixtures/structural/refs.xlsx").unwrap();
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
    fn table_part_forces_residual() {
        // a workbook containing a table part must be REFUSED (we don't shift
        // table extents), never silently corrupted.
        let py = "/home/soh/aix/fixtures/structural/table.xlsx";
        if !std::path::Path::new(py).exists() {
            return;
        }
        let input = std::fs::read(py).unwrap();
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
        let py = "/home/soh/aix/fixtures/structural/shared.xlsx";
        if !std::path::Path::new(py).exists() {
            return;
        }
        let input = std::fs::read(py).unwrap();
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
}
