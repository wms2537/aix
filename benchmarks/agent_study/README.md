# agent_study — guarded vs unguarded LIVE-AGENT study (insert-row@2)

A harness for measuring what a **certify-or-refuse guard** buys (and costs) when a
LIVE LLM agent performs a structural spreadsheet edit on real workbooks. The prior
live slice (`../live3way_truth.py`) was circular: "agent correct" and the guard
verdict were the same predicate (both equality-to-xlq). Here the three instruments
are **independent**:

| role | instrument | independence |
|---|---|---|
| ground truth | `shift_correctness_real.ref_shift` — the reference shifter validated against **two engines** (LibreOffice + `formulas`, 0 divergences in conformance_v2) | never sees the guard; pure text predicate |
| guard (guarded arm) | `foreign_certify.certify_foreign` — the direct graph-hypothesis checker ((fn,deps,O) triples, σ-premise via `experiments/generality/router.certify_edit`), engine-free | **NOT** equality-to-xlq; never sees the truth predicate |
| shipped artifact | openpyxl `insert_rows` for structure + zip surgery splicing **only** the agent's `<f>` bodies | nothing of xlq or the guard's reference transform is in the file |

## Pipeline

```bash
PY=/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/abenv/bin/python
# (any python3 with openpyxl works)

# 1. generate tasks (already run; tasks.json checked in this dir)
$PY prep.py 30

# 2. ORCHESTRATOR: run live LLM agents over tasks.json, collect
#    agent_outputs.json = { "<task file>": { "<orig cell A1>": "<corrected formula>", ... }, ... }
#    (one entry per task file; formulas may carry a leading '='; cells the agent
#     does not answer ship with openpyxl's behavior: formula text left UNSHIFTED)

# 3. score both arms
$PY score.py /path/to/agent_outputs.json results_live.json

# smoke test of the harness itself (synthetic agents, no LLM):
$PY synthetic_agents.py perfect && $PY score.py agent_outputs_perfect.json results_smoke_perfect.json
$PY synthetic_agents.py sloppy  && $PY score.py agent_outputs_sloppy.json  results_smoke_sloppy.json
```

## Task format (`tasks.json`)

live3way's compact format — `{file, sheet, k, cells:[{cell,row,col,formula,cached_value}]}`
— plus a `difficulty` tag: `n_formulas`, `has_absolute_refs`, `has_ranges`, and the
truth-coverage fields `truth_evaluable_cells`, `truth_shift_cells`, `truth_total`.
Formula text is XML-unescaped (`<`, not `&lt;`); score.py re-escapes on splice.
The task: a blank row is inserted at row 2 of `sheet`; the agent must return, for
every listed cell (keyed by its ORIGINAL address), the formula that belongs at its
post-insert position.

## Outcomes and metrics

Per task: `agent_correct` = every truth-evaluable cell of the **built artifact**
matches `ref_shift` (normalized text; cells outside the grammar are skipped and
counted). Then

- **guarded** ∈ {`shipped_correct`, `shipped_CORRUPT_false_cert`,
  `refused_correct` (=COST), `refused_incorrect` (=SAVE)} — CERTIFIED ships,
  REFUSED blocks (guard exceptions/unparseable → REFUSED, fail closed);
- **unguarded** ∈ {`shipped_correct`, `shipped_CORRUPT`} — always ships.

Summary: corruption incidence per arm, SAVES, COST rate (refused-correct /
agent-correct = completion loss on correct work), `FALSE_CERT_must_be_0` (the
soundness claim), and `COST_split` (see caveat 2).

## Smoke results (synthetic agents; harness verification, not an agent claim)

**(a) perfect agent** (ref_shift where in-grammar; xlq's transform — agent-side
only — for the out-of-grammar cells; 17/196 cells fell back to *unchanged* where
xlq refused):

| metric | value |
|---|---|
| tasks scored | 21 |
| unguarded: shipped_correct / CORRUPT | 21 / **0** |
| guarded: shipped_correct / false certs | 16 / **0** |
| guarded: refused_correct (cost) | 5 (all `truth_partial` → see caveat 2; these artifacts really do carry unshifted out-of-grammar cells) |
| guarded: refused_incorrect (saves) | 0 |

