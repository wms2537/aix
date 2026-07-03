# Paper Draft v1

**Date:** 2026-07-03 · **Phase:** 6 · **Status:** completed

## Context
Phases 0–2 (setup, ~50-paper literature review with decision archaeology,
four theory-reviewed falsifiable claims) fed the writing pipeline. Phases
3–5 (PoC/experiments/analysis) were satisfied by the completed systems work
+ the AXLE-bench measurements + the Excel-arbitrated disagreement triage —
this is a write-up of finished, frozen artifacts.

## Content
- Spine written before dispatch: narrative-arc, motivation-surface-map,
  writing-rationale-matrix, evidence-sheet (all in paper/).
- 15-agent workflow: 11 section writers (dependency groups) → assemble with
  story-integrity check → 3 independent role-scoped reviewers (methods /
  results / story, no shared context) → editor synthesis → targeted revision.
- Independent reviews: WEAK_ACCEPT / ACCEPT / WEAK_ACCEPT (accept-leaning,
  no rejects). Editor synthesis found 5 blocking/major issues; all fixed:
  1. write-path oversell → reframed to built (enforcement-by-absence) vs
     specified (mediated write layer, v0.2, not measured);
  2. comparison tolerance + the 1,311 denominator → defined with full
     reconstructable arithmetic (38 numeric-mismatch + 47 non-comparable =
     85; 1,273+38=1,311; 1,311+47=1,358), tolerance attributed to
     agreement.json per-case labels;
  3. arbitration protocol → specified (MS docs 2026-07-02 vs ECMA-376);
  4. `#NAME!` → `#NAME?` (bound to coverage.json — a real drift in the
     evidence sheet itself, caught and corrected);
  5. tables renumbered 1–6, §-refs fixed under late-Related-Work order.
- Final: 14,594 words, Abstract + 11 numbered sections + References
  (36 sources), 6 tables. paper/paper.md.

## Decision
Draft v1 accepted (editor synthesis accept-leaning, all blocking issues
resolved). Artifact package assembled (ARTIFACT.md). Optional venue: SE
technical/tools track (ICSE/FSE/ASE) or arXiv cs.SE first.

## Next Steps
- Optional PDF/LaTeX render for submission.
- User decisions (outward-facing, deferred to user): publish repo to GitHub;
  file the IronCalc coverage report + disagreement bug list upstream; arXiv
  submission. None done without explicit go-ahead.
