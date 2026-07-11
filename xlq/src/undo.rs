//! `xlq undo <file>` — transactionally restore the previous committed snapshot.
//!
//! Undo is a wholesale snapshot restore, so it needs NO knowledge of the op it
//! reverses: it hands the prior snapshot's full bytes to the SAME journal::commit
//! the write path uses, inheriting lock -> immutable rev file -> fsync -> atomic
//! rename -> appended receipt with zero write-discipline duplication. It targets
//! the receipt whose result_hash == the head receipt's base_hash (the state
//! before the last apply/restructure), which keeps the hash chain linked so a
//! following `verify` passes and a following `apply` sees ChainStatus::Ok.
//!
//! Fails closed: a missing or corrupt backup, no prior snapshot (genesis), or an
//! out-of-band edit each refuse and leave the file untouched (exit 1).

use anyhow::Result;
use serde_json::{json, Value};

use crate::journal::{self, ChainStatus};

fn get_str<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn get_u64(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

pub fn run(file: &str, actor: Option<&str>) -> Result<Value> {
    // Hold the advisory lock across the whole operation (same as apply/restructure).
    let _lock = journal::lock(file)?;

    let entries = journal::read_entries(file)?;
    if entries.is_empty() {
        return Ok(json!({ "command": "undo", "error": "nothing_to_undo" }));
    }

    let current = crate::hash::sha256_file(file)?;
    let timestamp = journal::iso_timestamp(None);
    let resolved_actor = journal::resolve_actor(actor);

    // Refuse an out-of-band edit — record an adoption marker and stop, exactly as
    // apply/restructure do, so a re-run then undoes cleanly.
    match journal::chain_status(file, &current)? {
        ChainStatus::Ok => {}
        ChainStatus::Genesis => {
            return Ok(json!({ "command": "undo", "error": "nothing_to_undo" }));
        }
        ChainStatus::ExternalEdit => {
            let marker =
                journal::append_adoption_marker(file, &current, &timestamp, &resolved_actor)?;
            return Ok(json!({
                "command": "undo",
                "error": "external_edit_detected",
                "actual": current,
                "adopted_rev": marker.rev,
            }));
        }
    }

    // The state before the last committed op is the receipt whose result_hash
    // equals the head receipt's base_hash. Scan from the END so repeated undo/redo
    // toggles pick the most recent matching snapshot.
    let head = entries.last().expect("non-empty");
    let head_rev = get_u64(head, "rev");
    let target_hash = get_str(head, "base_hash").to_string();
    let target = match entries.iter().rev().find(|e| get_str(e, "result_hash") == target_hash) {
        Some(t) => t,
        // Genesis apply's base is the pristine ORIGINAL, which was never snapshotted
        // as a rev file (commit overwrites the book with the result), so there is
        // nothing to restore to.
        None => return Ok(json!({ "command": "undo", "error": "no_prior_snapshot" })),
    };
    let target_rev = get_u64(target, "rev");

    // Fail-closed backup guards: the snapshot must exist and hash to its receipt.
    let snapshot = journal::rev_path(file, target_rev);
    let bytes = match std::fs::read(&snapshot) {
        Ok(b) => b,
        Err(_) => {
            return Ok(json!({
                "command": "undo",
                "error": "backup_missing",
                "restored_rev": target_rev,
            }))
        }
    };
    if crate::hash::sha256_bytes(&bytes) != target_hash {
        return Ok(json!({
            "command": "undo",
            "error": "backup_corrupt",
            "restored_rev": target_rev,
        }));
    }

    // Restore via the shared write discipline. base = current head state, result =
    // the restored prior state, so the chain stays linked (new base == old head
    // result).
    let new_rev = journal::next_rev(file)?;
    let ops = json!({ "undo_of_rev": head_rev, "restored_rev": target_rev });
    let receipt = journal::commit(
        file,
        &bytes,
        new_rev,
        "undo",
        &current,
        &target_hash,
        ops,
        &timestamp,
        &resolved_actor,
        None,
        None,
    )?;

    Ok(json!({
        "command": "undo",
        "rev": receipt.rev,
        "base_hash": current,
        "result_hash": target_hash,
        "undone_rev": head_rev,
        "restored_rev": target_rev,
    }))
}
