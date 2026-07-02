//! `xlq inspect` — privacy-safe workbook census.
//!
//! CONTRACT: pub fn run(path: &str, redact: bool) -> anyhow::Result<serde_json::Value>
//!
//! Output schema (documented in docs/census-spec.md; keep them in sync):
//! {
//!   "xlq": {"version": env!("CARGO_PKG_VERSION"), "command": "inspect"},
//!   "file": {"name": <basename only — never the full path>, "bytes": u64,
//!             "sha256": <hex>},
//!   "sheets": [{"name" (or "sheet_N" when redacted), "state", "rows", "cols",
//!               "cells", "formulas", "errors": {"#REF!": n, ...}}]
//!             (rows/cols are the populated extent; 0/0 for an empty sheet),
//!   "defined_names": <count only by default; names array only when !redact>,
//!   "functions": {"SUM": 120, ...}   (Excel vocabulary only; see census.rs),
//!   "unsupported_functions": [...],
//!   "volatile_functions": [...],
//!   "user_defined_calls": {"count": n, "call_sites": n,
//!                          "names": [...] only when !redact} — UDFs, add-in
//!                          functions, called LAMBDA defined names,
//!   "ooxml_parts": {"has_vba": bool, "has_pivot_cache": bool,
//!                    "has_external_links": bool, "has_charts": bool,
//!                    "has_comments": bool, "part_count": n},
//!   "coverage": {"engine": "ironcalc <ver>", "reliable": bool,
//!                "unsupported_features": [...] only when non-empty}
//! }
//!
//! Implementation notes:
//! - Load via ironcalc::import::load_from_xlsx(path, "en", "UTC", "en").
//!   When the load fails with "array formulas" (legacy CSE formulas make
//!   ironcalc return NotImplemented), retry from an in-memory normalized
//!   copy with the `t="array"` formula attribute stripped: the census does
//!   not evaluate, so the formula TEXT is all it needs. The census then
//!   reports coverage.unsupported_features = ["legacy array formulas (CSE)"]
//!   and coverage.reliable = false.
//! - Error tallies: model.get_all_cells() + get_cell_type()==ErrorValue +
//!   get_formatted_cell_value() for the error literal.
//! - ooxml_parts: open the file AGAIN as a zip (`zip` crate) and scan part
//!   names: xl/vbaProject.bin (vba), xl/pivotCache/ (pivot),
//!   xl/externalLinks/ (external links), xl/charts/ (charts),
//!   xl/comments* or xl/threadedComments/ (comments — modern Excel stores
//!   comments as threaded comments; legacy xl/commentsN.xml holds "notes").
//!   IronCalc drops parts it does not model, so the zip is the ground
//!   truth, not the Model.
//! - "coverage.reliable" is false when unsupported_functions is non-empty,
//!   when user-defined callables are present (the engine cannot evaluate
//!   them), or when unsupported_features is non-empty.
//! - PRIVACY INVARIANT: no cell values, no formula bodies, no full paths in
//!   the output. Sheet/defined names and user-defined callable names are
//!   structural metadata; `--redact` anonymizes/omits them for stricter
//!   policies. Error messages (which main.rs echoes into the stdout JSON
//!   payload) carry the file BASENAME only, never the full path.

use anyhow::{anyhow, Context, Result};
use ironcalc::base::types::CellType;
use ironcalc::base::Model;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<file>".to_string())
}

