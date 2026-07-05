# Generality — the moat is a format class, not a spreadsheet accident

The sharpest attack on the reframe (all four consult lenses converged on it):
*"'semantic redundancy' dissolves into memoization (ubiquitous, buys only
bounded-confidence checks) + static declarative dependency structure (the real
engine-free lever). The 96.6% is a property of the spreadsheet reference LANGUAGE
being static, not of caching. Notebooks HAVE full redundancy yet the exact tier
collapses to zero. So redundancy either generalizes trivially (moat-less) or the
engine-free tier doesn't ride on it."*

**We concede it and build on the concession.** The engine-free EXACT tier does
NOT ride on redundancy — it rides on STATIC DEPENDENCY STRUCTURE. The durable
thesis is the *intersection*: **engine-free certified structural/relabeling edits
over formats with static references that also cache their outputs.** This
directory proves that intersection is (a) non-empty beyond the grid and (b) has a
mechanical boundary, by running ONE format-parametric certifier core (`core.py`,
no engine ever invoked) over three different domains.

## The shared core
`core.py` reduces any artifact to the exact triple the Lean `Computation`
carries — `fn` (name-free operation), `deps` (edges), `O` (self-oracle) — and
`certify` checks the two hypotheses of the machine-checked `eval_iso_invariant`
(function-preservation + edge-preservation under σ). That single function
certifies all three formats. The shared core IS the generality claim made
concrete.

## Result (census.json)

| Format | Dependency structure | Exact tier | Evidence |
|---|---|---|---|
| Excel `.xlsx` (grid) | static explicit cell refs | **96.6% files, 99.6% cells** | benchmarks/tier_coverage.json |
| SQLite STORED gen-columns (relational, **non-grid**) | static sibling-column refs | **100% files (25/25)** | every rename CERTIFIED engine-free AND falsifiably loop-confirmed against SQLite's own stored values; a poisoned expr REFUSED and confirmed to change values (demo_sqlite.py) |
| Jupyter notebooks (real, from the environment) | **implicit** namespace deps | **0** (structural zero) | 30 real notebooks; no static graph extractable → exact tier honestly unavailable; embedded outputs (self-oracle) present → probabilistic tier |

The boundary lands exactly where Theorem 1's precondition (static references)
predicts, across a grid format, a relational format, and a Turing-complete
notebook. SQLite is the load-bearing evidence: a genuinely different semantic
domain where the exact tier fires and the certificate is *falsifiably confirmed*
against the engine's ground truth without the certifier ever invoking the engine.
Jupyter is the deliberate contrast that makes the frontier a law, not luck.

## Honest scope (locked in from the consult)
- The engine-free guarantee covers value-*preserving* structural/relabeling edits
  — graph-iso can only prove "no computed value changed." This is high-value, not
  low: structural edits (insert/delete row, rename-and-propagate) are exactly
  where the status quo silently corrupts (openpyxl shifts no references → wrong
  values; measured 0/6). Certifying they were done right IS the safety guarantee.
- SQLite `STORED` generated columns are structurally within the static fragment
  (SQLite forbids subqueries/non-determinism there), so they are ~100% exact; the
  dynamic-reference escape (the INDIRECT analog) lives at the dynamic-SQL/view
  layer, out of the stored-column model.
- Many sampled notebooks were un-executed (no embedded outputs); the self-oracle
  is present only for executed cells. The exact-tier-zero result is structural and
  holds regardless.

## Files
- `core.py` — format-parametric certifier (no engine invoked).
- `adapter_sqlite.py`, `adapter_ipynb.py` — the two format adapters.
- `demo_sqlite.py` — the exact-tier demo + falsification loop.
- `census.py` / `census.json` — the measured three-format tier census.
