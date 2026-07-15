//! Surgical OOXML editing — the core of the fidelity guarantee.
//!
//! CONTRACT:
//!   /// One cell to write into a sheet part. `formula` None = literal value
//!   /// cell; Some = formula cell whose cached <v> is `value`.
//!   pub struct CellEdit { pub sheet: String, pub row: i32, pub col: i32,
//!                         pub formula: Option<String>, pub value: CellValue }
//!   pub enum CellValue { Number(f64), Str(String), Bool(bool), Blank }
//!
//!   /// Rewrite ONLY the sheet parts containing an edited cell; copy every
//!   /// other zip part BYTE-FOR-BYTE from `input`. Returns the new .xlsx
//!   /// bytes. Drops xl/calcChain.xml if present (stale otherwise). Zip
//!   /// output is normalized (fixed mtimes, stable order) for reproducible
//!   /// hashing.
//!   pub fn surgical_write(input: &[u8], edits: &[CellEdit]) -> Result<Vec<u8>>
//!
//!   /// The list of zip part names in `input` (for the fidelity proof /
//!   /// preservation reporting).
//!   pub fn part_names(input: &[u8]) -> Result<Vec<String>>
//!
//!   /// Map a sheet NAME to its part path (xl/worksheets/sheetN.xml) via
//!   /// xl/workbook.xml + xl/_rels/workbook.xml.rels.
//!   pub fn sheet_part(input: &[u8], sheet_name: &str) -> Result<String>
//!
//! THE FIDELITY PROPERTY (must hold, and there is a test for it):
//!   For every part P in the output whose name is NOT a sheet part that
//!   received an edit, bytes(P_out) == bytes(P_in).

use anyhow::{anyhow, bail, Context, Result};
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

/// Compare an XML element/attribute name to a target LOCAL name, ignoring any
/// namespace prefix (`x:sheet`/`sheet` both match `b"sheet"`).
pub(crate) fn local_name_eq(name: &[u8], local: &[u8]) -> bool {
    let n = match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    };
    n == local
}

/// One cell to write into a sheet part. `formula` None = literal value cell;
/// Some = formula cell whose cached <v> is `value`.
#[derive(Debug, Clone)]
pub struct CellEdit {
    pub sheet: String,
    pub row: i32,
    pub col: i32,
    pub formula: Option<String>,
    pub value: CellValue,
}

/// The literal (or cached, for a formula) value written into a cell.
#[derive(Debug, Clone)]
pub enum CellValue {
    Number(f64),
    Str(String),
    Bool(bool),
    Blank,
}

pub fn surgical_write(input: &[u8], edits: &[CellEdit]) -> Result<Vec<u8>> {
    // Resolve each edited sheet NAME to its part path once, then bucket edits
    // by part path. A sheet name that does not resolve is a hard error: we
    // must never silently drop a requested edit.
    let workbook_xml = read_part(input, "xl/workbook.xml")?;
    let rels_xml = read_part(input, "xl/_rels/workbook.xml.rels")?;

    let mut by_part: BTreeMap<String, Vec<&CellEdit>> = BTreeMap::new();
    for edit in edits {
        let part = resolve_sheet_part(&workbook_xml, &rels_xml, &edit.sheet)
            .with_context(|| format!("resolve sheet {}", edit.sheet))?;
        by_part.entry(part).or_default().push(edit);
    }

    let mut archive =
        zip::ZipArchive::new(Cursor::new(input)).map_err(|e| anyhow!("open workbook zip: {e}"))?;

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // Normalized options: fixed 1980-01-01 mtime + deterministic per-entry
    // metadata so identical input+edits hash identically.
    let base_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    // One decompression budget across the whole workbook (anti-bomb; see
    // read_entry_capped).
    let mut budget = total_cap();
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| anyhow!("read zip entry: {e}"))?;
        let name = file.name().to_string();

        // Drop the calc chain: keeping a stale one is the only cross-part
        // hazard; Excel rebuilds it on open.
        if name == "xl/calcChain.xml" {
            continue;
        }

        if file.is_dir() {
            writer
                .add_directory(name, base_opts)
                .map_err(|e| anyhow!("write dir entry: {e}"))?;
            continue;
        }

        let sz = file.size();
        let mut bytes = read_entry_capped(file, sz, &name, &mut budget)?;

        if let Some(part_edits) = by_part.get(&name) {
            bytes = rewrite_sheet(&bytes, part_edits).with_context(|| format!("rewrite {name}"))?;
        }

        writer
            .start_file(&name, base_opts)
            .map_err(|e| anyhow!("start part {name}: {e}"))?;
        writer
            .write_all(&bytes)
            .map_err(|e| anyhow!("write part {name}: {e}"))?;
    }

    let cursor = writer.finish().map_err(|e| anyhow!("finalize zip: {e}"))?;
    Ok(cursor.into_inner())
}

// Part of the module's public contract (the fidelity proof / preservation
// reporting). apply.rs computes the fidelity tally on a files-only basis via
// read_parts, so this is currently unused by the integrator; kept as documented
// API and covered by tests below.
#[allow(dead_code)]
pub fn part_names(input: &[u8]) -> Result<Vec<String>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(input)).map_err(|e| anyhow!("open workbook zip: {e}"))?;
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| anyhow!("read zip entry: {e}"))?;
        names.push(file.name().to_string());
    }
    Ok(names)
}

// Part of the module's public contract (name -> part path). apply.rs resolves
// sheets via ironcalc indices instead, so this is currently unused by the
// integrator; kept as documented API and covered below.
#[allow(dead_code)]
pub fn sheet_part(input: &[u8], sheet_name: &str) -> Result<String> {
    let workbook_xml = read_part(input, "xl/workbook.xml")?;
    let rels_xml = read_part(input, "xl/_rels/workbook.xml.rels")?;
    resolve_sheet_part(&workbook_xml, &rels_xml, sheet_name)
}

// ---------------------------------------------------------------------------
// zip part access — bounded against decompression bombs
// ---------------------------------------------------------------------------

