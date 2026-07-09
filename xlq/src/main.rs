mod apply;
mod calc;
mod census;
mod certify;
mod diff;
mod hash;
mod inspect;
mod journal;
mod ooxml;
mod patch;
mod refshift;
mod restructure;
mod structural;
mod value;

use clap::{Parser, Subcommand};

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
        /// Path to the .xlsx file to modify
        file: String,
        /// Path to the patch JSON (base_hash + typed ops); see patch.rs
        patch: String,
        /// Predict affected cells / new errors / watch values without writing
        #[arg(long)]
        dry_run: bool,
        /// Actor recorded in the receipt (falls back to $XLQ_ACTOR, else "unknown")
        #[arg(long)]
        actor: Option<String>,
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

fn main() {
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
        } => apply::run(&file, &patch, dry_run, actor.as_deref()),
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
    };
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("serialize report")
            );
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
