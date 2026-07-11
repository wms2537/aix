//! The hash-chained receipt journal + lock + rev files + atomic swap.
//! Faithful to docs/receipt-journal-spec.md and docs/specs/v02-architecture.md.
//!
//! CONTRACT:
//!   pub struct Receipt { pub rev: u64, pub kind: String, pub base_hash: String,
//!       pub result_hash: String, pub ops: serde_json::Value, pub timestamp: String,
//!       pub actor: String, pub engine_version: String,
//!       pub clock: Option<i64>, pub seed: Option<u64> }
//!
//!   /// Acquire the advisory lock `<book>.xlq.lock` (O_EXCL); RAII guard
//!   /// releases on drop. Err = `lock_held`.
//!   pub fn lock(book_path: &str) -> Result<LockGuard>
//!
//!   /// Journal path for a workbook: `<book>.xlq.jsonl` (one per original).
//!   pub fn journal_path(book_path: &str) -> String
//!
//!   /// Next rev number = last receipt.rev + 1 (0 if genesis / no journal).
//!   pub fn next_rev(book_path: &str) -> Result<u64>
//!
//!   /// Verify base_hash against the journal chain. Returns:
//!   ///   Genesis (no/empty journal),
//!   ///   Ok (base_hash == last receipt.result_hash),
//!   ///   ExternalEdit (journal exists, current hash != last result_hash) —
//!   ///     caller appends an adoption marker then may proceed.
//!   pub fn chain_status(book_path: &str, current_hash: &str) -> Result<ChainStatus>
//!
//!   /// Write new bytes as `<book>.rev-N.xlsx` (never overwrite an existing
//!   /// rev file), fsync, then atomically rename onto `book_path`. Appends
//!   /// the receipt. Returns the receipt.
//!   pub fn commit(book_path: &str, new_bytes: &[u8], rev: u64, kind: &str, base_hash: &str,
//!       result_hash: &str, ops: serde_json::Value, actor: &str,
//!       clock: Option<i64>, seed: Option<u64>) -> Result<Receipt>
//!
//! actor resolution: explicit arg, else $XLQ_ACTOR, else "unknown".
//! engine_version = the same ENGINE string calc.rs emits.
//! Timestamps: pass from the caller (no wall-clock in library) OR use a fixed
//! recorded clock; determinism per the spec.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

// Single-sourced from the vendored engine (base/src/constants.rs) so a receipt
// records exactly which engine produced its cached values — no hand-sync; the
// version segment tracks the linked crate's Cargo.toml at compile time.
use ironcalc::base::ENGINE_PROVENANCE as ENGINE_VERSION;

#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub rev: u64,
    pub kind: String,
    pub base_hash: String,
    pub result_hash: String,
    pub ops: serde_json::Value,
    pub timestamp: String,
    pub actor: String,
    pub engine_version: String,
    pub clock: Option<i64>,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStatus {
    Genesis,
    Ok,
    ExternalEdit,
}

