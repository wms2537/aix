//! `xlq apply` — orchestrates the transactional surgical write.
//!
//! CONTRACT: pub fn run(file: &str, patch_path: &str, dry_run: bool,
//!                      actor: Option<&str>) -> anyhow::Result<serde_json::Value>
//!
//! Flow (docs/specs/v02-architecture.md):
//!  1. Load patch (patch::load); precondition sha256(file)==patch.base_hash
//!     else return a `revision_mismatch` error payload {expected, actual}.
//!  2. PREDICT (also the whole dry-run path): load a COPY of the file into
//!     IronCalc, snapshot all stored values, apply the ops via
//!     set_user_input, evaluate(), snapshot again. Affected = cells whose
//!     value changed OR that the ops set directly. Build:
//!     `{ affected:[{sheet,cell,stored,recomputed,formula,volatile}],`
//!     `new_errors:[…], watch:[{cell,before,after}], coverage:{…} }`.
//!     Dry-run RETURNS this (command "apply", "dry_run": true), writes nothing.
//!  3. WRITE (real apply): acquire journal::lock; chain_status(current_hash);
//!     turn the affected set into ooxml::CellEdit list; ooxml::surgical_write;
//!     journal::commit(...) writes the rev file + atomic swap + receipt.
//!  4. Return { command:"apply", dry_run:false, rev, base_hash, result_hash,
//!     affected_count, receipt:{…}, fidelity:{parts_total, parts_rewritten,
//!     parts_byte_identical} }.
//!
//! Coverage honesty: if the affected graph uses unsupported/policy-limited/
//! user-defined functions, the dry-run still returns the prediction with
//! coverage.write_reliable=false, but a REAL apply refuses to write (predicted
//! cached values would be unreliable). Errors carry basenames only.

use anyhow::{anyhow, Context, Result};
use ironcalc::base::cell::CellValue as IcCellValue;
use ironcalc::base::expressions::utils::number_to_column;
use ironcalc::base::types::CellType;
use ironcalc::base::Model;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use crate::journal::{self, ChainStatus};
use crate::ooxml::{self, CellEdit, CellValue};
use crate::patch;

// Single-sourced from the vendored engine (base/src/constants.rs); the
// cross-module invariant ("must match calc.rs ENGINE") now holds by construction
// because both alias the same const. census/coverage consumers compare against it.
use ironcalc::base::ENGINE_PROVENANCE as ENGINE;

// Excel volatile set (same list census.rs uses); a cell whose own formula
// calls one is time/randomness dependent.
const VOLATILE_FUNCTIONS: [&str; 8] = [
    "NOW",
    "TODAY",
    "RAND",
    "RANDBETWEEN",
    "OFFSET",
    "INDIRECT",
    "CELL",
    "INFO",
];

// The NON-DETERMINISTIC subset: their value depends on wall-clock time or
// randomness, so a cached value written for them is not reproducible run to
// run. This engine build cannot pin them (NOW/TODAY read the wall clock, RAND
// calls rand::random), so a real apply must REFUSE when one of these appears in
// the affected dependency graph — even a pinned clock/seed in the patch cannot
// make the cache reproducible. OFFSET/INDIRECT/CELL/INFO are volatile in the
// recalc sense but produce deterministic values, so they do not block a write.
const NONDETERMINISTIC_VOLATILE: [&str; 4] = ["NOW", "TODAY", "RAND", "RANDBETWEEN"];

// Functions where our own differential oracle (benchmarks/agreement.json,
// triage-analysis.md) found the prediction engine SILENTLY computes a value
// that disagrees with Excel — i.e. it returns a wrong number, not an error the
// coverage gate would already catch. A real apply must NOT persist an
// engine-computed cached <v> for a cell whose formula uses one of these: the
// cache could be wrong, reopening the value-corruption class the boundary
// exists to prevent. This is the differential oracle made load-bearing for
// write safety, not merely for validation: the oracle's confusion matrix IS
// the write-reliability gate. (Error/coverage-gap functions like AREAS, GROWTH,
// FILTER are already refused by the unsupported/policy gate.)
const ENGINE_DIVERGENT: [&str; 10] = [
    "CONVERT",    // CONVERT(68,"F","C") = 19.65 vs Excel 20 (additive offset)
    "TRIM",       // does not collapse internal whitespace runs
    "ROW",        // ROW(range) returns a scalar, not the array Excel spills
    "SUMPRODUCT", // does not coerce boolean arrays
    "MAXA",       // A-family: ignores text/boolean cells Excel counts as 0/1
    "MINA",
    "STDEVA",
    "SECOND",          // truncates instead of rounding sub-second times
    "PRICE",           // basis-3 returns exact par instead of Excel's value
    "PERCENTRANK.EXC", // truncates to decimal places, not significant digits
];

type Pos = (u32, i32, i32);

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

fn cell_ref(col: i32, row: i32) -> String {
    match number_to_column(col) {
        Some(letters) => format!("{letters}{row}"),
        None => format!("R{row}C{col}"),
    }
}

fn formula_is_volatile(formula: &str) -> bool {
    crate::census::extract_function_names(formula)
        .iter()
        .any(|n| VOLATILE_FUNCTIONS.contains(&n.as_str()))
}

