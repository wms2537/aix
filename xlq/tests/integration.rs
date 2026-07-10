//! End-to-end integration tests.
//!
//! Generates the five fixture workbooks ONCE (shared across tests via
//! `OnceLock`) by shelling out to the compiled `xlq-fixtures` binary, then
//! exercises the compiled `xlq` binary (`inspect`, `diff`, `calc`) against
//! them — the same checks a human would run by hand:
//!
//!   cargo run --bin xlq-fixtures -- <dir>   2> <dir>/planted-defects.json
//!   cargo run --bin xlq -- inspect <dir>/branch-consolidation.xlsx
//!   cargo run --bin xlq -- inspect --redact <dir>/branch-consolidation.xlsx
//!   cargo run --bin xlq -- diff <a.xlsx> <b.xlsx>
//!   cargo run --bin xlq -- calc <dir>/payroll.xlsx

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Generate fixtures exactly once into a per-run temp directory and capture
/// the planted-defect manifest that the generator prints on stderr.
fn fixtures_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("xlq-integration-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create fixtures temp dir");
        let out = Command::new(env!("CARGO_BIN_EXE_xlq-fixtures"))
            .arg(&dir)
            .output()
            .expect("spawn xlq-fixtures");
        assert!(
            out.status.success(),
            "xlq-fixtures failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // The generator's contract: planted-defect JSON goes to stderr.
        std::fs::write(dir.join("planted-defects.json"), &out.stderr)
            .expect("write planted-defects.json");
        dir
    })
}

fn fixture(name: &str) -> String {
    fixtures_dir()
        .join(name)
        .to_str()
        .expect("utf8 path")
        .to_owned()
}

/// Run the xlq binary; assert success; return (parsed JSON, raw stdout).
fn xlq(args: &[&str]) -> (Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xlq"))
        .args(args)
        .output()
        .expect("spawn xlq");
    assert!(
        out.status.success(),
        "xlq {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout is utf8");
    let json: Value = serde_json::from_str(&stdout).expect("stdout parses as JSON");
    (json, stdout)
}

#[test]
fn failing_command_exits_one_with_json_error_payload_and_no_full_paths() {
    // main.rs error path: exit code 1, human diagnostic on stderr, and a
    // machine-readable {"error": ...} JSON payload on stdout that carries
    // the file BASENAME only — never the directory.
    let out = Command::new(env!("CARGO_BIN_EXE_xlq"))
        .args(["inspect", "/tmp/xlq-secret-client-dir/missing.xlsx"])
        .output()
        .expect("spawn xlq");
    assert_eq!(out.status.code(), Some(1), "failure must exit 1");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("xlq error:"),
        "stderr diagnostic missing: {stderr}"
    );

    let stdout = String::from_utf8(out.stdout).expect("stdout is utf8");
    let json: Value = serde_json::from_str(&stdout).expect("error payload parses as JSON");
    let message = json["error"].as_str().expect("error key present");
    assert!(
        message.contains("missing.xlsx"),
        "basename missing: {message}"
    );
    assert!(
        !stdout.contains("xlq-secret-client-dir"),
        "directory leaked into stdout payload: {stdout}"
    );
}

#[test]
fn fixtures_generate_all_files_and_defect_manifest() {
    let dir = fixtures_dir();
    for name in [
        "branch-consolidation.xlsx",
        "stock-reconciliation.xlsx",
        "payroll.xlsx",
        "claims.xlsx",
        "perf-large.xlsx",
    ] {
        assert!(dir.join(name).is_file(), "missing fixture {name}");
    }
    let manifest = std::fs::read_to_string(dir.join("planted-defects.json"))
        .expect("read planted-defects.json");
    let json: Value = serde_json::from_str(&manifest).expect("defect manifest is JSON");
    let defects = json["defects"].as_array().expect("defects array");
    assert!(!defects.is_empty(), "no planted defects reported");
    for d in defects {
        for key in ["file", "sheet", "row", "col", "kind"] {
            assert!(!d[key].is_null(), "defect missing key {key}: {d}");
        }
    }
    // The planted #DIV/0! from the spec must be in the manifest.
    assert!(
        defects
            .iter()
            .any(|d| d["file"] == "branch-consolidation.xlsx"
                && d["sheet"] == "Branch3"
                && d["kind"].as_str().unwrap_or("").contains("div0")),
        "Branch3 div0 defect missing from manifest"
    );
}

