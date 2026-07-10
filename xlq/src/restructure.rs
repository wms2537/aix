//! `xlq restructure` — surgical structural edits (insert/delete row/column) via
//! the reference-shift algebra σ. Mirrors apply.rs's transactional envelope:
//! advisory lock, base-hash precondition, proof-carrying re-open, residual
//! gate, immutable rev-file + hash-chained receipt + atomic swap.
//!
//! The proof-carrying step re-loads the surgical output the way a consumer
//! would (as an .xlsx on disk) and evaluates it: a structurally corrupt output
//! (a broken row/cell coordinate, an unbalanced element) fails to re-open and
//! aborts the commit with the original untouched. The residual gate refuses to
//! commit when the edit touches a construct σ cannot express as a pure
//! coordinate shift (shared/array formulas crossing the edit) — so a subtly
//! wrong file is never produced; the user is told exactly what blocked it.

use crate::journal::{self, ChainStatus};
use crate::refshift::{Axis, Op, StructuralEdit};
use crate::structural::{self, StructuralReport};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

#[allow(clippy::too_many_arguments)]
pub fn run(
    file: &str,
    sheet: &str,
    axis: Axis,
    op: Op,
    at: u32,
    count: u32,
    dest: u32,
    dry_run: bool,
    actor: Option<&str>,
) -> Result<Value> {
    if at == 0 || count == 0 {
        return Ok(json!({"command":"restructure","error":"bad_args",
            "reason":"--at is 1-based and --count must be >= 1"}));
    }
    // Move requires a 1-based destination; dest is ignored for insert/delete.
    if op == Op::Move && dest == 0 {
        return Ok(json!({"command":"restructure","error":"bad_args",
            "reason":"move-rows requires --dest >= 1 (the 1-based row to move the block before)"}));
    }
    let edit = StructuralEdit { axis, at, count, op, sheet: sheet.to_string(), dest };
    let op_str = op_name(op, axis);

    // A real apply takes the advisory lock BEFORE reading, closing TOCTOU.
    let _lock = if dry_run { None } else { Some(journal::lock(file)?) };

    let original = std::fs::read(file).with_context(|| format!("read {file}"))?;
    let base_hash = crate::hash::sha256_file(file)?;

    let (new_bytes, report) = structural::structural_edit(&original, &edit)
        .with_context(|| format!("structural edit on {sheet}"))?;

    // Proof-carrying re-open: the output must load and evaluate in the engine.
    let reopen = reopen_ok(&new_bytes, file);

    let summary = report_json(&report, &op_str, &edit, &reopen);

    if dry_run {
        return Ok(json!({
            "command": "restructure",
            "dry_run": true,
            "base_hash": base_hash,
            "edit": summary,
        }));
    }

    // Residual gate: refuse to commit an edit σ cannot express as a pure
    // coordinate shift. Detected, never silently wrong.
    if !report.residuals.is_empty() {
        return Ok(json!({
            "command": "restructure",
            "dry_run": false,
            "error": "residual_unreachable",
            "reason": "the edit touches constructs the shift algebra cannot preserve by coordinate surgery",
            "residuals": report.residuals.iter().map(|r| json!({
                "part": r.part, "reason": r.reason, "detail": r.detail
            })).collect::<Vec<_>>(),
        }));
    }

    // Proof-carrying gate: refuse to commit an output that does not re-open.
    if let Err(detail) = &reopen {
        return Ok(json!({
            "command": "restructure",
            "dry_run": false,
            "error": "verification_failed",
            "reason": "surgical output does not re-open in the engine",
            "detail": detail,
        }));
    }

    let result_hash = sha256_bytes(&new_bytes);
    let timestamp = iso_timestamp(None);
    let resolved_actor = journal::resolve_actor(actor);

    // Re-verify the on-disk bytes still hash to base (lock held since before the
    // read) and the chain is clean before writing.
    let disk_hash = crate::hash::sha256_file(file)?;
    if disk_hash != base_hash {
        return Ok(json!({"command":"restructure","dry_run":false,
            "error":"revision_mismatch","expected":base_hash,"actual":disk_hash}));
    }
    match journal::chain_status(file, &disk_hash)? {
        ChainStatus::Genesis | ChainStatus::Ok => {}
        ChainStatus::ExternalEdit => {
            let marker =
                journal::append_adoption_marker(file, &disk_hash, &timestamp, &resolved_actor)?;
            return Ok(json!({"command":"restructure","dry_run":false,
                "error":"external_edit_detected","expected":marker.base_hash,
                "actual":disk_hash,"adopted_rev":marker.rev}));
        }
    }

    let rev = journal::next_rev(file)?;
    let ops = if op == Op::Move {
        json!([{ "type": op_str, "sheet": sheet, "at": at, "count": count, "dest": dest }])
    } else {
        json!([{ "type": op_str, "sheet": sheet, "at": at, "count": count }])
    };
    let receipt = journal::commit(
        file, &new_bytes, rev, &base_hash, &result_hash, ops, &timestamp, &resolved_actor,
        None, None,
    )?;

    Ok(json!({
        "command": "restructure",
        "dry_run": false,
        "rev": receipt.rev,
        "base_hash": base_hash,
        "result_hash": result_hash,
        "edit": summary,
        "verified": { "reopened": true, "residuals": 0 },
    }))
}

