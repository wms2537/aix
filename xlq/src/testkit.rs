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

use crate::refshift::{Axis, Op, StructuralEdit};
use crate::structural::{self, StructuralReport};
use anyhow::{anyhow, Context, Result};
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
        && (va == vb || matches!((va.parse::<f64>(), vb.parse::<f64>()), (Ok(x), Ok(y)) if x == y))
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