#[test]
fn inspect_reports_functions_and_planted_div0_without_leaking_values() {
    let path = fixture("branch-consolidation.xlsx");
    let (json, stdout) = xlq(&["inspect", &path]);

    assert_eq!(json["xlq"]["command"], "inspect");
    let functions = json["functions"].as_object().expect("functions object");
    assert!(!functions.is_empty(), "function tally is empty");
    assert!(functions.contains_key("SUM"), "SUM missing from tally");
    assert_eq!(json["unsupported_functions"], serde_json::json!([]));

    // The planted #DIV/0! (Branch3!H16) must show in that sheet's error map.
    let sheets = json["sheets"].as_array().expect("sheets array");
    let branch3 = sheets
        .iter()
        .find(|s| s["name"] == "Branch3")
        .expect("Branch3 sheet present");
    assert_eq!(
        branch3["errors"]["#DIV/0!"], 1,
        "planted #DIV/0! not reported"
    );

    // Privacy: inspect output must never contain cell values. Read known
    // values straight out of the workbook and grep the raw output for them.
    let model = ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en").expect("load fixture");
    let b2 = model.get_formatted_cell_value(0, 2, 2).expect("Branch1!B2");
    let a2 = model.get_formatted_cell_value(0, 2, 1).expect("Branch1!A2");
    assert!(
        !b2.is_empty() && !a2.is_empty(),
        "sentinel cells unexpectedly empty"
    );
    for leaked in [&b2, &a2] {
        assert!(
            !stdout.contains(leaked.as_str()),
            "inspect output leaked cell value {leaked:?}"
        );
    }
}

#[test]
fn inspect_redact_anonymizes_sheet_and_defined_names() {
    let path = fixture("branch-consolidation.xlsx");
    let (json, _) = xlq(&["inspect", "--redact", &path]);
    let names: Vec<&str> = json["sheets"]
        .as_array()
        .expect("sheets array")
        .iter()
        .map(|s| s["name"].as_str().expect("name"))
        .collect();
    assert_eq!(
        names,
        ["sheet_1", "sheet_2", "sheet_3", "sheet_4", "sheet_5", "sheet_6"],
        "sheet names not anonymized"
    );
    let dn = json["defined_names"]
        .as_object()
        .expect("defined_names object");
    assert!(dn.contains_key("count"));
    assert!(
        !dn.contains_key("names"),
        "redacted output must omit defined-name list"
    );
}

#[test]
fn diff_of_identical_files_reports_zero_changes() {
    let path = fixture("stock-reconciliation.xlsx");
    let (json, _) = xlq(&["diff", &path, &path]);
    assert_eq!(json["sheets_added"], serde_json::json!([]));
    assert_eq!(json["sheets_removed"], serde_json::json!([]));
    assert_eq!(json["changes"], serde_json::json!([]));
    assert_eq!(json["truncated"], false);
    assert_eq!(json["summary"]["changed"], 0);
    assert_eq!(json["summary"]["added"], 0);
    assert_eq!(json["summary"]["removed"], 0);
}

