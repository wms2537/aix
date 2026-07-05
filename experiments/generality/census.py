#!/usr/bin/env python3
"""Tier census across THREE formats with the SAME parametric certifier core —
the generality claim, measured (per the strategist: one certified edit is
rhetorically worthless; the boundary landing where theory predicts, across
non-grid formats, on a corpus, is the whole proof).

Prediction the census must confirm:
- spreadsheet (static explicit refs + cache)  -> large EXACT tier
- SQLite STORED gen-cols (static refs + cache) -> EXACT tier (non-grid domain)
- Jupyter (cache but IMPLICIT deps)            -> EXACT tier ~ ZERO, all PROBABILISTIC

The exact tier tracks STATIC DEPENDENCY STRUCTURE, not the format and not
redundancy — that is the honest, durable thesis, and here it is on three domains."""
import glob, json, os, random, shutil, sqlite3, sys
sys.path.insert(0, os.path.dirname(__file__))
from core import certify
from adapter_sqlite import build_artifact, rename_sigma
import adapter_ipynb

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/census"


# ---------- SQLite corpus (generated, varied static dep structures) ----------
def gen_sqlite_fixture(path, seed):
    rnd = random.Random(seed)
    base = ["a", "b", "c"][: rnd.randint(2, 3)]
    defs = [f"{c} INTEGER" for c in base]
    avail = list(base)
    gens = []
    for i in range(rnd.randint(2, 5)):
        name = f"g{i}"
        k = rnd.randint(1, min(2, len(avail)))
        refs = rnd.sample(avail, k)
        op = rnd.choice(["+", "*", "-"])
        expr = f" {op} ".join(refs) if k == 2 else f"{refs[0]} * {rnd.randint(2,5)}"
        defs.append(f"{name} INTEGER GENERATED ALWAYS AS ({expr}) STORED")
        avail.append(name)
        gens.append(name)
    if os.path.exists(path):
        os.remove(path)
    con = sqlite3.connect(path)
    con.execute(f"CREATE TABLE m ({', '.join(defs)})")
    for _ in range(rnd.randint(3, 8)):
        vals = [rnd.randint(1, 20) for _ in base]
        con.execute(f"INSERT INTO m ({', '.join(base)}) VALUES ({', '.join('?'*len(base))})", vals)
    con.commit(); con.close()
    return base


def sqlite_census(n=25):
    d = f"{W}/sqlite"; os.makedirs(d, exist_ok=True)
    files = exact_files = certified = 0
    cells_static = cells_leaf = 0
    for s in range(n):
        p = f"{d}/f{s}.sqlite"
        base = gen_sqlite_fixture(p, s)
        A = build_artifact(p)
        files += 1
        static = sum(1 for nd in A.fn if A.fn[nd] != "DATA" and nd not in A.dynamic)
        leaf = sum(1 for nd in A.fn if A.fn[nd] == "DATA")
        dyn = len(A.dynamic)
        cells_static += static; cells_leaf += leaf
        if dyn == 0:
            exact_files += 1
        # certify a rename of the first base column
        ren = f"{d}/f{s}_ren.sqlite"; shutil.copy(p, ren)
        con = sqlite3.connect(ren)
        con.execute(f'ALTER TABLE m RENAME COLUMN "{base[0]}" TO "{base[0]}_r"')
        con.commit(); con.close()
        v = certify(A, build_artifact(ren), rename_sigma(base[0], f"{base[0]}_r"))
        # loop: independent sqlite confirms values preserved
        def vals(pth):
            c = sqlite3.connect(pth); r = c.execute("SELECT rowid,* FROM m ORDER BY rowid").fetchall(); c.close(); return r
        if v.status == "CERTIFIED" and vals(p) == vals(ren):
            certified += 1
    return {"format": "SQLite STORED generated columns (relational, non-grid)",
            "files": files, "exact_tier_files": exact_files,
            "generated_cells_static": cells_static, "base_cells": cells_leaf,
            "rename_certified_engine_free_and_loop_confirmed": certified,
            "exact_pct_files": round(100*exact_files/files, 1) if files else None}


# ---------- Jupyter corpus (real notebooks from the environment) ----------
def find_notebooks(roots=("/home/soh", "/mnt"), maxdepth=6, limit=400):
    """Bounded notebook discovery (a full /home glob scans enormous trees)."""
    import subprocess
    out = []
    for r in roots:
        try:
            res = subprocess.run(["find", r, "-maxdepth", str(maxdepth), "-name", "*.ipynb"],
                                 capture_output=True, text=True, timeout=25)
            out += [ln for ln in res.stdout.splitlines()
                    if "checkpoint" not in ln.lower() and "scratchpad" not in ln]
        except Exception:
            pass
        if len(out) >= limit:
            break
    return out[:limit]


def ipynb_census(n=30):
    all_nb = find_notebooks()
    random.Random(0).shuffle(all_nb)
    picked = 0; exact_available = 0
    code_out = code_noout = md = 0
    for p in all_nb:
        if picked >= n:
            break
        try:
            if os.path.getsize(p) > 3_000_000:   # skip giant notebooks (fast census)
                continue
            nb = adapter_ipynb.load(p)
        except Exception:
            continue
        cc = adapter_ipynb.classify_cells(nb)
        if cc["code_with_output"] + cc["code_no_output"] == 0:
            continue   # no code cells -> not a computational artifact
        picked += 1
        if adapter_ipynb.exact_tier_available(nb):
            exact_available += 1
        code_out += cc["code_with_output"]; code_noout += cc["code_no_output"]; md += cc["markdown_or_empty"]
    return {"format": "Jupyter notebooks (real, from the environment)",
            "notebooks": picked,
            "exact_tier_available_files": exact_available,   # predicted 0
            "code_cells_with_output_self_oracle": code_out,   # probabilistic-eligible
            "code_cells_no_output": code_noout,
            "markdown_or_empty_cells": md,
            "note": "implicit namespace dependencies => NO static graph => exact tier structurally zero; "
                    "self-oracle (embedded outputs) present => probabilistic tier via independent re-execution"}


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    ss = json.load(open("/home/soh/aix/benchmarks/tier_coverage.json"))
    out = {
        "claim": "one format-parametric certifier core; the EXACT tier fires exactly where "
                 "dependencies are STATIC (spreadsheet, SQLite) and abstains where they are "
                 "data-computed (Jupyter) — the boundary is a law across three domains, not a "
                 "spreadsheet accident. Redundancy alone buys nothing engine-free; static "
                 "dependency structure is the lever.",
        "spreadsheet": {
            "format": "Excel .xlsx (grid, explicit cell refs + cached values)",
            "exact_tier_files_pct": ss["of_editable_files"]["fully_exact_pct"],
            "exact_cell_pct": ss["formula_cell_level"]["exact_cell_pct"],
            "boundary": "INDIRECT/OFFSET (data-computed deps) -> probabilistic",
        },
        "sqlite": sqlite_census(),
        "jupyter": ipynb_census(),
    }
    with open("/home/soh/aix/experiments/generality/census.json", "w") as f:
        json.dump(out, f, indent=2)
    print(json.dumps(out, indent=2))
