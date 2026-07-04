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

## Correctness (benchmarks/roundtrip_correctness.json) — 182/182, engine-free

For each of the 182 safe files: insert a blank row at k, then delete row k (net
identity). xlq carries each cell's Excel-computed cached value (`<v>`) along the
shift, so the round-tripped caches must equal the original's cell-for-cell — any
wrong shift on insert OR delete would move a value to the wrong cell.

> **182 / 182 (100%) preserve every Excel cached value cell-for-cell.** Zero
> mismatches.

This is checked against Excel's ground truth (the cached values written by the
authoring tool), engine-free. It proves σ's insert and delete are exact inverses
— hence both correct — on real files with real shared formulas, cross-sheet
references, and mixed absolute/relative refs.

### A finding in xlq's favor
An earlier version of this oracle used LibreOffice's recompute as the reference
and reported 16 "mismatches" — all in precision-sensitive FINANCIAL/STATISTICAL
files. Investigation showed these were NOT xlq errors: **LibreOffice
reconstructs SHARED formulas differently from Excel** (e.g. NPV over a shared
range: Excel's cache 11.78, LibreOffice's on-the-fly reconstruction 11.41).
xlq's expansion produces the formula matching Excel's cached value; LibreOffice
on xlq's *expanded* output also computes 11.78. So xlq's shared-formula handling
agrees with Excel where LibreOffice's own reconstruction does not — the reason
the correctness oracle must use Excel's caches, not an LO recompute.

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
