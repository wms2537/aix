#!/usr/bin/env python3
"""Router demo on a MIXED edit (structural + value fill) — the certified-
decomposition + audit-surface-collapse story, made concrete, plus botch
detection. Same three-column relational fixture; SQLite is only storage."""
import os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from adapter_sqlite import build_artifact, rename_sigma
from router import certify_edit

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/genrouter"


def build(path, exprs, base_rows, base_cols=("a", "b")):
    if os.path.exists(path):
        os.remove(path)
    defs = [f"{c} INTEGER" for c in base_cols] + [
        f"{c} INTEGER GENERATED ALWAYS AS ({e}) STORED" for c, e in exprs.items()]
    con = sqlite3.connect(path)
    con.execute(f"CREATE TABLE m ({', '.join(defs)})")
    for row in base_rows:
        con.execute(f"INSERT INTO m ({','.join(base_cols)}) VALUES ({','.join('?'*len(base_cols))})", row)
    con.commit(); con.close()


EXPRS = {"c": "a + b", "d": "c * 2", "e": "a * b + c"}
ROWS = [(3, 4), (10, 20), (1, 1), (7, 2), (5, 5)]

if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    orig = f"{W}/orig.sqlite"; build(orig, EXPRS, ROWS)
    A = build_artifact(orig)
    print(f"artifact: {len(A.fn)} cells (5 cols x 5 rows)\n")
    print("MIXED EDIT: rename column a->x (structural) AND set b=99 in row 1 (value fill)\n")

    # --- faithful mixed edit: rename a->x, then set b=99 in row 1 ---
    good = f"{W}/good.sqlite"; shutil.copy(orig, good)
    con = sqlite3.connect(good)
    con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"')     # structural (trusted engine)
    con.execute("UPDATE m SET b=99 WHERE rowid=1")            # value fill (recomputes c,d,e row1)
    con.commit(); con.close()
    A_good = build_artifact(good)
    # the agent DECLARES: structural rename a->x + one value fill at (b, row 1)
    sigma = rename_sigma("a", "x")
    declared = {("b", 1)}
    cert = certify_edit(A, A_good, sigma, declared)
    print(f"FAITHFUL mixed edit -> {cert.status}")
    print(f"   {cert.reason}")
    print(f"   audit surface (cells to re-check): {cert.audit_surface}")
    print(f"   collapse: {int(cert.collapse_ratio*100)}% of the artifact certified untouched, "
          f"audit {len(cert.audit_surface)}/{cert.total_nodes} cells\n")

    # --- botched mixed edit: same, but the agent ALSO corrupts e's expression ---
    bad = f"{W}/bad.sqlite"
    build(bad, {"c": "a + b", "d": "c * 2", "e": "a + b + c"}, ROWS)  # e op corrupted
    con = sqlite3.connect(bad)
    con.execute('ALTER TABLE m RENAME COLUMN "a" TO "x"')
    con.execute("UPDATE m SET b=99 WHERE rowid=1")
    con.commit(); con.close()
    A_bad = build_artifact(bad)
    cert2 = certify_edit(A, A_bad, sigma, declared)      # same declaration
    print(f"BOTCHED mixed edit (agent silently corrupts e's formula) -> {cert2.status}")
    print(f"   {cert2.reason}\n")

    ok = (cert.status == "CERTIFIED" and len(cert.audit_surface) == 4
          and cert2.status == "REFUSED")
    print("RESULT:", "router certifies the scaffold, bounds the audit surface to the "
          "value-fill cone, and catches the undeclared botch" if ok else "unexpected")
    sys.exit(0 if ok else 1)