**(b) sloppy agent** (10% of truth-visible shift cells — 10/98 — left unshifted,
seed 42):

| metric | value |
|---|---|
| tasks scored | 21 (7 corrupted by construction) |
| unguarded: shipped_CORRUPT | **7** (incidence 0.333) |
| guarded: shipped_CORRUPT (false certs) | **0** |
| guarded: refused_incorrect (SAVES) | **7** (save rate 1.0) |
| guarded: refused_correct (cost) | 3 (all `truth_partial`) |

## Honest limitations of this harness

1. **Single op, single sheet.** Only insert-row@2 on the first worksheet; no
   claim generalizes to other ops/axes/multi-sheet edits from this study.
2. **The reference-shifter grammar bounds the truth set.** `ref_shift` refuses
   any formula containing tables, cross-sheet refs, whole-row/col refs — and,
   because its `RANGE_FN` gate matches *every* range (not only function-endpoint
   ranges like `A9:CHOOSE(...)`), **any formula containing a range at all**. In
   this task set 59/196 cells are truth-excluded and 12/21 tasks are truth-partial.
   Consequences: (i) `FALSE_CERT=0` is claimed only relative to truth-visible
   cells — a corruption hidden in a truth-skipped cell of a CERTIFIED task would
   not be counted; (ii) a `refused_correct` on a truth-partial task may actually
   be a hidden save — `COST_split` separates unambiguous cost (truth-total tasks)
   from ambiguous refusals. In both smoke runs *all* refused-correct were
   truth-partial, and inspection showed real unshifted out-of-grammar cells.
3. **Corpus yield is 21, not 30.** Under the no-guessing gates the 231-file corpus
   yields only 21 scorable tasks regardless of the size band (134 files have
   shared-formula followers whose text can't be presented to the agent and which
   openpyxl expands into guard-unaccountable nodes; 25 have no truth-visible
   shift; plus uncertifiable/volatile/trivial files).
4. **Prep pre-restricts to the guard-modelable universe**, so the measured COST
   *understates* the guard's real-world refusal cost: workbooks with whole-col,
   cross-sheet, table, defined-name, or volatile constructs — which the
   fail-closed checker refuses wholesale (see `../foreign_certify.json`: 1/23
   faithful foreign edits certified on the unfiltered corpus) — never become
   tasks, because neither the truth nor the guard could rule on them. The cost
   measured here is the *residual* conservatism (e.g. function-endpoint ranges,
   ref-lookalike tokens inside string literals).
5. **Known σ-model vs xlq divergence** (observed in smoke (a)): for
   `A9:CHOOSE(...)`-style ranges the guard's σ demands the cell endpoint shift
   (`A9→A10`, matching Excel); xlq leaves it. Fail-closed (a refusal source),
   not a soundness hole.
6. **Volatile/position-dependent functions excluded up front** (OFFSET, INDIRECT,
   ROW, ...): `foreign_certify.extract` deliberately skips those cells, so an
   agent error there is invisible to the guard; including them would have made
   false-cert accounting meaningless. This is a real blind spot of the guard,
   scoped out of the study rather than hidden.
7. **Text-match truth.** Correctness is normalized (whitespace/case) formula-text
   equality with the reference shift. A semantically equivalent but rewritten
   formula (reordered arguments, added parentheses) scores as wrong. Agents must
   correct references only, not restyle formulas.
8. **Artifact fidelity depends on openpyxl.** If openpyxl drops a formula in the
   round-trip, the cell scores wrong (`missing_formula_in_artifact` is tracked;
   0 in both smoke runs). Both arms score the same artifact, so builder noise
   cannot bias the arm comparison, but it could depress absolute agent scores.
9. **Unanswered cells ship unshifted** (openpyxl's behavior) and are scored as
   part of the artifact; files absent from agent_outputs.json are not scored at
   all (listed in `tasks_without_agent_output`).

Files: `prep.py` (task generation), `score.py` (artifact build + truth + both
arms), `synthetic_agents.py` (smoke agents), `tasks.json` (21 tasks),
`agent_outputs_{perfect,sloppy}.json` + `results_smoke_{perfect,sloppy}.json`
(smoke artifacts).
