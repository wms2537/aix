//! In-crate TEST HARNESS (compiled only under `cfg(test)`).
//!
//! xlq is a BINARY-ONLY crate (no lib target), so its internals — `structural::structural_edit`,
//! `diff::snapshot`, `census::function_census`, `refshift` — are unreachable from `tests/*.rs`.
//! The strongest value oracles need those internals, so they live here and are called from the
//! in-crate `#[cfg(test)] mod tests_*` property modules. Black-box (binary-shelling) tests use
//! `tests/common/mod.rs` instead.
//!
//! The centrepiece is [`cache_soundness`]: the STORED-vs-EVALUATED cache oracle. A structural
//! edit changes computed values, so any formula cache the output still carries must equal the
//! engine's recomputation of that output — otherwise a cache reader (Excel without
//! `fullCalcOnLoad`, openpyxl `data_only`, pandas) sees a stale value. This is the property that
//! a blank-cache fixture could never exercise, which is why the flagship stale-cache HIGH bug
//! survived 31 rounds; the corpus under `tests/fixtures/corpus/` carries POPULATED caches so it
//! can.

// A shared test HARNESS: not every property module consumes every helper/field, so unused-API
// warnings here are expected and would otherwise trip `clippy -D warnings` on --all-targets.
#![allow(dead_code)]

use crate::refshift::{shift_index, Axis, Op, StructuralEdit};
use crate::structural::{self, StructuralReport};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A committed corpus workbook plus the faithful edits it supports. `ironcalc_faithful` is true
/// when the engine's evaluation agrees with the stored caches (a clean value baseline); it is
/// false for fixtures that intentionally carry constructs the vendored engine cannot resolve
/// (so the value-differential properties skip them, while reference/reflexivity properties still
/// run).
pub(crate) struct CorpusCase {
    pub(crate) name: &'static str,
    pub(crate) bytes: Vec<u8>,
    pub(crate) edit_sheet: &'static str,
    pub(crate) faithful_edits: Vec<StructuralEdit>,
    pub(crate) constructs: Vec<&'static str>,
    pub(crate) ironcalc_faithful: bool,
}

fn read_corpus(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/corpus/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("read corpus fixture {path}: {e}"))
}

fn row(sheet: &str, op: Op, at: u32, count: u32) -> StructuralEdit {
    StructuralEdit {
        axis: Axis::Row,
        at,
        count,
        op,
        sheet: sheet.to_string(),
        dest: 0,
    }
}

fn col(sheet: &str, op: Op, at: u32, count: u32) -> StructuralEdit {
    StructuralEdit {
        axis: Axis::Col,
        at,
        count,
        op,
        sheet: sheet.to_string(),
        dest: 0,
    }
}

fn move_rows(sheet: &str, at: u32, count: u32, dest: u32) -> StructuralEdit {
    StructuralEdit {
        axis: Axis::Row,
        at,
        count,
        op: Op::Move,
        sheet: sheet.to_string(),
        dest,
    }
}

/// A spread of edits that AFFECT the data band (so caches would go stale if not invalidated):
/// inserts and deletes above/inside the band, plus a move.
fn band_edits(sheet: &str) -> Vec<StructuralEdit> {
    vec![
        row(sheet, Op::Insert, 1, 1),
        row(sheet, Op::Insert, 3, 2),
        row(sheet, Op::Insert, 5, 1),
        row(sheet, Op::Delete, 5, 1),
        row(sheet, Op::Delete, 2, 3),
        col(sheet, Op::Insert, 1, 1),
        move_rows(sheet, 2, 2, 8),
    ]
}

