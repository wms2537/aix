//! Shared BLACK-BOX test harness: shells the compiled `xlq` binary and mutates workbook bytes.
//!
//! xlq is a binary-only crate, so `tests/*.rs` cannot reach its internals; these tests drive the
//! real CLI end-to-end. This module is `mod common;`-included into each test file, so every
//! helper is compiled into each test binary whether or not that binary uses it — hence the
//! module-wide `allow(dead_code)`.

#![allow(dead_code)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The result of one `xlq` invocation. `code` is the process exit code (certify REFUSED exits 1,
/// which is expected — so we do NOT assert success here).
pub struct Run {
    pub code: i32,
    pub json: Value,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn status(&self) -> &str {
        self.json
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    pub fn reason(&self) -> &str {
        self.json
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    pub fn certified(&self) -> bool {
        self.status() == "CERTIFIED"
    }
    pub fn refused(&self) -> bool {
        self.status() == "REFUSED"
    }
}

pub fn xlq(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_xlq"))
        .args(args)
        .output()
        .expect("spawn xlq");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    Run {
        code: out.status.code().unwrap_or(-1),
        json,
        stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

// ---- the committed corpus (same files the in-crate harness reads) ------------------------

pub fn corpus_names() -> &'static [&'static str] {
    &[
        "sum_band.xlsx",
        "crosssheet.xlsx",
        "settings.xlsx",
        "names.xlsx",
        "security.xlsx",
        "constructs.xlsx",
    ]
}

pub fn corpus_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus")
        .join(name)
}

pub fn corpus_bytes(name: &str) -> Vec<u8> {
    std::fs::read(corpus_path(name)).expect("read corpus fixture")
}

// ---- edits ------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Edit {
    pub sheet: &'static str,
    pub op: &'static str,
    pub at: u32,
    pub count: u32,
    pub dest: Option<u32>,
}

impl Edit {
    pub fn insert_rows(sheet: &'static str, at: u32, count: u32) -> Self {
        Edit {
            sheet,
            op: "insert-rows",
            at,
            count,
            dest: None,
        }
    }
    pub fn delete_rows(sheet: &'static str, at: u32, count: u32) -> Self {
        Edit {
            sheet,
            op: "delete-rows",
            at,
            count,
            dest: None,
        }
    }
    pub fn insert_cols(sheet: &'static str, at: u32, count: u32) -> Self {
        Edit {
            sheet,
            op: "insert-cols",
            at,
            count,
            dest: None,
        }
    }
    pub fn move_rows(sheet: &'static str, at: u32, count: u32, dest: u32) -> Self {
        Edit {
            sheet,
            op: "move-rows",
            at,
            count,
            dest: Some(dest),
        }
    }
    /// The op flags shared by restructure and certify (so the two can never drift).
    fn flags(&self) -> Vec<String> {
        let mut v = vec![
            "--sheet".into(),
            self.sheet.into(),
            "--op".into(),
            self.op.into(),
            "--at".into(),
            self.at.to_string(),
            "--count".into(),
            self.count.to_string(),
        ];
        if let Some(d) = self.dest {
            v.push("--dest".into());
            v.push(d.to_string());
        }
        v
    }
}

/// A spread of edits every single-sheet corpus fixture supports.
pub fn faithful_edits(sheet: &'static str) -> Vec<Edit> {
    vec![
        Edit::insert_rows(sheet, 1, 1),
        Edit::insert_rows(sheet, 3, 2),
        Edit::delete_rows(sheet, 5, 1),
        Edit::insert_cols(sheet, 1, 1),
        Edit::move_rows(sheet, 2, 2, 8),
    ]
}

// ---- temp workbooks (isolated dir per file so restructure sidecars stay contained) --------

pub struct TempWb {
    dir: PathBuf,
    file: PathBuf,
}

impl Drop for TempWb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl TempWb {
    pub fn path(&self) -> &Path {
        &self.file
    }
    pub fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.file).expect("read temp workbook")
    }
}

fn unique_dir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "xlq-bb-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

pub fn temp_from_bytes(bytes: &[u8], filename: &str) -> TempWb {
    let dir = unique_dir();
    let file = dir.join(filename);
    std::fs::write(&file, bytes).expect("write temp workbook");
    TempWb { dir, file }
}

pub fn temp_from_corpus(name: &str) -> TempWb {
    temp_from_bytes(&corpus_bytes(name), name)
}

