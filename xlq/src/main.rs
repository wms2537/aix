mod apply;
mod calc;
mod census;
mod diff;
mod hash;
mod inspect;
mod journal;
mod ooxml;
mod patch;
mod refshift;
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