/// Per-part and per-workbook decompression caps. An .xlsx is a zip; a crafted
/// "decompression bomb" declares a small compressed size but expands to gigabytes,
/// OOM-ing the process — a real threat for a tool whose whole premise is handling
/// UNTRUSTED workbooks. xlq controls all of ITS OWN zip reads (via `zip` 2.4.2),
/// so every one routes through [`read_entry_capped`], which bounds both the
/// up-front reservation (defeats the declared-size over-allocation attack) and the
/// actual decompressed length (defeats the real bomb). Defaults are generous — a
/// real single sheet part rarely exceeds tens of MiB — and overridable via
/// `XLQ_MAX_PART_BYTES` / `XLQ_MAX_TOTAL_BYTES` for the rare legitimately-huge model.
pub(crate) const PART_DECOMPRESS_CAP: u64 = 512 << 20; // 512 MiB / part
pub(crate) const TOTAL_DECOMPRESS_CAP: u64 = 2 << 30; // 2 GiB / workbook

fn env_cap(var: &str, default: u64) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

/// The per-part decompression cap, honoring the `XLQ_MAX_PART_BYTES` override.
pub(crate) fn part_cap() -> u64 {
    env_cap("XLQ_MAX_PART_BYTES", PART_DECOMPRESS_CAP)
}

/// The per-workbook decompression cap, honoring the `XLQ_MAX_TOTAL_BYTES`
/// override. Callers seed a mutable budget with this before a per-entry loop.
pub(crate) fn total_cap() -> u64 {
    env_cap("XLQ_MAX_TOTAL_BYTES", TOTAL_DECOMPRESS_CAP)
}

/// Read one zip entry fully into memory, failing closed on a decompression bomb.
/// `declared_size` is the entry's (untrusted) central-directory uncompressed size,
/// used ONLY to size the reservation — never trusted for the cap. `budget` is the
/// remaining per-workbook allowance; it is decremented by the bytes actually read,
/// so a workbook of many individually-under-cap parts that together exceed the
/// total cap still fails closed.
pub(crate) fn read_entry_capped<R: Read>(
    entry: R,
    declared_size: u64,
    name: &str,
    budget: &mut u64,
) -> Result<Vec<u8>> {
    let cap = part_cap().min(*budget);
    // Reserve at most the smaller of declared size, the remaining cap, and a sane
    // 8 MiB start — never allocate gigabytes from an attacker-declared size.
    let reserve = declared_size.min(cap).min(8 << 20) as usize;
    let mut bytes = Vec::with_capacity(reserve);
    // Read one byte PAST the cap so an over-cap entry is detected deterministically,
    // whatever size it declared. saturating_add so a cap of u64::MAX (e.g. an
    // XLQ_MAX_*_BYTES override) does not wrap to 0 and fail OPEN.
    let n = entry
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| anyhow!("read part {name}: {e}"))? as u64;
    if n > cap {
        bail!(
            "decompression_bomb: part '{name}' exceeds the decompression cap \
             ({cap} bytes) — refusing (raise XLQ_MAX_PART_BYTES / XLQ_MAX_TOTAL_BYTES if intentional)"
        );
    }
    *budget -= n;
    Ok(bytes)
}

pub(crate) fn read_part(input: &[u8], name: &str) -> Result<Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(input)).map_err(|e| anyhow!("open workbook zip: {e}"))?;
    // OPC part names compare CASE-INSENSITIVELY (ECMA-376 Part 2), but zip by_name is
    // case-sensitive. Prefer an exact hit; otherwise fall back to a case-insensitive
    // match so a re-cased entry (e.g. Sheet1.XML.rels) cannot hide a part from the
    // fidelity/residual scans — which would fail OPEN.
    let resolved = if archive.file_names().any(|n| n == name) {
        name.to_string()
    } else {
        match archive
            .file_names()
            .find(|n| n.eq_ignore_ascii_case(name))
            .map(|n| n.to_string())
        {
            Some(n) => n,
            None => return Err(anyhow!("missing part {name}")),
        }
    };
    let file = archive
        .by_name(&resolved)
        .map_err(|_| anyhow!("missing part {name}"))?;
    let sz = file.size();
    // Single named part: a fresh per-part budget is correct here.
    let mut budget = total_cap();
    read_entry_capped(file, sz, name, &mut budget)
}

