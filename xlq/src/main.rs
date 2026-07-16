mod apply;
mod calc;
mod census;
mod certify;
mod diff;
mod hash;
mod inspect;
mod journal;
mod log;
mod ooxml;
mod patch;
mod refshift;
mod restructure;
mod structural;
mod undo;
mod value;
mod verify;

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests_cache_soundness;
#[cfg(test)]
mod tests_corpus_lint;

use clap::{Parser, Subcommand};
use std::sync::Mutex;

/// xlq — agent-safe operations on Excel workbooks.
///
/// All commands emit machine-readable JSON on stdout; logs and diagnostics
/// go to stderr. Read commands never modify the target file.
#[derive(Parser)]
#[command(name = "xlq", version, about, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Privacy-safe census of a workbook: sheets, formula/function tallies,
    /// error counts, unsupported features, file hash. No cell values.
    Inspect {
        /// Path to the .xlsx file
        file: String,
        /// Replace sheet and defined names with anonymous placeholders
        #[arg(long)]
        redact: bool,
    },
    /// Cell-level positional diff of two workbooks (values and formulas).
    Diff {
        /// Baseline .xlsx
        old: String,
        /// Comparison .xlsx
        new: String,
    },
    /// Headless recalculation, report-only: compares values stored in the
    /// file with freshly recomputed values. Never writes.
    Calc {
        /// Path to the .xlsx file
        file: String,
    },
    /// Apply a typed patch surgically: rewrites only the sheet parts that
    /// contain a changed cell, leaving every other OOXML part (charts,
    /// pivots, VBA, styles) byte-identical to the input. --dry-run predicts
    /// the effect without writing.
    Apply {
        /// Path to the .xlsx file to modify (omit only with --schema)
        file: Option<String>,
        /// Path to the patch JSON (base_hash + typed ops); see patch.rs (omit only with --schema)
        patch: Option<String>,
        /// Predict affected cells / new errors / watch values without writing
        #[arg(long)]
        dry_run: bool,
        /// Actor recorded in the receipt (falls back to $XLQ_ACTOR, else "unknown")
        #[arg(long)]
        actor: Option<String>,
        /// Print the JSON Schema of the patch format and exit (no file needed)
        #[arg(long)]
        schema: bool,
    },
    /// Surgical structural edit: insert/delete rows or columns, shifting every
    /// reference (formulas, cross-sheet, defined names, charts, pivots) via the
    /// reference-shift algebra while keeping non-coordinate bytes identical.
    /// --dry-run predicts the shift without writing.
    Restructure {
        /// Path to the .xlsx file to modify
        file: String,
        /// Sheet to edit
        #[arg(long)]
        sheet: String,
        /// Operation: insert-rows | delete-rows | insert-cols | delete-cols | move-rows
        #[arg(long)]
        op: String,
        /// 1-based row/column index to insert before / start deleting at / start moving from
        #[arg(long)]
        at: u32,
        /// Number of rows/columns
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// move-rows only: 1-based ORIGINAL-coordinate row to move the block before (required)
        #[arg(long, default_value_t = 0)]
        dest: u32,
        /// Predict the shift without writing
        #[arg(long)]
        dry_run: bool,
        /// Actor recorded in the receipt
        #[arg(long)]
        actor: Option<String>,
    },
    /// Engine-free certification that a FOREIGN edited workbook equals xlq's own
    /// proven-faithful structural transform of the original. Computes xlq's
    /// transform of <original>, then certifies <edited> matches it up to stripped
    /// caches / number formats — REFUSING on any formula/value/added/removed
    /// difference (or when xlq cannot itself prove the op on this file).
    Certify {
        /// Original .xlsx (the baseline the transform is computed from)
        original: String,
        /// Edited .xlsx (the untrusted foreign edit to certify)
        edited: String,
        /// Sheet the structural edit was applied to
        #[arg(long)]
        sheet: String,
        /// Operation: insert-rows | delete-rows | insert-cols | delete-cols | move-rows
        #[arg(long)]
        op: String,
        /// 1-based row/column index the op was applied at
        #[arg(long)]
        at: u32,
        /// Number of rows/columns
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// move-rows only: 1-based ORIGINAL-coordinate row the block was moved before
        #[arg(long, default_value_t = 0)]
        dest: u32,
    },
    /// Print the receipt history recorded in <file>.xlq.jsonl (rev, kind,
    /// timestamp, hashes, actor, per-entry chain-linkage). Read-only.
    Log {
        /// Path to the .xlsx file
        file: String,
    },
    /// Recompute <file>'s hash and check it against the latest receipt's
    /// result_hash, plus the whole receipt hash-chain linkage. Detects
    /// out-of-band tampering; exits 1 when verification fails.
    Verify {
        /// Path to the .xlsx file
        file: String,
    },
    /// Transactionally restore the previous committed snapshot (records a new
    /// 'undo' receipt). Fails closed on a missing/corrupt backup or an
    /// out-of-band edit.
    Undo {
        /// Path to the .xlsx file
        file: String,
        /// Actor recorded in the receipt
        #[arg(long)]
        actor: Option<String>,
    },
    /// Deliberately panic — the only reliable trigger for the panic-firewall
    /// integration test (hidden; namespaced with __; never a user surface).
    #[command(name = "__panic", hide = true)]
    Panic,
    /// Test-only batch driver for the Lean↔Rust tokenizer differential
    /// (hidden from help; not part of the public CLI surface). Reads TSV
    /// lines from stdin — formula \t axis(row|col) \t op(insert|delete)
    /// \t at \t count — and prints exactly one line per input line: the
    /// shifted formula (tabs/newlines escaped as \t and \n), or the
    /// literal token __REFUSE__ when the formula trips the fail-closed
    /// unquoted-non-ASCII-qualifier guard.
    #[command(name = "__shift-formula-batch", hide = true)]
    ShiftFormulaBatch,
}

