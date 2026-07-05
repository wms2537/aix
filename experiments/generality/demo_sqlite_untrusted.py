#!/usr/bin/env python3
"""UNTRUSTED-agent threat model on SQLite — the openpyxl-0/6 analog, on a
non-grid format. The verification panel correctly flagged that the earlier demo
renamed via TRUSTED `ALTER TABLE` (SQLite rewrites the expressions correctly, so
certification was true by construction). This exercises the ACTUAL threat: an
agent produces a VALID-but-WRONG edit, and the engine-free certifier must CATCH
it — value-faithfulness is decided from the graph alone, never trusting the
agent's asserted output."""
import os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from core import certify
from adapter_sqlite import build_artifact, rename_sigma

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/gensql2"


def build(path, exprs):
    """exprs: dict col-> generated expr; base cols a,b."""
    if os.path.exists(path):
        os.remove(path)
    defs = ["a INTEGER", "b INTEGER"] + [
        f"{c} INTEGER GENERATED ALWAYS AS ({e}) STORED" for c, e in exprs.items()]
    con = sqlite3.connect(path)
    con.execute(f"CREATE TABLE m ({', '.join(defs)})")
    for row in [(3, 4), (10, 20), (1, 1), (7, 2), (5, 5)]:
        con.execute("INSERT INTO m (a,b) VALUES (?,?)", row)
    con.commit(); con.close()


def vals(path):
    con = sqlite3.connect(path)
    r = con.execute("SELECT rowid,* FROM m ORDER BY rowid").fetchall()
    con.close()
    return r


ORIG_EXPRS = {"c": "a + b", "d": "c * 2", "e": "a * b + c"}

if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    orig = f"{W}/orig.sqlite"; build(orig, ORIG_EXPRS)
    A = build_artifact(orig)
    print("Task: rename column a -> x, propagate through generated columns.\n")

    cases = [
        # (label, agent's produced exprs after 'renaming a->x', sigma)
        ("FAITHFUL agent (correct rename+propagate)",
         {"c": "x + b", "d": "c * 2", "e": "x * b + c"}, rename_sigma("a", "x")),
        ("BOTCHED agent A: over-edits e (x*b+c -> x+b+c) while renaming",
         {"c": "x + b", "d": "c * 2", "e": "x + b + c"}, rename_sigma("a", "x")),
        ("BOTCHED agent B: silently drops a dependency (e loses +c)",
         {"c": "x + b", "d": "c * 2", "e": "x * b"}, rename_sigma("a", "x")),
        ("BOTCHED agent C: wrong operator on c (x+b -> x*b)",
         {"c": "x * b", "d": "c * 2", "e": "x * b + c"}, rename_sigma("a", "x")),
    ]
    # rebuild base->x for the edited files (rename base column a->x too)
    good = bad_caught = 0
    for label, exprs, sig in cases:
        p = f"{W}/{label[:6].replace(' ','_')}.sqlite"
        # edited schema: base columns x,b + the agent's generated exprs
        if os.path.exists(p): os.remove(p)
        defs = ["x INTEGER", "b INTEGER"] + [
            f"{c} INTEGER GENERATED ALWAYS AS ({e}) STORED" for c, e in exprs.items()]
        con = sqlite3.connect(p)
        con.execute(f"CREATE TABLE m ({', '.join(defs)})")
        for row in [(3, 4), (10, 20), (1, 1), (7, 2), (5, 5)]:
            con.execute("INSERT INTO m (x,b) VALUES (?,?)", row)
        con.commit(); con.close()

        v = certify(A, build_artifact(p), sig)
        preserved = (vals(orig) == vals(p))    # engine ground truth
        faithful = label.startswith("FAITHFUL")
        # correctness of the certifier's verdict vs ground truth
        verdict_ok = ((v.status == "CERTIFIED") == preserved)
        print(f"{label}")
        print(f"   certifier: {v.status:10s} | engine ground truth: values preserved = {preserved}"
              f" | certifier {'CORRECT' if verdict_ok else 'WRONG'}")
        if faithful and v.status == "CERTIFIED" and preserved:
            good += 1
        if (not faithful) and v.status == "REFUSED" and not preserved:
            bad_caught += 1
    print()
    total_bad = sum(1 for c in cases if not c[0].startswith("FAITHFUL"))
    print(f"RESULT: faithful edit certified = {good == 1}; "
          f"botched edits caught (engine-free) = {bad_caught}/{total_bad}")
    print("This is the untrusted-agent threat model (openpyxl-0/6 analog) on a "
          "relational format: the certifier decides from the dependency graph, "
          "never trusting the agent's file.")
    sys.exit(0 if (good == 1 and bad_caught == total_bad) else 1)