pub fn run(path: &str, redact: bool) -> Result<serde_json::Value> {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("path has no file name component"))?;

    let (model, unsupported_features) = load_model(path, &name)?;

    let bytes = std::fs::metadata(path)
        .with_context(|| format!("stat {name}"))?
        .len();
    let sha256 = crate::hash::sha256_file(path)?;

    let properties = model.get_worksheets_properties();
    let sheet_count = properties.len();
    let mut cell_counts = vec![0u64; sheet_count];
    let mut formula_counts = vec![0u64; sheet_count];
    let mut error_tallies: Vec<BTreeMap<String, u64>> = vec![BTreeMap::new(); sheet_count];

    for cell in model.get_all_cells() {
        let sheet = cell.index as usize;
        if sheet >= sheet_count {
            continue;
        }
        cell_counts[sheet] += 1;
        let has_formula = model
            .get_cell_formula(cell.index, cell.row, cell.column)
            .map_err(|e| anyhow!(e))?
            .is_some();
        if has_formula {
            formula_counts[sheet] += 1;
        }
        let cell_type = model
            .get_cell_type(cell.index, cell.row, cell.column)
            .map_err(|e| anyhow!(e))?;
        if matches!(cell_type, CellType::ErrorValue) {
            // Error literals ("#REF!", "#DIV/0!", ...) are not user data.
            let literal = model
                .get_formatted_cell_value(cell.index, cell.row, cell.column)
                .map_err(|e| anyhow!(e))?;
            *error_tallies[sheet].entry(literal).or_insert(0) += 1;
        }
    }

    let mut sheets = Vec::with_capacity(sheet_count);
    for (i, props) in properties.iter().enumerate() {
        let dimension = model
            .workbook
            .worksheet(i as u32)
            .map_err(|e| anyhow!(e))?
            .dimension();
        // dimension() reports 1/1 even for a sheet with no data; the spec
        // defines rows/cols as the populated extent, so an empty sheet is 0/0.
        let (rows, cols) = if cell_counts[i] == 0 {
            (0, 0)
        } else {
            (dimension.max_row, dimension.max_column)
        };
        let sheet_name = if redact {
            format!("sheet_{}", i + 1)
        } else {
            props.name.clone()
        };
        sheets.push(json!({
            "name": sheet_name,
            "state": props.state,
            "rows": rows,
            "cols": cols,
            "cells": cell_counts[i],
            "formulas": formula_counts[i],
            "errors": error_tallies[i],
        }));
    }

    let defined = &model.workbook.defined_names;
    let defined_names = if redact {
        json!({ "count": defined.len() })
    } else {
        json!({
            "count": defined.len(),
            "names": defined.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
        })
    };

    let census = crate::census::function_census(&model);
    let reliable = census.unsupported.is_empty()
        && census.user_defined.is_empty()
        && unsupported_features.is_empty();

    let user_defined_call_sites: u64 = census.user_defined.values().sum();
    let user_defined_calls = if redact {
        json!({
            "count": census.user_defined.len(),
            "call_sites": user_defined_call_sites,
        })
    } else {
        json!({
            "count": census.user_defined.len(),
            "call_sites": user_defined_call_sites,
            "names": census.user_defined.keys().collect::<Vec<_>>(),
        })
    };

    let mut coverage = json!({"engine": "ironcalc 0.7.1+e50ccea8 (vendored master)", "reliable": reliable});
    if !unsupported_features.is_empty() {
        coverage["unsupported_features"] = json!(unsupported_features);
    }

    Ok(json!({
        "xlq": {"version": env!("CARGO_PKG_VERSION"), "command": "inspect"},
        "file": {"name": name, "bytes": bytes, "sha256": sha256},
        "sheets": sheets,
        "defined_names": defined_names,
        "functions": census.tallies,
        "unsupported_functions": census.unsupported,
        "volatile_functions": census.volatile_present,
        "user_defined_calls": user_defined_calls,
        "ooxml_parts": ooxml_parts(path)?,
        "coverage": coverage,
    }))
}

/// Load the workbook. When ironcalc rejects it because of legacy CSE array
/// formulas (`<f t="array">` without `cm="1"`), retry from a normalized
/// in-memory copy so those workbooks — a prime audience for a compatibility
/// census — still get one. Returns the model plus the engine features the
/// workbook needs that the engine lacks.
fn load_model(path: &str, name: &str) -> Result<(Model<'static>, Vec<String>)> {
    match ironcalc::import::load_from_xlsx(path, "en", "UTC", "en") {
        Ok(model) => Ok((model, Vec::new())),
        Err(err) if err.to_string().contains("array formulas") => {
            let model = load_with_cse_normalized(path)
                .with_context(|| format!("load workbook {name} (CSE-normalized copy)"))?;
            Ok((model, vec!["legacy array formulas (CSE)".to_string()]))
        }
        Err(err) => Err(anyhow!(err)).with_context(|| format!("load workbook {name}")),
    }
}

