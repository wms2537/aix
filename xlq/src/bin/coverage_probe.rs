//! Coverage probe: which Excel functions does the ironcalc engine recognize?
//!
//! Usage: `cargo run --bin coverage-probe -- <functions.txt>`
//!
//! Reads a newline-separated list of function names from argv[1] (lines
//! starting with `#` and blank lines are ignored), probes each against
//! ironcalc via the `=NAME(1)` / `#NAME?` technique (reusing
//! `census::probe_support` — the exact code path xlq's `inspect` uses),
//! and writes a JSON object `{ "FUNCTION": supported_bool, ... }` to stdout.
//!
//! Semantics: "supported" means the engine RECOGNIZES the name (does not
//! return `#NAME?` for `=NAME(1)`). A recognized function may still reject
//! the probe's dummy argument with #VALUE!/#NUM!/etc. — that is correct
//! Excel behavior (unknown names error before argument validation) and
//! still counts as supported.

// Reuse the tested probe logic from the main binary's census module.
// (xlq has no lib target, so pull the module in by path.)
#[path = "../census.rs"]
#[allow(dead_code)] // only probe_support is used here; the rest serves the xlq binary
mod census;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: coverage-probe <functions.txt>");
        return ExitCode::FAILURE;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let names: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_uppercase())
        .collect();

    let unsupported: BTreeSet<String> = census::probe_support(&names).into_iter().collect();
    let matrix: BTreeMap<String, bool> = names
        .into_iter()
        .map(|n| {
            let supported = !unsupported.contains(&n);
            (n, supported)
        })
        .collect();

    match serde_json::to_string_pretty(&matrix) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: serialize: {e}");
            ExitCode::FAILURE
        }
    }
}