/// The committed corpus, read fresh each call. Every case except an intentionally engine-
/// unfaithful one carries POPULATED, engine-agreeing caches.
pub(crate) fn corpus() -> Vec<CorpusCase> {
    vec![
        CorpusCase {
            name: "sum_band.xlsx",
            bytes: read_corpus("sum_band.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: band_edits("Sheet1"),
            constructs: vec![
                "sum-range",
                "absolute-ref",
                "straddle-range",
                "populated-cache",
            ],
            ironcalc_faithful: true,
        },
        CorpusCase {
            name: "crosssheet.xlsx",
            bytes: read_corpus("crosssheet.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: band_edits("Sheet1"),
            constructs: vec!["cross-sheet-ref", "cross-sheet-sum", "populated-cache"],
            ironcalc_faithful: true,
        },
        CorpusCase {
            name: "settings.xlsx",
            bytes: read_corpus("settings.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: vec![
                row("Sheet1", Op::Insert, 5, 1),
                row("Sheet1", Op::Insert, 1, 1),
            ],
            constructs: vec![
                "precision-as-displayed",
                "downstream-formula",
                "populated-cache",
            ],
            ironcalc_faithful: true,
        },
        CorpusCase {
            name: "names.xlsx",
            bytes: read_corpus("names.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: band_edits("Sheet1"),
            constructs: vec!["defined-name", "name-shift", "populated-cache"],
            ironcalc_faithful: true,
        },
        CorpusCase {
            name: "security.xlsx",
            bytes: read_corpus("security.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: band_edits("Sheet1"),
            constructs: vec!["connections", "protection", "customui", "populated-cache"],
            ironcalc_faithful: true,
        },
        CorpusCase {
            name: "constructs.xlsx",
            bytes: read_corpus("constructs.xlsx"),
            edit_sheet: "Sheet1",
            faithful_edits: band_edits("Sheet1"),
            constructs: vec![
                "merge-cells",
                "data-validation",
                "hyperlink",
                "defined-name",
                "populated-cache",
            ],
            ironcalc_faithful: true,
        },
    ]
}

/// Run xlq's proven structural transform on `bytes`.
pub(crate) fn transform(
    bytes: &[u8],
    edit: &StructuralEdit,
) -> Result<(Vec<u8>, StructuralReport)> {
    structural::structural_edit(bytes, edit)
}

/// A uniquely-named temp file that deletes itself on drop — bridges the byte APIs to the
/// PATH-only engine loader. Unique per process + counter so parallel tests never collide.
pub(crate) struct TempGuard(PathBuf);
impl TempGuard {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) fn write_temp(bytes: &[u8]) -> TempGuard {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "xlq-testkit-{}-{}.xlsx",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::write(&path, bytes).expect("write temp workbook");
    TempGuard(path)
}

fn load(bytes: &[u8]) -> Result<ironcalc::base::Model<'static>> {
    let g = write_temp(bytes);
    ironcalc::import::load_from_xlsx(&g.path().to_string_lossy(), "en", "UTC", "en")
        .map_err(|e| anyhow!("load workbook: {e}"))
}

/// Two cache signatures (`type:value`) are equal, tolerating a benign numeric renumber
/// (`n:55` vs `n:55.0`) but not a type change (`n:55` vs `str:55`). Mirrors certify's own
/// `caches_equal` so the property tests the same equivalence production uses.
fn caches_equal(a: &str, b: &str) -> bool {
    let (ta, va) = a.split_once(':').unwrap_or(("n", a));
    let (tb, vb) = b.split_once(':').unwrap_or(("n", b));
    ta == tb
        && (va == vb
            || matches!((va.parse::<f64>(), vb.parse::<f64>()), (Ok(x), Ok(y)) if nums_equal_at_excel_precision(x, y)))
}

/// Mirrors certify's 15-significant-figure equality (Excel's storage precision) so a cache
/// carrying IEEE-754 rounding noise is not treated as a difference.
fn nums_equal_at_excel_precision(x: f64, y: f64) -> bool {
    if x == y {
        return true;
    }
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    let round15 = |v: f64| {
        if v == 0.0 {
            "0e0".to_string()
        } else {
            format!("{v:.14e}")
        }
    };
    round15(x) == round15(y)
}

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
            | "#ERROR!"
    )
}

/// True when the engine cannot be trusted to reproduce Excel's values for this workbook — an
/// unsupported / policy-limited / user-defined function, a VOLATILE function (whose value
/// depends on the clock/RNG), or precision-as-displayed (`fullPrecision="0"`, where Excel
/// computes on rounded displayed values but the engine computes at full precision). The cache
/// oracle SKIPS such a workbook rather than risk a false verdict.
fn engine_unverifiable(bytes: &[u8], model: &ironcalc::base::Model) -> bool {
    let c = crate::census::function_census(model);
    !(c.unsupported.is_empty()
        && c.policy_limited.is_empty()
        && c.user_defined.is_empty()
        && c.volatile_present.is_empty())
        || full_precision(bytes)
}

/// Whether the workbook forces precision-as-displayed (`<calcPr fullPrecision="0">`).
fn full_precision(bytes: &[u8]) -> bool {
    let Ok(x) = crate::ooxml::read_part(bytes, "xl/workbook.xml") else {
        return false;
    };
    let text = String::from_utf8_lossy(&x);
    // crude but sufficient for the corpus: a fullPrecision attribute set to 0/false.
    text.contains("fullPrecision=\"0\"") || text.contains("fullPrecision='0'")
}

/// A concrete cache-soundness violation for a readable message.
#[derive(Debug)]
pub(crate) struct CacheMismatch(pub(crate) String);

/// THE FLAGSHIP ORACLE. Every formula cache that `output` still carries must equal the engine's
/// recomputation of `output`; otherwise the file misrepresents a computed value to any reader
/// that does not itself recompute. A correctly transformed workbook carries NO formula caches
/// (xlq blanks them all), so this holds vacuously — but a regression that copied a stale cache
/// verbatim makes a surviving cache disagree with the recomputation and is caught. Returns Ok
/// (SKIP) when the engine cannot faithfully reproduce the values (see [`engine_unverifiable`]).
pub(crate) fn cache_soundness(output: &[u8]) -> Result<(), CacheMismatch> {
    use ironcalc::base::cell::CellValue;
    let mut model = load(output).map_err(|e| CacheMismatch(format!("load output: {e}")))?;
    if engine_unverifiable(output, &model) {
        return Ok(());
    }
    // Recompute every value. The STORED cache is read from the raw XML `<v>` (a blanked formula
    // cell reads back from the engine as `#ERROR!`, NOT null, so the model snapshot is the WRONG
    // stored-cache reader — read the bytes, exactly as certify does).
    model.evaluate();
    let names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();
    // Evaluated signature per (sheet, A1), rendered like formula_cache_map's `type:value`.
    let mut eval: std::collections::HashMap<(String, String), String> =
        std::collections::HashMap::new();
    for cell in model.get_all_cells() {
        if !matches!(
            model.get_cell_formula(cell.index, cell.row, cell.column),
            Ok(Some(_))
        ) {
            continue;
        }
        let Some(name) = names.get(cell.index as usize) else {
            continue;
        };
        let Ok(a1) = crate::diff::a1(cell.row, cell.column) else {
            continue;
        };
        let sig = match model.get_cell_value_by_index(cell.index, cell.row, cell.column) {
            Ok(CellValue::Number(n)) => format!("n:{n}"),
            Ok(CellValue::Boolean(b)) => format!("b:{}", if b { "1" } else { "0" }),
            Ok(CellValue::String(s)) if is_excel_error(&s) => format!("e:{s}"),
            Ok(CellValue::String(s)) => format!("str:{s}"),
            _ => continue,
        };
        eval.insert((name.clone(), a1), sig);
    }
    // Compare each STORED cache (raw XML) against the recomputation.
    let sheets = crate::ooxml::all_sheets(output)
        .map_err(|e| CacheMismatch(format!("enumerate sheets: {e}")))?;
    for (name, part) in &sheets {
        let Ok(xml) = crate::ooxml::read_part(output, part) else {
            continue;
        };
        for (a1, stored) in structural::formula_cache_map(&xml) {
            match eval.get(&(name.clone(), a1.clone())) {
                Some(ev) if caches_equal(&stored, ev) => {}
                other => {
                    return Err(CacheMismatch(format!(
                        "{name}!{a1} carries a STALE cache: stored {stored:?} but the engine \
                         recomputes {:?}",
                        other
                    )));
                }
            }
        }
    }
    Ok(())
}

/// The image of a 1-based cell `(row, col)` on `sheet` under this edit's σ: on the EDITED sheet
/// the affected axis moves (None if the line is deleted / pushed off-grid); a cell on any other
/// sheet keeps its position (its VALUE may still change through a cross-reference, which the
/// value-differential accounts for by comparing values at the mapped position).
pub(crate) fn sigma(sheet: &str, row: i32, col: i32, edit: &StructuralEdit) -> Option<(i32, i32)> {
    if !sheet.eq_ignore_ascii_case(&edit.sheet) {
        return Some((row, col));
    }
    match edit.axis {
        Axis::Row => shift_index(row as u32, edit).map(|r| (r as i32, col)),
        Axis::Col => shift_index(col as u32, edit).map(|c| (row, c as i32)),
    }
}

fn is_error_raw(v: &Value) -> bool {
    matches!(v, Value::String(s) if s.starts_with('#'))
}

/// Two raw evaluated values are equal, tolerating a benign numeric renumber (`55` vs `55.0`).
fn raw_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => matches!(
            (x.as_f64(), y.as_f64()),
            (Some(p), Some(q)) if p == q
        ),
        _ => a == b,
    }
}

