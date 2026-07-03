# Reproducible Artifact Package

Companion to *An Enforcement Boundary for LLM Agents Operating on Spreadsheet
Artifacts* (`paper/paper.md`). Everything below regenerates from source on a
Linux machine with Rust 1.96, Python 3.14 + openpyxl 3.1.5, and LibreOffice
24.8 headless. Repo pinned at the paper's measurement commit; the vendored
engine is IronCalc master `e50ccea8` + our patches under `vendor/upstream/`.

## The system (xlq)
- `xlq/` — the Rust CLI. Build: `cargo build --release --manifest-path xlq/Cargo.toml`.
- Commands: `xlq inspect <file>` (privacy-safe census), `xlq diff <a> <b>`
  (semantic cell diff with the `cached_value` kind), `xlq calc <file>`
  (recompute-aware audit). All read-only by construction.
- Specs: `docs/census-spec.md`, `docs/receipt-journal-spec.md` (v0.2 draft),
  `docs/specs/full-catalog-semantics.md` (the 522-function behavior spec).
- Agent skill: `skills/xlq/SKILL.md` (the safe inspect→diff→calc loop; the
  direct answer to Claude Code issue #22044).
- Tests: `cargo test` (xlq ≥95% line coverage/file; engine 2,187+ green).

## The benchmarks (AXLE-bench)
- `benchmarks/README.md` — the five-axis suite + comparison matrix.
- `benchmarks/run_all.sh` — meta-runner (skips axes whose tools are absent).
- Coverage: `cargo run --release --bin coverage-probe -- benchmarks/excel-functions.txt`
  → `benchmarks/coverage.json` (522 recognized / 505 evaluable / 17 policy).
- Oracle: `benchmarks/run_oracle.sh` → `benchmarks/agreement.json`
  (1,659 cases / 492 functions); triage in `benchmarks/triage-analysis.md`
  (24 IronCalc / 41 LibreOffice / 7 both / 13 spec-ambiguous, Excel-arbitrated).
- Perf/fidelity/token: `benchmarks/run_bench.sh` → `benchmarks/results.json`.
- Fixtures: `fixtures/*.xlsx` (4 workload twins + a 100k-formula perf file),
  generated deterministically by `cargo run --bin xlq-fixtures -- fixtures`.

## Upstream contributions (offered, not yet filed)
- `docs/upstream/ironcalc-coverage-report.md` — workload-weighted gap report.
- `docs/upstream/residual-functions-patch.md` — ENCODEURL/HYPERLINK/AGGREGATE
  + Tier-I functions implemented in the vendored engine, PR-ready.
- The disagreement matrix (`benchmarks/triage-analysis.md`) is a bug list for
  both IronCalc and LibreOffice maintainers.

## The paper and its research trail
- `paper/paper.md` — the manuscript. `paper/*.md` — narrative arc, surface
  map, rationale matrix, evidence sheet (every number's source).
- `research-log/` — the full journey: setup, ~50-paper literature review,
  decision archaeology, falsifiable claims, theory review, paper draft.
- `results.tsv` — the prediction ledger (retrospective for the systems work;
  labeled as such — see the paper's Limitations).

## Honesty notes (from the paper)
- LibreOffice is a *reference* engine, not ground truth; Excel documentation
  is the arbiter. No claim that "IronCalc is 97.1% correct."
- The write path (typed patch + dry-run + receipts) is *specified* (v0.2),
  not measured here; today's enforcement is by absence of a write path.
- Fidelity is reported in tiers (loss vs normalization vs cosmetic drift);
  irreversible corruption (charts/pivots/VBA) is documented externally
  (issue #22044) but not measured on our fixture corpus.