/// Split a watch ref "Sheet!A1" (or 'My Sheet'!A1) into (sheet, cell); a bare
/// "A1" resolves against the first worksheet.
fn split_ref(reference: &str) -> (Option<String>, &str) {
    match reference.rfind('!') {
        Some(i) => {
            let raw = &reference[..i];
            let name = raw.trim_matches('\'').to_string();
            (Some(name), &reference[i + 1..])
        }
        None => (None, reference),
    }
}

fn read_parts(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).context("open xlsx archive for fidelity check")?;
    let mut parts = BTreeMap::new();
    // Anti-bomb: this fidelity check decompresses EVERY part of both the original
    // and the written workbook (called twice per apply), so it is a prime OOM
    // target. One budget per call bounds it.
    let mut budget = crate::ooxml::total_cap();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).context("read archive entry")?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let sz = entry.size();
        let buf = crate::ooxml::read_entry_capped(entry, sz, &name, &mut budget)?;
        parts.insert(name, buf);
    }
    Ok(parts)
}

/// Raw stored value of a cell as JSON (null|string|number|bool). Reuses the
/// shared value machinery so the comparison basis matches `calc`/`diff`.
fn raw(model: &Model, pos: Pos) -> Result<Value> {
    crate::value::raw_cell_value(model, pos.0, pos.1, pos.2)
}