/// Rewrite the worksheet XML so `<f t="array" …>` becomes a plain `<f …>`
/// and load the result from a temp copy. The census never evaluates, so the
/// formula text is all it needs; the original file is not touched.
fn load_with_cse_normalized(path: &str) -> Result<Model<'static>> {
    let file = std::fs::File::open(path).context("open workbook")?;
    let mut archive = zip::ZipArchive::new(file).context("read zip container")?;

    let tmp = std::env::temp_dir().join(format!(
        "xlq-cse-{}-{}.xlsx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    {
        let out = std::fs::File::create(&tmp).context("create temp workbook copy")?;
        let mut writer = zip::ZipWriter::new(out);
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).context("read zip entry")?;
            if entry.is_dir() {
                continue;
            }
            let entry_name = entry.name().to_string();
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut data).context("read zip entry data")?;
            if entry_name.starts_with("xl/worksheets/") && entry_name.ends_with(".xml") {
                let xml = String::from_utf8_lossy(&data).into_owned();
                data = strip_cse_array_attrs(&xml).into_bytes();
            }
            writer
                .start_file(entry_name, zip::write::SimpleFileOptions::default())
                .context("write zip entry header")?;
            writer.write_all(&data).context("write zip entry data")?;
        }
        writer.finish().context("finish temp workbook copy")?;
    }

    let result = ironcalc::import::load_from_xlsx(&tmp.to_string_lossy(), "en", "UTC", "en");
    let _ = std::fs::remove_file(&tmp);
    result.map_err(|e| anyhow!(e))
}

