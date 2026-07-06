# The certify-or-refuse router — the two remaining levers, driven

Two levers the panel + chair named as the path forward, now built and measured.

## Lever 1 — the composition rule (structural ⊸ value), formalized

A real agent task is a certified structural scaffold + value fills. The router
factors it and the guarantee is machine-checked:
- `formal/SelfOracle.lean`: `eval_iso_invariant` (the scaffold is value-faithful
  under any semantics) + **`eval_local` / `audit_surface_bound`** (a node's value
  depends only on its dependency cone, so value fills change nothing outside their
  downstream cone). No `sorry`; axioms `[propext, Quot.sound]`.
- `router.py` (`certify_edit`) implements it: verify the edit MATCHES its
  declaration — every non-declared node is the σ-relabeling of the original in
  BOTH structure (fn/deps) AND self-oracle value (outside the fill cone). Any
  unaccounted change → REFUSED. On success, the audit surface = the declared
  fills' downstream cone; everything else is certified untouched.

`demo_router.py`: a mixed edit (rename column + set one value) → CERTIFIED with
the audit surface collapsed from 25 cells to 4 (**84% of the artifact certified
untouched**); a botched variant (agent silently corrupts a formula) → REFUSED.

## Lever 2 — shipped as a live agent guard, with measured saves

`guard_measure.py`: the router as a commit gate over 6 agent-proposed edits with
known ground truth (3 faithful, 3 botched; structural + mixed):

| Metric | Result |
|---|---|
| Botches caught (recall) | **3/3 = 1.0** |
| Silent corruptions missed | **0** |
| Faithful certified | **3/3** |
| False refusals | **0** |
| Avg audit-surface collapse | **89.1%** (100% pure-structural, 84% mixed, 83% reorder) |

This is the value proposition realized: every commit is *either* certified with a
**bounded, local audit surface** (the consumer checks the fill cone, ~11% of the
artifact, not the whole thing) *or* refused. Never silently wrong.

### The measurement found bugs in the router (rigor, self-applied)
The batch immediately caught two defects the demos missed:
1. A value edit to a LEAF (base) cell has no graph footprint, so the pure
   graph-iso check missed an undeclared base-cell edit → a **silent corruption**.
   Fixed: non-declared nodes must also preserve their self-oracle value.
2. That value check then wrongly refused a legitimate mixed edit, because a value
   fill's downstream cone (c,d,e) legitimately changes. Fixed: value preservation
   is required only OUTSIDE the declared fills' cone; structure is checked
   everywhere.

After both fixes: recall 1.0, false-refusal 0. The certifier is only trustworthy
because the measurement was adversarial to it.

## Honest scope
- Demonstrated engine-free on SQLite (the format-parametric core); the spreadsheet
  exact tier is xlq's implementation of the same principle.
- The audit-surface collapse assumes the agent DECLARES its value fills; the
  router verifies the declaration, catching any undeclared change (botch).
- Soundness of the exact tier still rests on extraction completeness; the
  value-preservation check outside the cone is the safety net against a missed
  dependency (a missed dep would change a value outside the declared cone →
  caught).
