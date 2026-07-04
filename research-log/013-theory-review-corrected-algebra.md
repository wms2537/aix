# Theory Review Outcome + Corrected Algebra (Phase 2, iter 2)

**Date:** 2026-07-04 · **Phase:** 2 · **Status:** completed · **Gate:** FAIL → corrected

## Prediction vs Reality (sciagent predict-then-run)
Predicted (medium confidence): σ was roughly right, absolute-flag subtlety the
main risk. **Reality: FAIL on THREE decisive counts.** Strong gradient — the
review shrank the hypothesis space hard and made the contribution sharper. The
absolute-flag claim was CONFIRMED correct; the failures were elsewhere, which I
did not predict. This is exactly the anti-fragile signal the method exists to
surface: had I built first, the delete path would have silently corrupted files.

## The three FAIL-level defects (now fixed)
1. **Delete `#REF!` was wrong for the common clip case.** `=SUM(A5:A10)` delete
   rows 5–6 must yield `=SUM(A5:A8)` (clamp+shift), NOT `#REF!`. `#REF!` is
   correct ONLY when the reference is ENTIRELY consumed. Rule 3 was also
   internally contradictory (a clipped range is both "anchor in band" and
   "straddling"). Replaced with a 6-case clamp model.
2. **Sheet scoping was unsound.** "Apply σ uniformly to every cross-sheet
   token" corrupts references to non-edited sheets and external workbooks. σ
   must be GATED: shift a token only if its resolved target is the edited sheet
   (or a 3D span containing it). Latent corruption bug, now a precondition.
3. **Shared formulas break byte-identity when the group interior is crossed by
   a cross-line relative ref.** Coordinate surgery can't express it → forces
   expanding `<f t="shared" si>` stubs into full `<f>` bodies (large
   non-coordinate rewrite). Now declared RESIDUAL (detect → expand-or-refuse).

## The CORRECTED algebra σ (frozen for the PoC)

### Insert n rows at k — independent per-endpoint (C2, replaces straddle prose)
For each reference endpoint at row r: `r' = r + n if r >= k else r`, applied to
head and tail INDEPENDENTLY. This reproduces every boundary for free:
- insert above the whole range (k ≤ head) → both shift (range moves down);
- insert strictly inside (head < k ≤ tail) → head fixed, tail shifts (GROW);
- top/bottom asymmetry (blank row excluded at top, included at bottom) falls
  out of endpoint tracking — no special case.

### Delete rows [k, k+n) — 6-case clamp (C1, replaces the buggy Rule 3)
Classify by the WHOLE reference after per-endpoint evaluation:
| head | tail | result |
|---|---|---|
| < k | < k | unchanged |
| ≥ k+n | ≥ k+n | both −= n (shift up) |
| < k | ≥ k+n | shrink: head fixed, tail −= n |
| in [k,k+n) | ≥ k+n | **clamp head to k** (new coord), tail −= n |
| < k | in [k,k+n) | **clamp tail to k−1**, head fixed |
| k ≤ head ≤ tail < k+n | (entirely consumed) | **`#REF!`** |

### Scoping precondition (C4)
Shift a token ONLY if its resolved target sheet == edited sheet, OR the edited
sheet lies within a 3D span `Sheet1:Sheet3!…`. NEVER shift external `[n]Sheet!…`
or other-sheet refs. Honor `definedName@localSheetId` scope.

### Absolute flag (C3, CONFIRMED correct)
`$` does NOT exempt a reference from a structural shift: `$A$5` → `$A$6` on
insert above row 5 (`$` governs fill/copy, never insert/delete — the documented
`INDIRECT("A5")` pin workaround exists precisely because `$A$5` is not immune).
Caveat: `INDIRECT`/`OFFSET` text-argument refs are OPAQUE — never A1-parse them.

### Axis selectivity + whole-row/col forms (C7)
A row op shifts only the row axis; column-only refs (`A:A`, `$A:$C`) are
untouched (and vice versa). Parse `5:5`, `$5:$5`, `A:A` as distinct forms.
Row-only refs never `#REF!` on row delete.

### R1C1 guard (C5)
Assert `calcPr@refMode` is A1/absent before applying the A1 grammar; abort to
residual otherwise.

## Grammar coverage (C6) — the reference-bearing tokens σ must reach
Beyond sheet `<f>` + `<c r>` + `<row r>`: cross-sheet refs, `<definedName>`
bodies (incl. `_xlnm.Print_Area` multi-area, `_xlnm.Print_Titles` whole-row/col),
chart `<c:f>`, pivotCache `worksheetSource@ref` + pivot `<location@ref>`,
`<mergeCell@ref>`, `<conditionalFormatting@sqref>` AND `<cfRule><formula>` bodies
(+ x14), `<dataValidation@sqref>` AND `<formula1>/<formula2>`, tables
`tableN.xml@ref` + `<autoFilter@ref>` + column removal, worksheet
`<autoFilter@ref>` + `<filterColumn@colId>`, sparklines `<xm:f>/<xm:sqref>`,
`<hyperlink@ref>`+`@location`, comments `<comment@ref>`+VML `<x:Anchor>` CSV,
view `<pane>/<selection@sqref>`, `<dimension@ref>`. Non-A1 numeric anchors
(`<col@min/@max>`, drawing `<xdr:col>/<xdr:row>` 0-based, VML anchor CSV) get a
SEPARATE integer handler, not the A1 grammar.

## The residual set (HONEST out-of-scope, reported never silently wrong)
- Shared-formula group whose interior is crossed by a cross-line relative ref →
  expand affected dependents to explicit `<f>` (non-coordinate rewrite) OR
  refuse; detected, never silently off-by-n.
- Array formulas spanning the edit line → Excel forbids splitting; guard
  (shift whole `ref` atomically or refuse interior split).
- Fully-consumed non-formula range elements (`mergeCell`, `dataValidation`,
  `cfRule` sqref, hyperlink) → REMOVE the element (structural), can't hold
  `#REF!` (C8).
- `calcChain.xml` → DROP (+ its Content_Types Override + rels) — rebuildable,
  not a coordinate shift (C9). Accepted non-coordinate touch in exactly 2 parts.
- Floating/legacy anchors → integer handler or declared fidelity loss.
- Stale chart `numCache`/`strCache` → `<c:f>` grows correctly; caches stale but
  Excel recomputes → value-equivalent, not byte-parity. Benign residual.
- Inserted cells inside a shared/table/calculated column carry propagated
  formula + copied style (generation within "inserted cell").

## Refined minimal-patch invariant (precise)
> On row insert with explicit `<c r>`/`<row r>`, the ONLY forced changes are
> `<row@r>`, `<c@r>`, and in-scope `<f>`/ref coordinates — ALL coordinates —
> plus physically inserted rows/cells. Byte-identity-except-coordinates holds
> for the common case; it is ESCAPED only by the enumerated residual set, which
> is reported, not hidden. (Files omitting `<c r>`/`<row r>` are even cleaner —
> positional, nothing to bump.)

## Decision
Algebra corrected and now defensible. Proceed to Phase 3 PoC with the corrected
σ and the reviewer's 4 mandatory fixtures. Gate: PoC must show σ-correctness
(recompute == engine) on the clip-delete case + minimal-patch invariant on the
insert case + correct DETECTION of the shared-formula/array residual.
