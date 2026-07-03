# Phase 2 — Falsifiable Claims (systems-paper form)

**Date:** 2026-07-03 · **Phase:** 2 · **Status:** completed

For a systems/measurement paper the "hypothesis" is the set of falsifiable
claims the artifacts substantiate. Each has a metric, a threshold, a
disconfirming outcome, and an anti-stacking note. Theory-review dispatched
after this.

## C1 — The fidelity gap is real and measurable
**Claim:** current agent-spreadsheet substrates cause structural damage that
task-success metrics cannot see, and it is measurable.
**Metric/threshold:** on the 5-fixture corpus, count OOXML parts changed and
cached values stripped/drifted on a single re-save per tool.
**Evidence:** openpyxl strips 101,961 cached values and drops sharedStrings;
LibreOffice rewrites 100% of parts + drifts 90,448 cached values (15 vs 17
sig-digit); IronCalc-master round-trips own files near-byte-identical but
rewrites foreign files. **Disconfirm:** if a mainstream substrate preserved
parts+caches losslessly, the gap would be tool-specific, not structural.
(Not disconfirmed.) **Anti-stacking:** this is a *measurement*, not a
combination — the contribution is measuring a dimension the field's
benchmarks omit (verified: none of SheetCopilot/SheetAgent/SpreadsheetBench
1+2/Finch measure it).

