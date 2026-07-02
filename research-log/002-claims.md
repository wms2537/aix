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

## Metrics summary (frozen, from evaluation-contract.md)
Agreement 97.1% (1273/1311) / 93.7% (1273/1358); coverage 522/505/17; token
7,502×; fidelity 101,961 stripped / 90,448 drifted / 100% parts rewritten;
57+ xlq tests, ≥95% line coverage per file; engine 2,187+ tests.

## Anti-stacking check (whole paper)
The paper is not "enforcement + differential testing + benchmark stacked."
It is one reframing — *the enforcement boundary for a legacy artifact class
must be format-aware and self-honest* — with the oracle as the mechanism
that earns the runtime's honesty and AXLE-bench as the evaluation that
measures the omitted dimension. Each part fails without the others: the
boundary needs the oracle to justify trusting its engine; the coverage
taxonomy needs the boundary to make it actionable; the benchmark needs the
fidelity axis to be novel.
