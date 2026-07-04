# Hypothesis — The Reference-Shift Algebra & Minimal-Patch Invariant (Phase 2)

**Date:** 2026-07-04 · **Phase:** 2 · **Status:** in-progress

## The falsifiable hypothesis
> A structural edit (insert/delete row/column at index k) on an OOXML
> spreadsheet can be realized as a **minimal reference-shift patch**: there
> exists a total shift function σ over cell/range references such that applying
> σ to exactly the reference-bearing tokens of every OOXML part — and inserting
> or deleting the affected `<row>`/`<c>` elements — produces a workbook that
> (a) recomputes to the same values as a full engine load-edit-resave, while
> (b) every byte that is NOT a reference coordinate or a physically
> inserted/removed cell is identical to the input. Byte-identity and
> correctness, which appear to conflict for structural edits, are simultaneously
> achievable.

Falsifier: if any reference class (in-sheet, cross-sheet, named range, chart
`<c:f>`, pivot source, merged/CF/DV) cannot be shifted by a local token rewrite
without re-serializing its part, OR if the shifted file's recompute diverges
from the engine reference, the minimal-patch invariant is false for that class
and must be reported as residual, not silently wrong.

## The reference-shift algebra σ (grounded in TACO's fixed/relative model)
For an insert of `n` rows at row index k (columns symmetric), a reference to
row r with absolute-flag a:
- **σ_row(r, a):** if the reference is a range endpoint or a cell anchor, then
  `r' = r + n` if `r >= k`, else `r' = r` (rows above the insert don't move).
  The absolute flag `$` does NOT exempt a reference from a STRUCTURAL shift —
  this is the subtle point: `$A$5` still becomes `$A$6` when a row is inserted
  above row 5 (absolute pins against FILL, not against insert/delete). TACO's
  fixed/relative distinction governs autofill propagation; structural shift is
  governed by position vs k. This distinction is a load-bearing correctness
  point and a likely place engines/libraries disagree (a measurable finding).
- **Range growth vs shift:** a range `A[k-1]:A[k+3]` that STRADDLES the insert
  (head < k <= tail) GROWS (tail += n, head fixed); a range entirely at/below k
  SHIFTS (both += n); entirely above is unchanged. This head/tail asymmetry is
  exactly TACO's head/tail endpoint model, now under structural edit.
- **#REF! on deletion:** for a delete of rows [k, k+n), any reference whose
  anchor falls inside the deleted band collapses to `#REF!`; a range that
  straddles shrinks; below shifts up.
- **Uniform application:** σ is applied to the SAME token grammar wherever an
  A1 reference appears: sheet `<f>` formulas and `r=` cell coords, cross-sheet
  `Sheet1!A5`, `<definedName>` bodies, chart `<c:f>Sheet1!$A$1:$A$10</c:f>`,
  pivotCache `<cacheSource><worksheetSource ref="A1:D20"/>`, `<mergeCell ref>`,
  `<conditionalFormatting sqref>`, `<dataValidation sqref>`. The novelty is that
  σ is ONE algebra applied across ALL parts by token surgery — the parts stay
  byte-identical except where σ fires.

## Why this is a reframe, not a stack (anti-stacking check)
Not "TACO + our zip surgery." TACO maintains an in-memory graph for recompute
speed and never touches the file; our claim is that the SAME reference algebra,
when pushed down to on-disk token surgery across all reference-bearing parts,
resolves the correctness-vs-fidelity conflict that made structural edits the
unsolved case. The conceptual move: **a structural edit is not a
re-serialization, it is a coordinate transformation applied minimally to the
reference tokens in place.** Excelsior said "lift to a model, regenerate"
(loses fidelity); we say "never leave the bytes; move only the coordinates."

## Theory-review gate (before any build)
Claims to stress-test (dispatch a theory reviewer):
1. Is σ TOTAL and CORRECT for all reference classes, incl. the absolute-flag
   subtlety and the head/tail straddle asymmetry, against Excel's documented
   insert/delete semantics? Where is it likely WRONG or under-defined?
2. Is byte-identity-except-σ actually achievable, or does inserting a `<row>`
   force re-indexing that ripples (e.g., every subsequent row's `r=` attribute,
   every cell `r=` in shifted rows) — meaning the "minimal" patch is large but
   still MINIMAL (each changed byte is a coordinate)? Confirm the invariant
   survives that ripple (changed coords are still "reference coordinates").
3. Which classes are genuinely out of reach by token surgery (VBA-embedded
   refs, chart cached number caches, defined names with structured-table refs)
   and must be honest residual?
4. Is the recompute-equivalence oracle sound given IronCalc's own limits?

## Next
Theory review → if PASS, Phase 3 PoC: insert 1 row into a fixture with an
in-sheet relative ref, an absolute ref, a straddling range, a cross-sheet ref,
a named range, and a chart series; predict-then-run; check σ-correctness
(recompute matches engine) AND minimal-patch invariant (non-coordinate bytes
identical). Record signal in results.tsv.