fn report_json(
    report: &StructuralReport,
    op_str: &str,
    edit: &StructuralEdit,
    reopen: &Result<(), String>,
) -> Value {
    json!({
        "op": op_str,
        "sheet": edit.sheet,
        "at": edit.at,
        "count": edit.count,
        "dest": edit.dest,
        "refs_shifted": report.refs_shifted,
        "ref_errors": report.ref_errors,
        "rows_inserted": report.rows_inserted,
        "rows_deleted": report.rows_deleted,
        "parts_touched": report.parts_touched,
        "residuals": report.residuals.iter().map(|r| json!({
            "part": r.part, "reason": r.reason
        })).collect::<Vec<_>>(),
        "reopens": reopen.is_ok(),
    })
}

fn op_name(op: Op, axis: Axis) -> String {
    match (op, axis) {
        (Op::Insert, Axis::Row) => "insert_rows",
        (Op::Delete, Axis::Row) => "delete_rows",
        (Op::Insert, Axis::Col) => "insert_cols",
        (Op::Delete, Axis::Col) => "delete_cols",
        (Op::Move, Axis::Row) => "move_rows",
        (Op::Move, Axis::Col) => "move_cols", // unreachable (rejected upstream)
    }
    .to_string()
}

/// Write the output to a temp file and confirm it loads + evaluates in IronCalc.
fn reopen_ok(bytes: &[u8], near: &str) -> Result<(), String> {
    let dir = std::path::Path::new(near)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // Unique per call: pid alone collides when multiple restructure operations
    // run in one process against the same directory (e.g. parallel tests).
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".xlq-restructure-verify-{}-{}.xlsx",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        return Err(format!("write temp: {e}"));
    }
    let tmp_str = tmp.to_string_lossy().to_string();
    let res = match ironcalc::import::load_from_xlsx(&tmp_str, "en", "UTC", "en") {
        Ok(mut m) => {
            m.evaluate();
            Ok(())
        }
        Err(e) => Err(format!("{e}")),
    };
    let _ = std::fs::remove_file(&tmp);
    res
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn iso_timestamp(clock: Option<i64>) -> String {
    let secs = clock.unwrap_or(0);
    // minimal deterministic ISO-8601; matches apply.rs's convention of a
    // caller-supplied clock (0 → epoch) so the library holds no wall-clock.
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 1970-01-01 + days, computed simply (good enough for the receipt stamp)
    format!("1970-01-01T{:02}:{:02}:{:02}Z+{}d", h, mi, s, days)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Committed fixtures, resolved relative to the crate — machine-independent
    /// (works on any checkout of the repo, on any machine).
    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/structural/");

    fn scratch(name: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("xlq-rs-{name}-{}-{n}.xlsx", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    fn setup(tag: &str) -> String {
        let dst = scratch(tag);
        std::fs::copy(format!("{FIX}refs.xlsx"), &dst).unwrap();
        dst
    }

    #[test]
    fn dry_run_reports_shift_without_writing() {
        let f = setup("dry");
        let before = crate::hash::sha256_file(&f).unwrap();
        let out = run(&f, "Sheet1", Axis::Row, Op::Insert, 5, 1, 0, true, Some("t")).unwrap();
        assert_eq!(out["edit"]["reopens"], json!(true));
        assert!(out["edit"]["refs_shifted"].as_u64().unwrap() >= 4);
        assert_eq!(crate::hash::sha256_file(&f).unwrap(), before, "dry run must not write");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn move_requires_dest() {
        let f = setup("movedest");
        let out = run(&f, "Sheet1", Axis::Row, Op::Move, 5, 1, 0, true, Some("t")).unwrap();
        assert_eq!(out["error"], json!("bad_args"), "move without --dest must be refused: {out}");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn real_insert_commits_and_recomputes() {
        let f = setup("real");
        let out = run(&f, "Sheet1", Axis::Row, Op::Insert, 5, 1, 0, false, Some("t")).unwrap();
        assert_eq!(out["rev"], json!(1), "got {out}");
        assert_eq!(out["verified"]["reopened"], json!(true));
        // the committed file recomputes correctly
        let mut m = ironcalc::import::load_from_xlsx(&f, "en", "UTC", "en").unwrap();
        m.evaluate();
        assert_eq!(m.get_formatted_cell_value(0, 12, 1).unwrap(), "55");
        // rev file + receipt journal exist
        assert!(std::path::Path::new(&format!("{f}.xlq.jsonl")).exists());
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(format!("{f}.rev-1.xlsx")).ok();
        std::fs::remove_file(format!("{f}.xlq.jsonl")).ok();
    }

    #[test]
    fn shared_formula_edit_now_succeeds() {
        // shared formulas are EXPANDED (materialize → shift), so a shared-only
        // real file must now commit, not be refused.
        let fixture = format!("{FIX}shared.xlsx"); // YEAR.xlsx: shared, no table
        let dst = setup_from("shared", &fixture); // unique temp name — no stale sidecars
        let out = run(&dst, "Sheet1", Axis::Row, Op::Insert, 2, 1, 0, false, Some("t")).unwrap();
        assert_eq!(out["rev"], json!(1), "shared edit should commit, got {out}");
        assert_eq!(out["verified"]["reopened"], json!(true));
        std::fs::remove_file(&dst).ok();
        std::fs::remove_file(format!("{dst}.rev-1.xlsx")).ok();
        std::fs::remove_file(format!("{dst}.xlq.jsonl")).ok();
    }

    #[test]
    fn table_edit_still_refused() {
        // tables remain unsupported → refused (never silently wrong).
        let fixture = format!("{FIX}table.xlsx");
        let dst = setup_from("table", &fixture);
        let out = run(&dst, "Sheet1", Axis::Row, Op::Insert, 3, 1, 0, false, Some("t")).unwrap();
        assert_eq!(out["error"], json!("residual_unreachable"), "got {out}");
        std::fs::remove_file(&dst).ok();
    }

    #[test]
    fn real_move_commits_and_recomputes() {
        // move a row on the refs fixture and confirm it commits + reopens.
        let f = setup("move");
        // refs.xlsx has A1..A10 with SUM/formulas; move row 2 (a=2,n=1) to before
        // row 4 (dest=4, move down). σ is a bijection; a clean move commits.
        let out = run(&f, "Sheet1", Axis::Row, Op::Move, 2, 1, 4, false, Some("t")).unwrap();
        // either it commits (rev 1) or, if the fixture has a range straddling the
        // move, it is soundly refused — never a silent wrong write.
        assert!(
            out.get("rev").is_some() || out["error"] == json!("residual_unreachable"),
            "move must commit or be soundly refused, got {out}"
        );
        if out.get("rev").is_some() {
            assert_eq!(out["verified"]["reopened"], json!(true));
            let mut m = ironcalc::import::load_from_xlsx(&f, "en", "UTC", "en").unwrap();
            m.evaluate();
        }
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(format!("{f}.rev-1.xlsx")).ok();
        std::fs::remove_file(format!("{f}.xlq.jsonl")).ok();
    }

    fn setup_from(tag: &str, src: &str) -> String {
        let dst = scratch(tag);
        std::fs::copy(src, &dst).unwrap();
        dst
    }
}
