# Evidence Sheet — frozen numbers (cite these exactly)

All from benchmarks/*.json at commit 150fb66. Machine: Intel i7-8700K,
12 logical cores, Linux. Engine: ironcalc 0.7.1+e50ccea8 (vendored master).

## Coverage (coverage.json)
- catalog recognized: **522 / 522** (100%)
- locally evaluable: **505**
- policy-limited: **17** (WEBSERVICE #VALUE!; RTD #N/A; STOCKHISTORY/
  DETECTLANGUAGE/TRANSLATE/COPILOT/IMAGE #CONNECT!; CALL/REGISTER.ID
  #BLOCKED!; 7 CUBE functions #NAME!; GETPIVOTDATA #REF!)
- history: 345/522 (66.1%) on IronCalc 0.7.1 release → 494 (94.6%) on
  vendored master → 497 after 3 residuals → 505 after Tier-I implementations

## Differential oracle (agreement.json)
- **1,659 cases across 492 functions**
- agree: **1,273** (exact 935 + within-tolerance 338)
- both-value agreement: **97.1%** (1273/1311, cases where both engines
  return a value)
- including one-side-error rows: **93.7%** (1273/1358)
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

## Fidelity / preservation (results.json, D + D2)
- openpyxl single re-save across the 5-fixture corpus strips **101,961
  cached formula values** and drops xl/sharedStrings.xml + xl/metadata.xml;
  on branch-consolidation: 442 cached_value changes, parts 16→14.
- LibreOffice convert drifts **90,448 cached values** (15- vs 17-sig-digit
  serialization) and rewrites 100% of OOXML parts.
- IronCalc-master round-trips its own files near-byte-identical; on FOREIGN
  (openpyxl-authored) files it rewrites parts and re-adds sharedStrings/
  metadata — stated honestly, motivates surgical-patch roadmap (v0.2).

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