/// Preflight guard: stream every zip entry of the workbook at `path` through the
/// decompression caps BEFORE the file is handed to the vendored ironcalc engine.
///
/// Commands that read the workbook through xlq's own capped readers (structural
/// edit, surgical write, `read_part`) are already bounded. But `calc`/`inspect`/
/// `diff`/`certify`/`apply` hand a user path straight to
/// `ironcalc::import::load_from_xlsx`, which decompresses the whole workbook via a
/// SEPARATE, unbounded transitive `zip` that xlq cannot bound without patching the
/// engine. This guard streams the same entries under the same caps first: if every
/// entry stays under the cap for xlq, ironcalc reading them stays under the cap
/// too — turning a potential OOM into a clean fail-closed error. Bytes are streamed
/// to a sink (no allocation).
///
/// Residual (honest): a malformed archive that ironcalc's zip parses differently
/// than xlq's could still slip past; bounding the engine's own reader is tracked as
/// a follow-up. This guard also does not address the quick-xml XML-parse DoS
/// vectors (handled separately by the quick-xml >=0.41 bump).
pub(crate) fn guard_decompression(path: &str) -> Result<()> {
    let f = std::fs::File::open(path).map_err(|e| anyhow!("open workbook: {e}"))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| anyhow!("open workbook zip: {e}"))?;
    let mut budget = total_cap();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| anyhow!("read zip entry: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let cap = part_cap().min(budget);
        // saturating_add so a u64::MAX cap does not wrap to 0 and fail OPEN.
        let n = std::io::copy(&mut entry.take(cap.saturating_add(1)), &mut std::io::sink())
            .map_err(|e| anyhow!("read part {name}: {e}"))?;
        if n > cap {
            bail!(
                "decompression_bomb: part '{name}' exceeds the decompression cap \
                 ({cap} bytes) — refusing (raise XLQ_MAX_PART_BYTES / XLQ_MAX_TOTAL_BYTES if intentional)"
            );
        }
        budget -= n;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// sheet NAME -> part path resolution
// ---------------------------------------------------------------------------

pub(crate) fn resolve_sheet_part(
    workbook_xml: &[u8],
    rels_xml: &[u8],
    sheet_name: &str,
) -> Result<String> {
    let rid = sheet_rid(workbook_xml, sheet_name)?
        .ok_or_else(|| anyhow!("no sheet named {sheet_name}"))?;
    let target = rid_target(rels_xml, &rid)?
        .ok_or_else(|| anyhow!("no relationship {rid} for sheet {sheet_name}"))?;
    // workbook.xml lives in xl/, so its rels targets resolve against "xl".
    Ok(resolve_target("xl", &target))
}

/// All sheets as (name, part_path), in workbook order. Used by structural edits
/// to rewrite cross-sheet references on every sheet part.
pub(crate) fn all_sheets(input: &[u8]) -> Result<Vec<(String, String)>> {
    let workbook_xml = read_part(input, "xl/workbook.xml")?;
    let rels_xml = read_part(input, "xl/_rels/workbook.xml.rels")?;
    let mut reader = Reader::from_reader(workbook_xml.as_slice());
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            // Namespace-aware: match the `sheet` element and the relationship-id
            // attribute by LOCAL name. The relationships-namespace prefix is arbitrary
            // (`r:id`, `r2:id`, …), so keying on the literal `r:id` let a prefix rebind
            // hide a sheet. Local name "id" uniquely identifies the rel id — `sheetId`
            // has local name "sheetId", not "id".
            Event::Empty(e) | Event::Start(e) if local_name_eq(e.name().as_ref(), b"sheet") => {
                let mut nm: Option<String> = None;
                let mut rid: Option<String> = None;
                for a in e.attributes().flatten() {
                    let key = a.key.as_ref();
                    if local_name_eq(key, b"name") {
                        nm = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()
                            .map(|c| c.into_owned());
                    } else if local_name_eq(key, b"id") {
                        rid = Some(String::from_utf8_lossy(&a.value).into_owned());
                    }
                }
                if let (Some(n), Some(r)) = (nm, rid) {
                    if let Some(t) = rid_target(&rels_xml, &r)? {
                        out.push((n, resolve_target("xl", &t)));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn sheet_rid(workbook_xml: &[u8], name: &str) -> Result<Option<String>> {
    let mut reader = Reader::from_reader(workbook_xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"sheet" => {
                let mut nm: Option<String> = None;
                let mut rid: Option<String> = None;
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        b"name" => {
                            nm = a
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|c| c.into_owned());
                        }
                        b"r:id" => {
                            rid = Some(String::from_utf8_lossy(&a.value).into_owned());
                        }
                        _ => {}
                    }
                }
                if nm.as_deref() == Some(name) {
                    return Ok(rid);
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

fn rid_target(rels_xml: &[u8], rid: &str) -> Result<Option<String>> {
    let mut reader = Reader::from_reader(rels_xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"Relationship" => {
                let mut id: Option<String> = None;
                let mut target: Option<String> = None;
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        b"Id" => id = Some(String::from_utf8_lossy(&a.value).into_owned()),
                        b"Target" => {
                            target = a
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|c| c.into_owned());
                        }
                        _ => {}
                    }
                }
                if id.as_deref() == Some(rid) {
                    return Ok(target);
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

pub(crate) fn resolve_target(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// sheet XML surgery
// ---------------------------------------------------------------------------

struct ExistingCell {
    style: Option<Vec<u8>>,
    raw: Vec<u8>,
}

fn rewrite_sheet(src: &[u8], edits: &[&CellEdit]) -> Result<Vec<u8>> {
    // row -> (col -> edit)
    let mut edit_rows: BTreeMap<i32, BTreeMap<i32, &CellEdit>> = BTreeMap::new();
    for e in edits {
        edit_rows.entry(e.row).or_default().insert(e.col, e);
    }
    // Bounding box of the edits, so the worksheet <dimension> can be widened to
    // stay a valid superset of the used range when an edit lands beyond it.
    let bounds = edit_bounds(edits);

    let mut reader = Reader::from_reader(src);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();
    let mut in_sheet_data = false;

    loop {
        buf.clear();
        let ev = reader.read_event_into(&mut buf)?;
        match ev {
            Event::Eof => break,
            Event::Empty(e) if !in_sheet_data && e.name().as_ref() == b"dimension" => {
                writer.write_event(Event::Empty(widen_dimension_tag(&e, bounds)))?;
            }
            Event::Start(e) if e.name().as_ref() == b"sheetData" => {
                in_sheet_data = true;
                writer.write_event(Event::Start(e.into_owned()))?;
            }
            Event::Empty(e) if e.name().as_ref() == b"sheetData" => {
                // Self-closing <sheetData/> but we have rows to insert:
                // expand it into <sheetData> ...rows... </sheetData>.
                writer.write_event(Event::Start(e.into_owned()))?;
                while let Some((r, cells)) = edit_rows.pop_first() {
                    write_new_row(&mut writer, r, &cells)?;
                }
                writer.write_event(Event::End(BytesEnd::new("sheetData")))?;
            }
            Event::End(e) if e.name().as_ref() == b"sheetData" => {
                while let Some((r, cells)) = edit_rows.pop_first() {
                    write_new_row(&mut writer, r, &cells)?;
                }
                in_sheet_data = false;
                writer.write_event(Event::End(e.into_owned()))?;
            }
            Event::Start(e) if in_sheet_data && e.name().as_ref() == b"row" => {
                let rnum = row_num(&e);
                flush_inserts_before(&mut writer, &mut edit_rows, rnum)?;
                if let Some(cells) = edit_rows.remove(&rnum) {
                    modify_existing_row(&mut reader, &mut writer, e.into_owned(), &cells)?;
                } else {
                    writer.write_event(Event::Start(e.into_owned()))?;
                }
            }
            Event::Empty(e) if in_sheet_data && e.name().as_ref() == b"row" => {
                let rnum = row_num(&e);
                flush_inserts_before(&mut writer, &mut edit_rows, rnum)?;
                if let Some(cells) = edit_rows.remove(&rnum) {
                    // Empty <row/> with edits: expand and add generated cells,
                    // widening the row's spans hint to the inserted columns.
                    let min_col = *cells.keys().next().unwrap();
                    let max_col = *cells.keys().next_back().unwrap();
                    writer.write_event(Event::Start(maybe_widen_row_spans(
                        e.into_owned(),
                        min_col,
                        max_col,
                    )))?;
                    for edit in cells.values() {
                        writer.get_mut().write_all(&gen_cell(edit, None))?;
                    }
                    writer.write_event(Event::End(BytesEnd::new("row")))?;
                } else {
                    writer.write_event(Event::Empty(e.into_owned()))?;
                }
            }
            other => {
                writer.write_event(other)?;
            }
        }
    }

    // Invariant (module lines 58-59): never silently drop a requested edit. If
    // any edit is still pending here, its target row/sheetData was never matched
    // — e.g. a worksheet serialized with a namespace prefix (<x:sheetData>). Fail
    // loudly instead of returning byte-unchanged bytes and a false success.
    if !edit_rows.is_empty() {
        let missed: Vec<String> = edit_rows
            .iter()
            .flat_map(|(r, cols)| cols.keys().map(move |c| a1(*r, *c)))
            .collect();
        bail!(
            "surgical edit matched no <sheetData> structure; {} edit(s) would be \
             silently dropped ({}). The worksheet may use an XML namespace prefix \
             xlq does not handle.",
            missed.len(),
            missed.join(", ")
        );
    }

    Ok(writer.into_inner())
}

/// Bounding box (min_row, min_col, max_row, max_col) of an edit set.
fn edit_bounds(edits: &[&CellEdit]) -> Option<(i32, i32, i32, i32)> {
    let mut it = edits.iter();
    let first = it.next()?;
    let (mut r0, mut c0, mut r1, mut c1) = (first.row, first.col, first.row, first.col);
    for e in it {
        r0 = r0.min(e.row);
        c0 = c0.min(e.col);
        r1 = r1.max(e.row);
        c1 = c1.max(e.col);
    }
    Some((r0, c0, r1, c1))
}

/// Rebuild a `<dimension>` tag whose `ref` is widened to cover `bounds` (the
/// edit box). Returns the tag unchanged when there is nothing to widen or the
/// ref cannot be parsed, so a well-formed sheet is only ever left more correct.
fn widen_dimension_tag(
    e: &BytesStart,
    bounds: Option<(i32, i32, i32, i32)>,
) -> BytesStart<'static> {
    let widened = bounds.and_then(|b| {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == b"ref")
            .and_then(|a| widen_ref(&a.value, b))
    });
    let new_ref = match widened {
        Some(r) => r,
        None => return e.to_owned(),
    };
    let mut ne = BytesStart::new("dimension");
    let mut wrote_ref = false;
    for a in e.attributes().flatten() {
        if a.key.as_ref() == b"ref" {
            ne.push_attribute(("ref", new_ref.as_str()));
            wrote_ref = true;
        } else {
            ne.push_attribute((a.key.as_ref(), a.value.as_ref()));
        }
    }
    if !wrote_ref {
        ne.push_attribute(("ref", new_ref.as_str()));
    }
    ne
}

/// Widen a range ref ("A1:B2" or "A1") to also contain `b`. Returns `None` when
/// the existing ref already covers `b` (no rewrite needed) or cannot be parsed.
fn widen_ref(refv: &[u8], b: (i32, i32, i32, i32)) -> Option<String> {
    let s = std::str::from_utf8(refv).ok()?;
    let (start, end) = s.split_once(':').unwrap_or((s, s));
    let (sr, sc) = parse_ref(start.as_bytes())?;
    let (er, ec) = parse_ref(end.as_bytes())?;
    let (r0, c0, r1, c1) = (sr.min(er), sc.min(ec), sr.max(er), sc.max(ec));
    let (nr0, nc0, nr1, nc1) = (r0.min(b.0), c0.min(b.1), r1.max(b.2), c1.max(b.3));
    if (nr0, nc0, nr1, nc1) == (r0, c0, r1, c1) {
        return None; // already a superset
    }
    Some(format!(
        "{}{}:{}{}",
        col_to_letters(nc0),
        nr0,
        col_to_letters(nc1),
        nr1
    ))
}

fn flush_inserts_before(
    writer: &mut Writer<Vec<u8>>,
    edit_rows: &mut BTreeMap<i32, BTreeMap<i32, &CellEdit>>,
    rnum: i32,
) -> Result<()> {
    while let Some((&r, _)) = edit_rows.first_key_value() {
        if r < rnum {
            let (r, cells) = edit_rows.pop_first().unwrap();
            write_new_row(writer, r, &cells)?;
        } else {
            break;
        }
    }
    Ok(())
}

fn modify_existing_row(
    reader: &mut Reader<&[u8]>,
    writer: &mut Writer<Vec<u8>>,
    start: BytesStart<'static>,
    cells: &BTreeMap<i32, &CellEdit>,
) -> Result<()> {
    let existing = read_row_cells(reader)?;

    // Merge: existing cells verbatim, edited cols replaced (preserving the
    // existing style index), new cols inserted in column order.
    let mut out: BTreeMap<i32, Vec<u8>> = BTreeMap::new();
    for (col, ec) in &existing {
        out.insert(*col, ec.raw.clone());
    }
    for (col, edit) in cells {
        let style = existing.get(col).and_then(|e| e.style.clone());
        out.insert(*col, gen_cell(edit, style.as_deref()));
    }

    // Preserve the original <row ...> start tag (ht/style/…). Its `spans` hint
    // is only rebuilt when an edit added a column outside the existing span, so
    // an untouched row stays byte-for-byte and a widened one stays consistent.
    let min_col = *out.keys().next().unwrap();
    let max_col = *out.keys().next_back().unwrap();
    writer.write_event(Event::Start(maybe_widen_row_spans(start, min_col, max_col)))?;
    for bytes in out.values() {
        writer.get_mut().write_all(bytes)?;
    }
    writer.write_event(Event::End(BytesEnd::new("row")))?;
    Ok(())
}

/// Return the `<row>` start tag with its `spans` widened to cover
/// `[min_col, max_col]`, or the tag unchanged when the existing `spans` already
/// covers that range (byte-identical) or cannot be parsed (multi-range spans
/// are left verbatim rather than risk narrowing them).
fn maybe_widen_row_spans(
    start: BytesStart<'static>,
    min_col: i32,
    max_col: i32,
) -> BytesStart<'static> {
    let spans = start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"spans")
        .map(|a| a.value.into_owned());
    let (lo, hi) = match &spans {
        Some(v) => match parse_spans(v) {
            Some((lo, hi)) if lo <= min_col && hi >= max_col => return start, // covers
            Some((lo, hi)) => (lo.min(min_col), hi.max(max_col)),
            None => return start, // unparseable (e.g. multi-range) — leave as-is
        },
        None => (min_col, max_col),
    };
    let spans_val = format!("{lo}:{hi}");
    let mut ne = BytesStart::new("row");
    let mut wrote = false;
    for a in start.attributes().flatten() {
        if a.key.as_ref() == b"spans" {
            ne.push_attribute(("spans", spans_val.as_str()));
            wrote = true;
        } else {
            ne.push_attribute((a.key.as_ref(), a.value.as_ref()));
        }
    }
    if !wrote {
        ne.push_attribute(("spans", spans_val.as_str()));
    }
    ne
}

/// Parse a single-range `spans` value ("min:max"). Returns `None` for empty or
/// multi-range values, which the caller then leaves untouched.
fn parse_spans(v: &[u8]) -> Option<(i32, i32)> {
    let s = std::str::from_utf8(v).ok()?;
    let (a, b) = s.split_once(':')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Consume events up to and including the row's `</row>`, capturing each `<c>`
/// verbatim (byte-identical token copy) keyed by column.
fn read_row_cells(reader: &mut Reader<&[u8]>) -> Result<BTreeMap<i32, ExistingCell>> {
    let mut cells: BTreeMap<i32, ExistingCell> = BTreeMap::new();
    let mut buf = Vec::new();
    // Positional column counter for cells that omit `r`: a bare <c> takes the
    // column after the previous cell (1-based), exactly as Excel reads them.
    let mut next_col = 1i32;
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) if e.name().as_ref() == b"c" => {
                let (col_opt, style) = cell_meta(&e);
                let col = col_opt.unwrap_or(next_col);
                next_col = col + 1;
                let mut tmp = Writer::new(Vec::new());
                tmp.write_event(Event::Start(e.into_owned()))?;
                // Copy through the matching </c>.
                let mut depth = 1i32;
                let mut inner = Vec::new();
                loop {
                    inner.clear();
                    let ev = reader.read_event_into(&mut inner)?;
                    match &ev {
                        Event::Start(_) => depth += 1,
                        Event::End(_) => depth -= 1,
                        Event::Eof => bail!("unterminated <c>"),
                        _ => {}
                    }
                    tmp.write_event(ev.into_owned())?;
                    if depth == 0 {
                        break;
                    }
                }
                cells.insert(
                    col,
                    ExistingCell {
                        style,
                        raw: tmp.into_inner(),
                    },
                );
            }
            Event::Empty(e) if e.name().as_ref() == b"c" => {
                let (col_opt, style) = cell_meta(&e);
                let col = col_opt.unwrap_or(next_col);
                next_col = col + 1;
                let mut tmp = Writer::new(Vec::new());
                tmp.write_event(Event::Empty(e.into_owned()))?;
                cells.insert(
                    col,
                    ExistingCell {
                        style,
                        raw: tmp.into_inner(),
                    },
                );
            }
            Event::End(e) if e.name().as_ref() == b"row" => break,
            Event::Eof => bail!("unterminated <row>"),
            // Stray whitespace/text inside a row is dropped (Excel emits none).
            _ => {}
        }
    }
    Ok(cells)
}

fn write_new_row(
    writer: &mut Writer<Vec<u8>>,
    r: i32,
    cells: &BTreeMap<i32, &CellEdit>,
) -> Result<()> {
    let min = *cells.keys().next().unwrap();
    let max = *cells.keys().next_back().unwrap();
    let start = format!("<row r=\"{r}\" spans=\"{min}:{max}\">");
    writer.get_mut().write_all(start.as_bytes())?;
    for edit in cells.values() {
        writer.get_mut().write_all(&gen_cell(edit, None))?;
    }
    writer.get_mut().write_all(b"</row>")?;
    Ok(())
}

/// Column (from the `r` attribute, if present) and style index of a `<c>`.
/// `None` column means the cell omitted `r` — a legal positional cell whose
/// column the caller must derive from its position in the row, NOT default to
/// a fixed key (doing so collapses every r-less cell onto one another).
fn cell_meta(e: &BytesStart) -> (Option<i32>, Option<Vec<u8>>) {
    let mut col = None;
    let mut style = None;
    for a in e.attributes().flatten() {
        match a.key.as_ref() {
            b"r" => {
                if let Some((_, c)) = parse_ref(&a.value) {
                    col = Some(c);
                }
            }
            b"s" => style = Some(a.value.into_owned()),
            _ => {}
        }
    }
    (col, style)
}

fn row_num(e: &BytesStart) -> i32 {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == b"r" {
            return std::str::from_utf8(&a.value)
                .ok()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// cell serialization
// ---------------------------------------------------------------------------

fn gen_cell(edit: &CellEdit, style: Option<&[u8]>) -> Vec<u8> {
    let r = a1(edit.row, edit.col);
    let s_attr = match style {
        Some(s) => format!(" s=\"{}\"", String::from_utf8_lossy(s)),
        None => String::new(),
    };
    let out = match &edit.formula {
        Some(f) => {
            let ff = xml_escape_text(f.strip_prefix('=').unwrap_or(f));
            match &edit.value {
                CellValue::Number(n) => match fmt_num(*n) {
                    Some(v) => format!("<c r=\"{r}\"{s_attr}><f>{ff}</f><v>{v}</v></c>"),
                    // Non-finite has no valid xsd:double form; an overflowed or
                    // undefined numeric result is #NUM! in Excel. Emit an error
                    // cell rather than a `<v>inf</v>` that corrupts the part.
                    None => format!("<c r=\"{r}\"{s_attr} t=\"e\"><f>{ff}</f><v>#NUM!</v></c>"),
                },
                CellValue::Bool(b) => format!(
                    "<c r=\"{r}\"{s_attr} t=\"b\"><f>{ff}</f><v>{}</v></c>",
                    if *b { 1 } else { 0 }
                ),
                CellValue::Str(t) => format!(
                    "<c r=\"{r}\"{s_attr} t=\"str\"><f>{ff}</f><v>{}</v></c>",
                    xml_escape_text(t)
                ),
                CellValue::Blank => format!("<c r=\"{r}\"{s_attr}><f>{ff}</f></c>"),
            }
        }
        None => match &edit.value {
            CellValue::Number(n) => match fmt_num(*n) {
                Some(v) => format!("<c r=\"{r}\"{s_attr}><v>{v}</v></c>"),
                None => format!("<c r=\"{r}\"{s_attr} t=\"e\"><v>#NUM!</v></c>"),
            },
            CellValue::Str(t) => format!(
                "<c r=\"{r}\"{s_attr} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                xml_escape_text(t)
            ),
            CellValue::Bool(b) => format!(
                "<c r=\"{r}\"{s_attr} t=\"b\"><v>{}</v></c>",
                if *b { 1 } else { 0 }
            ),
            CellValue::Blank => format!("<c r=\"{r}\"{s_attr}/>"),
        },
    };
    out.into_bytes()
}

/// The `<v>` body for a numeric cell, or `None` when `n` is non-finite
/// (inf/-inf/NaN) — those have no valid OOXML numeric representation and the
/// caller must emit an error cell instead of an invalid `<v>`.
fn fmt_num(n: f64) -> Option<String> {
    if n.is_finite() {
        Some(format!("{n}"))
    } else {
        None
    }
}

/// Escape cell text for XML content. Besides the `&`/`<`/`>` entities, this
/// DROPS the control characters XML 1.0 forbids outright (0x00–0x08, 0x0B,
/// 0x0C, 0x0E–0x1F) — tab/LF/CR are the only allowed sub-0x20 codepoints, and
/// the forbidden ones cannot even be represented as numeric refs, so a raw
/// (or `&#1;`) byte would make the whole part non-well-formed and Excel would
/// reject the workbook. Stray control chars are common in imported/pasted data.
fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' => {}
            _ => out.push(c),
        }
    }
    out
}

fn a1(row: i32, col: i32) -> String {
    format!("{}{}", col_to_letters(col), row)
}

fn col_to_letters(mut col: i32) -> String {
    let mut s = Vec::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        s.push(b'A' + rem as u8);
        col = (col - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

fn parse_ref(r: &[u8]) -> Option<(i32, i32)> {
    let mut col = 0i32;
    let mut i = 0;
    while i < r.len() && r[i].is_ascii_alphabetic() {
        col = col * 26 + (r[i].to_ascii_uppercase() - b'A' + 1) as i32;
        i += 1;
    }
    let digits_start = i;
    let mut row = 0i32;
    while i < r.len() && r[i].is_ascii_digit() {
        row = row * 10 + (r[i] - b'0') as i32;
        i += 1;
    }
    if col == 0 || i == digits_start {
        return None;
    }
    Some((row, col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const MACRO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/t1/macro.xlsm");
    const PIVOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/t1/pivot-chart.xlsx"
    );

    fn read_fixture(path: &str) -> Vec<u8> {
        std::fs::read(path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"))
    }

    #[test]
    fn read_entry_capped_refuses_bomb() {
        // A decompression bomb expands far past its declared size; an "infinite"
        // reader with a small budget must be refused deterministically.
        let mut budget = 1024u64;
        let err = read_entry_capped(std::io::repeat(0u8), 16, "bomb", &mut budget).unwrap_err();
        assert!(format!("{err:#}").contains("decompression_bomb"), "{err:#}");
    }

    #[test]
    fn read_entry_capped_reads_normal_and_debits_budget() {
        let data = vec![7u8; 500];
        let mut budget = 10_000u64;
        let out = read_entry_capped(&data[..], 500, "part", &mut budget).unwrap();
        assert_eq!(out, data, "under-cap entry read fully");
        assert_eq!(budget, 10_000 - 500, "budget debited by bytes read");
    }

    #[test]
    fn read_entry_capped_enforces_total_across_parts() {
        // Two parts each under the per-part cap but together over the small
        // remaining budget: the second fails closed via cap = part_cap.min(budget).
        let mut budget = 800u64;
        let first = read_entry_capped(&vec![1u8; 500][..], 500, "p1", &mut budget).unwrap();
        assert_eq!(first.len(), 500);
        assert_eq!(budget, 300);
        let err = read_entry_capped(&vec![1u8; 500][..], 500, "p2", &mut budget).unwrap_err();
        assert!(
            format!("{err:#}").contains("decompression_bomb"),
            "total cap: {err:#}"
        );
    }

    fn parts_map(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut map = BTreeMap::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            map.insert(name, b);
        }
        map
    }

    fn tmp_path(tag: &str) -> String {
        let dir = std::env::temp_dir().join("xlq-ooxml-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}.xlsx", tag, std::process::id()))
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn parse_and_build_refs_roundtrip() {
        assert_eq!(parse_ref(b"A1"), Some((1, 1)));
        assert_eq!(parse_ref(b"B7"), Some((7, 2)));
        assert_eq!(parse_ref(b"AA10"), Some((10, 27)));
        assert_eq!(a1(1, 1), "A1");
        assert_eq!(a1(7, 2), "B7");
        assert_eq!(a1(10, 27), "AA10");
        assert_eq!(col_to_letters(26), "Z");
        assert_eq!(col_to_letters(28), "AB");
    }

    // (1) Editing one number cell leaves every OTHER part byte-identical and
    //     the part-name set unchanged (modulo the intentionally dropped
    //     calcChain.xml).
    #[test]
    fn number_edit_preserves_all_other_parts_byte_identical() {
        let input = read_fixture(MACRO);
        let sheet = sheet_part(&input, "Data").unwrap();
        assert_eq!(sheet, "xl/worksheets/sheet1.xml");

        let edits = vec![CellEdit {
            sheet: "Data".to_string(),
            row: 2,
            col: 2,
            formula: None,
            value: CellValue::Number(999.0),
        }];
        let out = surgical_write(&input, &edits).unwrap();

        let in_parts = parts_map(&input);
        let out_parts = parts_map(&out);

        let in_names: BTreeSet<_> = in_parts.keys().cloned().collect();
        let mut expected = in_names.clone();
        expected.remove("xl/calcChain.xml");
        let out_names: BTreeSet<_> = out_parts.keys().cloned().collect();
        assert_eq!(out_names, expected, "part-name set drifted");

        for (name, bytes) in &out_parts {
            if name == &sheet {
                assert_ne!(bytes, &in_parts[name], "sheet part was not rewritten");
            } else {
                assert_eq!(
                    bytes, &in_parts[name],
                    "non-edited part {name} not byte-identical"
                );
            }
        }
        // The edited value made it in.
        let s = String::from_utf8_lossy(&out_parts[&sheet]);
        assert!(
            s.contains("<c r=\"B2\"><v>999</v></c>"),
            "edit not applied: {s}"
        );
    }

    // (2) Editing a cell in the pivot+chart workbook leaves charts / pivot
    //     caches / pivot tables byte-identical.
    #[test]
    fn pivot_chart_parts_untouched() {
        let input = read_fixture(PIVOT);
        let edits = vec![CellEdit {
            sheet: "Sheet1".to_string(),
            row: 2,
            col: 1, // A2 = 222 (a plain number)
            formula: None,
            value: CellValue::Number(4242.0),
        }];
        let out = surgical_write(&input, &edits).unwrap();
        let in_parts = parts_map(&input);
        let out_parts = parts_map(&out);

        for (name, bytes) in &in_parts {
            if name.starts_with("xl/charts/")
                || name.starts_with("xl/pivotCache")
                || name.starts_with("xl/pivotTables")
                || name.starts_with("xl/drawings/")
            {
                assert_eq!(&out_parts[name], bytes, "feature part {name} was modified");
            }
        }
        assert!(!out_parts.contains_key("xl/calcChain.xml"));
    }

    // (3) Editing macro.xlsm leaves xl/vbaProject.bin byte-identical.
    #[test]
    fn vba_project_untouched() {
        let input = read_fixture(MACRO);
        let edits = vec![CellEdit {
            sheet: "Data".to_string(),
            row: 3,
            col: 2,
            formula: None,
            value: CellValue::Number(7.0),
        }];
        let out = surgical_write(&input, &edits).unwrap();
        let in_parts = parts_map(&input);
        let out_parts = parts_map(&out);
        assert_eq!(
            out_parts["xl/vbaProject.bin"], in_parts["xl/vbaProject.bin"],
            "vbaProject.bin changed"
        );
    }

    // (4) An inline-string edit leaves sharedStrings.xml byte-identical.
    #[test]
    fn inline_string_edit_leaves_shared_strings_untouched() {
        let input = read_fixture(MACRO);
        let edits = vec![CellEdit {
            sheet: "Data".to_string(),
            row: 2,
            col: 2,
            formula: None,
            value: CellValue::Str("hello & <world>".to_string()),
        }];
        let out = surgical_write(&input, &edits).unwrap();
        let in_parts = parts_map(&input);
        let out_parts = parts_map(&out);
        assert_eq!(
            out_parts["xl/sharedStrings.xml"], in_parts["xl/sharedStrings.xml"],
            "sharedStrings.xml changed by an inline-string edit"
        );
        let sheet = String::from_utf8_lossy(&out_parts["xl/worksheets/sheet1.xml"]);
        assert!(
            sheet.contains(
                "t=\"inlineStr\"><is><t xml:space=\"preserve\">hello &amp; &lt;world&gt;</t></is>"
            ),
            "inline string not escaped/written: {sheet}"
        );
    }

    // (5) Inserting a NEW cell and a NEW row produces XML that IronCalc can
    //     re-read, with the inserted values present.
    #[test]
    fn inserted_cell_and_row_reload_in_ironcalc() {
        let input = read_fixture(MACRO);
        let edits = vec![
            // New cell in an existing row (row 2 currently has A2,B2).
            CellEdit {
                sheet: "Data".to_string(),
                row: 2,
                col: 3, // C2
                formula: None,
                value: CellValue::Number(555.0),
            },
            // Brand-new row beyond the sheet's used range.
            CellEdit {
                sheet: "Data".to_string(),
                row: 10,
                col: 1, // A10
                formula: None,
                value: CellValue::Str("inserted".to_string()),
            },
        ];
        let out = surgical_write(&input, &edits).unwrap();

        let path = tmp_path("insert");
        std::fs::write(&path, &out).unwrap();
        let model = ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en")
            .expect("IronCalc must re-read the surgically edited workbook");
        let sheet = model
            .get_worksheets_properties()
            .iter()
            .position(|p| p.name == "Data")
            .unwrap() as u32;
        assert_eq!(model.get_formatted_cell_value(sheet, 2, 3).unwrap(), "555");
        assert_eq!(
            model.get_formatted_cell_value(sheet, 10, 1).unwrap(),
            "inserted"
        );
        let _ = std::fs::remove_file(&path);
    }

    // Dropping calcChain leaves a dangling relationship + content-type
    // override, yet IronCalc (and Excel) tolerate a missing calc cache: the
    // complex pivot+chart workbook must still re-read after surgery.
    #[test]
    fn pivot_chart_reloads_after_calcchain_drop() {
        let input = read_fixture(PIVOT);
        let edits = vec![CellEdit {
            sheet: "Sheet1".to_string(),
            row: 2,
            col: 1,
            formula: None,
            value: CellValue::Number(4242.0),
        }];
        let out = surgical_write(&input, &edits).unwrap();
        let path = tmp_path("pivot-reload");
        std::fs::write(&path, &out).unwrap();
        let model = ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en")
            .expect("pivot+chart workbook must reload after calcChain drop");
        let sheet = model
            .get_worksheets_properties()
            .iter()
            .position(|p| p.name == "Sheet1")
            .unwrap() as u32;
        assert_eq!(model.get_formatted_cell_value(sheet, 2, 1).unwrap(), "4242");
        let _ = std::fs::remove_file(&path);
    }

    // Determinism: identical input + edits produce byte-identical output.
    #[test]
    fn output_is_deterministic() {
        let input = read_fixture(MACRO);
        let edits = vec![CellEdit {
            sheet: "Data".to_string(),
            row: 2,
            col: 2,
            formula: None,
            value: CellValue::Number(1.0),
        }];
        let a = surgical_write(&input, &edits).unwrap();
        let b = surgical_write(&input, &edits).unwrap();
        assert_eq!(a, b, "surgical_write is not reproducible");
    }

    // A formula edit writes <f>/<v> and preserves the existing style index.
    #[test]
    fn formula_edit_preserves_style_and_writes_cached_value() {
        let input = read_fixture(PIVOT);
        // Sheet1!F2 carries s="1" and a shared string; overwrite with a formula.
        let edits = vec![CellEdit {
            sheet: "Sheet1".to_string(),
            row: 2,
            col: 6, // F2
            formula: Some("=A2+1".to_string()),
            value: CellValue::Number(223.0),
        }];
        let out = surgical_write(&input, &edits).unwrap();
        let out_parts = parts_map(&out);
        let sheet = String::from_utf8_lossy(&out_parts["xl/worksheets/sheet1.xml"]);
        assert!(
            sheet.contains("<c r=\"F2\" s=\"1\"><f>A2+1</f><v>223</v></c>"),
            "formula cell malformed: {sheet}"
        );
    }

    fn edit(row: i32, col: i32, value: CellValue) -> CellEdit {
        CellEdit {
            sheet: "S".to_string(),
            row,
            col,
            formula: None,
            value,
        }
    }

    fn rewrite(src: &str, edits: &[CellEdit]) -> Result<String> {
        let refs: Vec<&CellEdit> = edits.iter().collect();
        rewrite_sheet(src.as_bytes(), &refs).map(|b| String::from_utf8(b).unwrap())
    }

    // A non-finite cached value has no valid xsd:double form; it must become an
    // Excel error cell (#NUM!), never `<v>inf</v>`/`<v>NaN</v>`.
    #[test]
    fn nonfinite_number_becomes_error_cell() {
        let lit = String::from_utf8(gen_cell(
            &edit(1, 1, CellValue::Number(f64::INFINITY)),
            None,
        ))
        .unwrap();
        assert_eq!(lit, "<c r=\"A1\" t=\"e\"><v>#NUM!</v></c>", "got {lit}");
        let nan =
            String::from_utf8(gen_cell(&edit(1, 1, CellValue::Number(f64::NAN)), None)).unwrap();
        assert!(nan.contains("t=\"e\"") && nan.contains("#NUM!") && !nan.contains("NaN</v>"));
        let f = CellEdit {
            sheet: "S".into(),
            row: 1,
            col: 1,
            formula: Some("=1/0".into()),
            value: CellValue::Number(f64::NEG_INFINITY),
        };
        let out = String::from_utf8(gen_cell(&f, None)).unwrap();
        assert_eq!(
            out, "<c r=\"A1\" t=\"e\"><f>1/0</f><v>#NUM!</v></c>",
            "got {out}"
        );
    }

    // XML-1.0-illegal control characters must be dropped, not passed through raw
    // (raw or numeric-ref, both make the part non-well-formed).
    #[test]
    fn illegal_control_chars_are_stripped() {
        assert_eq!(xml_escape_text("bad\u{1}char\u{1f}!"), "badchar!");
        // Legal whitespace survives; entities still escape.
        assert_eq!(xml_escape_text("a\tb\nc & <d>"), "a\tb\nc &amp; &lt;d&gt;");
        let out = String::from_utf8(gen_cell(
            &edit(1, 1, CellValue::Str("x\u{0}y".into())),
            None,
        ))
        .unwrap();
        assert!(!out.contains('\u{0}'), "raw NUL leaked: {out:?}");
        assert!(out.contains(">xy<"), "control char not stripped: {out}");
    }

    // Positional (r-less) cells must NOT all collapse onto one key: editing the
    // middle of three r-less cells must preserve the other two.
    #[test]
    fn positional_rless_cells_are_preserved() {
        let src = "<worksheet><sheetData><row r=\"1\">\
                   <c><v>10</v></c><c><v>20</v></c><c><v>30</v></c></row></sheetData></worksheet>";
        let out = rewrite(src, &[edit(1, 2, CellValue::Number(99.0))]).unwrap();
        assert!(out.contains("<v>10</v>"), "A1 lost: {out}");
        assert!(out.contains("<v>30</v>"), "C1 lost: {out}");
        assert!(
            out.contains("<c r=\"B1\"><v>99</v></c>"),
            "B1 edit missing: {out}"
        );
        assert!(
            !out.contains("<v>20</v>"),
            "old B1 value should be replaced: {out}"
        );
    }

    // A namespace-prefixed worksheet the writer cannot match must ERROR, never
    // return byte-unchanged bytes and a false success (module invariant).
    #[test]
    fn prefixed_sheetdata_errors_instead_of_silent_drop() {
        let src = "<x:worksheet xmlns:x=\"urn:main\"><x:sheetData><x:row r=\"1\">\
                   <x:c r=\"A1\"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>";
        let err = rewrite(src, &[edit(1, 1, CellValue::Number(42.0))]).unwrap_err();
        assert!(
            format!("{err}").contains("silently dropped"),
            "wrong error: {err}"
        );
    }

    // An edit beyond the stored <dimension> widens it to a valid superset.
    #[test]
    fn dimension_is_widened_to_cover_inserts() {
        let src = "<worksheet><dimension ref=\"A1:B2\"/><sheetData>\
                   <row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData></worksheet>";
        let out = rewrite(src, &[edit(5, 5, CellValue::Number(7.0))]).unwrap();
        assert!(
            out.contains("<dimension ref=\"A1:E5\"/>"),
            "dimension not widened: {out}"
        );
        // An edit already inside the range leaves dimension byte-identical.
        let inside = rewrite(src, &[edit(1, 1, CellValue::Number(9.0))]).unwrap();
        assert!(
            inside.contains("<dimension ref=\"A1:B2\"/>"),
            "dimension churned: {inside}"
        );
    }

    // Adding a column past a row's spans widens the spans hint.
    #[test]
    fn row_spans_widened_when_cell_added_beyond() {
        let src = "<worksheet><sheetData><row r=\"1\" spans=\"1:2\">\
                   <c r=\"A1\"><v>1</v></c><c r=\"B1\"><v>2</v></c></row></sheetData></worksheet>";
        let out = rewrite(src, &[edit(1, 5, CellValue::Number(7.0))]).unwrap();
        assert!(out.contains("spans=\"1:5\""), "spans not widened: {out}");
    }
}