/// A concrete value-faithfulness violation.
#[derive(Debug)]
pub(crate) struct ValueDrift(pub(crate) String);

/// True if any formula in the workbook is POSITION-INTRINSIC — its value depends on WHERE a cell
/// or range sits (`ROW`/`COLUMN`/`ROWS`/`COLUMNS`/`OFFSET`/`INDIRECT`/`ADDRESS`/`AREAS`) — so a
/// structural edit legitimately changes its value and the value-differential must not apply.
fn has_position_intrinsic_fns(bytes: &[u8]) -> bool {
    const INTR: &[&str] = &[
        "ROW(",
        "ROWS(",
        "COLUMN(",
        "COLUMNS(",
        "OFFSET(",
        "INDIRECT(",
        "ADDRESS(",
        "AREAS(",
    ];
    let Ok(sheets) = crate::ooxml::all_sheets(bytes) else {
        return true; // can't tell -> assume yes (skip the property)
    };
    for (_, part) in sheets {
        if let Ok(xml) = crate::ooxml::read_part(bytes, &part) {
            let up = String::from_utf8_lossy(&xml).to_uppercase();
            if INTR.iter().any(|f| up.contains(f)) {
                return true;
            }
        }
    }
    false
}

/// VALUE-DIFFERENTIAL: an INSERT or MOVE preserves every computed value, merely relocating the
/// edited sheet's cells under σ (an inserted row is blank — contributing 0 to a sum — and a move
/// is a pure permutation, so no datum that feeds a formula is added or removed). Evaluate the
/// input and the output, then for each clean input value assert the output holds an EQUAL clean
/// value at the mapped position — so a σ MIS-shift (a straddling range that changes a SUM, a name
/// whose cell-shaped tail was wrongly shifted turning the value into `#NAME?`, an off-grid
/// materialization) is caught even though the output's blanked caches reveal nothing.
///
/// NOT applicable to DELETE (removing a data row legitimately changes a SUM) or to a workbook with
/// position-intrinsic functions (whose value depends on absolute position); both return Ok (skip),
/// as does an engine-unverifiable workbook.
pub(crate) fn value_faithful(
    input: &[u8],
    edit: &StructuralEdit,
    output: &[u8],
) -> Result<(), ValueDrift> {
    if edit.op == Op::Delete || has_position_intrinsic_fns(input) {
        return Ok(());
    }
    let mut m0 = load(input).map_err(|e| ValueDrift(format!("load input: {e}")))?;
    if engine_unverifiable(input, &m0) {
        return Ok(());
    }
    m0.evaluate();
    let s0 = crate::diff::snapshot(&m0).map_err(|e| ValueDrift(format!("input snapshot: {e}")))?;
    let mut m1 = load(output).map_err(|e| ValueDrift(format!("load output: {e}")))?;
    m1.evaluate();
    let s1 = crate::diff::snapshot(&m1).map_err(|e| ValueDrift(format!("output snapshot: {e}")))?;

    for (sheet, cells) in &s0 {
        for ((r, c), snap) in cells {
            // Only clean (non-error, populated) input values are subject to the invariant.
            if snap.raw.is_null() || is_error_raw(&snap.raw) {
                continue;
            }
            let Some((r1, c1)) = sigma(sheet, *r, *c, edit) else {
                continue; // pushed off-grid by an insert — an overflow, not a value drift
            };
            let out = s1.get(sheet).and_then(|m| m.get(&(r1, c1)));
            match out {
                Some(o) if raw_eq(&snap.raw, &o.raw) => {}
                other => {
                    return Err(ValueDrift(format!(
                        "{sheet}!({r},{c}) value {:?} must relocate to ({r1},{c1}) under σ, but the \
                         output there holds {:?} (edit {:?})",
                        snap.raw,
                        other.map(|o| &o.raw),
                        edit.op
                    )));
                }
            }
        }
    }
    Ok(())
}

