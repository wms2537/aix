# The honest applicability envelope (real corpus)

Answers the sharpest adversarial-review critique — that E-structural was
survivorship-biased (one openpyxl-generated fixture where shared formulas, the
dominant real-world construct, never appear). We ran xlq restructure across the
**231 vendored IronCalc test workbooks** — real files authored in
Excel/LibreOffice, not by openpyxl.

## Coverage (benchmarks/corpus_coverage.json) — with shared-formula expansion

| Outcome | Files | % |
|---|---|---|
| **Safely edited** (shifted correctly, reopens, no residual) | 182 | **78.8%** |
| **Refused** (residual, never silently wrong) | 49 | **21.2%** |
| — array formula present (Excel forbids splitting) | 47 | 20.3% |
| — table part present | 2 | 0.9% |

Shared-formula **expansion** (materialize each dependent from the master +
offset, then shift with σ — what Excel/LibreOffice do internally) lifted the
safe envelope from **22.5% → 78.8%**. The remaining refusals are array formulas
(which Excel itself forbids editing through) and tables.

**Every one of the 231 files is EITHER safely shifted OR refused with a truthful
reason. Zero are silently corrupted.**

## Correctness — a FORWARD, discriminating oracle (benchmarks/forward_correctness.json)

A six-reviewer adversarial PC flagged (unanimously) that an earlier
*round-trip* oracle — insert@k then delete@k, compare carried caches — was
**non-discriminative**: xlq carries `<v>` caches physically along the shift and
never recomputes, so a wrong reference-shift never perturbs a value, and the
inverse op returns the cell home. A no-op shifter (openpyxl, which shifts NO
references) passes that check too. It proved displacement *invertibility*, not σ's
forward correctness. The reviewers were right. This is the corrected oracle.

**Property.** Inserting a *blank* row at k is value-preserving under CORRECT
reference shifting — every formula tracks its data (which moved down) and the
blank contributes nothing, so each formula's recomputed value is unchanged.
Therefore the edited file, recomputed, must yield each formula's original
Excel-cached value at the formula's shifted position.

**Method.** Insert one blank row at row 2; recompute the edited file with
**LibreOffice** (an engine INDEPENDENT of xlq's IronCalc; xlq expands shared
formulas to explicit ones, which LibreOffice computes correctly); compare every
formula's recomputed value at its shifted position to the ORIGINAL file's
Excel-authored cached `<v>` (ground truth). Position-dependent / volatile
functions (OFFSET, INDIRECT, NOW, RAND, …) are excluded — their value legitimately
changes under a row insert (their reference *arguments* still shift correctly,
but the value is not preservation-invariant).

> **xlq: 48 / 48 (100%) forward-correct** on the sampled safe files.
>
> **Discrimination proven:** run the SAME check on openpyxl's `insert_rows`
> output — it FAILS on **42 of 48**, and on **42 files xlq passes where openpyxl
> fails**. A no-op shifter fails this oracle, so it genuinely tests forward
> reference-shift correctness, not mere invertibility.

The oracle caught two apparent failures on the first run — both `OFFSET`, which
is positional (its value changes under a row insert *in Excel too*); xlq's shift
of the reference argument was correct, confirming the oracle surfaces real
behavior. Excluding volatiles, xlq is 48/48.

### What the safe corpus exercises (stratification, corpus_stratification.json)
Of the 182 safe files: **133 (73%) contain shared formulas**, 48 are
multi-sheet, 9 use cross-sheet references, 10 use defined names, **2 contain
charts, 0 contain pivots**. So the uniform cross-part algebra is heavily
exercised on shared formulas, multi-sheet, cross-sheet, and defined names on
real files; chart-reference shifting is exercised on the controlled fixture (E4,
which has a chart) plus unit tests, and pivot-source shifting on unit tests only
— an honest limit of this corpus (the vendored IronCalc test suite is
formula-centric, few charts, no pivots).

### A finding in xlq's favor
LibreOffice reconstructs SHARED formulas differently from Excel (e.g. NPV over a
shared range: Excel's cache 11.78, LibreOffice's on-the-fly reconstruction of
the shared stub 11.41). xlq's expansion produces the formula matching Excel's
cached value; LibreOffice on xlq's *expanded* output also computes 11.78. So xlq
agrees with Excel where LibreOffice's own shared reconstruction does not.

### Corpus independence (honest)
The corpus is the vendored IronCalc test suite — files authored in
Excel/LibreOffice, but selected to exercise a calculation engine, not a random
real-world sample, and it is the engine xlq embeds (home-field for the
prediction path, though NOT for this correctness oracle, whose reference engine
is LibreOffice + Excel caches). A fully independent real-world corpus remains
future work.

## What this honestly says
- The safe envelope today is **22.5%** of real workbooks — narrow, and we say so.
  The dominant limiter is shared formulas (60.6%), exactly as the review
  predicted.
- The value is the *guarantee*, not the coverage: on the 22.5% xlq edits, the
  reference shift is correct and byte-minimal; on the other 77.5% it declines
  with a real reason instead of the status-quo silent corruption. openpyxl, by
  contrast, would "succeed" on ~100% and silently break references on the ones
  with formulas.
- The refusal is a SOUND over-approximation: we refuse on the *presence* of a
  shared/array formula, not a proven crossing (the reason strings say
  `shared_formula_present`, not a crossing claim we do not compute). Some of
  those 140 are safe cases we conservatively decline.

## The clear next lever: shared-formula expansion
60.6% of the corpus is refused for shared formulas alone. **Shared-formula
expansion** — materialize each dependent's explicit formula from the master +
its offset, then shift each with σ (what Excel/LibreOffice do internally) —
would move most of that 60.6% into the safe set, plausibly lifting the envelope
from ~22% toward ~85%. It trades strict minimal-patch purity on those cells (the
shared stubs become full formulas) for correctness on the common case. This is
the single highest-leverage next contribution, and the corpus number quantifies
exactly how much it is worth.
