# Research Setup — Surgical Structural Edits (Phase 0)

**Date:** 2026-07-04 · **Phase:** 0 · **Status:** in-progress

## Context
The adversarial PC (mean −1.0, no fatals left) named ONE lever that lifts both
coverage and novelty: structural edits (row/column insert-delete, range move)
with fidelity preserved. Every reviewer flagged "only the easy edit class is
built"; the chair called structural edits "a genuine research problem, not zip
surgery." This is the grind-for-top-venue research project, run through the
sciagent methodology with the arXiv MCP for literature.

## Idea DNA

- **Problem.** Perform a structural edit (insert/delete row/column, move a
  range) on an OOXML `.xlsx` SURGICALLY — preserving fidelity — while
  correctly shifting everything a structural edit invalidates: in-sheet
  relative/absolute formula references, cross-sheet references INTO the edited
  sheet, defined names, merged cells, conditional-formatting and
  data-validation ranges, table ranges, chart data references
  (`xl/charts/*`), and pivot-cache source ranges (`xl/pivotCache*`). The
  load-into-engine-and-re-serialize approach destroys fidelity (rewrites all
  parts); no existing tool does structural edits surgically.

- **Assumption (inferred, load-bearing).** The v0.2 fidelity property
  ("every part without an edited cell is byte-identical") is the WRONG
  invariant for structural edits, and that is the crux. A structural edit
  inherently touches parts we want to preserve: a chart referencing `A1:A10`
  MUST become `A1:A11` after inserting a row, and a pivot source range must
  grow. So byte-identity is impossible for parts that reference the edited
  region. The right invariant is a REFINEMENT:
  > **Minimal reference-shift patch.** After a structural edit, the only bytes
  > that differ from the input are (a) reference coordinates that the edit
  > provably shifts, changed by exactly the shift delta, and (b) the cells
  > physically inserted/removed. Every non-reference byte of every part —
  > chart styling, pivot layout, VBA, drawing geometry, number formats — is
  > identical.
  This turns "preserve the file" into "preserve everything except the
  coordinates the edit logically moves, and move those minimally."

- **Novelty claim.**
  1. A **reference-shift algebra** for structural OOXML edits: a total
     function that, given a structural op (insert/delete row/col at index k,
     count n) and any A1/absolute/range/cross-sheet/structured reference,
     yields the shifted reference (or `#REF!` when the anchor is deleted),
     applied uniformly across ALL reference-bearing OOXML parts, not just the
     edited sheet.
  2. A **minimal-semantic-patch** writer that edits only reference coordinates
     in each part (via token-level XML surgery) so all non-reference bytes stay
     identical — extending the byte-identity fidelity property to the case
     where preservation and correctness appear to conflict.
  3. **Proof-carrying structural apply**: re-load the output, verify (a) every
     reference resolves to the logically-correct shifted target, (b) the
     non-reference bytes of every part are unchanged, (c) the file recomputes
     correctly; abort otherwise.
  4. The invariant is **provably minimal** (only edit-shifted coordinates
     change) — a checkable property, not a claim.

- **Domain.** Systems / software engineering (document transformation,
  spreadsheet tooling) for LLM-agent safety.

- **Success criteria.** On the charts/pivots/VBA corpus + new structural
  fixtures: after insert-row / insert-col / delete-row / delete-col / move,
  (i) all reference classes shift correctly (formulas, cross-sheet, named
  ranges, merged cells, CF, DV, charts, pivot caches) — verified against
  IronCalc recompute AND Excel-documented shift semantics; (ii) every
  non-reference byte identical (the minimal-patch invariant holds); (iii) the
  file re-opens (ironcalc + LibreOffice) and computes the same values a
  full-recompute engine gives; (iv) proof-carrying verification passes;
  (v) openpyxl/LibreOffice comparison shows they cannot do this surgically.
  Target: the minimal-patch invariant holds on ≥N structural edits across the
  reference classes, with the residual (references we cannot yet shift, e.g.
  inside VBA) explicitly reported, not silently wrong.

- **Scope constraints.** Local compute; vendored IronCalc for recompute
  oracle; new structural fixtures needed (with cross-sheet refs, named ranges,
  CF/DV, charts, pivots). Research intensity Deep (the arXiv MCP lit review
  must establish exactly what is and is not novel here).

## Evaluation contract (immutable once set)
- Recompute oracle: IronCalc load→evaluate of the STRUCTURALLY-EDITED file
  vs the reference (a full load-edit-resave through the engine, which is
  correct-but-fidelity-destroying). The engine gives correctness ground truth
  for VALUES; the minimal-patch invariant is checked by byte-diff.
- Reference-shift ground truth: Excel's documented insert/delete shift rules
  (relative shifts, absolute-row/col pinning, range endpoint growth, `#REF!`
  on anchor deletion) — the arbiter for whether a shift is correct.
- Minimal-patch metric: after the edit, partition every changed byte-run into
  {reference-coordinate change | inserted/removed cell | OTHER}; OTHER must be
  empty for the invariant to hold. Per-part, per-op, never aggregated.

## Next
Phase 1 literature review via arXiv MCP: spreadsheet refactoring/reference
adjustment, OOXML/document transformation, incremental/surgical file editing,
program-transformation reference rewriting, spreadsheet dependence analysis.
Then hypothesis + theory review, PoC (insert-row), build, evaluate, paper.
