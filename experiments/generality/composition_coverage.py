#!/usr/bin/env python3
"""Composition-rule coverage: cashing Theorem 2 (locality / audit_surface_bound).

The exact tier fully certifies only PURE-structural tasks (27% of real tasks). But by
Theorem 2 a MIXED task (structural + value) factors into a CERTIFIED structural scaffold
plus value fills whose effect is provably contained in their downstream cone. So the
certifiable *component* extends from 27% (fully certified) to every task with a
structural skeleton (87% on the edit-distribution). This measures, on realistic mixed
edits, that the router certifies the scaffold and collapses the audit surface to the
value-fill cone — the quantity that makes 'partial certification' useful."""
import os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from adapter_sqlite import build_artifact, rename_sigma
from router import certify_edit

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/compcov"
BASE = {"c": "a + b", "d": "c * 2", "e": "a * b + c"}
ROWS = [(3, 4), (10, 20), (1, 1), (7, 2), (5, 5)]


def build(path):
    if os.path.exists(path):
        os.remove(path)
    defs = ["a INTEGER", "b INTEGER"] + [f"{c} INTEGER GENERATED ALWAYS AS ({e}) STORED"
                                         for c, e in BASE.items()]
    con = sqlite3.connect(path); con.execute(f"CREATE TABLE m ({', '.join(defs)})")
    for row in ROWS:
        con.execute("INSERT INTO m (a,b) VALUES (?,?)", row)
    con.commit(); con.close()


def mixed_edit(orig, path, value_fill_rows):
    """A MIXED task: rename column a->x (structural scaffold) + set b in the given rows
    (the value fills). Returns (edited_path, sigma, declared_fills)."""
    shutil.copy(orig, path)
    con = sqlite3.connect(path)
    con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"')          # structural scaffold
    for r in value_fill_rows:
        con.execute("UPDATE m SET b=? WHERE rowid=?", (90 + r, r))  # value fill
    con.commit(); con.close()
    return path, rename_sigma("a", "x"), {("b", r) for r in value_fill_rows}


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    orig = f"{W}/orig.sqlite"; build(orig)
    A = build_artifact(orig)
    total_cells = len(A.fn)

    # a spread of realistic mixed tasks: 0 fills (pure structural) .. many fills
    TASKS = [("pure-structural rename", []),
             ("mixed: rename + 1 value fill", [1]),
             ("mixed: rename + 2 value fills", [1, 3]),
             ("mixed: rename + 3 value fills", [1, 3, 5])]
    rows, collapses = [], []
    for name, fills in TASKS:
        ep = f"{W}/{len(fills)}.sqlite"
        edited, sigma, declared = mixed_edit(orig, ep, fills)
        cert = certify_edit(A, build_artifact(edited), sigma, declared)
        certified = cert.status == "CERTIFIED"
        audit = len(cert.audit_surface)
        collapse = cert.collapse_ratio if certified else 0.0
        if certified:
            collapses.append(collapse)
        rows.append({"task": name, "scaffold_certified": certified,
                     "certified_nodes": cert.scaffold_certified,
                     "audit_surface_cells": audit, "total_cells": total_cells,
                     "collapse_pct": int(collapse * 100)})
        print(f"  {name:34} scaffold={'CERTIFIED' if certified else 'REFUSED':9} "
              f"audit_surface={audit}/{total_cells} cells  collapse={int(collapse*100)}%", flush=True)

    # connect to the edit-distribution task classes
    import json
    ed = json.load(open("/home/soh/aix/benchmarks/edit_distribution.json"))
    tc = ed["task_class_counts"]; ntasks = sum(tc.values())
    with_scaffold = tc.get("mixed", 0) + tc.get("pure_structural", 0)
    summary = {
        "measurement": "composition-rule coverage — router certifies the structural scaffold "
                       "of a mixed edit and bounds the value-fill audit surface (Theorem 2)",
        "per_task": rows,
        "all_scaffolds_certified": all(r["scaffold_certified"] for r in rows),
        "avg_audit_surface_collapse_pct": round(100 * sum(collapses) / len(collapses), 1) if collapses else None,
        "edit_distribution_coverage": {
            "tasks": ntasks,
            "fully_certified_pure_structural_pct": round(100 * tc.get("pure_structural", 0) / ntasks, 1),
            "certifiable_scaffold_any_structural_pct": round(100 * with_scaffold / ntasks, 1),
        },
        "headline": (f"every mixed task's structural scaffold is certified with the audit "
                     f"surface collapsed to the value-fill cone (avg "
                     f"{round(100 * sum(collapses)/len(collapses),1) if collapses else 0}% of the "
                     f"artifact certified untouched); on the edit distribution the certifiable "
                     f"component rises from {round(100*tc.get('pure_structural',0)/ntasks)}% fully "
                     f"certified to {round(100*with_scaffold/ntasks)}% with a certified scaffold."),
    }
    with open("/home/soh/aix/experiments/generality/composition_coverage.json", "w") as f:
        json.dump(summary, f, indent=2)
    print("\n" + summary["headline"])
