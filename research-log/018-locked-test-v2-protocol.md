# Pre-registered protocol: locked test v2 (run exactly once)

**Date:** 2026-07-10 · **Phase:** 5 (iteration 5) · **Cycle:** 1 · **Status:** in-progress

## Why a v2

v1's panel-identified scope limits: EUSES leg collapsed to one category (prefix
sampling); single guard op; cross-sheet formulas entirely outside the truth grammar
(Enron: skipped > checked); measurement artifacts inflating the fail-closed cost
(stdout pollution; undecoded sheet-name entities — cost_breakdown.json, exploratory);
n=21 single-model agent study; one dbt project. v2 closes each with pre-registered
rules. The systems under test are FROZEN at current HEAD (includes both post-v1
defect fixes + the qualifier guard — v2 also confirms those fixes at scale).

## Corpora (new data; rules fixed before acquisition)

- **EUSES-full**: convert ALL remaining raw .xls (target: all 4,652 across 11
  categories); eligibility as v1; cap 500 eligible in sorted order per category
  proportionally? NO — cap 500 eligible in sorted-filename order over the WHOLE
  corpus, PLUS report per-category composition (the fix is disclosure + coverage,
  not reweighting).
- **Enron-random**: seeded random sample (seed 20260710) of 1,500 .xls paths from the
  FULL retained archive listing (20,872), replacing v1's lexicographic prefix;
  convert first 800 of the sample in sorted order; eligibility as v1; cap 500.
- Native-xlsx corpus: none available in-session — the LibreOffice conversion caveat
  REMAINS and is disclosed (unchanged from v1).

## Pre-registered harness changes (measurement layer only)

1. `zip_sheet_name` XML-entity decode (v1 artifact: 15 files refused on '&' names).
2. Robust certify-JSON extraction (v1 artifact: vendored-ironcalc println! pollutes
   stdout; parse the JSON object from the tail — 12 v1 ERROR files certify).
3. Cross-sheet truth grammar: ref_shift extended to `'Name'!A1` / `Name!A1` (ASCII
   names): shift iff the qualifier names the edited sheet; else unchanged. Closes
   the largest v1 skipped class. Whole-col/row, tables, function-endpoint ranges
   stay out-of-grammar (skipped, counted).
4. Per-file watchdog (from v1, disclosed there).
All changes dev-tier re-verified before the run (identity on the committed dev
numbers where surfaces overlap; extended-grammar coverage reported separately).

## Legs and pre-registered predictions (ledger rows committed with this entry)

L1 5-op shift correctness (4 v1 ops + move-rows@3x2→8), both corpora:
  predict 0 mismatches (high) — the encoding defect is fixed; non-ASCII-qualifier
  files now REFUSE at restructure (counted as refusals, not corruption).
L2 prevalence: full-EUSES LOWER than v1's 69.3% (database category was formula-rich;
  direction, low); Enron-random within ±10 pts of 89.2% (low-med).
L3 guard: 0 false certifications (high); artifact-corrected own-pipeline cost:
  EUSES ≤ 14% and Enron ≤ 29% (= v1 measured minus attributed artifacts, medium);
  externalLink share of Enron denylist refusals 40–60% (medium, from exploratory
  breakdown).
L4 q̂ M2v: full-EUSES pooled within ±0.15 of 0.478 (low); Enron-random within ±0.10
  of 0.566 (low-med).
L5 agent study v2: n ≥ 100 tasks (dev-fixture corpus, same generator as v1, gates
  relaxed only by pre-registered rule: shared-formula filter stays, three-instrument
  agreement stays; if eligible < 100 use all), 2 live models (fast + mid tier), one
  arm each: predict 0 false certs (high); mid-tier model errs on FEWER tasks than
  fast (medium); guard blocks 100% of erroneous tasks (medium-high).
L6 dbt: +2 public projects (acquisition rule: the two most-starred GitHub search
  hits for real dbt projects with models/ that are not demos at acquisition time;
  disclosed): parse coverage 20–60% each (low); faithful-rename CERTIFIED / dangling
  REFUSED on each covered subgraph (high).

## Irreversibility

Run once per leg; results recorded whatever they are; harness bugs fixed with
disclosure; systems under test frozen at the pre-registration commit. v1 numbers
remain the paper's v1 numbers; v2 reported alongside, not replacing.

## Next steps
1. Commit this + ledger rows. 2. Launch conversions/acquisitions. 3. Apply
pre-registered harness changes + dev-tier verification. 4. Run once. 5. Analyze.
