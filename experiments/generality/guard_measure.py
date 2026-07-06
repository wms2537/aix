#!/usr/bin/env python3
"""Measured saves of the certify-or-refuse ROUTER deployed as a live agent guard.

A batch of agent-proposed edits with KNOWN ground truth (faithful vs botched,
structural vs mixed) is passed through the router acting as a commit gate. We
measure what a deployment would: are botches caught (recall), are faithful edits
passed without false refusals (precision), and how much does the router collapse
the human audit surface on the faithful commits?"""
import json, os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from adapter_sqlite import build_artifact, rename_sigma
from router import certify_edit

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/genguard"
BASE = {"c": "a + b", "d": "c * 2", "e": "a * b + c"}
ROWS = [(3, 4), (10, 20), (1, 1), (7, 2), (5, 5)]


def build(path, exprs, base_cols=("a", "b"), edits=None):
    if os.path.exists(path):
        os.remove(path)
    defs = [f"{c} INTEGER" for c in base_cols] + [
        f"{c} INTEGER GENERATED ALWAYS AS ({e}) STORED" for c, e in exprs.items()]
    con = sqlite3.connect(path)
    con.execute(f"CREATE TABLE m ({', '.join(defs)})")
    for row in ROWS:
        con.execute(f"INSERT INTO m ({','.join(base_cols)}) VALUES ({','.join('?'*len(base_cols))})", row)
    if edits:
        for e in edits:
            con.execute(e)
    con.commit(); con.close()


def make_edit(kind, orig):
    """Produce (edited_path, sigma, declared_fills, faithful?) for one agent edit."""
    p = f"{W}/{kind}.sqlite"
    sig = rename_sigma("a", "x")
    if kind == "faithful_rename":
        shutil.copy(orig, p)
        con = sqlite3.connect(p); con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.commit(); con.close()
        return p, sig, set(), True
    if kind == "faithful_mixed":
        shutil.copy(orig, p)
        con = sqlite3.connect(p)
        con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.execute("UPDATE m SET b=99 WHERE rowid=1")
        con.commit(); con.close()
        return p, sig, {("b", 1)}, True
    if kind == "faithful_reorder":   # add an independent column (structural), no value change
        build(p, {**BASE, "f": "b + b"})
        con = sqlite3.connect(p); con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.commit(); con.close()
        # sigma unchanged (a->x); the new column f is present in BOTH orig-model? No: orig has no f.
        # so this is an injected column -> should be declared. Model f as a declared structural add:
        return p, sig, {(("f"), r) for r in range(1, 6)}, True
    if kind == "botch_wrong_op":     # rename but c's op silently changed
        build(p, {"c": "a * b", "d": "c * 2", "e": "a * b + c"})
        con = sqlite3.connect(p); con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.commit(); con.close()
        return p, sig, set(), False
    if kind == "botch_dropped_dep":  # e loses its +c dependency
        build(p, {"c": "a + b", "d": "c * 2", "e": "a * b"})
        con = sqlite3.connect(p); con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.commit(); con.close()
        return p, sig, set(), False
    if kind == "botch_undeclared_fill":  # rename + an UNDECLARED value change to b row2
        shutil.copy(orig, p)
        con = sqlite3.connect(p)
        con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"'); con.execute("UPDATE m SET b=77 WHERE rowid=2")
        con.commit(); con.close()
        return p, sig, {("b", 1)}, False    # declares row1 fill but silently changed row2
    raise ValueError(kind)


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    orig = f"{W}/orig.sqlite"; build(orig, BASE)
    A = build_artifact(orig)

    KINDS = ["faithful_rename", "faithful_mixed", "faithful_reorder",
             "botch_wrong_op", "botch_dropped_dep", "botch_undeclared_fill"]
    tp = tn = fp = fn = 0
    collapses = []
    rows = []
    for k in KINDS:
        p, sig, declared, faithful = make_edit(k, orig)
        cert = certify_edit(A, build_artifact(p), sig, declared)
        certified = (cert.status == "CERTIFIED")
        rows.append({"edit": k, "faithful": faithful, "verdict": cert.status,
                     "collapse_pct": int(cert.collapse_ratio*100) if certified else None})
        if faithful and certified: tp += 1; collapses.append(cert.collapse_ratio)
        elif (not faithful) and (not certified): tn += 1
        elif faithful and (not certified): fp += 1     # false refusal
        else: fn += 1                                   # SILENT CORRUPTION (missed botch)
    n_faithful = sum(1 for k in KINDS if "faithful" in k)
    n_botch = len(KINDS) - n_faithful
    summary = {
        "guard": "certify-or-refuse router as a live agent commit gate (SQLite, engine-free)",
        "edits": len(KINDS),
        "faithful_edits": n_faithful, "botched_edits": n_botch,
        "botches_caught": tn, "botches_missed_SILENT_CORRUPTION": fn,
        "faithful_certified": tp, "faithful_false_refused": fp,
        "recall_botches": round(tn / n_botch, 3) if n_botch else None,
        "false_refusal_rate": round(fp / n_faithful, 3) if n_faithful else None,
        "avg_audit_surface_collapse_pct": round(100*sum(collapses)/len(collapses), 1) if collapses else None,
        "per_edit": rows,
    }
    with open("/home/soh/aix/experiments/generality/guard_measure.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps(summary, indent=2))
    ok = (fn == 0 and fp == 0)
    print("\nRESULT:", "0 silent corruptions, 0 false refusals — the guard is sound on this set"
          if ok else "check fp/fn")
    sys.exit(0 if ok else 1)
