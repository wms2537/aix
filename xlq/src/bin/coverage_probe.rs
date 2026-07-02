//! Coverage probe: three-number honest accounting of catalog coverage.
//!
//! Usage: `cargo run --bin coverage-probe -- <functions.txt>`
//!
//! Reads a newline-separated list of catalog function names from argv[1]
//! (lines starting with `#` and blank lines are ignored), probes each
//! against ironcalc via `census::probe_support` — the exact code path xlq's
//! `inspect` uses — and writes a JSON report to stdout with the three
//! numbers required by the spec's "Coverage accounting rule"
//! (docs/specs/full-catalog-semantics.md):
//!
//!   1. `catalog_recognized`  — names the engine's parser/dispatcher accepts
//!      (any result but an unknown-name rejection). The seven CUBE functions
//!      answer `#NAME?` BY DESIGN when no OLAP connection exists, so the
//!      plain #NAME? heuristic cannot judge them; `census::probe_support`
//!      carries the carve-out (they are probed with zero arguments, where a
//!      recognized function fails argument-count validation with `#ERROR!`
//!      and an unknown name still answers `#NAME?`).
//!   2. `locally_evaluable`   — recognized minus the policy-limited set:
//!      functions whose real semantics the engine computes from local data.
//!   3. `policy_limited`      — recognized functions whose value depends on
//!      an external service, OLAP connection, PivotTable model, or native
//!      code that xlq refuses to execute by design; each returns the
//!      documented desktop-Excel refusal literal. Emitted with the
//!      per-function literal and one-line reason
//!      (`census::POLICY_LIMITED_FUNCTIONS`, sourced from the spec).
//!
//! Per-name classification is emitted under `functions`
//! (`"locally_evaluable" | "policy_limited" | "unrecognized"`).

// Reuse the tested probe logic from the main binary's census module.
// (xlq has no lib target, so pull the module in by path.)
#[path = "../census.rs"]
#[allow(dead_code)] // only part of the module is used here; the rest serves the xlq binary
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
    // Deduplicate (preserving first-occurrence order) so the three counts
    // are taken over the same universe as `catalog_size` (`matrix.len()`,
    // which deduplicates by construction): a name listed twice in the
    // catalog file must not be counted twice.
    let mut seen = BTreeSet::new();
    let names: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_uppercase())
        .filter(|l| seen.insert(l.clone()))
        .collect();

    let unrecognized: BTreeSet<String> = census::probe_support(&names).into_iter().collect();

    let mut catalog_recognized = 0u64;
    let mut locally_evaluable = 0u64;
    let mut policy_limited = 0u64;
    let mut matrix: BTreeMap<String, &'static str> = BTreeMap::new();
    for name in &names {
        let class = if unrecognized.contains(name) {
            "unrecognized"
        } else if census::policy_limited_literal(name).is_some() {
            catalog_recognized += 1;
            policy_limited += 1;
            "policy_limited"
        } else {
            catalog_recognized += 1;
            locally_evaluable += 1;
            "locally_evaluable"
        };
        matrix.insert(name.clone(), class);
    }

    // Per-function literal + reason for the policy set, restricted to names
    // actually present in the probed catalog (all 17 are, today).
    let probed: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let policy_detail: Vec<serde_json::Value> = census::POLICY_LIMITED_FUNCTIONS
        .iter()
        .filter(|(n, _, _)| probed.contains(n))
        .map(|(name, literal, reason)| {
            serde_json::json!({
                "function": name,
                "literal": literal,
                "reason": reason,
            })
        })
        .collect();

    let report = serde_json::json!({
        "catalog_size": matrix.len(),
        "catalog_recognized": catalog_recognized,
        "locally_evaluable": locally_evaluable,
        "policy_limited": policy_limited,
        "unrecognized": unrecognized,
        "policy_limited_detail": policy_detail,
        "functions": matrix,
    });

    match serde_json::to_string_pretty(&report) {
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