/// Produce xlq's OWN faithful transform of a corpus fixture: copy it to an isolated temp dir and
/// restructure it IN PLACE. The returned TempWb's file is then xlq's transform; the Run is the
/// restructure result (inspect `.json["residuals"]` to see if it refused).
pub fn transform(name: &str, edit: &Edit) -> (TempWb, Run) {
    let wb = temp_from_corpus(name);
    let path = wb.path().to_string_lossy().into_owned();
    let mut args = vec!["restructure", &path];
    let flags = edit.flags();
    let flag_refs: Vec<&str> = flags.iter().map(String::as_str).collect();
    args.extend(flag_refs.iter().copied());
    args.push("--actor");
    args.push("t");
    let run = xlq(&args);
    (wb, run)
}

/// Whether a restructure Run committed (no residuals, no error).
pub fn committed(run: &Run) -> bool {
    run.code == 0
        && run.json.get("error").is_none()
        && run
            .json
            .get("residuals")
            .and_then(Value::as_array)
            .map(|a| a.is_empty())
            .unwrap_or(true)
}

/// certify(orig, edited, edit).
pub fn certify(orig: &Path, edited: &Path, edit: &Edit) -> Run {
    let o = orig.to_string_lossy().into_owned();
    let e = edited.to_string_lossy().into_owned();
    let mut args = vec!["certify", &o, &e];
    let flags = edit.flags();
    let flag_refs: Vec<&str> = flags.iter().map(String::as_str).collect();
    args.extend(flag_refs.iter().copied());
    xlq(&args)
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Establish that xlq's transform of `name` under `edit` CERTIFIES, then return the (committed)
/// transform for further mutation. Panics if the transform refuses or the baseline fails to
/// certify (either would make a mangle assertion prove nothing).
pub fn certified_baseline(name: &str, edit: &Edit) -> TempWb {
    let (wb, run) = transform(name, edit);
    assert!(
        committed(&run),
        "{name}: transform must commit for the mangle baseline:\n{}",
        run.stdout
    );
    let base = certify(&corpus_path(name), wb.path(), edit);
    assert!(
        base.certified(),
        "{name}: the unmangled transform must CERTIFY (else the mangle proves nothing):\n{}",
        base.stdout
    );
    wb
}

/// Apply `mangler` to xlq's certified transform of `name` and assert certify now REFUSES — the
/// divergence is a value/security change certify must not launder. Rejects a no-op mangle.
pub fn assert_mangle_refused(name: &str, edit: &Edit, mangler: fn(&[u8]) -> Vec<u8>, label: &str) {
    let wb = certified_baseline(name, edit);
    let mangled = mangler(&wb.bytes());
    assert_ne!(
        mangled,
        wb.bytes(),
        "{name} [{label}]: mangle was a no-op — test is vacuous"
    );
    let mwb = temp_from_bytes(&mangled, name);
    let cert = certify(&corpus_path(name), mwb.path(), edit);
    assert!(
        cert.refused(),
        "{name} [{label}]: this divergence from xlq's transform MUST be REFUSED, got status={:?} \
         reason={:?}\n{}",
        cert.status(),
        cert.reason(),
        cert.stdout
    );
}

/// Apply a benign, value-preserving change and assert certify still CERTIFIES (over-refusal guard).
pub fn assert_variant_certifies(name: &str, edit: &Edit, f: fn(&[u8]) -> Vec<u8>, label: &str) {
    let wb = certified_baseline(name, edit);
    let variant = f(&wb.bytes());
    let vwb = temp_from_bytes(&variant, name);
    let cert = certify(&corpus_path(name), vwb.path(), edit);
    assert!(
        cert.certified(),
        "{name} [{label}]: a benign value-preserving change must still CERTIFY, got status={:?} \
         reason={:?}\n{}",
        cert.status(),
        cert.reason(),
        cert.stdout
    );
}

// ---- zip part surgery (shared by the mangle/benign libraries) ----------------------------

pub fn list_parts(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().to_string())
        .collect()
}

