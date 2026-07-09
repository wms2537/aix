#!/usr/bin/env python3
"""dbt adapter: a dbt-style project directory -> the format-independent
Artifact triple, so the SAME core (experiments/generality) that certifies
spreadsheets and SQLite certifies dbt model refactors ENGINE-FREE.

The mapping (model-level granularity, NOT column-level):
  Node      = model name (models/<name>.sql) or a source leaf "source:<schema>.<table>"
  fn(node)  = the model's SQL with every {{ ref('X') }} / {{ source('a','b') }}
              replaced by an ordered positional slot #i (same target -> same
              slot), then case-folded and whitespace-collapsed OUTSIDE single-
              quoted string literals. A rename that updates every ref() leaves
              fn identical — exactly what fn-preservation must see.
              Source leaves: fn = "DATA".
  deps(node)= the ordered, deduplicated list of referenced nodes (first
              occurrence order). Sources are leaf DATA nodes with deps = [].
  O(node)   = the MATERIALIZED table contents: (column names, rows sorted into
              canonical order). For sources, O comes straight from the declared
              seed data. For models, O is produced ONCE by materializing the
              ORIGINAL project (mini-dbt executor over duckdb, topological
              order) — this is the embedded self-oracle the certifier carries.
              The certify step itself NEVER executes SQL.

HONEST SCOPE — this is a faithful MINI-dbt, not dbt-core:
  * models/*.sql + {{ ref('x') }} + {{ source('a','b') }} only.
  * NO jinja beyond ref/source (no macros, config(), loops, vars), NO tests,
    snapshots, seeds-from-csv, incremental models, or two-arg ref('pkg','x').
    Any leftover jinja after ref/source substitution marks the node `dynamic`
    (data-computed deps) — it is EXCLUDED from the exact tier, never guessed at.
  * Normalization is FAIL-CLOSED: SQL that differs only in comments or in
    token-level formatting the collapse doesn't equate (e.g. `a>1` vs `a > 1`)
    is REFUSED, never silently accepted.
"""
import datetime
import glob
import os
import re
import sys
from decimal import Decimal

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                "..", "generality"))
from core import Artifact

# ---- reference syntax (the modeled jinja subset) ----
COMBINED = re.compile(
    r"\{\{\s*ref\(\s*['\"](?P<ref>\w+)['\"]\s*\)\s*\}\}"
    r"|\{\{\s*source\(\s*['\"](?P<ss>\w+)['\"]\s*,\s*['\"](?P<st>\w+)['\"]\s*\)\s*\}\}"
)
JINJA_LEFTOVER = re.compile(r"\{\{|\{%|\{#")   # anything we did NOT model
_STR_LIT = re.compile(r"'(?:[^']|'')*'")        # single-quoted SQL string literal


def source_node(schema: str, table: str) -> str:
    return f"source:{schema}.{table}"


# ---- fn normalization ----
def _fold(seg: str) -> str:
    """Case-fold + collapse whitespace runs to a single space (non-literal SQL)."""
    return re.sub(r"\s+", " ", seg).lower()


def _normalize_text(s: str) -> str:
    """Fold everything EXCEPT single-quoted string literals (literals are
    case/space significant — folding them could conflate different values)."""
    parts, last = [], 0
    for m in _STR_LIT.finditer(s):
        parts.append(_fold(s[last:m.start()]))
        parts.append(m.group(0))
        last = m.end()
    parts.append(_fold(s[last:]))
    return "".join(parts).strip()


def normalize_model(sql: str):
    """SQL text -> (name-free op, ordered unique dep nodes, dynamic?).
    Same slot semantics as core.normalize_expr: repeated refs to the same
    target reuse the same #i slot."""
    deps = []

    def repl(m):
        node = m.group("ref") or source_node(m.group("ss"), m.group("st"))
        if node not in deps:
            deps.append(node)
        return f" #{deps.index(node)} "

    body = COMBINED.sub(repl, sql.strip().rstrip(";").strip())
    dynamic = bool(JINJA_LEFTOVER.search(body))   # unmodeled jinja -> not exact tier
    return _normalize_text(body), deps, dynamic


# ---- project reading ----
def read_models(project_dir: str) -> dict:
    """models/**/*.sql -> {model_name: raw_sql}. Model name = file stem."""
    models = {}
    pattern = os.path.join(project_dir, "models", "**", "*.sql")
    for path in sorted(glob.glob(pattern, recursive=True)):
        name = os.path.splitext(os.path.basename(path))[0]
        if name in models:
            raise ValueError(f"duplicate model name {name!r} (dbt requires unique)")
        with open(path) as f:
            models[name] = f.read()
    return models