#[test]
fn diff_detects_exactly_one_edited_cell() {
    let original = fixture("claims.xlsx");
    let modified = fixtures_dir()
        .join("claims-modified.xlsx")
        .to_str()
        .expect("utf8 path")
        .to_owned();

    // Edit one plain-value cell (Claims!B5, a branch label) and save a copy.
    let mut model =
        ironcalc::import::load_from_xlsx(&original, "en", "UTC", "en").expect("load claims.xlsx");
    let claims_idx = model
        .get_worksheets_properties()
        .iter()
        .position(|p| p.name == "Claims")
        .expect("Claims sheet") as u32;
    model
        .set_user_input(claims_idx, 5, 2, "TAMPERED".to_owned())
        .expect("set B5");
    ironcalc::export::save_to_xlsx(&model, &modified).expect("save modified copy");

    let (json, _) = xlq(&["diff", &original, &modified]);
    let changes = json["changes"].as_array().expect("changes array");
    assert_eq!(
        changes.len(),
        1,
        "expected exactly one change, got: {changes:?}"
    );
    let change = &changes[0];
    assert_eq!(change["sheet"], "Claims");
    assert_eq!(change["cell"], "B5");
    assert_eq!(change["kind"], "value");
    assert_eq!(change["new"]["value"], "TAMPERED");
    assert_eq!(json["summary"]["changed"], 1);
}

#[test]
fn calc_payroll_reports_coverage_and_zero_recalc_drift() {
    let path = fixture("payroll.xlsx");
    let (json, _) = xlq(&["calc", &path]);
    assert_eq!(json["xlq"]["command"], "calc");

    let coverage = json["coverage"]
        .as_object()
        .expect("coverage block present");
    assert_eq!(
        coverage["engine"],
        "ironcalc 0.7.1+e50ccea8 (vendored master)"
    );
    // Fixtures must only use functions the engine supports.
    assert_eq!(
        coverage["reliable"], true,
        "fixture uses unsupported functions"
    );
    assert_eq!(coverage["unsupported_functions"], serde_json::json!([]));

    assert!(json["summary"]["formulas"].as_u64().expect("formulas") > 0);
    // Fixtures are saved post-evaluate with no volatile functions, so a
    // fresh recalculation must change nothing.
    assert_eq!(json["summary"]["changed"], 0);
    assert_eq!(json["truncated"], false);
    assert_eq!(json["changed"], serde_json::json!([]));
}

/// Exit-code contract: 0 = effect/answer produced, 1 = operational refusal/
/// failure, 2 = malformed invocation. An agent branches on these in a shell, so
/// a certify REFUSED must NOT read as success.
mod exit_codes {
    use std::path::Path;
    use std::process::Command;

    fn run(args: &[&str]) -> i32 {
        Command::new(env!("CARGO_BIN_EXE_xlq"))
            .args(args)
            .output()
            .expect("spawn xlq")
            .status
            .code()
            .expect("exit code")
    }

    fn fixture(name: &str) -> String {
        format!("{}/../fixtures/structural/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn certify_refused_exits_1_certified_exits_0() {
        let orig = fixture("refs.xlsx");
        let dir = std::env::temp_dir().join(format!("xlq-exit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.xlsx");
        std::fs::copy(&orig, &good).unwrap();
        let good = good.to_str().unwrap();
        // xlq's own faithful transform certifies (exit 0)
        assert_eq!(
            run(&["restructure", good, "--sheet", "Sheet1", "--op", "insert-rows",
                  "--at", "2", "--count", "1", "--actor", "t"]),
            0
        );
        assert_eq!(
            run(&["certify", &orig, good, "--sheet", "Sheet1", "--op", "insert-rows",
                  "--at", "2", "--count", "1"]),
            0,
            "faithful transform must certify (exit 0)"
        );
        // the untouched original is NOT the insert-row transform → REFUSED (exit 1)
        assert_eq!(
            run(&["certify", &orig, &orig, "--sheet", "Sheet1", "--op", "insert-rows",
                  "--at", "2", "--count", "1"]),
            1,
            "REFUSED must exit 1 so a shell 'if xlq certify' does not ship it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_op_exits_2_missing_file_exits_1() {
        let orig = fixture("refs.xlsx");
        assert_eq!(
            run(&["restructure", &orig, "--sheet", "Sheet1", "--op", "bogus", "--at", "2"]),
            2,
            "malformed --op is a usage error (exit 2)"
        );
        assert!(!Path::new("/no/such/file.xlsx").exists());
        assert_eq!(run(&["inspect", "/no/such/file.xlsx"]), 1);
    }
}