pub fn run(file: &str, patch_path: &str, dry_run: bool, actor: Option<&str>) -> Result<Value> {
    let file_name = basename(file);
    let patch = patch::load(patch_path)?;

    // For a REAL apply, take the advisory lock BEFORE the precondition hash is
    // read, and hold it across the entire check -> predict -> verify -> write
    // sequence (spec step 2). Acquiring it only just before the write would
    // leave the base_hash check and prediction on unlocked bytes (TOCTOU): the
    // file could change underneath, and we would mutate content the prediction
    // never saw. A dry run writes nothing, so it takes no lock.
    let _lock_guard = if dry_run {
        None
    } else {
        Some(journal::lock(file)?)
    };

    // (1) Precondition: the file must currently hash to the patch's base_hash.
    let actual_hash = crate::hash::sha256_file(file)?;
    if actual_hash != patch.base_hash {
        return Ok(json!({
            "command": "apply",
            "error": "revision_mismatch",
            "expected": patch.base_hash,
            "actual": actual_hash,
        }));
    }

    // (2) PREDICT. load_from_xlsx reads the file into an in-memory model; every
    // mutation below is on that copy, so the original file is never touched
    // until the surgical write in step (4).
    // Anti-bomb preflight before ironcalc's unbounded zip loads the user file.
    crate::ooxml::guard_decompression(file)
        .with_context(|| format!("load workbook {file_name}"))?;
    let mut model = ironcalc::import::load_from_xlsx(file, "en", "UTC", "en")
        .map_err(|e| anyhow!(e))
        .with_context(|| format!("load workbook {file_name}"))?;

    let sheet_names: Vec<String> = model
        .get_worksheets_properties()
        .into_iter()
        .map(|p| p.name)
        .collect();
    let resolve_sheet = |name: &str| -> Option<u32> {
        sheet_names.iter().position(|n| n == name).map(|i| i as u32)
    };

    // Snapshot BEFORE: raw value of every populated cell + the set already
    // holding an error (so `new_errors` reports only errors the patch creates).
    let before_cells = model.get_all_cells();
    let mut before_raw: BTreeMap<Pos, Value> = BTreeMap::new();
    let mut before_err: BTreeSet<Pos> = BTreeSet::new();
    let mut universe: BTreeSet<Pos> = BTreeSet::new();
    for c in &before_cells {
        let pos = (c.index, c.row, c.column);
        before_raw.insert(pos, raw(&model, pos)?);
        if model
            .get_cell_type(pos.0, pos.1, pos.2)
            .map_err(|e| anyhow!(e))?
            == CellType::ErrorValue
        {
            before_err.insert(pos);
        }
        universe.insert(pos);
    }

    // Watch: capture BEFORE values now, against the pristine model.
    struct Watch {
        label: String,
        pos: Option<Pos>,
        before: Value,
    }
    let mut watches: Vec<Watch> = Vec::new();
    for reference in &patch.watch {
        let (sheet_opt, cell) = split_ref(reference);
        let sheet_idx = match &sheet_opt {
            Some(name) => resolve_sheet(name),
            None => sheet_names.first().map(|_| 0u32),
        };
        let pos = match (sheet_idx, patch::parse_a1(cell)) {
            (Some(s), Ok((row, col))) => Some((s, row, col)),
            _ => None,
        };
        let before = match pos {
            Some(p) => raw(&model, p)?,
            None => Value::Null,
        };
        watches.push(Watch {
            label: reference.clone(),
            pos,
            before,
        });
    }

    // Apply the ops. Each op target is affected even if its value is unchanged.
    let mut op_targets: BTreeSet<Pos> = BTreeSet::new();
    for op in &patch.ops {
        let (sheet, cell) = match op {
            patch::Op::SetCell { sheet, cell, .. } => (sheet, cell),
            patch::Op::SetFormula { sheet, cell, .. } => (sheet, cell),
        };
        let s =
            resolve_sheet(sheet).ok_or_else(|| anyhow!("unknown sheet {sheet} in {file_name}"))?;
        let (row, col) = patch::parse_a1(cell)?;
        let input = match op {
            patch::Op::SetCell { value, .. } => patch::value_to_input(value)?,
            patch::Op::SetFormula { formula, .. } => {
                format!("={}", formula.strip_prefix('=').unwrap_or(formula))
            }
        };
        model
            .set_user_input(s, row, col, input)
            .map_err(|e| anyhow!(e))
            .with_context(|| format!("apply op at {cell} in {file_name}"))?;
        let pos = (s, row, col);
        op_targets.insert(pos);
        universe.insert(pos);
    }

    model.evaluate();

    for c in model.get_all_cells() {
        universe.insert((c.index, c.row, c.column));
    }

    // Affected = cells whose raw value changed OR that an op set directly.
    let mut affected: BTreeSet<Pos> = BTreeSet::new();
    for &pos in &universe {
        let before = before_raw.get(&pos).cloned().unwrap_or(Value::Null);
        let after = raw(&model, pos)?;
        if op_targets.contains(&pos) || after != before {
            affected.insert(pos);
        }
    }

    // Build the affected list + gather the functions the affected cells use.
    // `predicted` records, per affected cell, the raw value the dry-run model
    // computes. A real apply must re-load its own surgical output and prove
    // every one of these landed (proof-carrying apply) before committing.
    let mut affected_json: Vec<Value> = Vec::with_capacity(affected.len());
    let mut affected_functions: BTreeSet<String> = BTreeSet::new();
    let mut predicted: BTreeMap<Pos, Value> = BTreeMap::new();
    for &pos in &affected {
        let (s, row, col) = pos;
        let stored = before_raw.get(&pos).cloned().unwrap_or(Value::Null);
        let recomputed = raw(&model, pos)?;
        predicted.insert(pos, recomputed.clone());
        let formula = model
            .get_cell_formula(s, row, col)
            .map_err(|e| anyhow!(e))
            .with_context(|| format!("read formula for {} in {file_name}", cell_ref(col, row)))?;
        if let Some(f) = &formula {
            for name in crate::census::extract_function_names(f) {
                affected_functions.insert(name);
            }
        }
        let volatile = formula.as_deref().map(formula_is_volatile).unwrap_or(false);
        let sheet = sheet_names
            .get(s as usize)
            .cloned()
            .unwrap_or_else(|| format!("sheet_{s}"));
        affected_json.push(json!({
            "sheet": sheet,
            "cell": cell_ref(col, row),
            "stored": stored,
            "recomputed": recomputed,
            "formula": formula,
            "volatile": volatile,
        }));
    }

    // new_errors: cells now ErrorValue that were not before.
    let mut new_errors: Vec<Value> = Vec::new();
    for &pos in &universe {
        let (s, row, col) = pos;
        let is_err =
            model.get_cell_type(s, row, col).map_err(|e| anyhow!(e))? == CellType::ErrorValue;
        if is_err && !before_err.contains(&pos) {
            let literal = model
                .get_formatted_cell_value(s, row, col)
                .map_err(|e| anyhow!(e))?;
            new_errors.push(json!({
                "sheet": sheet_names.get(s as usize).cloned().unwrap_or_else(|| format!("sheet_{s}")),
                "cell": cell_ref(col, row),
                "error": literal,
            }));
        }
    }

    // Watch after-values.
    let watch_json: Vec<Value> = watches
        .iter()
        .map(|w| -> Result<Value> {
            let after = match w.pos {
                Some(p) => raw(&model, p)?,
                None => Value::Null,
            };
            Ok(if w.pos.is_some() {
                json!({"cell": w.label, "before": w.before, "after": after})
            } else {
                json!({"cell": w.label, "error": "invalid_ref"})
            })
        })
        .collect::<Result<_>>()?;

    // Coverage: whole-workbook census for the honesty report, plus an
    // affected-scoped gate (values we would cache are unreliable if any
    // affected cell's formula uses an unsupported/policy-limited/UDF name).
    let census = crate::census::function_census(&model);
    let mut bad: BTreeSet<String> = BTreeSet::new();
    bad.extend(census.unsupported.iter().cloned());
    bad.extend(census.policy_limited.keys().cloned());
    bad.extend(census.user_defined.keys().cloned());
    let unreliable_in_affected: Vec<String> =
        affected_functions.intersection(&bad).cloned().collect();
    // Non-deterministic volatiles in the affected graph also make the write
    // unreliable: their cached values are not reproducible and this engine
    // cannot pin them (see NONDETERMINISTIC_VOLATILE). A real apply must refuse.
    let nondeterministic_in_affected: Vec<String> = affected_functions
        .iter()
        .filter(|n| NONDETERMINISTIC_VOLATILE.contains(&n.as_str()))
        .cloned()
        .collect();
    // Oracle-divergent functions in the affected graph: the engine may compute
    // a silently-wrong cached value here, so a real apply must refuse.
    let oracle_divergent_in_affected: Vec<String> = affected_functions
        .iter()
        .filter(|n| ENGINE_DIVERGENT.contains(&n.as_str()))
        .cloned()
        .collect();
    let write_reliable = unreliable_in_affected.is_empty()
        && nondeterministic_in_affected.is_empty()
        && oracle_divergent_in_affected.is_empty();
    let coverage = json!({
        "engine": ENGINE,
        "reliable": census.unsupported.is_empty()
            && census.policy_limited.is_empty()
            && census.user_defined.is_empty(),
        "unsupported_functions": census.unsupported,
        "policy_limited_functions": census.policy_limited,
        "volatile_functions": census.volatile_present,
        "user_defined_functions": census.user_defined.keys().cloned().collect::<Vec<_>>(),
        "write_reliable": write_reliable,
        "unreliable_in_affected": unreliable_in_affected.clone(),
        "nondeterministic_in_affected": nondeterministic_in_affected.clone(),
        "oracle_divergent_in_affected": oracle_divergent_in_affected.clone(),
    });

    if dry_run {
        return Ok(json!({
            "command": "apply",
            "dry_run": true,
            "file": {"name": file_name, "sha256": actual_hash},
            "affected_count": affected_json.len(),
            "affected": affected_json,
            "new_errors": new_errors,
            "watch": watch_json,
            "coverage": coverage,
        }));
    }

    // (3) Coverage gate for a REAL write: a prediction whose cached values are
    // unreliable must never be committed.
    if !write_reliable {
        return Ok(json!({
            "command": "apply",
            "dry_run": false,
            "error": "coverage_unreliable",
            "unreliable_functions": unreliable_in_affected,
            "nondeterministic_functions": nondeterministic_in_affected,
            "oracle_divergent_functions": oracle_divergent_in_affected,
            "affected_count": affected_json.len(),
            "coverage": coverage,
        }));
    }

    // Build the ooxml edits: each affected cell's formula (no leading '=') and
    // its recomputed cached value. An error-valued cell cannot be represented
    // by the CellValue variants, so refuse rather than corrupt the cell type.
    let mut edits: Vec<CellEdit> = Vec::with_capacity(affected.len());
    for &pos in &affected {
        let (s, row, col) = pos;
        let sheet = sheet_names
            .get(s as usize)
            .cloned()
            .ok_or_else(|| anyhow!("sheet index {s} out of range in {file_name}"))?;
        let formula = model
            .get_cell_formula(s, row, col)
            .map_err(|e| anyhow!(e))?
            .map(|f| f.strip_prefix('=').unwrap_or(&f).to_string());
        let ic = model
            .get_cell_value_by_index(s, row, col)
            .map_err(|e| anyhow!(e))?;
        let ct = model.get_cell_type(s, row, col).map_err(|e| anyhow!(e))?;
        let value = match ic {
            IcCellValue::None => CellValue::Blank,
            IcCellValue::Number(n) => CellValue::Number(n),
            IcCellValue::Boolean(b) => CellValue::Bool(b),
            IcCellValue::String(text) => {
                if ct == CellType::ErrorValue {
                    return Err(anyhow!(
                        "refusing to write error-valued cell {} in {file_name}",
                        cell_ref(col, row)
                    ));
                }
                CellValue::Str(text)
            }
        };
        edits.push(CellEdit {
            sheet,
            row,
            col,
            formula,
            value,
        });
    }

    // (4) WRITE. The lock has been held since before the precondition hash was
    // read. Re-read the file now and re-verify it STILL hashes to base_hash: the
    // bytes we are about to surgically mutate must be exactly the bytes the
    // precondition and the prediction were computed against. This closes the
    // TOCTOU gap where an editor that ignored the advisory lock changed the file
    // after step (1) — we refuse rather than apply predicted values onto content
    // they were never computed for.
    let timestamp = journal::iso_timestamp(patch.clock);
    let resolved_actor = journal::resolve_actor(actor.or(patch.actor.as_deref()));

    let original_bytes = std::fs::read(file).with_context(|| format!("read {file_name}"))?;
    let disk_hash = crate::hash::sha256_bytes(&original_bytes);
    if disk_hash != patch.base_hash {
        return Ok(json!({
            "command": "apply",
            "dry_run": false,
            "error": "revision_mismatch",
            "expected": patch.base_hash,
            "actual": disk_hash,
        }));
    }

    let chain = match journal::chain_status(file, &disk_hash)? {
        ChainStatus::Genesis => "genesis",
        ChainStatus::Ok => "ok",
        // ExternalEdit: the on-disk hash matches the patch base_hash (verified
        // above) but is NOT the last committed result — the file was edited
        // outside xlq. Spec: REFUSE this patch and append an external_edit
        // marker adopting the current hash. The user must re-issue the patch,
        // which then sees an Ok chain and proceeds. We never write in the same
        // run that first detects the divergence.
        ChainStatus::ExternalEdit => {
            let marker =
                journal::append_adoption_marker(file, &disk_hash, &timestamp, &resolved_actor)?;
            return Ok(json!({
                "command": "apply",
                "dry_run": false,
                "error": "external_edit_detected",
                "expected": marker.base_hash,
                "actual": disk_hash,
                "adopted_rev": marker.rev,
            }));
        }
    };

    let new_bytes = ooxml::surgical_write(&original_bytes, &edits)?;
    let result_hash = crate::hash::sha256_bytes(&new_bytes);

    // PROOF-CARRYING APPLY. Nothing is on disk yet (commit does the write), so
    // if this verification fails the original file is untouched. We re-load the
    // surgical output the SAME way a downstream consumer would (as an .xlsx on
    // disk), evaluate it, and prove every predicted affected cell landed with
    // the value the dry-run predicted. This closes the gap between "the writer
    // intended the edit" and "the written file verifiably computes the edit":
    // a subtly-malformed <c>, a wrong cached value, or a corrupt insertion is
    // caught HERE and aborts the commit, rather than being asserted by a
    // result_hash the consumer cannot check.
    {
        let verify_dir = std::path::Path::new(file)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        match verify_output(
            &new_bytes,
            &predicted,
            &sheet_names,
            &verify_dir,
            &result_hash,
        )? {
            VerifyOutcome::LoadFailed(detail) => {
                // The output does not even re-open: the strongest failure.
                return Ok(json!({
                    "command": "apply",
                    "dry_run": false,
                    "error": "verification_failed",
                    "reason": "surgical output does not re-open in the engine",
                    "detail": detail,
                }));
            }
            VerifyOutcome::Mismatch(mismatches) => {
                // The output re-opened but at least one predicted cell did not
                // land: refuse to commit and leave the original untouched.
                return Ok(json!({
                    "command": "apply",
                    "dry_run": false,
                    "error": "verification_failed",
                    "reason": "re-loaded output disagrees with the dry-run prediction",
                    "mismatches": mismatches,
                }));
            }
            VerifyOutcome::Verified => {}
        }
    }

    // Fidelity proof: every input part that is NOT a rewritten sheet part must
    // survive byte-for-byte (a dropped calcChain counts as rewritten). Count on
    // the SAME files-only basis read_parts uses (zip directory entries are
    // excluded); using part_names for the total would count dir entries in
    // parts_total but never in the byte-identical tally, over-reporting churn.
    let original_parts = read_parts(&original_bytes)?;
    let written_parts = read_parts(&new_bytes)?;
    let parts_total = original_parts.len();
    let mut parts_byte_identical = 0usize;
    for (name, bytes) in &original_parts {
        if written_parts.get(name) == Some(bytes) {
            parts_byte_identical += 1;
        }
    }
    let parts_rewritten = parts_total - parts_byte_identical;

    // ENFORCE the fidelity property (not merely report it). The ONLY parts
    // allowed to differ from the input are (a) sheet parts that received an
    // edit and (b) the deliberately-dropped rebuildable caches xl/calcChain.xml
    // and xl/volatileDependencies.xml. If any OTHER part
    // changed, was dropped, or was added, the surgical write did not preserve
    // fidelity — abort the commit (original untouched). This makes the fidelity
    // property a per-apply CHECK, not just a by-construction argument.
    let edited_sheet_parts: BTreeSet<String> = {
        let mut s = BTreeSet::new();
        for e in &edits {
            if let Ok(part) = ooxml::sheet_part(&original_bytes, &e.sheet) {
                s.insert(part);
            }
        }
        s
    };
    let mut fidelity_violations: Vec<Value> = Vec::new();
    let all_names: BTreeSet<&String> = original_parts.keys().chain(written_parts.keys()).collect();
    for name in all_names {
        let before = original_parts.get(name);
        let after = written_parts.get(name);
        if before == after {
            continue;
        }
        let allowed = edited_sheet_parts.contains(name)
            || name == "xl/calcChain.xml"
            || name == "xl/volatileDependencies.xml";
        if !allowed {
            let kind = match (before.is_some(), after.is_some()) {
                (true, false) => "dropped",
                (false, true) => "added",
                _ => "changed",
            };
            fidelity_violations.push(json!({"part": name, "kind": kind}));
        }
    }
    if !fidelity_violations.is_empty() {
        return Ok(json!({
            "command": "apply",
            "dry_run": false,
            "error": "fidelity_violation",
            "reason": "a part that contains no edited cell was not preserved byte-identical",
            "violations": fidelity_violations,
        }));
    }

    let rev = journal::next_rev(file)?;
    let ops_json = raw_ops(patch_path).unwrap_or_else(|| Value::Array(Vec::new()));
    let receipt = journal::commit(
        file,
        &new_bytes,
        rev,
        "apply",
        &actual_hash,
        &result_hash,
        ops_json,
        &timestamp,
        &resolved_actor,
        patch.clock,
        patch.seed,
    )?;

    Ok(json!({
        "command": "apply",
        "dry_run": false,
        "chain": chain,
        "rev": receipt.rev,
        "base_hash": receipt.base_hash,
        "result_hash": receipt.result_hash,
        "affected_count": affected_json.len(),
        "fidelity": {
            "parts_total": parts_total,
            "parts_rewritten": parts_rewritten,
            "parts_byte_identical": parts_byte_identical,
        },
        "verified": {
            "reopened": true,
            "cells_checked": predicted.len(),
            "all_landed": true,
            "fidelity_enforced": true,
            "non_edited_parts_byte_identical": true,
        },
        "receipt": {
            "rev": receipt.rev,
            "kind": receipt.kind,
            "base_hash": receipt.base_hash,
            "result_hash": receipt.result_hash,
            "ops": receipt.ops,
            "timestamp": receipt.timestamp,
            "actor": receipt.actor,
            "engine_version": receipt.engine_version,
            "clock": receipt.clock,
            "seed": receipt.seed,
        },
    }))
}