/// RAII advisory lock. The `<book>.xlq.lock` file is created O_EXCL on
/// acquisition and removed on drop. Holding the guard proves exclusive access.
#[derive(Debug)]
pub struct LockGuard {
    path: String,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best-effort: a failure to unlink (e.g. the file was already removed
        // out from under us) must not panic while unwinding.
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn journal_path(book_path: &str) -> String {
    format!("{book_path}.xlq.jsonl")
}

fn lock_path(book_path: &str) -> String {
    format!("{book_path}.xlq.lock")
}

fn rev_path(book_path: &str, rev: u64) -> String {
    format!("{book_path}.rev-{rev}.xlsx")
}

/// Resolve the receipt actor per the module policy: explicit arg wins, else
/// `$XLQ_ACTOR`, else `"unknown"`. The caller normally resolves this and hands
/// `commit` the final string; exposed so every entry point agrees on the rule.
pub fn resolve_actor(explicit: Option<&str>) -> String {
    match explicit {
        Some(a) if !a.is_empty() => a.to_string(),
        _ => std::env::var("XLQ_ACTOR")
            .ok()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

pub fn lock(book_path: &str) -> Result<LockGuard> {
    let path = lock_path(book_path);
    // create_new => O_EXCL: fails atomically if another holder exists.
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(f) => Ok(finish_lock(f, path)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Stale-lock recovery: a lock left by a crashed/killed run (its pid
            // is no longer alive) would otherwise wedge every future mutation on
            // `lock_held`. Break it exactly once, then retry the O_EXCL create.
            if try_break_stale_lock(&path) {
                match OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(f) => Ok(finish_lock(f, path)),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        Err(anyhow!("lock_held"))
                    }
                    Err(e) => Err(e).context("acquire lock"),
                }
            } else {
                Err(anyhow!("lock_held"))
            }
        }
        Err(e) => Err(e).context("acquire lock"),
    }
}

fn finish_lock(mut f: File, path: String) -> LockGuard {
    // The pid is recorded so a later run can tell a live holder from a stale
    // lock left by a crash (see try_break_stale_lock).
    let _ = writeln!(f, "{}", std::process::id());
    let _ = f.sync_all();
    LockGuard { path }
}

/// If the lock file names a pid that is provably not alive, remove it and
/// report success. Conservative: an unreadable/absent pid, or any pid that
/// might still be alive, is left in place (returns false → `lock_held`), so a
/// live holder is never broken. Guards against pid reuse by only ever breaking
/// a pid confirmed dead, never assuming death.
fn try_break_stale_lock(path: &str) -> bool {
    let pid: i32 = match std::fs::read_to_string(path) {
        Ok(c) => match c.trim().parse() {
            Ok(p) => p,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    if process_alive(pid) {
        return false;
    }
    std::fs::remove_file(path).is_ok()
}

/// Whether `pid` is a live process. Only decisive on Linux (via `/proc`);
/// anywhere else it returns true so a lock is never broken on a platform we
/// cannot check — a stale lock there stays a manual cleanup, but a live lock is
/// never stolen.
fn process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        return Path::new(&format!("/proc/{pid}")).exists();
    }
    #[allow(unreachable_code)]
    {
        true
    }
}

/// All journal receipts as raw JSON, in order (tolerant of extra/missing fields
/// so marker lines and future schema additions never break rev/hash extraction).
///
/// Crash-recovery discipline: `append_receipt` always writes a record then `\n`
/// then fsync, so a durably-committed line ALWAYS ends in a newline. A single
/// crash-torn TRAILING line — identifiable because the file does not end in a
/// newline — was never durably committed and is dropped. Any OTHER unparseable
/// line (an interior line, or a newline-terminated final line) signals real
/// corruption and fails loudly (`journal_corrupt`) rather than silently skipping
/// records. This is the single source of truth for reading the journal, shared by
/// the write path (`last_entry`) and the `log`/`verify`/`undo` read verbs, so a
/// crash-recovered journal reads identically across all of them.
pub(crate) fn read_entries(book_path: &str) -> Result<Vec<serde_json::Value>> {
    let path = journal_path(book_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context("read journal"),
    };
    let has_trailing_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let last_nonblank = match lines.iter().rposition(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => out.push(v),
            Err(e) => {
                // Only a non-newline-terminated FINAL line is a legitimately torn
                // (never-durably-committed) append; drop it. Everything else is
                // real corruption — fail closed.
                if i == last_nonblank && !has_trailing_newline {
                    break;
                }
                return Err(e).context("journal_corrupt: interior journal line failed to parse");
            }
        }
    }
    Ok(out)
}

/// Last journal entry as raw JSON, or None when the journal is absent or holds
/// no valid (non-torn) receipt. Torn-tail/interior-corruption discipline is in
/// [`read_entries`].
fn last_entry(book_path: &str) -> Result<Option<serde_json::Value>> {
    Ok(read_entries(book_path)?.pop())
}

/// Highest N among existing `<book>.rev-N.xlsx` files (0 if none). Consulted so
/// numbering survives a crash between rev-file creation and receipt append: the
/// journal alone would hand back an N whose rev file already exists and wedge
/// every future apply on `rev_exists` (spec: numbering continues from the
/// highest existing rev file + 1; a collision is an error, not an overwrite).
fn highest_rev_file(book_path: &str) -> u64 {
    let path = Path::new(book_path);
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };
    let fname = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return 0,
    };
    let prefix = format!("{fname}.rev-");
    let mut max = 0u64;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    if let Some(num) = rest.strip_suffix(".xlsx") {
                        if let Ok(n) = num.parse::<u64>() {
                            max = max.max(n);
                        }
                    }
                }
            }
        }
    }
    max
}