fn parse_structural_op(op: &str) -> Option<(refshift::Axis, refshift::Op)> {
    use refshift::{Axis, Op};
    match op {
        "insert-rows" => Some((Axis::Row, Op::Insert)),
        "delete-rows" => Some((Axis::Row, Op::Delete)),
        "insert-cols" => Some((Axis::Col, Op::Insert)),
        "delete-cols" => Some((Axis::Col, Op::Delete)),
        "move-rows" => Some((Axis::Row, Op::Move)),
        _ => None,
    }
}

/// Abort the batch loudly on a malformed input line: a differential harness
/// must fail closed, never silently skew the line pairing.
fn batch_die(lineno: usize, msg: &str) -> ! {
    eprintln!("__shift-formula-batch: line {lineno}: {msg}");
    std::process::exit(2);
}

/// Test-only stdin driver behind the hidden `__shift-formula-batch`
/// subcommand: one TSV line in, exactly one line out. Bypasses the JSON
/// report path — output is the raw shifted formula per line.
fn shift_formula_batch() {
    use refshift::{Axis, Op, StructuralEdit};
    use std::io::BufRead;
    let stdin = std::io::stdin();
    for (idx, line) in stdin.lock().lines().enumerate() {
        let lineno = idx + 1;
        let line = line.unwrap_or_else(|e| batch_die(lineno, &format!("stdin read: {e}")));
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 5 {
            batch_die(
                lineno,
                "expected 5 tab-separated fields: formula, axis, op, at, count",
            );
        }
        let formula = fields[0];
        let axis = match fields[1] {
            "row" => Axis::Row,
            "col" => Axis::Col,
            _ => batch_die(lineno, "axis must be row|col"),
        };
        let op = match fields[2] {
            "insert" => Op::Insert,
            "delete" => Op::Delete,
            _ => batch_die(lineno, "op must be insert|delete"),
        };
        let at: u32 = fields[3]
            .parse()
            .unwrap_or_else(|_| batch_die(lineno, "at must be a u32"));
        let count: u32 = fields[4]
            .parse()
            .unwrap_or_else(|_| batch_die(lineno, "count must be a u32"));
        // Mirror the edit layer's fail-closed guard: refuse formulas with
        // unquoted non-ASCII sheet qualifiers instead of shifting them.
        if refshift::has_unquoted_non_ascii_qualifier(formula) {
            println!("__REFUSE__");
            continue;
        }
        let edit = StructuralEdit {
            axis,
            at,
            count,
            op,
            sheet: "S".into(),
            dest: 0,
        };
        let (shifted, _) = refshift::shift_formula(formula, "S", &edit);
        println!("{}", shifted.replace('\t', "\\t").replace('\n', "\\n"));
    }
}

/// Captured panic message (basename-sanitized location + payload), set by the
/// panic hook and read by `main()` after `catch_unwind`. A panic is fatal, so a
/// single last-writer-wins slot is sufficient.
static PANIC_MSG: Mutex<Option<String>> = Mutex::new(None);

/// Install a panic hook that captures a machine-usable, path-safe message and
/// suppresses Rust's default multi-line stderr dump. The source LOCATION is
/// reduced to its basename: a panic inside a dependency carries an absolute
/// `file!()` under `~/.cargo/registry`, which would otherwise leak the home
/// directory into the stdout JSON — the no-full-paths guarantee must hold even
/// on a crash.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "internal panic".to_string());
        let loc = info
            .location()
            .map(|l| {
                let base = std::path::Path::new(l.file())
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("{base}:{}", l.line())
            })
            .unwrap_or_else(|| "unknown".to_string());
        if let Ok(mut slot) = PANIC_MSG.lock() {
            *slot = Some(format!("internal error at {loc}: {payload}"));
        }
    }));
}

