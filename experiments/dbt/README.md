# dbt adapter — the format-parametric certifier on a second practitioner domain

Certifies **dbt model refactors ENGINE-FREE** with the *unchanged* core from
`experiments/generality/` (`core.py` + `router.py` — neither file modified).
This is the generality claim exercised where practitioners actually get hurt:
an agent "renames a staging model" across a dbt project and either did it
faithfully, or left a dangling `ref()`, or silently changed logic on the way.

## The mapping

| Artifact triple | dbt meaning |
|---|---|
| Node | model name (`models/<name>.sql`); sources are leaf nodes `source:<schema>.<table>` |
| `fn(node)` | the model's SQL with every `{{ ref('X') }}` / `{{ source('a','b') }}` replaced by an ordered slot `#i` (same target, same slot), case-folded + whitespace-collapsed **outside** single-quoted string literals. Sources: `"DATA"`. |
| `deps(node)` | ordered, deduplicated list of referenced nodes (first-occurrence order). The `ref()` graph is dbt's whole point — static, syntactic, extractable without execution. |
| `O(node)` | the **materialized table contents** — `(column names, canonically sorted rows)`. This is the self-oracle. It is produced **once**, from the ORIGINAL project, and carried; the certify step never recomputes it. Source leaves get O straight from the declared seed data. |

`router.certify_edit(orig, edited, sigma, declared_fills)` then works verbatim:
nodes are hashable strings instead of `(col, row)` tuples, and the core is
generic over `Hashable` — nothing was adapted in the proven core.

## Where the engine runs (and where it must not)

- duckdb executes SQL in exactly two places, **neither part of a certificate**:
  1. once, to materialize the ORIGINAL project's DAG — building the embedded
     self-oracle (the analog of Excel's cached values / SQLite's STORED
     columns, which dbt itself does not persist in-repo);
  2. in the demo's *falsification loops*, to independently confirm that a
     REFUSED edit really changes values and a CERTIFIED-with-fills edit really
     preserves everything outside the audit cone.
- The certify path builds the edited artifact with `materialize_models=False`:
  no SQL is parsed by any engine, no query is run (the demo asserts the edited
  artifacts carry no model `O`; `duckdb` is only imported inside the
  materializer). Certification is the graph-iso check of Theorem 1 plus the
  self-oracle equality on leaves.

## Honest scope — this is a mini-dbt, deliberately

- **Executor subset**: `models/*.sql` + single-arg `{{ ref('x') }}` +
  `{{ source('a','b') }}` only. No dbt-core. No macros, `config()`, jinja
  control flow, `var()`, two-arg `ref('pkg','x')`, tests, snapshots, seeds,
  incremental materializations, or hooks. Any jinja left after ref/source
  substitution marks the node `dynamic` — data-computed dependencies, exact
  tier honestly unavailable (mirrors the Jupyter boundary). We never guess.
- **Model-level granularity, not column-level.** A declared fill's audit
  surface is whole downstream *models*, not the affected columns. Column-level
  lineage would shrink the cone further; we do not claim it.
- **Fail-closed normalization.** `fn` is text modulo ref-slots, case, and
  whitespace runs (string literals kept verbatim so `'ABC'` never conflates
  with `'abc'`). Consequences: a comment-only edit, a `a>1` -> `a > 1`
  reformat, or quoted-identifier case games are REFUSED, not certified —
  refusing a faithful edit is the safe failure mode; certifying an unfaithful
  one is the one we never allow.
- **Sources are trusted declared inputs.** A ref to a source not in the
  declaration (or any leaf without an oracle entry) fails closed in the router.
- **What a certificate means here**: for a CERTIFIED verdict, every model
  outside the declared-fill cone provably materializes to the same table under
  ANY deterministic SQL semantics (Theorem 1) — without running dbt or a
  warehouse. It does *not* validate the intentional fills; it collapses the
  human audit to exactly those.

## The four scenarios (`demo_dbt.py`, results in `dbt_results.json`)

Mini project: 3 sources + 7 models (stg_orders / stg_customers / stg_payments
-> int_orders_enriched -> fct_revenue / dim_customers -> rpt_kpis), 10 nodes.

| # | Edit | Verdict | Confirmed |
|---|---|---|---|
| a | rename `stg_orders` -> `staging_orders`, every `ref()` updated | **CERTIFIED** (100% untouched, empty audit surface) | engine-free by construction |
| b | rename, but `int_orders_enriched` still refs the old name (dangling) | **REFUSED** (`int_orders_enriched` unaccounted) | — |
| c | rename + silent `SUM(paid)` -> `AVG(paid)` in `fct_revenue` | **REFUSED** (`fct_revenue` unaccounted) | independent re-materialization: values really differ |
| d | rename + that logic change **declared** as a fill | **CERTIFIED** scaffold; audit surface = `[fct_revenue, rpt_kpis]` (fill + downstream cone); **collapse ratio 0.8** | re-materialization: outside-cone values preserved, fill value changed |

## Run it

```sh
pip install duckdb   # the only dependency beyond the stdlib
python experiments/dbt/demo_dbt.py
```

Exits 0 iff all four scenarios produce the expected verdicts and both
ground-truth loop checks agree with the certificates. `DBT_WORK=<dir>` overrides
the temp directory the generated projects are written to.