/// Next revision number: one past whichever is higher — the journal's last rev
/// or the highest existing rev file. Genesis (no journal, no rev files) is 1,
/// per spec (rev starts at 1, strictly increasing by 1 per receipt).
pub fn next_rev(book_path: &str) -> Result<u64> {
    let journal_rev = match last_entry(book_path)? {
        None => 0,
        Some(v) => v
            .get("rev")
            .and_then(|r| r.as_u64())
            .ok_or_else(|| anyhow!("journal_corrupt"))?,
    };
    Ok(journal_rev.max(highest_rev_file(book_path)) + 1)
}

pub fn chain_status(book_path: &str, current_hash: &str) -> Result<ChainStatus> {
    match last_entry(book_path)? {
        None => Ok(ChainStatus::Genesis),
        Some(v) => {
            let last_result = v.get("result_hash").and_then(|h| h.as_str()).unwrap_or("");
            if last_result == current_hash {
                Ok(ChainStatus::Ok)
            } else {
                Ok(ChainStatus::ExternalEdit)
            }
        }
    }
}

fn append_receipt(book_path: &str, receipt: &Receipt) -> Result<()> {
    let path = journal_path(book_path);
    let line = serde_json::to_string(receipt).context("serialize receipt")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("open journal for append")?;
    f.write_all(line.as_bytes()).context("append receipt")?;
    f.write_all(b"\n").context("append receipt")?;
    f.sync_all().context("fsync journal")?;
    Ok(())
}

/// Best-effort fsync of the directory holding `book_path` so a completed
/// rename is durable across a crash. Ignored where unsupported.
fn sync_parent_dir(book_path: &str) {
    let dir = Path::new(book_path).parent();
    let dir = match dir {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };
    if let Ok(f) = File::open(&dir) {
        let _ = f.sync_all();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn commit(
    book_path: &str,
    new_bytes: &[u8],
    rev: u64,
    kind: &str,
    base_hash: &str,
    result_hash: &str,
    ops: serde_json::Value,
    timestamp: &str,
    actor: &str,
    clock: Option<i64>,
    seed: Option<u64>,
) -> Result<Receipt> {
    // 1. Immutable history: write <book>.rev-N.xlsx. create_new refuses to
    //    clobber an existing rev — the chain never rewrites the past.
    let rp = rev_path(book_path, rev);
    let mut rev_file = match OpenOptions::new().write(true).create_new(true).open(&rp) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(anyhow!("rev_exists"))
        }
        Err(e) => return Err(e).context("create rev file"),
    };
    rev_file.write_all(new_bytes).context("write rev file")?;
    rev_file.sync_all().context("fsync rev file")?;
    drop(rev_file);

    // 2. Publish latest: write a sibling temp, fsync, atomically rename onto
    //    the book so the book always holds the newest committed content.
    let tmp = format!("{book_path}.xlq.tmp-{}-{}", std::process::id(), rev);
    let mut tmp_file = File::create(&tmp).context("create temp")?;
    if let Err(e) = tmp_file
        .write_all(new_bytes)
        .and_then(|_| tmp_file.sync_all())
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).context("write temp");
    }
    drop(tmp_file);
    if let Err(e) = std::fs::rename(&tmp, book_path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).context("atomic rename");
    }
    sync_parent_dir(book_path);

    // 3. Record the receipt (append-only, fsynced).
    let receipt = Receipt {
        rev,
        kind: kind.to_string(),
        base_hash: base_hash.to_string(),
        result_hash: result_hash.to_string(),
        ops,
        timestamp: timestamp.to_string(),
        actor: actor.to_string(),
        engine_version: ENGINE_VERSION.to_string(),
        clock,
        seed,
    };
    append_receipt(book_path, &receipt)?;
    Ok(receipt)
}

