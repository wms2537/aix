#!/usr/bin/env python3
"""SQLite adapter: a .sqlite file with STORED generated columns -> the
format-independent Artifact triple, ENGINE-FREE.

- The dependency graph comes from parsing the CREATE TABLE DDL (generated-column
  expressions reference sibling columns by name — static, syntactic). SQLite is
  used only to READ the stored schema text and the STORED values (persisted
  bytes; STORED columns are NOT recomputed on read) — the certifier never asks
  SQLite to EVALUATE an edited file. That is exactly Excel's role for .xlsx.
- Nodes are cells (column, rowid); a generated cell depends on its sibling cells
  in the same row. This mirrors the spreadsheet model precisely.
"""
import re, sqlite3
from core import Artifact, normalize_expr

GEN = re.compile(
    r'"?(?P<name>[A-Za-z_][A-Za-z_0-9]*)"?\s+[A-Za-z]+\s+'
    r'GENERATED\s+ALWAYS\s+AS\s*\((?P<expr>.*?)\)\s*(?P<kind>STORED|VIRTUAL)',
    re.I | re.S)

# a generated-column expression is DYNAMIC (data-computed deps, exact tier
# unavailable) if it selects a reference by data — SQLite forbids subqueries in
# generated columns, but a CASE that returns different columns by value is the
# in-language INDIRECT analog we flag.
DYNAMIC_HINT = re.compile(r'\bCASE\b.*\bTHEN\b\s*"?[A-Za-z_]', re.I | re.S)


def table_columns(con, table):
    cur = con.execute(f'PRAGMA table_xinfo("{table}")')
    cols = []
    for row in cur.fetchall():
        # (cid, name, type, notnull, dflt, pk, hidden) ; hidden: 2/3 = generated
        cols.append((row[1], row[6]))   # (name, hidden-flag)
    return cols


def build_artifact(db_path, table="m"):
    con = sqlite3.connect(db_path)
    # schema text (storage read) -> generated-column expressions
    ddl = con.execute(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name=?", (table,)
    ).fetchone()[0]
    cols = [c[0] for c in table_columns(con, table)]
    colset = set(cols)
    SQLFUNCS = {"case", "when", "then", "else", "end", "cast", "as", "abs",
                "coalesce", "min", "max", "round", "length"}

    def is_ref(tok):
        return tok in colset and tok.lower() not in SQLFUNCS

    gen = {}          # colname -> (op, deps_cols, dynamic?)
    for m in GEN.finditer(ddl):
        name, expr = m.group("name"), m.group("expr")
        # double-quotes only delimit identifiers in SQL DDL exprs; strip them so
        # a rename that emits "x"+b normalizes identically to the original a+b.
        op, deps = normalize_expr(expr.replace('"', ''), is_ref)
        gen[name] = (op, deps, bool(DYNAMIC_HINT.search(expr)))

    # read all cells (storage read; STORED values are persisted, not recomputed)
    collist = ", ".join('"' + c + '"' for c in cols)
    rows = con.execute(f'SELECT rowid, {collist} FROM "{table}"').fetchall()
    con.close()

    fn, deps, O, dynamic = {}, {}, {}, set()
    for r in rows:
        rid = r[0]
        vals = dict(zip(cols, r[1:]))
        for c in cols:
            node = (c, rid)
            O[node] = vals[c]
            if c in gen:
                op, dcols, dyn = gen[c]
                fn[node] = op
                deps[node] = [(dc, rid) for dc in dcols]
                if dyn:
                    dynamic.add(node)
            else:
                fn[node] = "DATA"      # leaf: a base column cell
                deps[node] = []
    return Artifact(fn=fn, deps=deps, O=O, dynamic=dynamic)


def rename_sigma(old, new):
    """The relabeling for a column rename old->new: (old, r) |-> (new, r)."""
    def sigma(node):
        c, r = node
        return (new if c == old else c, r)
    return sigma
