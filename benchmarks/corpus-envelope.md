# The honest applicability envelope (real corpus)

Answers the sharpest adversarial-review critique — that E-structural was
survivorship-biased (one openpyxl-generated fixture where shared formulas, the
dominant real-world construct, never appear). We re-ran xlq restructure
(dry-run insert-row at row 2 on the first visible sheet) across the **231
vendored IronCalc test workbooks** — real files authored in Excel/LibreOffice,
not by openpyxl.

## Result (benchmarks/corpus_coverage.json)

| Outcome | Files | % |
|---|---|---|
| **Safely edited** (shifted correctly, reopens, no residual) | 52 | **22.5%** |
| **Refused** (residual, never silently wrong) | 179 | **77.5%** |
| — shared formula present | 140 | 60.6% |
| — array formula present | 47 | 20.3% |
| — table part present | 2 | 0.9% |

**Every one of the 231 files is EITHER safely shifted OR refused with a truthful
reason. Zero are silently corrupted.** That is the invariant, measured on real
files rather than a fixture where the failure modes were selected out.

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
