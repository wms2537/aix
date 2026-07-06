# The certify-or-refuse router — two levers, honestly scoped

Built, adversarially verified, and rescoped to what the evidence supports. The
verification found a real soundness bug and a real mislabel; both are fixed here.

## Lever 1 — the composition rule (structural ⊸ value), formalized — BANKABLE

A real agent task is a certified structural scaffold + value fills. The router
factors it and the guarantee is machine-checked:
- `formal/SelfOracle.lean`: `eval_iso_invariant` (the scaffold is value-faithful
  under any semantics) + **`eval_local` / `audit_surface_bound`** (a node's value
  depends only on its dependency cone, so value fills change nothing outside their
  downstream cone). No `sorry`; axioms `[propext, Quot.sound]`.
- `router.py` (`certify_edit`) enforces a sound declaration-matching discipline:
  every non-declared node must be the σ-relabeling of the original in structure
  (fn/deps — Theorem 1) and, outside the fill cone, in value. A COMPUTED node is
  covered by Theorem 1; a LEAF must be confirmed against the self-oracle and
  **fails closed** if its oracle entry is missing. Any unaccounted change →
  REFUSED. Under/over-declaring is contained: under-declaring a change is caught
  as an undeclared change; over-declaring only inflates the audit surface, never
  hides a change; a wrong σ → scaffold mismatch → REFUSED.

This turns "collapse the audit surface" from a slogan into a **bounded guarantee**:
the set of values a consumer must re-check is provably contained in the declared
fills' downstream cone; everything else keeps its Theorem-1 value.

## Lever 2 — a correctness HARNESS for the router (NOT an agent evaluation)

`guard_measure.py` is a small self-authored branch-correctness harness. Honest
scope (per adversarial review, which was right): there is no LLM agent here, the
6 cases are hand-built to exercise each REFUSE branch, and the ground truth is
self-authored — so it **cannot establish real-world efficacy**. Its real value:
it is adversarial to the router and hardened it. It currently passes:
0 silent corruptions, 0 false refusals, and the confirmed fail-open exploit now
fails closed (`test_partial_oracle`).

### Three real defects the harness + review hardened (rigor, self-applied)
1. A value edit to a LEAF cell has no graph footprint → silent-corruption miss →
   fixed with a self-oracle value check on non-declared nodes.
2. That check falsely refused legitimate fill cones → fixed cone-aware (structure
   everywhere; value preserved only OUTSIDE the fill cone).
3. **The value check FAILED OPEN on a missing oracle entry** — a leaf absent from
   `O` with a silently changed value certified (confirmed exploit, 3→999
   "CERTIFIED"). Fixed **fail-closed**: a leaf whose value cannot be confirmed is
   never certified untouched. (Did not fire on SQLite only because that adapter
   writes `O` for every cell — an accident of the format; spreadsheets with
   referenced-empty / stripped-cache cells violate leaf-`O`-totality.)

## What the collapse number is, honestly
`collapse% = 1 − |cone|/total` is a property of the workbook+edit, not the tool.
It scales with untouched row count and **craters to ~0% on shared-upstream edits**
— exactly the financial-model case (a rate/assumption fans out to the whole
model). So the *formal bound* (`audit_surface_bound`) is the contribution; the
per-fixture percentage is not a burden-reduction claim and pure-rename (cone=∅,
tautological 100%) is excluded from any average.

## The one thing that gates acceptance (not yet built)
An **LLM-agent-in-the-loop A/B on a non-self-authored spreadsheet corpus** —
agent-with-router vs agent-without — with an **INDEPENDENT engine oracle** (label
faithful/corrupted by full-recompute divergence, never by the self-oracle `O`),
botches being the agent's OWN real mistakes, scored on task completion AND
untouched-content fidelity. This converts Lever 2 from unit-test to evaluation and
is the missing interventional axis. Scaffolded at `agent_ab/` (next).