/// Record that the workbook diverged from the chain (edited outside xlq) and
/// that this run adopts the current on-disk state as legitimate. Journal-only
/// event: no rev file, no atomic swap. Per the spec this is an `external_edit`
/// receipt with its OWN strictly-increasing rev, `base_hash` = the last
/// receipt's `result_hash` (the expectation that broke), `result_hash` =
/// `adopted_hash` (so a subsequent `chain_status` against the adopted hash
/// reports `Ok`), and an empty `ops` array. Errors if there is no prior receipt
/// (adoption only applies after `ExternalEdit`, never at genesis).
pub fn append_adoption_marker(
    book_path: &str,
    adopted_hash: &str,
    timestamp: &str,
    actor: &str,
) -> Result<Receipt> {
    let prev = last_entry(book_path)?.ok_or_else(|| anyhow!("no_chain_to_adopt"))?;
    let prior_hash = prev
        .get("result_hash")
        .and_then(|h| h.as_str())
        .unwrap_or("")
        .to_string();
    let rev = next_rev(book_path)?;
    let receipt = Receipt {
        rev,
        kind: "external_edit".to_string(),
        base_hash: prior_hash,
        result_hash: adopted_hash.to_string(),
        ops: serde_json::Value::Array(Vec::new()),
        timestamp: timestamp.to_string(),
        actor: actor.to_string(),
        engine_version: ENGINE_VERSION.to_string(),
        clock: None,
        seed: None,
    };
    append_receipt(book_path, &receipt)?;
    Ok(receipt)
}

