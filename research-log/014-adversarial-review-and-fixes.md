# Adversarial Review of the Structural-Edit Contribution + Fixes (Phase 4/5)

**Date:** 2026-07-04 · **Phase:** 4→5 · **Status:** completed

## The review
28-agent adversarial workflow (5 dimensions → per-finding adversarial verify →
mock-PC verdict). It attacked σ-correctness, minimal-patch soundness,
evaluation fairness, novelty, and residual honesty. Result: **9 CONFIRMED
defects of 22** (empirically reproduced against the actual code, not traced).
This is the anti-fragile signal working: several confirmed defects violated the
headline "never silently wrong" invariant.

## Confirmed defects and their fixes

| # | Severity | Defect | Fix | Test |
|---|---|---|---|---|
| 1 | **FATAL** | whole-column refs (`A:A`) silently unshifted under COLUMN ops — scanner `scan_ref_body` rejected them so they never reached the (correct) core algebra | rewrote the range-validity predicate to accept both-whole-column / both-whole-row / both-full-cell | `formula_whole_column_under_col_op_shifts`, `..._delete_consumed_is_ref` |
| 2 | major | CF/DV formula BODIES not shifted (only their `sqref`) → rule region and tested cell disagreed by a row | generalized the formula-tag handler to `<f>`, `<formula>`, `<formula1>`, `<formula2>` | `cf_and_dv_formula_bodies_shift` |
| 3 | major | 3D span (`Sheet1:Sheet3!`) shifted only when the edited sheet was a named ENDPOINT; interior tabs silently stale | conservative gate: `has_unverifiable_3d_span` → residual → refuse | `threeD_interior_span_forces_residual` |
| 4 | major | table parts (`xl/tables/*.xml`) not routed → stale extent / structured refs | pre-scan: any table part → residual → refuse | `table_part_forces_residual` |
| 5 | major | "applied uniformly across ALL parts" overclaimed | coverage broadened (2,4); the rest REFUSED, and the docs now say exactly what shifts vs what is refused | — |
| 6 | major | eval asserted recompute-equivalence but never executed it | eval now runs `xlq calc` on the committed file: SUM recomputes to 760 (executed) | eval `recompute_ok=True` |
| 7 | **fatal** | residual refusal advertised as narrow (shared/array only) while many constructs slipped through silently | the residual gate is now BROAD: shared, array, tables, 3D-interior all refused → "never silently wrong" restored | — |
| 8 | **fatal** | claim "shared/array is a rare edge case" REFUTED — shared formulas are common (autofill), so refusing them limits real-world use | acknowledged as the key open limitation (see scope); the policy is honest (refuse > corrupt), expansion is the next step | — |
| 9 | major | "degrades gracefully" overstated the decline | reframed: it is a safety refusal, common on shared-formula-heavy files | — |

## The principle restored
"Never silently wrong" = for every reference-bearing construct, EITHER shift it
correctly OR refuse the edit. The review found constructs that did NEITHER
(silent corruption). Every one is now shifted-correctly (whole-column, CF/DV
bodies) or refused (tables, 3D-interior, shared, array). 184 tests, 0 failures.

## Corrected coverage (honest)
**SHIFTS correctly:** cell `<f>` formulas, `<c r>`/`<row r>` coords, cross-sheet
refs (scoped), defined names, chart `<c:f>`, pivot `worksheetSource`, mergeCell
/CF/DV/dimension/selection `sqref`, CF `<formula>` + DV `<formula1/2>` bodies,
whole-column/whole-row refs, absolute refs, 3D spans anchored on the edited
sheet.
**REFUSES (residual, never silently wrong):** shared-formula groups, array
formulas, workbooks with table parts, 3D spans not anchored on the edited sheet.

## The key remaining limitation (honest)
Shared formulas are common (Excel/autofill emit them for filled ranges), and xlq
currently REFUSES any edit touching them. The refusal is safe (never wrong) but
limits real-world coverage. **Shared-formula EXPANSION** — materializing the
group into explicit per-cell formulas, then shifting each — is the clear next
contribution; it trades strict minimal-patch purity (the expanded stubs become
full formulas) for correctness on the common case, exactly as Excel/LibreOffice
do internally.

## Novelty verdict (from the review)
novelty-vs-prior: all four findings PARTIAL (none CONFIRMED as "obvious/known")
— the reference-shift algebra + minimal-patch invariant resolving the
fidelity-vs-correctness conflict survived the "this is obvious" attack.