fn parse_a1(s: &str) -> Option<(u32, u32)> {
    let letters: String = s.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits: String = s
        .chars()
        .skip_while(char::is_ascii_alphabetic)
        .take_while(char::is_ascii_digit)
        .collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col = 0u32;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    Some((col, digits.parse().ok()?))
}

/// OUTPUT WELL-FORMEDNESS: every `<c r=…>` coordinate a transform emits must be a valid A1 cell
/// INSIDE the grid (col ≤ XFD/16384, row ≤ 1048576), and no two cells on a sheet may share a
/// coordinate. Catches an off-grid materialization (`<c r="A1048577">`) or a duplicate-coordinate
/// interior delete — invalid outputs a downstream tool would choke on.
pub(crate) fn wellformed(output: &[u8]) -> Result<(), String> {
    let sheets = crate::ooxml::all_sheets(output).map_err(|e| e.to_string())?;
    for (name, part) in &sheets {
        let xml = crate::ooxml::read_part(output, part).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&xml);
        let mut seen = std::collections::HashSet::new();
        let mut rest: &str = &text;
        while let Some(p) = rest.find("<c r=\"") {
            rest = &rest[p + 6..];
            let end = rest.find('"').ok_or("unterminated cell ref")?;
            let cell = &rest[..end];
            let (col, row) =
                parse_a1(cell).ok_or_else(|| format!("{name}: malformed cell ref {cell:?}"))?;
            if col == 0 || col > 16384 || row == 0 || row > 1_048_576 {
                return Err(format!(
                    "{name}: OFF-GRID cell {cell} (col {col}, row {row})"
                ));
            }
            if !seen.insert((col, row)) {
                return Err(format!("{name}: DUPLICATE coordinate {cell}"));
            }
            rest = &rest[end..];
        }
    }
    Ok(())
}

