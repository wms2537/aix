#!/usr/bin/env python3
"""SQLite EXACT-tier generality demo — the crown jewel on a non-grid format.

Builds a relational fixture with STORED generated columns (the self-oracle),
applies agent edits, and certifies them ENGINE-FREE with the SAME core that
certifies spreadsheets. Then CLOSES THE LOOP: independently runs SQLite to
confirm the engine-free certificate matches the engine's ground truth
(falsifiable, per the skeptic's demand)."""
import os, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from core import certify
from adapter_sqlite import build_artifact, rename_sigma

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/gensql"


def build_fixture(path):
    if os.path.exists(path):
        os.remove(path)
    con = sqlite3.connect(path)
    con.executescript("""
      CREATE TABLE m (
        a INTEGER, b INTEGER,
        c INTEGER GENERATED ALWAYS AS (a + b) STORED,
        d INTEGER GENERATED ALWAYS AS (c * 2) STORED,
        e INTEGER GENERATED ALWAYS AS (a * b + c) STORED
      );
      INSERT INTO m (a,b) VALUES (3,4),(10,20),(1,1),(7,2),(5,5);
    """)
    con.commit(); con.close()


def stored_values(path):
    # compare by POSITION (column names may have changed under a rename); the
    # stored computed values must be identical for a faithful relabeling.
    con = sqlite3.connect(path)
    vals = con.execute("SELECT rowid, * FROM m ORDER BY rowid").fetchall()
    con.close()
    return vals


def make_rename(src, dst, old, new):
    shutil.copy(src, dst)
    con = sqlite3.connect(dst)
    con.execute(f'ALTER TABLE m RENAME COLUMN "{old}" TO "{new}"')  # updates gen exprs
    con.commit(); con.close()


def make_poison(src, dst):
    """Rebuild with column c's expression silently rewired a+b -> a*b (a wrong
    'edit' an agent might emit). Same schema names, different computation."""
    if os.path.exists(dst):
        os.remove(dst)
    con = sqlite3.connect(dst)
    con.executescript("""
      CREATE TABLE m (
        a INTEGER, b INTEGER,
        c INTEGER GENERATED ALWAYS AS (a * b) STORED,   -- POISONED: was a + b
        d INTEGER GENERATED ALWAYS AS (c * 2) STORED,
        e INTEGER GENERATED ALWAYS AS (a * b + c) STORED
      );
      INSERT INTO m (a,b) VALUES (3,4),(10,20),(1,1),(7,2),(5,5);
    """)
    con.commit(); con.close()


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    orig = f"{W}/orig.sqlite"
    build_fixture(orig)
    A_orig = build_artifact(orig)
    print(f"fixture: {len(A_orig.fn)} cells, self-oracle read from disk (STORED)\n")

    # --- Edit 1: rename column a -> x (a faithful relabeling) ---
    ren = f"{W}/rename.sqlite"; make_rename(orig, ren, "a", "x")
    A_ren = build_artifact(ren)
    v1 = certify(A_orig, A_ren, rename_sigma("a", "x"))
    loop1 = (stored_values(orig) == stored_values(ren))   # engine ground truth
    print(f"EDIT 1  rename a->x   -> {v1.status}: {v1.reason}")
    print(f"        loop check (independent sqlite): stored values preserved = {loop1}  "
          f"[certificate {'MATCHES' if (v1.status=='CERTIFIED')==loop1 else 'CONTRADICTS'} engine]\n")

    # --- Edit 2: poison column c's expression (a wrong edit) ---
    poi = f"{W}/poison.sqlite"; make_poison(orig, poi)
    A_poi = build_artifact(poi)
    v2 = certify(A_orig, A_poi, lambda n: n)              # same names -> identity sigma
    loop2 = (stored_values(orig) == stored_values(poi))
    print(f"EDIT 2  poison c=a*b  -> {v2.status}: {v2.reason}")
    print(f"        loop check (independent sqlite): stored values preserved = {loop2}  "
          f"[refusal {'CORRECT' if (v2.status=='REFUSED') and not loop2 else 'WRONG'}: values did change]\n")

    ok = (v1.status == "CERTIFIED" and loop1 and v2.status == "REFUSED" and not loop2)
    print("RESULT:", "engine-free certifier agrees with engine ground truth on both edits"
          if ok else "MISMATCH — investigate")
    sys.exit(0 if ok else 1)
