//! `xlq log <file>` — read the transactional journal and print receipt history.
//!
//! Pure read: never writes, always exits 0. Per-entry `verified` is journal
//! LINKAGE only (this receipt's base_hash == the previous receipt's result_hash
//! and rev strictly increases); deep file-hash verification is the `verify` verb.
//! Fields are read tolerantly (as JSON values) so a marker line or a future
//! schema addition never breaks a read.

use anyhow::Result;
use serde_json::{json, Value};

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn get_str<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn get_u64(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

pub fn run(file: &str) -> Result<Value> {
    let entries = crate::journal::read_entries(file)?;
    let mut receipts = Vec::with_capacity(entries.len());
    let mut prev: Option<&Value> = None;
    for e in &entries {
        // Genesis (first receipt) is verified by definition; every later receipt
        // must link to the previous one and advance the revision.
        let verified = match prev {
            None => true,
            Some(p) => {
                get_str(e, "base_hash") == get_str(p, "result_hash")
                    && get_u64(e, "rev") > get_u64(p, "rev")
            }
        };
        receipts.push(json!({
            "rev": e.get("rev"),
            "kind": e.get("kind"),
            "timestamp": e.get("timestamp"),
            "base_hash": e.get("base_hash"),
            "result_hash": e.get("result_hash"),
            "actor": e.get("actor"),
            "engine_version": e.get("engine_version"),
            "clock": e.get("clock"),
            "seed": e.get("seed"),
            "verified": verified,
        }));
        prev = Some(e);
    }
    Ok(json!({
        "command": "log",
        "file": basename(file),
        "count": entries.len(),
        "receipts": receipts,
    }))
}
