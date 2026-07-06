#!/usr/bin/env python3
"""Correctness HARNESS for the certify-or-refuse router (NOT an agent evaluation).

HONEST SCOPE (per adversarial review): this is a small, self-authored branch-
correctness harness, not a "measured live agent guard." There is no LLM agent
here — `make_edit()` hand-builds edits that exercise each REFUSE branch, plus a
regression for a confirmed soundness bug. Its value is that it hardened the
router (it found two real defects, below); its n and self-authored ground truth
CANNOT establish real-world efficacy against an untrusted agent. The genuine
interventional evaluation — an LLM agent's OWN edits, with-router vs without,
labeled by an INDEPENDENT engine oracle (not the self-oracle) — is separate
(experiments/generality/agent_ab/) and is the thing that gates acceptance.

Bugs this harness hardened:
  1. a value edit to a LEAF cell has no graph footprint -> was a silent-
     corruption miss -> fixed with a self-oracle value check on non-declared nodes;
  2. that check falsely refused legitimate fill cones -> fixed cone-aware;
  3. the value check FAILED OPEN on a missing oracle entry (a leaf absent from O
     with a changed value certified) -> fixed FAIL-CLOSED (see test_partial_oracle).
"""
import json, os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from core import Artifact
from adapter_sqlite import build_artifact, rename_sigma
from router import certify_edit


def test_partial_oracle():
    """Regression for the confirmed fail-open soundness bug: a LEAF with no
    oracle entry must NOT be certifiable untouched (fail closed)."""
    orig = Artifact(fn={'a': 'DATA', 'b': 'DATA', 'c': '#0+#1'},
                    deps={'a': [], 'b': [], 'c': ['a', 'b']}, O={'a': 1, 'c': 4})  # b absent from O
    edited = Artifact(fn={'a': 'DATA', 'b': 'DATA', 'c': '#0+#1'},
                      deps={'a': [], 'b': [], 'c': ['a', 'b']}, O={'a': 1, 'c': 4})
    cert = certify_edit(orig, edited, lambda n: n, set())
    return cert.status == "REFUSED"    # fail-closed: cannot certify an unverifiable leaf

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
    collapses = []          # NON-tautological only (excludes pure renames, cone=∅ by definition)
    rows = []
    for k in KINDS:
        p, sig, declared, faithful = make_edit(k, orig)
        cert = certify_edit(A, build_artifact(p), sig, declared)
        certified = (cert.status == "CERTIFIED")
        rows.append({"edit": k, "faithful": faithful, "verdict": cert.status,
                     "collapse_pct": int(cert.collapse_ratio*100) if certified else None})
        if faithful and certified:
            tp += 1
            if declared:                              # only edits with a real value fill
                collapses.append(cert.collapse_ratio)
        elif (not faithful) and (not certified): tn += 1
        elif faithful and (not certified): fp += 1     # false refusal
        else: fn += 1                                   # SILENT CORRUPTION (missed botch)
    n_faithful = sum(1 for k in KINDS if "faithful" in k)
    n_botch = len(KINDS) - n_faithful
    exploit_closed = test_partial_oracle()             # fail-closed regression
    summary = {
        "what_this_is": "SELF-AUTHORED correctness harness for the router — NOT an "
                        "agent evaluation. Establishes branch correctness + a soundness "
                        "regression, not real-world efficacy (see module docstring).",
        "cases": len(KINDS),
        "faithful": n_faithful, "botched": n_botch,
        "botches_refused": tn, "botches_missed_SILENT_CORRUPTION": fn,
        "faithful_certified": tp, "faithful_false_refused": fp,
        "partial_oracle_exploit_fail_closed": exploit_closed,
        "audit_collapse_note": "collapse% = 1 - |cone|/total is a property of the "
                               "workbook+edit, NOT the tool; it scales with untouched "
                               "row count and CRATERS to ~0% on shared-upstream edits "
                               "(the financial-model case). Reported only for the "
                               "value-fill cases, pure-rename (tautological 100%) excluded.",
        "collapse_pct_value_fill_cases": [int(c*100) for c in collapses],
        "per_case": rows,
    }
    with open("/home/soh/aix/experiments/generality/guard_measure.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps(summary, indent=2))
    ok = (fn == 0 and fp == 0 and exploit_closed)
    print("\nRESULT:", "harness green: 0 silent corruptions, 0 false refusals, "
          "partial-oracle exploit fails closed — router hardened on these cases "
          "(NOT an efficacy claim)" if ok else "HARNESS RED — check fp/fn/exploit")
    sys.exit(0 if ok else 1)
