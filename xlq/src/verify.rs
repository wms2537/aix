//! `xlq verify <file>` — out-of-band tamper detection over the journal.
//!
//! Recompute the file's current hash and compare it to the head receipt's
//! result_hash, and walk the receipt hash-chain linkage. The top-level boolean
//! `verified` drives the process exit code (true -> 0, false -> 1) via the
//! uniform contract in main.rs. A file with no journal is a legitimate "nothing
//! to verify against" answer, not a failure -> exit 0.

use anyhow::Result;
use serde_json::{json, Value};

fn get_str<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn get_u64(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

pub fn run(file: &str) -> Result<Value> {
    let entries = crate::journal::read_entries(file)?;
    if entries.is_empty() {
        return Ok(json!({
            "command": "verify",
            "status": "no_journal",
            "verified": Value::Null,
        }));
    }

    let current = crate::hash::sha256_file(file)?;
    let head = entries.last().expect("non-empty");
    let head_result = get_str(head, "result_hash");
    let head_match = current == head_result;

    // Chain linkage: each receipt's base_hash must equal the previous receipt's
    // result_hash, and rev must STRICTLY increase — not by exactly 1, because
    // next_rev may legitimately skip an orphan rev file (see journal::next_rev).
    // Only linkage + strict monotonicity is guaranteed, so that is all we assert.
    let mut breaks = Vec::new();
    for i in 1..entries.len() {
        let cur = &entries[i];
        let prev = &entries[i - 1];
        let base = get_str(cur, "base_hash");
        let prev_result = get_str(prev, "result_hash");
        if base != prev_result || get_u64(cur, "rev") <= get_u64(prev, "rev") {
            breaks.push(json!({
                "at_rev": cur.get("rev"),
                "expected_base": prev_result,
                "found_base": base,
            }));
        }
    }
    let intact = breaks.is_empty();
    let verified = head_match && intact;

    Ok(json!({
        "command": "verify",
        "verified": verified,
        "head": { "expected": head_result, "actual": current, "match": head_match },
        "chain": { "intact": intact, "breaks": breaks },
        "receipts_checked": entries.len(),
    }))
}
