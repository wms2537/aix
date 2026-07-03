# Disagreement Confusion Matrix — Excel-arbitrated triage of the oracle

Arbiter = desktop Excel (M365 / documented behavior). Financial day-count and
pricing verdicts validated by reimplementing Excel's documented formulas in
Python ("computed"). "(med)" = medium-confidence, reviewer should spot-check.
Source: the 85 value-vs-value `disagree` cases in benchmarks/agreement.json.

## Confusion matrix (85 disagreements)

| Verdict | Count |
|---|---|
| IRONCALC_WRONG | 24 |
| LIBREOFFICE_WRONG | 41 |
| BOTH_WRONG | 7 |
| SPEC_AMBIGUOUS | 13 |
| UNDECIDABLE_HERE | 0 |

**Core-semantics families** (TRIM/CONVERT/ROW/SUMPRODUCT/POWER/SECOND/STDEVA/
MINA/MAXA/COUNT/ISNUMBER/TYPE/CHAR/CODE/OR/PRODUCT/SUM) — 18 cases, all
cleanly decidable, zero ambiguity: **8 IronCalc-wrong, 10 LibreOffice-wrong.**
- IronCalc failures: array/aggregation (ROW-over-range, SUMPRODUCT boolean
  coercion) + non-numeric-cell handling (MAXA/MINA/STDEVA ignore text/bool
  cells) + CONVERT F→C offset + TRIM not collapsing internal runs + SECOND
  truncating instead of rounding.
- LibreOffice failures: Excel type-coercion + domain-guard fidelity
  (booleans-as-numbers in COUNT/ISNUMBER/TYPE/CONCATENATE; POWER(0,0)→1;
  permissive domains where Excel errors).

**Financial family** (21 cases): LO-wrong 13, IC-wrong 3, BOTH 3, SPEC 2 —
dominated by LibreOffice day-count bugs (TBILL×4, DURATION, MDURATION) and
erroring on valid odd-coupon bonds (ODDFPRICE/ODDFYIELD); IronCalc's faults
are the opposite kind (accepting *invalid* bonds; PRICE basis-3 par bug).
**Distribution/complex** functions account for most SPEC_AMBIGUOUS
(transcendental precision, tail p-values, fp regression residuals,
version-gated LAMBDA/ISOMITTED).

## Confirmed real bugs (paper-citable)

**IronCalc:** CONVERT(68,"F","C")=19.65 (should be 20); TRIM doesn't collapse
internal whitespace; ROW-over-range returns scalar not array; SUMPRODUCT
doesn't coerce boolean arrays; MAXA/MINA/STDEVA ignore text/boolean cells;
SECOND truncates sub-second times; PRICE basis-3 returns exact par
(99.98979 expected); complex singularities (IMLN/IMCOT/IMCSC/IMCSCH of 0)
return "inf"/"NaNNaNi" instead of #NUM!; ODDFPRICE/ODDLPRICE accept invalid
date orderings; PERCENTRANK.EXC truncates to decimal places not sig-digits.

**LibreOffice:** boolean cells treated as plain numbers (COUNT/ISNUMBER/TYPE/
CONCATENATE — 4-fn cluster); T-bill day-count bugs (DSM=181 vs actual 184;
IronCalc matches Excel's documented formula exactly, verified); DURATION/
MDURATION deviate from textbook Macaulay (IronCalc matches to 1e-9);
Excel domain-guards not enforced (POWER(0,0)→1, ATAN2(0,0)→0, plus
BETADIST/CHIDIST/etc. negative-domain, BITLSHIFT/BITOR/MROUND/SYD/XNPV);
PERCENTRANK rounds instead of truncating; DOLLAR negative formatting.

## The both_error signal is real, not cosmetic
Of the 40-case both_error sample: 10 codes match (both = Excel), 30 mismatch —
and in ~every mismatch IronCalc returns Excel's exact error class (#NUM! for
domain/numeric violations, #N/A for unit mismatch) while LibreOffice collapses
them to #VALUE!. So "both_error, codes differ" is a genuine oracle signal
(which engine mirrors Excel's error taxonomy), and in this sample it uniformly
indicts LibreOffice's error mapping.

## Reviewer caveats
(a) ACCRINT-6 (the single BOTH_WRONG depending on Excel's un-runnable internal
algorithm); (b) ODDFPRICE/ODDFYIELD LO-wrong calls rest on LO erroring on
valid bonds — IronCalc's exact price digits not independently reproduced;
(c) PERCENTRANK truncate-vs-round and SKEW.P/SYD/CRITBINOM boundaries are the
medium-confidence items. Everything Python-reimplemented (DURATION, MDURATION,
PRICE-basis3, all four TBILL, PRICEMAT, AMORDEGRC) is high confidence.

## What this means for the paper
The oracle did not merely report a concordance number — with Excel as arbiter
it produced a *decidable* fault assignment for 72/85 disagreements (85%),
found real bugs in BOTH engines (24 IronCalc, 41 LibreOffice), and — the
anti-stacking result — the harness indicts its own engine 24 times, so it is
not rigged to flatter IronCalc. The core-semantics 8-vs-10 split is the clean
headline; the financial 13-vs-3 (LO-vs-IC) and the both_error error-taxonomy
finding are the depth.