/// Count of formula cells in `bytes` (across all worksheets) that carry a PRESENT, non-empty
/// stored `<v>` cache. Used to prove a corpus fixture actually exercises populated caches, and
/// that a transform's output blanked them.
pub(crate) fn populated_cache_count(bytes: &[u8]) -> usize {
    let mut total = 0;
    if let Ok(sheets) = crate::ooxml::all_sheets(bytes) {
        for (_, part) in sheets {
            if let Ok(xml) = crate::ooxml::read_part(bytes, &part) {
                total += structural::formula_cache_map(&xml).len();
            }
        }
    }
    total
}

/// Plant a stale cache into a workbook: give the first cache-less formula cell on `sheet` a wrong
/// stored `<v>`. Used by the poisoned-cache guard to prove [`cache_soundness`] actually detects
/// staleness rather than passing vacuously.
pub(crate) fn plant_stale_cache(output: &[u8], wrong_value: &str) -> Result<Vec<u8>> {
    let sheets = crate::ooxml::all_sheets(output)?;
    let (_, part) = sheets
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no worksheet"))?;
    let xml = crate::ooxml::read_part(output, &part)?;
    let text = String::from_utf8(xml).context("worksheet utf8")?;
    // Turn the first `<f>…</f></c>` (a formula cell with no cache) into `<f>…</f><v>WRONG</v></c>`.
    let needle = "</f></c>";
    let poisoned = text.replacen(needle, &format!("</f><v>{wrong_value}</v></c>"), 1);
    if poisoned == text {
        return Err(anyhow!("no cache-less formula cell to poison"));
    }
    replace_part(output, &part, poisoned.as_bytes())
}

/// Rewrite a single zip part, preserving all others. (Test-only; deterministic mtime.)
pub(crate) fn replace_part(bytes: &[u8], part: &str, new_data: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name().to_string();
        out.start_file(&name, opts)?;
        if name == part {
            out.write_all(new_data)?;
        } else {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            out.write_all(&buf)?;
        }
    }
    Ok(out.finish()?.into_inner())
}
