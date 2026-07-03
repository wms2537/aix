# Evidence Sheet — frozen numbers (cite these exactly)

All from benchmarks/*.json at commit 150fb66. Machine: Intel i7-8700K,
12 logical cores, Linux. Engine: ironcalc 0.7.1+e50ccea8 (vendored master).

## Coverage (coverage.json)
- catalog recognized: **522 / 522** (100%)
- locally evaluable: **505**
- policy-limited: **17** — REFRAMED into three honest sub-reasons (theory
  review: "policy-limited" was hiding three different reasons):
  - **capability-limited (14)** — need network/live data the runtime has no
    local access to: WEBSERVICE #VALUE!; RTD #N/A; STOCKHISTORY/
    DETECTLANGUAGE/TRANSLATE/COPILOT/IMAGE #CONNECT!; 7 CUBE functions #NAME!
  - **policy-blocked (2)** — deliberately refused for security (memo §16):
    CALL/REGISTER.ID (XLM) #BLOCKED!
  - **context-precondition (1)** — needs a workbook object the runtime
    doesn't model: GETPIVOTDATA #REF!
- catalog pinned: the 522 worksheet functions in Microsoft's alphabetical
  function list, fetched 2026-07-02 (benchmarks/excel-functions.txt).
  "Recognized" = the name parses and dispatches to defined behavior (not a
  parser unknown-name rejection).
- GUARD: "locally evaluable" ≠ "correct." Evaluability is coverage (C2);
  correctness is the oracle's burden (C3). Never read 505 as "505 correct."
- history: 345/522 (66.1%) on IronCalc 0.7.1 release → 494 (94.6%) on
  vendored master → 497 after 3 residuals → 505 after Tier-I implementations

## Differential oracle (agreement.json)
- **1,659 cases across 492 functions** (~3.4 cases/function — modest input
  diversity; state this openly, it means the disagreement rate is a LOWER
  bound; cases are hand-authored per-function, not coverage-guided fuzzing)
- **FRAMING (theory review): "agreement" is CONCORDANCE, not accuracy.**
  Two engines agreeing is necessary-not-sufficient for correctness (they
  read the same ECMA-376 + vendor prose and can share a wrong behavior).
  The scientific result is the DISAGREEMENT confusion matrix (below), NOT
  the concordance percentage. Never write "IronCalc is 97.1% correct."
- inter-engine concordance: **1,273/1,311 = 97.1%** on value-producing
  cases (935 exact + 338 within tolerance); 1,273/1,358 = 93.7% including
  one-side-error rows — report as concordance, demoted beneath the matrix.
- **Disagreement confusion matrix (Excel-arbitrated; benchmarks/
  triage-analysis.md) — THE HEADLINE RESULT C3 LEADS WITH:** of the 85
  value-vs-value disagreements — IRONCALC_WRONG 24, LIBREOFFICE_WRONG 41,
  BOTH_WRONG 7, SPEC_AMBIGUOUS 13, UNDECIDABLE 0. So 72/85 (85%) got a
  decidable Excel-arbitrated fault assignment. Core-semantics families (18
  cases, zero ambiguity): 8 IronCalc-wrong vs 10 LibreOffice-wrong.
  Financial (21): 13 LO-wrong / 3 IC-wrong. The harness indicts its OWN
  engine 24 times — anti-stacking evidence it is not rigged to flatter
  IronCalc. both_error signal: LibreOffice collapses Excel's #NUM!/#N/A into
  #VALUE! while IronCalc preserves Excel's taxonomy (genuine oracle signal).
  Named real bugs both directions — IronCalc: CONVERT F→C=19.65 (should 20),
  TRIM no internal-collapse, ROW-not-array, SUMPRODUCT boolean coercion,
  A-family ignore text cells, SECOND truncates, PRICE basis-3 par bug.
  LibreOffice: booleans-as-numbers (4-fn cluster), TBILL day-count bugs
  (DSM 181 vs 184), DURATION/MDURATION off, POWER(0,0)→1 vs Excel #NUM!.
- **Missing oracle, stated honestly:** Excel-the-binary is an executable
  oracle via COM/Office-JS but we lack a licensed Excel environment on this
  Linux machine; Excel *documentation* is the arbiter, and adding Excel as
  a third differential peer is the clearest strengthening (future work).
- disagree: 85; both_error: 196 (code-match 58, mismatch 138);
  lo_unsupported: 105 (LO lacks the function; +15 also IronCalc-errored);
  engine_error: 0
- verdict vocabulary: agree / disagree / both_error / lo_unsupported
- real bugs surfaced — IronCalc: CONVERT(68,"F","C")→19.65 vs Excel 20;
  ROW-over-range array lost; SUMPRODUCT boolean coercion. LibreOffice
  deviates from Excel: POWER(0,0)→1 (Excel #NUM!, IronCalc matches Excel);
  PERCENTRANK rounding; SECOND(0.999999). (Excel is the arbiter.)
- **LibreOffice is a REFERENCE engine, not ground truth.** Never phrase as
  "IronCalc is 97.1% correct." Phrase as "the two engines agree on 97.1% of
  value-producing cases; the 85 disagreements were triaged with Excel
  documentation as arbiter, yielding bugs in both."

## Fidelity / preservation (results.json, D + D2) — REFRAMED per theory review
CRITICAL: do NOT call all byte-mutation "damage." Three distinct tiers
(theory review, 2026-07-03); the honest framing motivates xlq's SEMANTIC
diff precisely because byte-identity and caches are untrustworthy.
- **T1 (irreversible loss):** NOT measured on our corpus. openpyxl is
  documented to strip charts/pivots/VBA on real financial models (issue
  #22044), but our fixtures have none (IronCalc cannot author them) — so we
  do NOT claim a measured T1 result; it is stated as a limitation + the
  external #22044 evidence.
- **T2 (semantics-preserving normalization; lossless but defeats
  byte-provenance):** openpyxl re-inlines shared strings (VERIFIED: 170
  shared-ref cells → 170 inline-string cells, every text value present — so
  "drops sharedStrings.xml" is NORMALIZATION, not loss; corrected). Both
  openpyxl and LibreOffice rewrite ~100% of OOXML parts, so byte-diffing is
  useless as an integrity oracle. IronCalc round-trips its own files
  near-byte-identical but rewrites foreign files similarly.
- **T2.5 (recoverable-but-breaks-cache-consumers):** openpyxl blanks ALL
  cached formula results — **101,961 `<v/>` cells across the corpus** (442
  on branch-consolidation). Recoverable by recomputation IF formulas are
  intact (they are), but any consumer that trusts caches sees nothing. This
  is the real, measured motivation for xlq's recompute-aware diff and the
  `cached_value` change-kind.
- **T3 (cosmetic sub-precision drift):** LibreOffice writes caches at 15 vs
  17 significant digits — **90,448 cells**. Below the semantic precision of
  an IEEE-754 double and recomputed on open. NOT damage; a legitimate
  serialization choice. Its only real consequence: it too defeats naive
  byte-provenance.
- **The defensible claim:** across every substrate, byte-identity is
  destroyed and cached values are untrustworthy (blanked by openpyxl,
  drifted by LibreOffice), so a correct integrity check must be SEMANTIC and
  recompute-aware — which is what xlq's diff/calc are. Report each tier
  separately with per-file distribution; state versions inline (openpyxl
  3.1.5, LibreOffice 24.8.7.2, IronCalc master e50ccea8); 5-file corpus is a
  stated external-validity limitation.

## Efficiency (results.json, A) — median-of-3 warm, perf-large.xlsx (~100k formulas)
- xlq calc (load + full recalc + stored-vs-recomputed audit): **1.264 s**
- ironcalc load-only: 0.599 s
- LibreOffice --convert-to (process spawn + load + save; NOT isolated
  recalc — Calc doesn't recalc xlsx on load by default): 1.494 s
- openpyxl load_workbook: 0.875 s, **calc n/a (no formula engine)**

## Agent-ergonomics / token efficiency (results.json, C)
- census vs naive full-cell JSON dump:
  - perf-large.xlsx: 999 B vs 7,239,303 B = **7,246.5×**
  - branch-consolidation.xlsx: 1,800 B vs 51,107 B = 28.4×
- (Do NOT compare head-to-head with SpreadsheetLLM's 25×: different task —
  ours is structure-only for mutation safety, theirs is lossy content
  compression for understanding/QA.)

## Test coverage
- xlq: 57→67 tests; every src/*.rs ≥95% line coverage (cargo-llvm-cov)
- vendored engine base/src/test: ~940 test fns; full suite 2,187+ green

## Motivation citations (from literature review)
- Panko: 94% of 88 audited spreadsheets contain ≥1 error; 5.2% cell error
  rate. Hermans & Murphy-Hill (Enron): 24% of formula-bearing corporate
  sheets contain an Excel error; 75% of sheets use only top-15 functions.
- Anthropic Claude Code issue #22044: agent xlsx skill corrupts financial
  models via openpyxl (real-world instance of the failure).
- Verified gap: SheetCopilot, SheetAgent, SpreadsheetBench 1+2, Finch —
  none measures file corruption/fidelity; SheetCopilot's checker skips
  unflagged properties BY DESIGN. Only OS-Harm measures agent side-effects
  at all (not file fidelity).
- Closest prior: Pista (2026, incl. Gulwani) — human step-level oversight,
  prompt-only constraints, no automated fidelity/coverage; its Limitations
  name our contribution as their future work.