/// Proof-carrying verification outcome.
enum VerifyOutcome {
    Verified,
    Mismatch(Vec<Value>),
    LoadFailed(String),
}

/// Re-load the surgical output as a real .xlsx (the way a consumer would) and
/// prove every predicted affected cell holds the predicted value. Writes a
/// temp copy in `dir` (removed before returning). This is the proof the
/// receipt's result_hash cannot itself provide: that the written file actually
/// re-opens and computes what the dry-run predicted.
fn verify_output(
    new_bytes: &[u8],
    predicted: &BTreeMap<Pos, Value>,
    sheet_names: &[String],
    dir: &std::path::Path,
    tag: &str,
) -> Result<VerifyOutcome> {
    let verify_path = dir.join(format!(".xlq-verify-{tag}.xlsx"));
    let verify_str = verify_path.to_string_lossy().into_owned();
    std::fs::write(&verify_path, new_bytes)
        .with_context(|| "write verification copy".to_string())?;
    let loaded =
        ironcalc::import::load_from_xlsx(&verify_str, "en", "UTC", "en").map_err(|e| anyhow!(e));
    let outcome = match loaded {
        Err(e) => VerifyOutcome::LoadFailed(format!("{e:#}")),
        Ok(mut vmodel) => {
            vmodel.evaluate();
            let mut mismatches: Vec<Value> = Vec::new();
            for (&(s, row, col), want) in predicted {
                let got = crate::value::raw_cell_value(&vmodel, s, row, col).unwrap_or(Value::Null);
                if &got != want {
                    let sheet = sheet_names
                        .get(s as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("sheet_{s}"));
                    mismatches.push(json!({
                        "sheet": sheet, "cell": cell_ref(col, row),
                        "predicted": want, "actual_in_output": got,
                    }));
                }
            }
            if mismatches.is_empty() {
                VerifyOutcome::Verified
            } else {
                VerifyOutcome::Mismatch(mismatches)
            }
        }
    };
    let _ = std::fs::remove_file(&verify_path);
    Ok(outcome)
}