pub fn read_part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut f = z.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Rewrite a single part (deterministic mtime), preserving all others.
pub fn replace_part(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let n = f.name().to_string();
        out.start_file(&n, opts).unwrap();
        if n == name {
            out.write_all(data).unwrap();
        } else {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            out.write_all(&buf).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

pub fn add_part(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let n = f.name().to_string();
        out.start_file(&n, opts).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        out.write_all(&buf).unwrap();
    }
    out.start_file(name, opts).unwrap();
    out.write_all(data).unwrap();
    out.finish().unwrap().into_inner()
}

fn first_worksheet(bytes: &[u8]) -> String {
    list_parts(bytes)
        .into_iter()
        .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
        .min()
        .expect("a worksheet part")
}

fn edit_first_worksheet(bytes: &[u8], f: impl FnOnce(String) -> String) -> Vec<u8> {
    let part = first_worksheet(bytes);
    let xml = String::from_utf8(read_part(bytes, &part).expect("worksheet")).expect("utf8");
    let new = f(xml);
    replace_part(bytes, &part, new.as_bytes())
}

// ---- MANGLE LIBRARY — each must make certify REFUSE (a VALUE/SECURITY divergence) ---------

pub mod mangle {
    use super::*;

    /// Change the first LITERAL data value (`<v>` not preceded by `</f>`) — a value divergence.
    pub fn change_input_value(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            // Find a `<v>…</v>` whose preceding char is `>` from a `<c …>` open (not `</f>`).
            let mut result = xml.clone();
            let mut search = 0usize;
            while let Some(rel) = xml[search..].find("<v>") {
                let pos = search + rel;
                let before = &xml[..pos];
                if !before.ends_with("</f>") {
                    // literal cell value — bump it.
                    if let Some(end) = xml[pos + 3..].find("</v>") {
                        let val = &xml[pos + 3..pos + 3 + end];
                        let bumped = format!("{}9", val);
                        result = format!(
                            "{}<v>{}</v>{}",
                            &xml[..pos],
                            bumped,
                            &xml[pos + 3 + end + 4..]
                        );
                        return result;
                    }
                }
                search = pos + 3;
            }
            result
        })
    }

    /// Append `+1` to the first formula body — a formula divergence.
    pub fn rewrite_first_formula(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            if let Some(open) = xml.find("<f>") {
                if let Some(rel_close) = xml[open..].find("</f>") {
                    let close = open + rel_close;
                    let body = &xml[open + 3..close];
                    return format!("{}<f>{}+1</f>{}", &xml[..open], body, &xml[close + 4..]);
                }
            }
            xml
        })
    }

    /// Drop the first literal cell entirely — a `removed` divergence.
    pub fn remove_first_literal_cell(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            // Remove the first `<c r="…"><v>…</v></c>` that is a literal (no `<f>`).
            let mut search = 0usize;
            while let Some(rel) = xml[search..].find("<c ") {
                let open = search + rel;
                if let Some(rel_end) = xml[open..].find("</c>") {
                    let end = open + rel_end + 4;
                    let cell = &xml[open..end];
                    if cell.contains("<v>") && !cell.contains("<f>") {
                        return format!("{}{}", &xml[..open], &xml[end..]);
                    }
                    search = end;
                } else {
                    break;
                }
            }
            xml
        })
    }

    /// Repoint a connections.xml web-query URL — a SECURITY divergence (SSRF/exfiltration).
    pub fn repoint_connection(bytes: &[u8]) -> Vec<u8> {
        if let Some(raw) = read_part(bytes, "xl/connections.xml") {
            let xml = String::from_utf8_lossy(&raw)
                .replace("data.internal.example", "evil.attacker.example");
            return replace_part(bytes, "xl/connections.xml", xml.as_bytes());
        }
        bytes.to_vec()
    }

    /// Strip sheet protection — a security-control removal the cell diff cannot see.
    pub fn strip_sheet_protection(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            if let Some(open) = xml.find("<sheetProtection") {
                if let Some(rel) = xml[open..].find("/>") {
                    return format!("{}{}", &xml[..open], &xml[open + rel + 2..]);
                }
            }
            xml
        })
    }

    /// Strip workbook protection.
    pub fn strip_workbook_protection(bytes: &[u8]) -> Vec<u8> {
        if let Some(raw) = read_part(bytes, "xl/workbook.xml") {
            let xml = String::from_utf8_lossy(&raw);
            if let Some(open) = xml.find("<workbookProtection") {
                if let Some(rel) = xml[open..].find("/>") {
                    let new = format!("{}{}", &xml[..open], &xml[open + rel + 2..]);
                    return replace_part(bytes, "xl/workbook.xml", new.as_bytes());
                }
            }
        }
        bytes.to_vec()
    }

    /// Inject an `onLoad` autorun callback into the customUI ribbon — a security divergence.
    pub fn inject_customui_onload(bytes: &[u8]) -> Vec<u8> {
        let part = list_parts(bytes)
            .into_iter()
            .find(|n| n.to_ascii_lowercase().starts_with("customui/"));
        if let Some(part) = part {
            if let Some(raw) = read_part(bytes, &part) {
                let xml = String::from_utf8_lossy(&raw).replacen(
                    "<customUI ",
                    r#"<customUI onLoad="Evil" "#,
                    1,
                );
                return replace_part(bytes, &part, xml.as_bytes());
            }
        }
        bytes.to_vec()
    }

    /// Add an UNKNOWN reference-bearing part (an `externalLinks` part — it carries live cell
    /// coordinates and is outside certify's verified/known-safe allowlist) — certify must fail
    /// closed on any such part. (volatileDependencies is no longer usable here: it is a
    /// rebuildable cache restructure now drops and certify allowlists.)
    pub fn inject_unknown_part(bytes: &[u8]) -> Vec<u8> {
        add_part(
            bytes,
            "xl/externalLinks/externalLink1.xml",
            br#"<externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><externalBook><sheetDataSet><sheetData sheetId="0"><row r="1"><cell r="A1"><v>1</v></cell></row></sheetData></sheetDataSet></externalBook></externalLink>"#,
        )
    }

    /// Retarget a defined name's refers-to (robust to xlq having shifted the row) — a non-cell
    /// reference divergence.
    pub fn retarget_defined_name(bytes: &[u8]) -> Vec<u8> {
        if let Some(raw) = read_part(bytes, "xl/workbook.xml") {
            let xml = String::from_utf8_lossy(&raw).into_owned();
            if let Some(pos) = xml.find("!$A$") {
                let after = pos + "!$A$".len();
                let end = after
                    + xml[after..]
                        .find(|c: char| !c.is_ascii_digit())
                        .unwrap_or(0);
                if end > after {
                    let new = format!("{}99{}", &xml[..after], &xml[end..]);
                    return replace_part(bytes, "xl/workbook.xml", new.as_bytes());
                }
            }
        }
        bytes.to_vec()
    }

    /// Move a mergeCell's extent (C1:D1, above an insert@5, so it is unshifted) — a structural
    /// reference divergence.
    pub fn move_mergecell(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            xml.replacen(r#"ref="C1:D1""#, r#"ref="C2:D2""#, 1)
        })
    }

    /// Retarget an internal hyperlink's destination (robust to xlq having shifted the row) — a
    /// structural reference divergence.
    pub fn retarget_hyperlink(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            let anchor = r#"location="Sheet1!"#;
            if let Some(pos) = xml.find(anchor) {
                let vstart = pos + anchor.len();
                if let Some(rel) = xml[vstart..].find('"') {
                    let vend = vstart + rel;
                    return format!("{}Z99{}", &xml[..vstart], &xml[vend..]);
                }
            }
            xml
        })
    }

    /// Flip the date epoch (`workbookPr@date1904`) — every date value shifts 1462 days, invisible
    /// to a serial-vs-serial cell diff.
    pub fn flip_date1904(bytes: &[u8]) -> Vec<u8> {
        if let Some(raw) = read_part(bytes, "xl/workbook.xml") {
            let xml = String::from_utf8_lossy(&raw);
            let new = if xml.contains("<workbookPr") {
                xml.replacen("<workbookPr", r#"<workbookPr date1904="1""#, 1)
            } else {
                xml.replacen("<sheets>", r#"<workbookPr date1904="1"/><sheets>"#, 1)
            };
            return replace_part(bytes, "xl/workbook.xml", new.as_bytes());
        }
        bytes.to_vec()
    }
}

// ---- BENIGN LIBRARY — each must still CERTIFY (a cosmetic, value-preserving change) --------

pub mod benign {
    use super::*;

    /// Drop every formula cache (`<v>` after `</f>`) — what openpyxl does; Excel recomputes.
    pub fn strip_formula_caches(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            let mut out = xml.clone();
            // Remove `</f><v>…</v>` -> `</f>` repeatedly.
            while let Some(fpos) = out.find("</f><v>") {
                let vstart = fpos + 4; // at "<v>"
                let Some(rel) = out[vstart..].find("</v>") else {
                    break;
                };
                let vend = vstart + rel + 4;
                out = format!("{}{}", &out[..vstart], &out[vend..]);
            }
            out
        })
    }

    /// Insert cosmetic whitespace into the worksheet root tag — a benign reserialization.
    pub fn reserialize_whitespace(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| xml.replacen("<sheetData>", "<sheetData >", 1))
    }

    /// Materialize empty STYLE-ONLY cells (`<c r="A500" s="0"/>` …) as a new far row — what Excel
    /// / LibreOffice write for the covered cells of a merged, formatted header row. Value-less, so
    /// display-only: certify must still CERTIFY. Appended as a well-formed row past the data so it
    /// stays in row/column order.
    pub fn materialize_empty_styled_cells(bytes: &[u8]) -> Vec<u8> {
        edit_first_worksheet(bytes, |xml| {
            xml.replacen(
                "</sheetData>",
                r#"<row r="500"><c r="A500" s="0"/><c r="B500" s="0"/></row></sheetData>"#,
                1,
            )
        })
    }
}
