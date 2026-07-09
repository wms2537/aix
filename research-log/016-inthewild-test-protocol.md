# Pre-registered protocol: locked in-the-wild test (run exactly once)

**Date:** 2026-07-09 · **Phase:** 5 (Path C conclude, step 1) · **Cycle:** 1 · **Iteration:** 3 · **Status:** in-progress

## Context

Path C approved (T002, user quoted in 015). This entry pre-registers the test
protocol and predictions BEFORE any test data is downloaded or inspected. Committed
first; downloads follow. The fixture corpus (vendor/upstream/xlsx/tests) was the
de-facto tuning/validation tier; EUSES and Enron are the locked test tier.

## Corpora and selection (fixed now)

- **EUSES** and **Enron** spreadsheet corpora (user consent quoted in 015).
- Both are largely `.xls`-era: convert with LibreOffice headless (`soffice
  --convert-to xlsx`); files that fail conversion are counted and excluded
  (reported, not hidden). **Conversion caveat (pre-registered):** conversion
  regenerates cached `<v>` values via LibreOffice recompute, so leg-4 collision
  structure is measured over LO-recomputed values rather than original Excel caches;
  value-collision structure survives recompute, but we disclose it.
- **Eligibility:** first sheet has ≥ 2 formula cells and openpyxl + zip parse succeed.
- **Cap (anti-cherry-pick):** if a corpus yields > 500 eligible files, take the
  first 500 in sorted-filename order. No other filtering, no post-hoc exclusions.

## Legs and pre-registered predictions (rows appended to results.tsv now)

1. **Deterministic shift correctness** (same method as committed
   `shift_correctness_real.py`, parameterized for corpus path only): xlq's output
   formulas vs the independent grid-validity reference shifter across the same 4 ops.
   Predict: **0 mismatches** (high confidence — the algebra is proven; the checker
   grammar bounds what is checked); **workbook residual/refusal rate HIGHER than
   fixtures** (real files carry more shared formulas/tables/names; predict 20–50%,
   low-medium confidence). Refusals are the fail-closed design working, but their
   rate is the honest COST number.
2. **Would-corrupt prevalence** (deterministic openpyxl-path analog): share of
   eligible files with ≥ 1 in-grammar formula reference requiring a shift under
   insert-row@2. Fixture analog: 85.5% confirmed-genuine. Predict **80–95%** among
   eligible files (medium).
3. **Guard verdicts, both paths** (engine-free, production `xlq certify`):
   (a) certify(xlq's own transform) → predict CERTIFIED wherever the transform
   applied (0 false refusals of own output, high confidence);
   (b) certify(openpyxl output) on would-corrupt files → predict **REFUSED on all;
   0 false certifications** (high confidence — this is the paper's central
   soundness claim, now on data development never touched).
4. **Coincidence q̂ (M2v off-by-one row)** via committed `coincidence_q.py`
   (corpus-path parameterized): predict pooled q̂ **≥ 0.178** (business sheets repeat
   values at least as much as calc fixtures; direction higher, low-medium). If
   runtime permits, the MC blind-spot floor (same seed 20260709): paper §5.8
   pre-states the direction — predict **LOWER than 11.8%** on business-dominated
   sheets (medium). This tests §5.8's transfer claim directly.
5. **dbt real project (T004):** pre-registered target = **GitLab data-team dbt
   project** (public, real production scale); fallback if license/access blocks:
   dbt-labs jaffle-shop-classic (disclosed as demo-grade). Protocol: run
   `adapter_dbt` extraction; report **parse coverage** (share of models within the
   mini-dbt subset — predict **< 30%**, low-medium confidence: production dbt uses
   macros/config heavily; a low number is an honest scope finding, not a failure to
   hide); on the covered subgraph, faithful rename → predict CERTIFIED, botched
   rename (dangling ref) → predict REFUSED (high). **No materialization** (their
   warehouse is unavailable) → the self-oracle transport leg is out of scope here;
   claims scoped to graph-premise checking. The adapter is NOT modified after seeing
   the project (that would unlock the test).

## Irreversibility rules

- Run once; results recorded whatever they are; all numbers enter the paper as
  test-tier numbers with the fixture numbers relabeled as development-tier.
- Measurement-harness bugs found mid-run may be fixed WITH DISCLOSURE in the log;
  the systems under test (xlq binary at HEAD 18a2209 lineage, adapter_dbt,
  router/certify) may NOT be modified in response to test data.
- The harness (corpus-path parameterization + orchestration script) is written and
  smoke-tested on the FIXTURE corpus (dev tier) before test data arrives.

## Problem alignment

Converts every fixture-proxy headline into evidence about the artifacts PROBLEM.md
is actually about — the named highest-value missing item from the final review.

## Next Steps

1. Append prediction rows to results.tsv; commit this entry (pre-registration lock).
2. Write/smoke-test the parameterized harness on fixtures.
3. Download corpora (consented); convert; run once; record.
