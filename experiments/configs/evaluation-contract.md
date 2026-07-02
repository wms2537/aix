# Evaluation Contract

## Immutable (read-only from this point)
- benchmarks/*.json measurement artifacts as of commit 150fb66 (coverage.json,
  agreement.json, results.json) — the paper reports these; re-running the
  harness to regenerate is allowed, editing values is not.
- Harness code: benchmarks/run_bench.sh, run_oracle.sh, run_all.sh,
  xlq/src/bin/coverage_probe.rs, xlq/src/bin/oracle_compare.rs,
  benchmarks/oracle-cases.json (case table), benchmarks/excel-functions.txt
  (catalog universe, Microsoft alphabetical list, fetched 2026-07-02).
- Fixture corpus: fixtures/*.xlsx + planted-defects.json.
- Comparison policy (oracle): numeric rel-tol 1e-9 / abs 1e-12 near zero,
  exact text/bool, error-class agreement with code-match reported separately.

## Mutable
- Paper text, figures, analyses, supplementary materials.
- NEW experiments (e.g., statistical treatments, additional baselines) may be
  ADDED with fresh predictions in results.tsv; they never modify existing
  artifacts.

## Primary metrics (frozen definitions)
- Cross-engine agreement: agree / (agree + disagree) where both engines
  produce a value (1,273/1,311 = 97.1%); secondary: including one-side-error
  rows (1,273/1,358 = 93.7%).
- Catalog coverage: three-number accounting — recognized 522/522, locally
  evaluable 505, policy-limited 17.
- Fidelity: OOXML parts dropped/added/changed on re-save; cached values
  stripped (openpyxl: 101,961) / drifted (LibreOffice: 90,448).
- Efficiency: median-of-3 warm wall time, perf-large.xlsx (~100k formulas).
- Agent-ergonomics: census bytes vs naive dump bytes (965 B vs 7,239,303 B).

## Honesty rules
- Retrospective measurements are labeled retrospective; the prediction ledger
  only claims predictions that were actually recorded before the run (the
  session logs contain several genuine ones — e.g., "~497/522 expected after
  residuals" vs measured 505+17, spec off-by-one documented).
- LibreOffice is a REFERENCE engine, not ground truth; disagreements are
  triaged with Excel documentation as arbiter.
