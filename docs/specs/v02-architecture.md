# xlq v0.2 — The Proper Solution: a Surgical Transactional Write Boundary

Written 2026-07-03 after the adversarial PC review (unanimous reject of v0.1's
framing) and the directive to reframe the problem and build the real solution.

## The problem, correctly framed
An LLM agent that edits a spreadsheet has no safe write primitive. Its only
option today is generate-openpyxl-code that loads → mutates → saves the whole
workbook, which:
- rewrites ~100% of the OOXML container (byte-provenance destroyed);
- destroys every part the library cannot model — MEASURED: openpyxl drops
  6/10 chart parts on a real pivot+chart workbook, and 100% of VBA
  (`vbaProject.bin`) on a macro workbook (both reproduced 2026-07-03);
- blanks cached formula values;
- offers no precondition check, no preview, no receipt, no rollback.

A read-only inspector (v0.1) observes this damage but does not prevent it. The
proper solution is the write primitive itself.

## The proper solution: surgical apply
`xlq apply <book.xlsx> <patch.json> [--dry-run]` performs a *surgical* edit:
it rewrites only the OOXML parts that contain an affected cell and copies
every other part byte-for-byte from the input.

### The fidelity property (provable, checkable)
> After `xlq apply`, every OOXML part that does not contain a cell changed by
> the patch is **byte-identical** to the input part.

Charts, pivot caches, VBA projects, drawings, styles, theme, external links —
all preserved by construction, because the writer never re-serializes them.
This is the property openpyxl, LibreOffice, and IronCalc's own `save_to_xlsx`
all fail (each rewrites the whole container). It directly closes the #22044
harm class.

### Algorithm
1. **Precondition.** `sha256(file) == patch.base_hash` else `revision_mismatch`.
2. **Lock.** Advisory lock `book.xlsx.xlq.lock` (O_EXCL) across the operation.
3. **Predict (also the dry-run path).** Load a COPY into IronCalc, apply the
   typed ops (`set_cell`, `set_formula`), `evaluate()`, and compute the full
   set of affected cells + their new values + any new formula errors +
   watch-list before/after + coverage flag. Dry-run returns this and stops.
4. **Surgical write.** Open the input `.xlsx` as a zip. Map affected cells →
   their sheet parts (`xl/worksheets/sheetN.xml`) via `workbook.xml` + rels.
   For each affected sheet part, parse its XML and edit ONLY the `<c>`
   elements for affected cells (set `<f>`/`<v>`, update the shared-string or
   inline value), preserving all other elements in that sheet (merged cells,
   conditional formatting, column widths…). Drop `xl/calcChain.xml` if
   present (Excel rebuilds it; keeping a stale one is the only cross-part
   hazard). Copy every other part byte-for-byte.
5. **Deterministic repackage.** Fixed zip entry mtimes + stable ordering so
   `result_hash` is reproducible.
6. **Atomic apply.** Write `book.rev-N.xlsx` (immutable history), fsync,
   atomically rename onto `book.xlsx`. Append a receipt to the hash-chained
   journal `book.xlsx.xlq.jsonl` `{rev, base_hash, result_hash, ops, ts,
   actor, engine_version, clock, seed, kind}`; genesis + external_edit
   adoption semantics per docs/receipt-journal-spec.md.

## Non-bypassability / the enforcement model (addresses the threat-model fatal)
xlq is an enforcement boundary for an agent CONFINED to it: the agent harness
grants the agent the `xlq` tool and NO raw filesystem write to `.xlsx` (no
openpyxl, no direct file write). Within that harness the surgical, preview-
gated, receipted path is the agent's only write capability, so it cannot
reach the #22044 failure mode. This is the standard sandbox model (an agent
is bounded by the tools its harness exposes, per the 2025–26 enforcement
literature); the boundary is non-bypassable *within the harness*, and we say
exactly that rather than claiming a guarantee against an agent with raw shell.
The shipped `skills/xlq/SKILL.md` + a restricted tool surface is the concrete
deployment.

## What this makes true that v0.1 could not claim
- "Enforcement boundary" is honest: a built write path that an agent is
  confined to, not "absence of a write path."
- A provable, measured fidelity guarantee vs the openpyxl/LibreOffice status
  quo, on real charts/pivots/VBA (the #22044 harm we previously could not
  measure).
- The interventional experiment becomes runnable: agent-with-xlq vs
  agent-with-openpyxl on real editing tasks, scoring task success AND
  feature survival.

## Experiment plan (closes the reviewers' fatals)
- **E1 Fidelity preservation (the provable property):** on a corpus with
  charts/pivots/VBA, run the same edit via (a) openpyxl codegen, (b)
  LibreOffice, (c) `xlq apply`. Report per-part survival. Expect xlq: every
  non-affected part byte-identical; openpyxl/LibreOffice: features dropped /
  parts rewritten. Per-file numbers, not corpus totals.
- **E2 Agent A/B (the interventional result):** an LLM agent completes real
  editing tasks (update a value, fix a formula, add a column) two ways —
  openpyxl-codegen vs xlq-apply — on real workbooks with charts/pivots/VBA.
  Score task success AND fidelity of untouched content. This is the
  experiment the thesis always demanded.
- **E3 Financial cross-check:** validate the oracle's financial verdicts
  against `numpy-financial` / an external bond library to retire the
  circularity objection (independent third implementation, not our own).
- **E4 Dry-run correctness:** dry-run's predicted affected-cells/new-errors
  match the actual post-apply state on the fixture corpus.

## Honest scope
IronCalc is still used for the dry-run PREDICTION (loading a copy to compute
affected cells) — but it never touches the written file; the write is pure
zip surgery. So engine-fidelity limits affect the PREDICTION quality (flagged
by coverage-honesty), not the preservation guarantee, which is engine-
independent. Corpus is still small but now contains the real T1 features;
we scope fidelity claims to "these workbooks / these substrates," not to the
field.
