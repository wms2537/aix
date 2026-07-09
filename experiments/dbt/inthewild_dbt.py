#!/usr/bin/env python3
"""LOCKED TEST leg 5 (research-log/016): a REAL public production dbt project through
the UNMODIFIED adapter_dbt + router. Graph-premise checking only (no warehouse ->
no O; the self-oracle transport leg is out of scope, as pre-registered).

Measures:
  1. Parse coverage: share of models inside the mini-dbt subset (non-dynamic jinja,
     resolvable refs). A LOW number is an honest scope finding (predicted <30%).
  2. Closed covered subgraph: models whose full transitive deps are covered/sources.
  3. Certify legs on that subgraph: faithful rename (target = covered model with the
     most covered dependents, deterministic) -> expect CERTIFIED; botched rename
     (lexicographically-first dependent's ref left dangling) -> expect REFUSED.

Usage: inthewild_dbt.py <project_dir> <out_json>
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "..", "generality"))
from adapter_dbt import read_models, normalize_model, rename_sigma   # UNMODIFIED
from core import Artifact                                            # UNMODIFIED
from router import certify_edit                                      # UNMODIFIED


def analyze(project_dir):
    models = read_models(project_dir)
    parsed, counts = {}, {"total": len(models), "dynamic": 0, "unresolved_ref": 0, "covered_parse": 0}
    for name, sql in models.items():
        skel, deps, dyn = normalize_model(sql)
        parsed[name] = {"skel": skel, "deps": deps, "dynamic": dyn}
    names = set(models)
    for name, p in parsed.items():
        if p["dynamic"]:
            counts["dynamic"] += 1; p["status"] = "dynamic"; continue
        unresolved = [d for d in p["deps"] if not d.startswith("source:") and d not in names]
        if unresolved:
            counts["unresolved_ref"] += 1; p["status"] = "unresolved"; continue
        counts["covered_parse"] += 1; p["status"] = "covered"
    # closed subgraph: covered models whose transitive model-deps are all covered
    closed = set()
    changed = True
    covered = {n for n, p in parsed.items() if p["status"] == "covered"}
    while changed:
        changed = False
        for n in covered - closed:
            ok = all(d.startswith("source:") or d in closed for d in parsed[n]["deps"])
            if ok:
                closed.add(n); changed = True
    return models, parsed, counts, closed


# Source leaves carry an identical sentinel O on BOTH sides: a rename refactor does
# not touch the warehouse, so "source tables unchanged" is the faithful statement.
# Without it the router's fail-closed leaf-oracle discipline refuses (correctly) any
# leaf whose value it cannot confirm. Model nodes carry no O (no warehouse) — the
# self-oracle transport leg stays out of scope, as pre-registered. DISCLOSED in 016.
_SRC_SENTINEL = (("__source_unchanged__",), ())


def build_artifact(parsed, closed):
    fn, deps, O = {}, {}, {}
    for n in closed:
        fn[n] = parsed[n]["skel"]
        deps[n] = list(parsed[n]["deps"])
        for d in parsed[n]["deps"]:
            if d.startswith("source:"):
                fn[d], deps[d], O[d] = "DATA", [], _SRC_SENTINEL
    return Artifact(fn=fn, deps=deps, O=O)


def rename_edit(parsed, closed, target, new_name, botch_dependent=None):
    """Edited (fn, deps) after renaming `target`; optionally leave one dependent's
    ref dangling (the botch)."""
    fn, deps, O = {}, {}, {}
    for n in closed:
        nn = new_name if n == target else n
        skel = parsed[n]["skel"]
        dl = []
        for d in parsed[n]["deps"]:
            if d == target and n != botch_dependent:
                dl.append(new_name)
            else:
                dl.append(d)             # botched dependent keeps the dangling old ref
            if d.startswith("source:"):
                fn[d], deps[d], O[d] = "DATA", [], _SRC_SENTINEL
        fn[nn], deps[nn] = skel, dl
    return Artifact(fn=fn, deps=deps, O=O)


if __name__ == "__main__":
    project_dir, out_json = sys.argv[1], sys.argv[2]
    models, parsed, counts, closed = analyze(project_dir)
    counts["closed_subgraph"] = len(closed)
    counts["parse_coverage"] = round(counts["covered_parse"] / counts["total"], 4) if counts["total"] else None
    counts["closed_coverage"] = round(len(closed) / counts["total"], 4) if counts["total"] else None

    result = {"benchmark": "LOCKED in-the-wild dbt leg (research-log/016)",
              "project": project_dir, "coverage": counts}

    # dependents within the closed subgraph
    dependents = {n: [m for m in closed if n in parsed[m]["deps"]] for n in closed}
    candidates = sorted(closed, key=lambda n: (-len(dependents[n]), n))
    if candidates and len(dependents[candidates[0]]) > 0:
        target = candidates[0]
        deps_of_target = sorted(dependents[target])
        A = build_artifact(parsed, closed)
        sigma = rename_sigma({target: target + "_renamed"})
        B_ok = rename_edit(parsed, closed, target, target + "_renamed")
        c1 = certify_edit(A, B_ok, sigma, set())
        B_bad = rename_edit(parsed, closed, target, target + "_renamed",
                            botch_dependent=deps_of_target[0])
        c2 = certify_edit(A, B_bad, sigma, set())
        result["certify_legs"] = {
            "rename_target": target, "dependents_in_subgraph": len(deps_of_target),
            "faithful_rename": c1.status, "botched_rename_dangling_ref": c2.status,
            "expected": "CERTIFIED / REFUSED",
        }
    else:
        result["certify_legs"] = {"skipped": "no closed-subgraph model with dependents",
                                  "note": "reported honestly; certify legs need a ref edge inside the covered subgraph"}

    json.dump(result, open(out_json, "w"), indent=2)
    print(json.dumps(result, indent=2))