/// The raw `ops` array straight from the patch file, so the receipt records
/// exactly what was requested without depending on Op: Serialize.
fn raw_ops(patch_path: &str) -> Option<Value> {
    let text = std::fs::read_to_string(patch_path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("ops").cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xlq-apply-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A tiny workbook: A1=10, A2=20, A3=SUM(A1:A2). Saved via ironcalc.
    fn build_fixture(path: &str) {
        let mut model = Model::new_empty("fixture", "en", "UTC", "en").unwrap();
        model.set_user_input(0, 1, 1, "10".to_string()).unwrap();
        model.set_user_input(0, 2, 1, "20".to_string()).unwrap();
        model
            .set_user_input(0, 3, 1, "=SUM(A1:A2)".to_string())
            .unwrap();
        model.evaluate();
        let _ = std::fs::remove_file(path);
        ironcalc::export::save_to_xlsx(&model, path).unwrap();
    }

    fn write_patch(path: &str, base_hash: &str) {
        let patch = json!({
            "base_hash": base_hash,
            "actor": "tester",
            "ops": [{"type": "set_cell", "sheet": "Sheet1", "cell": "A1", "value": 100}],
            "watch": ["Sheet1!A3"],
            "clock": null,
            "seed": null,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&patch).unwrap()).unwrap();
    }

    // Author a workbook and return its bytes; A1=v. Unique temp path per call
    // (parallel tests must not share a file).
    fn book_bytes(v: &str) -> Vec<u8> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = tmpdir("vbytes");
        let uniq = N.fetch_add(1, Ordering::Relaxed);
        let p = dir.join(format!("b-{v}-{uniq}.xlsx"));
        let ps = p.to_str().unwrap();
        let mut model = Model::new_empty("f", "en", "UTC", "en").unwrap();
        model.set_user_input(0, 1, 1, v.to_string()).unwrap();
        model.evaluate();
        let _ = std::fs::remove_file(ps);
        ironcalc::export::save_to_xlsx(&model, ps).unwrap();
        let b = std::fs::read(ps).unwrap();
        let _ = std::fs::remove_file(ps);
        b
    }

    #[test]
    fn real_apply_refuses_when_affected_uses_an_oracle_divergent_function() {
        // The differential oracle is load-bearing for write safety: a formula
        // using CONVERT (where the engine is known to disagree with Excel) must
        // not have its engine-computed cache committed.
        let dir = tmpdir("divergent");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let base = crate::hash::sha256_file(book).unwrap();
        let patch = dir.join("p.json");
        let patch = patch.to_str().unwrap();
        std::fs::write(
            patch,
            serde_json::to_vec(&json!({
                "base_hash": base,
                "ops": [{"type":"set_formula","sheet":"Sheet1","cell":"B1","formula":"CONVERT(68,\"F\",\"C\")"}],
            }))
            .unwrap(),
        )
        .unwrap();
        let before = crate::hash::sha256_file(book).unwrap();
        let report = run(book, patch, false, None).unwrap();
        assert_eq!(report["error"], json!("coverage_unreliable"));
        assert_eq!(report["oracle_divergent_functions"], json!(["CONVERT"]));
        // The file was NOT written.
        assert_eq!(crate::hash::sha256_file(book).unwrap(), before);
    }

    #[test]
    fn verify_output_passes_when_prediction_matches() {
        let dir = tmpdir("v-ok");
        let bytes = book_bytes("42");
        let mut predicted: BTreeMap<Pos, Value> = BTreeMap::new();
        predicted.insert((0, 1, 1), json!(42.0));
        let out = verify_output(&bytes, &predicted, &["Sheet1".into()], &dir, "ok").unwrap();
        assert!(matches!(out, VerifyOutcome::Verified));
    }

    #[test]
    fn verify_output_catches_a_wrong_landed_value() {
        // The output really holds A1=42, but we predicted 999: proof-carrying
        // apply must catch the disagreement and refuse (this is the guarantee
        // that a subtly-wrong surgical write cannot be committed).
        let dir = tmpdir("v-bad");
        let bytes = book_bytes("42");
        let mut predicted: BTreeMap<Pos, Value> = BTreeMap::new();
        predicted.insert((0, 1, 1), json!(999.0));
        let out = verify_output(&bytes, &predicted, &["Sheet1".into()], &dir, "bad").unwrap();
        match out {
            VerifyOutcome::Mismatch(m) => {
                assert_eq!(m.len(), 1);
                assert_eq!(m[0]["predicted"], json!(999.0));
                assert_eq!(m[0]["actual_in_output"], json!(42.0));
            }
            _ => panic!("expected Mismatch, prediction disagreed with output"),
        }
    }

    #[test]
    fn verify_output_catches_unloadable_output() {
        let dir = tmpdir("v-corrupt");
        let mut predicted: BTreeMap<Pos, Value> = BTreeMap::new();
        predicted.insert((0, 1, 1), json!(1.0));
        let out = verify_output(
            b"not a zip at all",
            &predicted,
            &["Sheet1".into()],
            &dir,
            "corrupt",
        )
        .unwrap();
        assert!(matches!(out, VerifyOutcome::LoadFailed(_)));
    }

    #[test]
    fn dry_run_predicts_affected_set_without_writing() {
        let dir = tmpdir("dry");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let base = crate::hash::sha256_file(book).unwrap();
        let patch = dir.join("patch.json");
        let patch = patch.to_str().unwrap();
        write_patch(patch, &base);

        let report = run(book, patch, true, None).expect("dry run");
        assert_eq!(report["command"], json!("apply"));
        assert_eq!(report["dry_run"], json!(true));

        // Setting A1 10->100 changes A1 and downstream A3 (30 -> 120).
        let affected = report["affected"].as_array().unwrap();
        let cells: BTreeSet<&str> = affected
            .iter()
            .map(|a| a["cell"].as_str().unwrap())
            .collect();
        assert!(cells.contains("A1"), "affected: {cells:?}");
        assert!(cells.contains("A3"), "affected: {cells:?}");
        assert_eq!(cells.len(), 2, "affected: {cells:?}");

        let watch = &report["watch"][0];
        assert_eq!(watch["cell"], json!("Sheet1!A3"));
        assert_eq!(watch["before"], json!(30.0));
        assert_eq!(watch["after"], json!(120.0));
        assert_eq!(report["coverage"]["write_reliable"], json!(true));

        // The dry run wrote nothing: the file hash is unchanged.
        assert_eq!(crate::hash::sha256_file(book).unwrap(), base);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revision_mismatch_is_reported_and_writes_nothing() {
        let dir = tmpdir("mismatch");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let patch = dir.join("patch.json");
        let patch = patch.to_str().unwrap();
        write_patch(patch, "deadbeef");

        let report = run(book, patch, false, None).expect("mismatch is Ok payload");
        assert_eq!(report["error"], json!("revision_mismatch"));
        assert_eq!(
            report["actual"],
            json!(crate::hash::sha256_file(book).unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_apply_commits_rev_and_receipt_and_reloads() {
        let dir = tmpdir("real");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let base = crate::hash::sha256_file(book).unwrap();
        let patch = dir.join("patch.json");
        let patch = patch.to_str().unwrap();
        write_patch(patch, &base);

        let report = run(book, patch, false, Some("integration")).expect("real apply");
        assert_eq!(report["dry_run"], json!(false));
        assert_eq!(report["base_hash"], json!(base));
        assert_eq!(report["receipt"]["actor"], json!("integration"));
        let rev = report["rev"].as_u64().unwrap();

        // Fidelity: at least one part rewritten, and most parts preserved.
        let identical = report["fidelity"]["parts_byte_identical"].as_u64().unwrap();
        let rewritten = report["fidelity"]["parts_rewritten"].as_u64().unwrap();
        assert!(rewritten >= 1, "expected a rewritten sheet part");
        assert!(identical >= 1, "expected preserved parts");

        // A rev file and a journal entry appear next to the book. journal names
        // the rev file `<book_path>.rev-N.xlsx` (book_path keeps its extension).
        let rev_file = dir.join(format!("book.xlsx.rev-{rev}.xlsx"));
        assert!(rev_file.exists(), "rev file missing: {rev_file:?}");
        let jpath = journal::journal_path(book);
        assert!(std::path::Path::new(&jpath).exists(), "journal missing");

        // The committed file reloads in ironcalc with A1=100, A3=120.
        let result_hash = report["result_hash"].as_str().unwrap();
        assert_eq!(crate::hash::sha256_file(book).unwrap(), result_hash);
        let mut model = ironcalc::import::load_from_xlsx(book, "en", "UTC", "en").unwrap();
        model.evaluate();
        assert_eq!(model.get_formatted_cell_value(0, 1, 1).unwrap(), "100");
        assert_eq!(model.get_formatted_cell_value(0, 3, 1).unwrap(), "120");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_patch_ops(path: &str, base_hash: &str, ops: Value, watch: Value) {
        let patch = json!({
            "base_hash": base_hash,
            "actor": "tester",
            "ops": ops,
            "watch": watch,
            "clock": 1751500000000i64,
            "seed": 1,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&patch).unwrap()).unwrap();
    }

    // An edit made outside xlq (breaking the chain) must be REFUSED with
    // external_edit_detected + a marker; only a re-issued patch may proceed.
    #[test]
    fn external_edit_is_refused_then_reissue_proceeds() {
        let dir = tmpdir("extedit");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let base = crate::hash::sha256_file(book).unwrap();
        let patch = dir.join("p1.json");
        let patch = patch.to_str().unwrap();
        write_patch(patch, &base);

        // First apply commits cleanly (chain = genesis).
        let r1 = run(book, patch, false, Some("t")).expect("first apply");
        assert_eq!(r1["dry_run"], json!(false));
        assert_eq!(r1["chain"], json!("genesis"));

        // Simulate an EXTERNAL edit: rewrite the file outside xlq so its hash no
        // longer matches the last receipt's result_hash.
        {
            let mut m = ironcalc::import::load_from_xlsx(book, "en", "UTC", "en").unwrap();
            let s = m
                .get_worksheets_properties()
                .iter()
                .position(|p| p.name == "Sheet1")
                .unwrap() as u32;
            m.set_user_input(s, 2, 1, "77".to_string()).unwrap();
            m.evaluate();
            let _ = std::fs::remove_file(book);
            ironcalc::export::save_to_xlsx(&m, book).unwrap();
        }
        let ext_hash = crate::hash::sha256_file(book).unwrap();
        let patch2 = dir.join("p2.json");
        let patch2 = patch2.to_str().unwrap();
        write_patch_ops(
            patch2,
            &ext_hash,
            json!([{"type": "set_cell", "sheet": "Sheet1", "cell": "A1", "value": 500}]),
            json!([]),
        );

        // First run against the diverged file: REFUSED, marker appended, nothing
        // written (file hash unchanged).
        let refused = run(book, patch2, false, Some("t")).expect("refusal is Ok payload");
        assert_eq!(refused["error"], json!("external_edit_detected"));
        assert_eq!(
            crate::hash::sha256_file(book).unwrap(),
            ext_hash,
            "file was written on refusal"
        );

        // Re-issue the SAME patch: the marker adopted ext_hash, so the chain is
        // Ok now and the apply proceeds.
        let ok = run(book, patch2, false, Some("t")).expect("reissue");
        assert_eq!(ok["error"], json!(null));
        assert_eq!(ok["chain"], json!("ok"));
        let mut m = ironcalc::import::load_from_xlsx(book, "en", "UTC", "en").unwrap();
        m.evaluate();
        assert_eq!(m.get_formatted_cell_value(0, 1, 1).unwrap(), "500");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A patch whose affected graph uses a non-deterministic volatile (NOW) is
    // reported unreliable in dry-run and REFUSED (never committed) on real apply.
    #[test]
    fn volatile_affected_write_is_refused() {
        let dir = tmpdir("volatile");
        let book = dir.join("book.xlsx");
        let book = book.to_str().unwrap();
        build_fixture(book);
        let base = crate::hash::sha256_file(book).unwrap();
        let patch = dir.join("p.json");
        let patch = patch.to_str().unwrap();
        write_patch_ops(
            patch,
            &base,
            json!([{"type": "set_formula", "sheet": "Sheet1", "cell": "A1", "formula": "=NOW()"}]),
            json!([]),
        );

        // Dry-run: reports the prediction but flags it unreliable.
        let dry = run(book, patch, true, None).expect("dry run");
        assert_eq!(dry["coverage"]["write_reliable"], json!(false));
        assert_eq!(
            dry["coverage"]["nondeterministic_in_affected"],
            json!(["NOW"])
        );

        // Real apply: refuses and writes nothing.
        let real = run(book, patch, false, None).expect("refusal is Ok payload");
        assert_eq!(real["error"], json!("coverage_unreliable"));
        assert_eq!(real["nondeterministic_functions"], json!(["NOW"]));
        assert_eq!(
            crate::hash::sha256_file(book).unwrap(),
            base,
            "file must be untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