fn main() {
    // Behave like a standard Unix filter under early pipe closure. Rust's runtime
    // sets SIGPIPE to SIG_IGN, so a closed stdout (`xlq inspect big.xlsx | head`)
    // turns the next `println!` into a BrokenPipe panic. Restoring SIG_DFL makes
    // the process die cleanly on a closed pipe (exit 141) and sidesteps the
    // chicken-and-egg of writing a JSON error to an already-dead stdout. Must run
    // before any output.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    install_panic_hook();

    // Firewall: a genuine internal panic becomes a machine-readable JSON error on
    // stdout with a stable exit 70 (EX_SOFTWARE) — distinct from the 0/1/2
    // refusal/usage contract — instead of a raw multi-line panic + exit 101. The
    // intended exit codes are unaffected: run()'s std::process::exit(..) calls
    // terminate WITHOUT unwinding, so they never reach this arm.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_err() {
        let msg = PANIC_MSG
            .lock()
            .ok()
            .and_then(|m| m.clone())
            .unwrap_or_else(|| "internal error".to_string());
        eprintln!("xlq internal error: {msg}");
        println!(
            "{}",
            serde_json::json!({ "error": msg, "internal_error": true })
        );
        std::process::exit(70);
    }
}

fn run() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Inspect { file, redact } => inspect::run(&file, redact),
        Command::Diff { old, new } => diff::run(&old, &new),
        Command::Calc { file } => calc::run(&file),
        Command::Apply {
            file,
            patch,
            dry_run,
            actor,
            schema,
        } => {
            if schema {
                // --schema needs no file: print the patch JSON Schema and exit 0.
                Ok(serde_json::json!({ "command": "apply", "schema": patch::schema() }))
            } else {
                match (file, patch) {
                    (Some(f), Some(p)) => apply::run(&f, &p, dry_run, actor.as_deref()),
                    // Positionals are optional only so --schema can omit them;
                    // otherwise both are required. bad_args -> exit 2 (usage),
                    // preserving what clap's own required-arg error used to give.
                    _ => Ok(serde_json::json!({
                        "command": "apply",
                        "error": "bad_args",
                        "reason": "file and patch are required unless --schema is given",
                    })),
                }
            }
        }
        Command::Restructure {
            file,
            sheet,
            op,
            at,
            count,
            dest,
            dry_run,
            actor,
        } => match parse_structural_op(&op) {
            Some((axis, operation)) => restructure::run(
                &file,
                &sheet,
                axis,
                operation,
                at,
                count,
                dest,
                dry_run,
                actor.as_deref(),
            ),
            None => Ok(serde_json::json!({
                "command": "restructure",
                "error": "bad_op",
                "reason": "--op must be insert-rows | delete-rows | insert-cols | delete-cols | move-rows",
            })),
        },
        Command::Certify {
            original,
            edited,
            sheet,
            op,
            at,
            count,
            dest,
        } => certify::run(&original, &edited, &sheet, &op, at, count, dest),
        Command::Log { file } => log::run(&file),
        Command::Verify { file } => verify::run(&file),
        Command::Undo { file, actor } => undo::run(&file, actor.as_deref()),
        Command::Panic => panic!("deliberate test panic — firewall check"),
        Command::ShiftFormulaBatch => {
            shift_formula_batch();
            return;
        }
    };
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("serialize report")
            );
            // Uniform exit-code contract (the JSON on stdout is unchanged):
            //   0 = the operation produced its intended effect/answer,
            //   1 = an operational refusal or failure (certify REFUSED, a
            //       restructure residual/verification failure, …),
            //   2 = a malformed invocation (bad --op / bad args).
            // This lets an agent branch on `xlq certify …` in a shell: a refusal
            // must NOT read as success, which is the bug this fixes.
            let code = outcome_exit_code(&value);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Err(err) => {
            eprintln!("xlq error: {err:#}");
            // This payload is machine-readable stdout: error messages built
            // by the commands carry file BASENAMES only (never full paths),
            // so the no-full-paths-in-stdout guarantee holds on failure too.
            let payload = serde_json::json!({ "error": format!("{err:#}") });
            println!("{payload}");
            std::process::exit(1);
        }
    }
}

/// Map a command's `Ok` JSON to an exit code (see the contract at the call site).
/// The JSON payload itself is never modified — only the process exit status.
fn outcome_exit_code(v: &serde_json::Value) -> i32 {
    // Malformed invocation → 2 (usage), consistent with clap's own arg errors.
    if let Some(kind) = v.get("error").and_then(|e| e.as_str()) {
        return if matches!(kind, "bad_op" | "bad_args") {
            2
        } else {
            1
        };
    }
    // Any other top-level error object, an explicit REFUSED, or a write that
    // did not verify → 1 (operational refusal/failure).
    if v.get("error").is_some() {
        return 1;
    }
    if v.get("status").and_then(|s| s.as_str()) == Some("REFUSED") {
        return 1;
    }
    if v.get("verified") == Some(&serde_json::Value::Bool(false)) {
        return 1;
    }
    0
}
