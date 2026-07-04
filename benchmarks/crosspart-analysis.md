# E6: cross-part uniformity, all four operations — closing the "validated in the wrong places" gap

The round-4 PC verdict was singular and unanimous: the structural-edit primitive
is built and the "unbuilt" fatal is closed, but the *novel* claim — σ applied
UNIFORMLY across every reference-bearing OOXML part — was under-evaluated exactly
where it is novel. Concretely: the 231-file real corpus is 173/182 single-sheet
flat grids (charts = 2, pivots = 0), so cross-part shifting rested on one
hand-built fixture (E4); and forward-correctness was shown only for insert-row,
not delete or column ops. E6 answers both.

## The cross-part fixture corpus
Ten workbooks (`fixtures/crosspart/cp01..10.xlsx`), each deliberately carrying
EVERY reference-bearing part the algorithm claims to shift: a bar **chart** over
a data column, **cross-sheet** references (a Report sheet reading Data),
**defined names** into the edited sheet, **conditional formatting**, **data
validation**, a **merged** region, and in-sheet formulas (a straddling SUM, a
single ref, an absolute ref). Geometry varies across the ten so edits land
above, inside, and below each referenced range.

## Result (benchmarks/crosspart_correctness.json)
Each fixture is edited with all four operations, and every reference-bearing
part is checked against an INDEPENDENT Python re-implementation of the shift
semantics (a different language and codebase from the Rust σ, so a shared bug is
unlikely):

| Operation | reference checks | correct |
|---|---|---|
| insert-row | 90 | **90 (100%)** |
| delete-row | 90 | **90 (100%)** |
| insert-col | 90 | **90 (100%)** |
| delete-col | 90 | **90 (100%)** |
| **total** | **360** | **360 (100%)** |

Each fixture contributes 9 cross-part reference checks per op (in-sheet SUM /
single / absolute, chart data ref, two cross-sheet refs, defined name, CF sqref,
merged ref). So the uniform cross-part algorithm is validated on **charts,
cross-sheet references, defined names, conditional formatting, and merged
regions**, over **delete and column paths** — not only insert-row on flat grids.

## Minimal-patch across the corpus (not only E4)
For each fixture (insert-row), every CHANGED non-sheet part (the chart and
`workbook.xml`) is checked: **20 of 20 changed non-sheet parts are
digit-stripped-identical** to their originals — the only bytes that changed are
reference coordinates. All 10 files re-open and evaluate. This extends the
minimal-patch check from a single fixture to the cross-part corpus.

## Honest residual
- **Pivots (0 files):** openpyxl cannot author pivot tables, so the cross-part
  corpus has none; pivot-source (`worksheetSource ref=`) shifting is covered by
  unit tests and the code path, but not by a corpus fixture. Stated as a gap.
- The expected-value oracle here is an independent Python re-implementation of
  the documented shift semantics, not Excel itself; it agrees with the forward
  value oracle (E5) and the unit tests, but a full Excel round-trip on
  chart/pivot references remains future work.
- These fixtures are author-generated (to control the reference inventory);
  they complement, not replace, the 231-file real corpus (E5).