/// Remove ` t="array"` (and its single-quoted form) from `<f …>` tags only.
/// Scoped to the tag so cell text containing the same characters is safe.
fn strip_cse_array_attrs(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(pos) = rest.find("<f") {
        let (head, tail) = rest.split_at(pos);
        out.push_str(head);
        let after = &tail[2..];
        let starts_tag_with_attrs = after.chars().next().is_some_and(|c| c.is_whitespace());
        if starts_tag_with_attrs {
            if let Some(end) = tail.find('>') {
                let tag = &tail[..=end];
                out.push_str(&tag.replace(" t=\"array\"", "").replace(" t='array'", ""));
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push_str(&tail[..2]);
        rest = after;
    }
    out.push_str(rest);
    out
}

fn ooxml_parts(path: &str) -> Result<serde_json::Value> {
    let name = basename(path);
    let file = std::fs::File::open(path).with_context(|| format!("open {name}"))?;
    let archive =
        zip::ZipArchive::new(file).with_context(|| format!("read zip container {name}"))?;

    let mut has_vba = false;
    let mut has_pivot_cache = false;
    let mut has_external_links = false;
    let mut has_charts = false;
    let mut has_comments = false;
    for entry in archive.file_names() {
        has_vba |= entry == "xl/vbaProject.bin";
        has_pivot_cache |= entry.starts_with("xl/pivotCache/");
        has_external_links |= entry.starts_with("xl/externalLinks/");
        has_charts |= entry.starts_with("xl/charts/");
        // Legacy notes (xl/commentsN.xml) and modern threaded comments
        // (xl/threadedComments/…) both count as comment parts.
        has_comments |= entry.starts_with("xl/comments") || entry.starts_with("xl/threadedComments/");
    }

    Ok(json!({
        "has_vba": has_vba,
        "has_pivot_cache": has_pivot_cache,
        "has_external_links": has_external_links,
        "has_charts": has_charts,
        "has_comments": has_comments,
        "part_count": archive.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_temp(model: &Model, tag: &str) -> String {
        let dir = std::env::temp_dir().join("xlq-inspect-tests");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{tag}-{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path = path.to_string_lossy().into_owned();
        ironcalc::export::save_to_xlsx(model, &path).expect("save temp xlsx");
        path
    }

    fn sentinel_model() -> Model<'static> {
        let mut model = Model::new_empty("book", "en", "UTC", "en").expect("new model");
        model.add_sheet("Payroll").expect("add sheet");
        model
            .set_user_input(0, 1, 1, "SECRET_VALUE_XYZ".to_string())
            .expect("set value");
        model
            .set_user_input(0, 2, 1, "=CONCATENATE(\"SECRET_FORMULA_LIT\",A1)".to_string())
            .expect("set formula");
        model
            .set_user_input(1, 3, 2, "=1/0".to_string())
            .expect("set error formula");
        model
            .new_defined_name("SecretRegion", None, "Payroll!$B$3")
            .expect("defined name");
        model.evaluate();
        model
    }

    #[test]
    fn output_never_contains_values_formula_bodies_or_full_paths() {
        let model = sentinel_model();
        let path = save_temp(&model, "privacy");
        let parent = Path::new(&path)
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        for redact in [false, true] {
            let report = run(&path, redact).expect("inspect");
            let text = serde_json::to_string(&report).expect("serialize");
            assert!(!text.contains("SECRET_VALUE_XYZ"), "cell value leaked: {text}");
            assert!(!text.contains("SECRET_FORMULA_LIT"), "formula body leaked: {text}");
            assert!(!text.contains(&parent), "full path leaked: {text}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reports_structure_and_redaction() {
        let model = sentinel_model();
        let path = save_temp(&model, "structure");

        let report = run(&path, false).expect("inspect");
        assert_eq!(
            report["file"]["name"],
            json!(Path::new(&path).file_name().unwrap().to_string_lossy())
        );
        assert_eq!(report["file"]["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(report["sheets"][0]["name"], json!("Sheet1"));
        assert_eq!(report["sheets"][1]["name"], json!("Payroll"));
        assert_eq!(report["sheets"][0]["cells"], json!(2));
        assert_eq!(report["sheets"][0]["formulas"], json!(1));
        assert_eq!(report["sheets"][1]["rows"], json!(3));
        assert_eq!(report["sheets"][1]["cols"], json!(2));
        assert_eq!(report["sheets"][1]["errors"]["#DIV/0!"], json!(1));
        assert_eq!(report["defined_names"]["count"], json!(1));
        assert_eq!(report["defined_names"]["names"], json!(["SecretRegion"]));
        assert_eq!(report["ooxml_parts"]["has_vba"], json!(false));
        assert!(report["ooxml_parts"]["part_count"].as_u64().unwrap() > 0);
        assert_eq!(report["coverage"]["engine"], json!("ironcalc 0.7.1+e50ccea8 (vendored master)"));
        assert_eq!(report["xlq"]["command"], json!("inspect"));

        let redacted = run(&path, true).expect("inspect redacted");
        assert_eq!(redacted["sheets"][0]["name"], json!("sheet_1"));
        assert_eq!(redacted["sheets"][1]["name"], json!("sheet_2"));
        assert_eq!(redacted["defined_names"]["count"], json!(1));
        assert!(redacted["defined_names"].get("names").is_none());
        let text = serde_json::to_string(&redacted).unwrap();
        assert!(!text.contains("Payroll"));
        assert!(!text.contains("SecretRegion"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_error_payload_contains_no_directory_components() {
        // The error string is what main.rs echoes into the stdout JSON
        // payload, so it must never carry the directory part of the path.
        let missing = "/tmp/xlq-secret-client-acme/payroll.xlsx";
        let err = run(missing, false).expect_err("missing file must fail");
        let text = format!("{err:#}");
        assert!(
            !text.contains("xlq-secret-client-acme"),
            "directory leaked into error payload: {text}"
        );
        assert!(text.contains("payroll.xlsx"), "basename missing: {text}");
    }

    #[test]
    fn empty_sheet_reports_zero_extent() {
        let mut model = Model::new_empty("book", "en", "UTC", "en").expect("new model");
        model.add_sheet("Empty").expect("add sheet");
        model
            .set_user_input(0, 3, 2, "hello".to_string())
            .expect("set value");
        model.evaluate();
        let path = save_temp(&model, "empty-sheet");

        let report = run(&path, false).expect("inspect");
        assert_eq!(report["sheets"][0]["rows"], json!(3));
        assert_eq!(report["sheets"][0]["cols"], json!(2));
        assert_eq!(report["sheets"][1]["cells"], json!(0));
        assert_eq!(report["sheets"][1]["rows"], json!(0));
        assert_eq!(report["sheets"][1]["cols"], json!(0));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn udf_names_never_leak_into_functions_and_redact_hides_them() {
        let mut model = Model::new_empty("book", "en", "UTC", "en").expect("new model");
        model
            .set_user_input(0, 1, 1, "=DealMargin_AcmeCorp(B1)+SUM(1,2)".to_string())
            .expect("set formula");
        model.evaluate();
        let path = save_temp(&model, "udf");

        let report = run(&path, false).expect("inspect");
        assert_eq!(report["functions"], json!({"SUM": 1}));
        assert_eq!(report["unsupported_functions"], json!([]));
        assert_eq!(report["user_defined_calls"]["count"], json!(1));
        assert_eq!(report["user_defined_calls"]["call_sites"], json!(1));
        assert_eq!(
            report["user_defined_calls"]["names"],
            json!(["DEALMARGIN_ACMECORP"])
        );
        // The engine cannot evaluate a UDF: the census must not claim it can.
        assert_eq!(report["coverage"]["reliable"], json!(false));

        let redacted = run(&path, true).expect("inspect redacted");
        assert_eq!(redacted["user_defined_calls"]["count"], json!(1));
        assert!(redacted["user_defined_calls"].get("names").is_none());
        let text = serde_json::to_string(&redacted).unwrap();
        assert!(
            !text.to_uppercase().contains("DEALMARGIN"),
            "UDF name leaked in redact mode: {text}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsupported_function_inside_defined_name_breaks_reliable() {
        let mut model = Model::new_empty("book", "en", "UTC", "en").expect("new model");
        // Function-bearing defined names arrive via import; push directly.
        model.workbook.defined_names.push(ironcalc::base::types::DefinedName {
            name: "HiddenCalc".to_string(),
            formula: "CUBEVALUE(Sheet1!$A$1:$A$2)".to_string(),
            sheet_id: None,
        });
        model
            .set_user_input(0, 1, 1, "=MIN(1,160)".to_string())
            .expect("set formula");
        model.evaluate();
        let path = save_temp(&model, "dn-unsupported");

        let report = run(&path, false).expect("inspect");
        assert_eq!(report["unsupported_functions"], json!(["CUBEVALUE"]));
        assert_eq!(report["coverage"]["reliable"], json!(false));

        let _ = std::fs::remove_file(&path);
    }

    /// Rewrite one part of a saved xlsx (or add a new one) and re-zip it.
    fn patch_zip_part(path: &str, part: &str, transform: impl Fn(Option<String>) -> String) {
        let bytes = std::fs::read(path).expect("read xlsx");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open xlsx zip");
        let out = std::fs::File::create(path).expect("rewrite xlsx");
        let mut writer = zip::ZipWriter::new(out);
        let mut found = false;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).expect("zip entry");
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).expect("read entry");
            if name == part {
                found = true;
                data = transform(Some(String::from_utf8(data).expect("utf8 part"))).into_bytes();
            }
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start file");
            writer.write_all(&data).expect("write entry");
        }
        if !found {
            writer
                .start_file(part.to_string(), zip::write::SimpleFileOptions::default())
                .expect("start new file");
            writer
                .write_all(transform(None).as_bytes())
                .expect("write new entry");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn legacy_cse_array_formula_still_gets_a_census() {
        let mut model = Model::new_empty("book", "en", "UTC", "en").expect("new model");
        model.set_user_input(0, 1, 1, "42".to_string()).expect("set value");
        model
            .set_user_input(0, 1, 2, "=MIN(A1,160)".to_string())
            .expect("set formula");
        model.evaluate();
        let path = save_temp(&model, "cse-array");

        // Turn B1's formula into a legacy CSE array formula. Engines at or
        // before ironcalc 0.7.1 rejected these with NotImplemented("array
        // formulas") and inspect fell back to a stripped temp copy; the
        // vendored master build (spill support landed upstream) loads them
        // natively, so the census must succeed either way — the fallback
        // stays in place as a guard for whatever the engine rejects next.
        patch_zip_part(&path, "xl/worksheets/sheet1.xml", |xml| {
            let xml = xml.expect("sheet1.xml present");
            assert!(xml.contains("<f>"), "expected a plain formula tag: {xml}");
            xml.replacen("<f>", "<f t=\"array\" ref=\"B1:B1\">", 1)
        });
        let loads_natively = ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en").is_ok();

        let report = run(&path, false).expect("inspect must survive CSE formulas");
        assert_eq!(report["functions"], json!({"MIN": 1}));
        assert_eq!(report["sheets"][0]["formulas"], json!(1));
        if loads_natively {
            assert_eq!(report["coverage"]["reliable"], json!(true));
        } else {
            assert_eq!(
                report["coverage"]["unsupported_features"],
                json!(["legacy array formulas (CSE)"])
            );
            assert_eq!(report["coverage"]["reliable"], json!(false));
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn threaded_comments_count_as_comments() {
        let model = sentinel_model();
        let path = save_temp(&model, "threaded-comments");

        let before = run(&path, false).expect("inspect");
        assert_eq!(before["ooxml_parts"]["has_comments"], json!(false));

        patch_zip_part(&path, "xl/threadedComments/threadedComment1.xml", |_| {
            "<?xml version=\"1.0\"?><ThreadedComments/>".to_string()
        });
        let after = run(&path, false).expect("inspect");
        assert_eq!(after["ooxml_parts"]["has_comments"], json!(true));

        let _ = std::fs::remove_file(&path);
    }
}