## C2 — Coverage honesty is achievable and more correct than binary
**Claim:** a formula runtime can recognize 100% of the Excel catalog with
*defined, documented* behavior, decomposed honestly, rather than claiming a
binary supported/unsupported number.
**Metric/threshold:** three-number probe: recognized / locally-evaluable /
policy-limited, each function's policy literal matching Excel docs.
**Evidence:** 522/522 recognized, 505 evaluable, 17 policy-limited with
Excel-exact literals (WEBSERVICE #VALUE!, RTD #N/A, CUBE #NAME?, XLM
#BLOCKED!, GETPIVOTDATA #REF!). **Disconfirm:** if any policy-limited
function's literal contradicted Excel's documented no-connection behavior,
the honesty claim fails for that cell. (Reviewer to verify against the
semantics spec.) **Anti-stacking:** the reframing *is* the contribution —
coverage as a 3-tuple with per-function documented behavior, not
supported-count++.

## C3 — Documentation-arbitered differential testing works where no
mechanized spec exists
**Claim:** cross-engine differential testing with informal-vendor-doc
arbitration (not a mechanized spec) surfaces real formula-engine bugs and
triages disagreements honestly.
**Metric/threshold:** agreement rate + a bug/inconsistency/underspecified
verdict per disagreement; bugs confirmed in both engines.
**Evidence:** 1,659 cases / 492 functions; 97.1% both-value agreement;
real IronCalc bugs (CONVERT F→C, ROW-over-range) AND LibreOffice deviations
(POWER(0,0), PERCENTRANK) surfaced. **Disconfirm:** if every disagreement
were a tolerance artifact or an unshared-function noise row, the oracle
would find nothing real. (Not disconfirmed — bugs are concrete.)
**Anti-stacking:** distinct from CSmith/PQS/JEST/SmartOracle by the *absent
mechanized spec* — Excel is documented only in vendor prose + ECMA-376, so
the arbiter is informal, a genuinely different (weaker but more general)
trust anchor. Claim wording frozen in 001-literature-review.md.

## C4 — The enforcement boundary is a coherent, format-aware pattern
**Claim:** read-only-by-construction + structural census + coverage-honesty
+ (spec'd) typed-patch-with-dry-run + hash-chained receipts compose into a
runtime that makes the safe path the only path for agent-file interaction —
format-aware in a way the 2025–26 enforcement wave is not.
**Metric/threshold:** qualitative (design) + the census token ratio (965 B
vs 7.24 MB = 7,502×) as the ergonomics evidence that structure-only reads
are cheap. **Disconfirm:** if the census lost information needed for safe
mutation, or the read path could write, the boundary would leak. (Regression
tests: read paths cannot write; sentinel value never leaks.)
**Anti-stacking:** the novelty is *format-awareness* (fidelity +
coverage-honesty), not receipts/transactions per se — those are cited as
domain instantiations of known mechanisms (Notarized Agents, Nitro, CT,
PROV; ACID-snapshot sandboxing).

## Metrics summary (frozen, from evaluation-contract.md; CORRECTED)
Concordance 97.1% (1273/1311 value-producing) — reported as concordance,
NOT accuracy, beneath the disagreement confusion matrix. Coverage 522
recognized / 505 evaluable / 17 policy-limited (14 capability + 2 policy +
1 context). Token 7,246.5× (perf-large) / 28.4× (branch-consolidation).
Fidelity three tiers: T2.5 openpyxl 101,961 cache-blanked; T3 LibreOffice
90,448 cosmetic drift; both rewrite ~100% parts (byte-provenance defeated);
T1 not measured (fixtures lack charts/pivots/VBA — limitation + #22044).
xlq ≥95% line coverage per file (67 tests); engine 2,187+ tests.

## Theory review (2026-07-03) — verdict NEEDS_REVISION, all folded in
Hostile top-venue methods review of C1–C4. Verdict was NEEDS_REVISION (not
flawed — "empirical spine is real and reproducible"). Corrections applied
BEFORE section-writing (branch-of-origin routing: fix the claims, not the
prose downstream):
- **C1:** stop calling byte-mutation "damage." Split into T1 irreversible
  loss (NOT measured on our corpus — fixtures lack charts/pivots/VBA; cite
  #22044 externally + state as limitation), T2 lossless normalization
  (openpyxl re-inlines strings — VERIFIED; both rewrite 100% of parts →
  defeats byte-provenance), T2.5 cache-blanking (openpyxl 101,961 `<v/>`;
  recoverable by recompute but breaks cache-trusting consumers), T3 cosmetic
  drift (LibreOffice 90,448 at 15-vs-17 digits — legitimate, not damage).
  The honest, sharper claim: byte-identity + caches are untrustworthy across
  all substrates → integrity checks must be SEMANTIC + recompute-aware
  (which is exactly xlq's diff/calc). Reframe LibreOffice as "part rewrite
  defeats byte-diff," never "drifts values as damage." Pin versions;
  5-file corpus is a stated limitation.
- **C2:** split "policy-limited (17)" into capability-limited (14) /
  policy-blocked (2) / context-precondition (1). Pin the catalog (522 as of
  the MS list, 2026-07-02). Add "evaluable ≠ correct" guard.
- **C3:** relabel 97.1% as CONCORDANCE with an explicit disavowal; LEAD
  with the Excel-arbitrated disagreement confusion matrix (the real result);
  state ~3.4 cases/function and hand-authored generation (disagreement rate
  is a lower bound); state the missing Excel-binary oracle as a limitation.
  FIX the CSmith framing: CSmith is ALSO prose-arbitrated (C-standard)
  differential voting — the *method* is the CSmith playbook; our
  contribution is the DOMAIN (spreadsheet engines, no mechanized spec) +
  an honest arbitration protocol + concrete bugs, NOT a new methodology.
  PQS/JEST/SmartOracle genuinely differ (they synthesize a stronger oracle).
- **C4:** narrow the novelty to one sentence — "the first LLM-agent
  enforcement boundary that is artifact-format-fidelity-aware" — and
  contrast with format-aware DLP / schema-aware DB sandboxes to pre-empt
  prior art. Drop "coherent" as a contribution (table stakes). DEMONSTRATE
  an operational discriminator that is implemented today: coverage-honesty
  refuses to assert reliability on a partially-evaluable workbook
  (reliable:false), and the semantic `cached_value` diff catches exactly the
  openpyxl cache-blanking that byte-diff and task-metrics miss. Novelty
  depends on C1/C2 surviving — fixed first.
- **Prediction ledger:** quarantine. Label retrospective/derivational,
  narrative-only, NOT cited as confirmatory evidence (a ledger of
  predictions derived from the observations it "confirms" has zero
  confirmatory power). Prospective out-of-sample testing = future work.

## Anti-stacking check (whole paper)
The paper is not "enforcement + differential testing + benchmark stacked."
It is one reframing — *the enforcement boundary for a legacy artifact class
must be format-aware and self-honest* — with the oracle as the mechanism
that earns the runtime's honesty and AXLE-bench as the evaluation that
measures the omitted dimension. Each part fails without the others: the
boundary needs the oracle to justify trusting its engine; the coverage
taxonomy needs the boundary to make it actionable; the benchmark needs the
fidelity axis to be novel.
