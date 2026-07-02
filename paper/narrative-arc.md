# Narrative Arc — the spine

## The fire
Not "spreadsheets are important." The specific fire: an LLM agent editing a
real workbook is operating blind on an artifact whose owner is *accountable*
for every cell, through a substrate (openpyxl driving OOXML) that silently
destroys structure the agent never intended to touch — and neither the agent
nor any benchmark can see the damage. We watched Anthropic's own xlsx skill
corrupt financial models (issue #22044). We measured a single openpyxl
re-save strip 101,961 cached formula values from a five-file corpus. The
agent's summary said "done." The field's benchmarks would have scored it a
success. That gap between *looks done* and *is intact* is the fire.

## Why this approach (and not the alternatives)
- Not a better prompt / skill: guidance cannot make carelessness
  impossible; the runtime is the only place a guarantee can live (the
  2025–26 enforcement wave agrees, but every one of those systems is
  format-blind — none understands the artifact it guards).
- Not a better agent: SheetCopilot/SheetAgent chase task success; the
  damage they don't measure is orthogonal to how smart the planner is.
- Not human-in-the-loop oversight (Pista): synchronous human auditing with
  model-generated explanations doesn't scale to unattended operation and
  never verifies that the explanation matches the executed action or that
  the file survived intact. Pista's own Limitations name the door we walk
  through: "solely system prompt instructions... programmatic controls" as
  future work.
- The approach: a format-aware enforcement boundary. Read-only by
  construction; a privacy-safe structural census so the agent reads
  structure not content; a differential-validated engine that reports its
  own evaluability blind spots per file (coverage-honesty); typed patches
  with dry-run and hash-chained receipts (spec'd) so every write is
  predicted, attributed, and reversible.

## The journey (predictions vs reality — from results.tsv)
- We predicted the engine we'd build on would need most of Phase 0/1 of the
  vision written from scratch. Reality (disconfirm, the strongest gradient):
  IronCalc *master* had already added ~148 functions since the pinned
  release — the honest move was to vendor master and contribute upstream,
  not rebuild. Coverage jumped 66.1%→94.6% by *reading the diff*, not
  coding. Lesson: the substrate moved faster than the plan; measure before
  building.
- We predicted "100% coverage" was a clean target. Reality (partial): 100%
  is only honest as *three numbers* — 522/522 recognized, 505 locally
  evaluable, 17 policy-limited returning the exact error literal Excel
  returns. Forcing WEBSERVICE to fetch URLs would violate the security
  thesis; the taxonomy is more correct than a bare percentage.
- We predicted the differential oracle would mostly confirm engine
  correctness. Reality (surprise): it surfaced real bugs in *both*
  directions — IronCalc miscomputing CONVERT(F→C) and dropping ROW-over-
  range arrays, *and* LibreOffice deviating from Excel on POWER(0,0) and
  PERCENTRANK. The oracle became a contribution, not just a check.
- We predicted diff was done after 58 tests. Reality (disconfirm, found by
  surface verification not review): an openpyxl re-save made diff report
  "1 change" while 442 formula results silently vanished — the exact
  failure the whole project exists to catch, hiding in our own tool. Added
  the cached_value change-kind. The bug that most validated the thesis was
  in our own hands.

## Load-bearing assumptions (state plainly; invite challenge)
1. The vendored engine's outputs are trustworthy *because* they are
   differential-validated against LibreOffice with Excel-doc arbitration —
   not because IronCalc is assumed correct. Where the oracle is silent or
   both engines are unvalidated, coverage-honesty flags it.
2. The census's privacy invariant (structure only, UDF names as user data)
   holds — substantiated by a regression test asserting a sentinel cell
   value never appears in output, but it is an invariant a reviewer should
   probe.
3. The prediction ledger is retrospective for the systems work; predictions
   are only claimed where genuinely recorded before the run. The paper says
   this in the open.

## What was tried and discarded
- Rebuilding the calc engine (the memo's ambition) — discarded when the
  upstream diff made it wasteful; contribute upstream instead.
- A bare "100% coverage" claim — discarded for the three-number taxonomy.
- Formatted-string comparison in diff/calc — discarded (masked sub-display
  drift); raw-value comparison with a format change-kind instead.
- Treating LibreOffice as ground truth — discarded; it is a reference, and
  Excel documentation is the arbiter (three-way verdict).