# ---- self-oracle value canonicalization ----
def _norm_val(v):
    if isinstance(v, Decimal):
        return int(v) if v == v.to_integral_value() else float(v)
    if isinstance(v, (datetime.date, datetime.datetime)):
        return v.isoformat()
    return v


def _freeze_rows(cols, rows):
    """(columns, rows) -> canonical, order-independent, name-free value."""
    normed = [tuple(_norm_val(v) for v in r) for r in rows]
    normed.sort(key=lambda r: tuple((v is None, str(v)) for v in r))
    return (tuple(cols), tuple(normed))


# ---- mini-dbt executor (ONLY for building the original self-oracle and for
# ---- independent ground-truth loop checks — never called by the certifier) ----
def _phys(node: str) -> str:
    return re.sub(r"\W+", "_", node)


def _lit(v):
    if v is None:
        return "NULL"
    if isinstance(v, bool):
        return "TRUE" if v else "FALSE"
    if isinstance(v, (int, float)):
        return repr(v)
    return "'" + str(v).replace("'", "''") + "'"


def _topo(models: dict, deps: dict):
    indeg = {m: 0 for m in models}
    dependents = {m: [] for m in models}
    for m in models:
        for d in deps[m]:
            if d in models:
                indeg[m] += 1
                dependents[d].append(m)
    order = sorted(m for m in models if indeg[m] == 0)
    i = 0
    while i < len(order):
        n = order[i]; i += 1
        for u in sorted(dependents[n]):
            indeg[u] -= 1
            if indeg[u] == 0:
                order.append(u)
    if len(order) != len(models):
        stuck = sorted(m for m in models if indeg[m] > 0)
        raise ValueError(f"cycle in ref() graph involving {stuck}")
    return order


def materialize_project(models: dict, deps: dict, source_data: dict) -> dict:
    """Execute the DAG in topological order with duckdb; return {model: O-value}.
    Raises on dangling refs / unmodeled jinja — we never guess."""
    import duckdb
    phys = {}
    for node in list(source_data) + list(models):
        p = _phys(node)
        if p in phys.values():
            raise ValueError(f"physical name collision for {node!r}")
        phys[node] = p

    con = duckdb.connect()  # in-memory
    for snode, (cols, rows) in source_data.items():
        if not rows:
            raise ValueError(f"source {snode!r} has no rows (cannot infer types)")
        vals = ", ".join("(" + ", ".join(_lit(v) for v in row) + ")" for row in rows)
        con.execute(f'CREATE TABLE "{phys[snode]}" AS '
                    f'SELECT * FROM (VALUES {vals}) AS v({", ".join(cols)})')

    def resolve(m):
        node = m.group("ref") or source_node(m.group("ss"), m.group("st"))
        if node not in phys:
            raise KeyError(f"dangling reference {node!r} — target does not exist")
        return f'"{phys[node]}"'

    O = {}
    for name in _topo(models, deps):
        sql = COMBINED.sub(resolve, models[name].strip().rstrip(";").strip())
        if JINJA_LEFTOVER.search(sql):
            raise ValueError(f"model {name!r} has unmodeled jinja; refusing to execute")
        con.execute(f'CREATE TABLE "{phys[name]}" AS {sql}')
        res = con.execute(f'SELECT * FROM "{phys[name]}"')
        cols = [d[0] for d in res.description]
        O[name] = _freeze_rows(cols, res.fetchall())
    con.close()
    return O


# ---- the adapter entry point ----
def build_artifact(project_dir: str, source_data: dict,
                   materialize_models: bool = True) -> Artifact:
    """dbt-style project -> Artifact.

    source_data: {source_node: (columns, rows)} — the declared leaf inputs.
    materialize_models=True  -> run the mini-dbt executor ONCE to embed the
                                self-oracle (use for the ORIGINAL artifact and
                                for independent ground-truth checks).
    materialize_models=False -> ENGINE-FREE: no SQL is executed; model nodes
                                carry no O (use for the EDITED artifact in the
                                certify path)."""
    models = read_models(project_dir)
    fn, deps, O, dynamic = {}, {}, {}, set()

    for snode, (cols, rows) in source_data.items():
        fn[snode] = "DATA"
        deps[snode] = []
        O[snode] = _freeze_rows(cols, rows)

    for name, sql in models.items():
        op, dlist, dyn = normalize_model(sql)
        fn[name] = op
        deps[name] = dlist
        if dyn:
            dynamic.add(name)

    if materialize_models:
        if dynamic:
            raise ValueError(f"cannot materialize: unmodeled jinja in {sorted(dynamic)}")
        O.update(materialize_project(models, deps, source_data))
    return Artifact(fn=fn, deps=deps, O=O, dynamic=dynamic)


def rename_sigma(mapping: dict):
    """σ for a model-rename refactor: node -> mapping.get(node, node)."""
    return lambda n: mapping.get(n, n)