/// Receipt timestamp as ISO-8601 UTC. Single source shared by every writing
/// command (previously each had its own copy — `restructure`'s was broken and
/// stamped `1970-01-01…` on every receipt). Determinism: a pinned `clock` (epoch
/// ms) is used verbatim; otherwise the wall clock is read here, at the write.
pub fn iso_timestamp(clock: Option<i64>) -> String {
    let ms = clock.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    });
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, mi, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Gregorian (year, month, day) for a day count since 1970-01-01
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "xlq-journal-{}-{}-{}",
                tag,
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
        fn book(&self, name: &str) -> String {
            self.path.join(name).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn read_lines(book: &str) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(journal_path(book)).unwrap();
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn genesis_commit_writes_rev1_and_journal_line() {
        let dir = TempDir::new("genesis");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        // Genesis rev is 1 (spec: rev starts at 1).
        assert_eq!(next_rev(&book).unwrap(), 1);
        assert_eq!(
            chain_status(&book, "anything").unwrap(),
            ChainStatus::Genesis
        );

        let r = commit(
            &book,
            b"v1-bytes",
            1,
            "restructure", // a non-"apply" kind — proves the param is honored,
            // not the previously-hardcoded "apply"
            "base0",
            "hash0",
            json!({"set": "A1"}),
            "2026-07-02T00:00:00Z",
            "alice",
            Some(7),
            Some(42),
        )
        .unwrap();

        assert_eq!(r.rev, 1);
        assert_eq!(r.kind, "restructure");
        // and the journal line records the kind too
        assert_eq!(read_lines(&book)[0]["kind"], json!("restructure"));
        assert_eq!(r.engine_version, ENGINE_VERSION);
        assert_eq!(r.actor, "alice");
        assert_eq!(r.clock, Some(7));
        assert_eq!(r.seed, Some(42));

        // rev-1 file holds the new bytes; book was atomically replaced.
        assert_eq!(std::fs::read(rev_path(&book, 1)).unwrap(), b"v1-bytes");
        assert_eq!(std::fs::read(&book).unwrap(), b"v1-bytes");

        let lines = read_lines(&book);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["rev"], json!(1));
        assert_eq!(lines[0]["result_hash"], json!("hash0"));
    }

    // Helper: two clean receipts, then whatever the test appends.
    fn two_receipts(dir: &TempDir, tag: &str) -> String {
        let book = dir.book(tag);
        std::fs::write(&book, b"orig").unwrap();
        commit(&book, b"v1", 1, "apply", "b0", "h1", json!({}), "t", "a", None, None).unwrap();
        commit(&book, b"v2", 2, "apply", "h1", "h2", json!({}), "t", "a", None, None).unwrap();
        book
    }

    #[test]
    fn torn_trailing_line_is_dropped_and_recovers() {
        // A crash mid-append leaves a partial final line with NO trailing newline
        // (append_receipt always writes record + '\n' + fsync). That record was
        // never durably committed, so it must be dropped and the journal recovers
        // — not wedge every future mutation on a hard parse error.
        let dir = TempDir::new("torn");
        let book = two_receipts(&dir, "m.xlsx");
        {
            let mut f = OpenOptions::new().append(true).open(journal_path(&book)).unwrap();
            f.write_all(br#"{"rev":3,"kin"#).unwrap(); // torn: no trailing newline
        }
        assert_eq!(read_entries(&book).unwrap().len(), 2, "torn tail dropped");
        assert_eq!(chain_status(&book, "h2").unwrap(), ChainStatus::Ok);
        let rev = next_rev(&book).unwrap();
        assert_eq!(rev, 3, "numbering continues from the recovered head");
        let r = commit(&book, b"v3", rev, "apply", "h2", "h3", json!({}), "t", "a", None, None)
            .unwrap();
        assert_eq!(r.rev, 3, "a fresh commit at the recovered rev succeeds");
    }

    #[test]
    fn interior_corruption_fails_loudly() {
        // A newline-terminated but unparseable INTERIOR line is real corruption,
        // not a torn append — fail closed, never silently skip to a later line.
        let dir = TempDir::new("interior");
        let book = two_receipts(&dir, "m.xlsx");
        let text = std::fs::read_to_string(journal_path(&book)).unwrap();
        let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        lines[0] = "{BROKEN".to_string(); // corrupt the FIRST line, keep newline-terminated
        std::fs::write(journal_path(&book), lines.join("\n") + "\n").unwrap();
        let err = format!("{:#}", read_entries(&book).unwrap_err());
        assert!(err.contains("journal_corrupt"), "interior corruption must fail loudly: {err}");
        assert!(next_rev(&book).is_err(), "next_rev must not silently use a later line");
    }

    #[test]
    fn complete_but_corrupt_tail_fails_loudly() {
        // A corrupt FINAL line that DOES end in a newline was durably written and
        // therefore must parse — the newline sentinel distinguishes it from a torn
        // append (which lacks the newline). Fail closed.
        let dir = TempDir::new("badtail");
        let book = two_receipts(&dir, "m.xlsx");
        {
            let mut f = OpenOptions::new().append(true).open(journal_path(&book)).unwrap();
            f.write_all(b"{durably-corrupt}\n").unwrap();
        }
        assert!(
            read_entries(&book).is_err(),
            "newline-terminated corrupt line must fail (not treated as torn)"
        );
    }

    #[test]
    fn second_commit_chains_to_rev2() {
        let dir = TempDir::new("chain");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        commit(
            &book, b"v1", 1, "apply", "base0", "hash0", json!({}), "t0", "a", None, None,
        )
        .unwrap();

        assert_eq!(next_rev(&book).unwrap(), 2);
        assert_eq!(chain_status(&book, "hash0").unwrap(), ChainStatus::Ok);

        // base_hash of rev-2 equals the previous receipt's result_hash.
        let r2 = commit(
            &book, b"v2", 2, "apply", "hash0", "hash1", json!({}), "t1", "a", None, None,
        )
        .unwrap();
        assert_eq!(r2.rev, 2);
        assert_eq!(r2.base_hash, "hash0");

        assert_eq!(std::fs::read(rev_path(&book, 1)).unwrap(), b"v1");
        assert_eq!(std::fs::read(rev_path(&book, 2)).unwrap(), b"v2");
        assert_eq!(std::fs::read(&book).unwrap(), b"v2");

        let lines = read_lines(&book);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["rev"], json!(2));
        assert_eq!(next_rev(&book).unwrap(), 3);
        assert_eq!(chain_status(&book, "hash1").unwrap(), ChainStatus::Ok);
    }

    // A crash between rev-file creation and receipt append leaves an orphan
    // rev file whose number the journal has not recorded. next_rev must jump
    // past it (not reissue it) so the next commit never wedges on rev_exists.
    #[test]
    fn next_rev_skips_orphan_rev_file() {
        let dir = TempDir::new("orphan");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        commit(
            &book, b"v1", 1, "apply", "b", "h1", json!({}), "t", "a", None, None,
        )
        .unwrap();
        // Simulate the crash: rev-2 exists on disk but no receipt was written.
        std::fs::write(rev_path(&book, 2), b"orphan").unwrap();

        // Journal's last rev is still 1, but next_rev jumps past the orphan.
        assert_eq!(next_rev(&book).unwrap(), 3);
        // The commit at that rev succeeds instead of returning rev_exists.
        let r = commit(
            &book, b"v3", 3, "apply", "h1", "h3", json!({}), "t", "a", None, None,
        )
        .unwrap();
        assert_eq!(r.rev, 3);
    }

    #[test]
    fn commit_refuses_to_overwrite_existing_rev() {
        let dir = TempDir::new("norewrite");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        commit(
            &book, b"v0", 0, "apply", "b", "h0", json!({}), "t", "a", None, None,
        )
        .unwrap();

        // Re-committing rev 0 must fail (history is immutable) and leave the
        // existing rev-0 bytes untouched.
        let err = commit(
            &book, b"OVERWRITE", 0, "apply", "b", "h0b", json!({}), "t", "a", None, None,
        )
        .unwrap_err();
        assert_eq!(format!("{err}"), "rev_exists");
        assert_eq!(std::fs::read(rev_path(&book, 0)).unwrap(), b"v0");
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = TempDir::new("lock");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        let g = lock(&book).unwrap();
        assert!(std::path::Path::new(&lock_path(&book)).exists());

        let err = lock(&book).unwrap_err();
        assert_eq!(format!("{err}"), "lock_held");

        drop(g);
        assert!(!std::path::Path::new(&lock_path(&book)).exists());
        // Reacquirable once released.
        let _g2 = lock(&book).unwrap();
    }

    #[test]
    fn external_edit_detected_against_last_result_hash() {
        let dir = TempDir::new("external");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        commit(
            &book, b"v0", 0, "apply", "b", "hash0", json!({}), "t", "a", None, None,
        )
        .unwrap();

        assert_eq!(chain_status(&book, "hash0").unwrap(), ChainStatus::Ok);
        assert_eq!(
            chain_status(&book, "some-other-hash").unwrap(),
            ChainStatus::ExternalEdit
        );
    }

    #[test]
    fn external_edit_marker_records_divergence_without_rev_file() {
        let dir = TempDir::new("adopt");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        let rev0 = next_rev(&book).unwrap(); // genesis -> 1
        commit(
            &book, b"v1", rev0, "apply", "b", "hash0", json!({}), "t", "a", None, None,
        )
        .unwrap();

        // Someone edited the file outside xlq -> its hash is now "external".
        assert_eq!(
            chain_status(&book, "external").unwrap(),
            ChainStatus::ExternalEdit
        );
        let m = append_adoption_marker(&book, "external", "t2", "a").unwrap();
        // Spec kind vocabulary + strictly-increasing rev + empty ops array.
        assert_eq!(m.kind, "external_edit");
        assert_eq!(m.rev, rev0 + 1);
        assert_eq!(m.base_hash, "hash0");
        assert_eq!(m.result_hash, "external");
        assert_eq!(m.ops, json!([]));

        // No rev FILE was created for the journal-only marker.
        assert!(!std::path::Path::new(&rev_path(&book, m.rev)).exists());
        // The chain now treats the adopted hash as its base.
        assert_eq!(chain_status(&book, "external").unwrap(), ChainStatus::Ok);
        // Numbering keeps strictly increasing past the marker.
        let rev_next = next_rev(&book).unwrap();
        assert_eq!(rev_next, m.rev + 1);

        // A real commit follows on top of the adopted hash.
        commit(
            &book, b"v2", rev_next, "apply", "external", "hash1", json!({}), "t3", "a", None, None,
        )
        .unwrap();
        assert_eq!(std::fs::read(&book).unwrap(), b"v2");
    }

    #[test]
    fn stale_lock_from_dead_pid_is_broken_but_live_pid_is_not() {
        let dir = TempDir::new("stalelock");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();

        // A lock naming a pid that cannot be alive is stale: lock() breaks it.
        std::fs::write(lock_path(&book), "2147483646\n").unwrap();
        let g = lock(&book).unwrap();
        drop(g);
        assert!(!std::path::Path::new(&lock_path(&book)).exists());

        // A lock naming a LIVE pid (our own) must never be broken.
        std::fs::write(lock_path(&book), format!("{}\n", std::process::id())).unwrap();
        assert_eq!(format!("{}", lock(&book).unwrap_err()), "lock_held");
    }

    #[test]
    fn next_rev_and_chain_status_on_missing_journal() {
        let dir = TempDir::new("missing");
        let book = dir.book("model.xlsx");
        std::fs::write(&book, b"original").unwrap();
        assert_eq!(next_rev(&book).unwrap(), 1);
        assert_eq!(chain_status(&book, "x").unwrap(), ChainStatus::Genesis);
    }

    #[test]
    fn resolve_actor_precedence() {
        assert_eq!(resolve_actor(Some("bob")), "bob");
        // Empty explicit falls through to env/unknown.
        std::env::remove_var("XLQ_ACTOR");
        assert_eq!(resolve_actor(Some("")), "unknown");
        assert_eq!(resolve_actor(None), "unknown");
    }
}
