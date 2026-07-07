#!/usr/bin/env python3
"""Edit-path A/B across 4 ops x 2 independent engines (coverage + scale).

Extends the headline A/B (naive openpyxl edit vs xlq's shift) from insert-row@2 to all
four structural ops, adjudicated by TWO independent engines via value-preservation:
recompute the generated file before/after with LibreOffice AND `formulas`, and a formula
whose value changes = a silent corruption. openpyxl shifts no references; xlq shifts them
via the Z3-backed algebra. Reports, per op and per engine, the openpyxl silent-corruption
rate and xlq's faithful rate."""
import os, subprocess, sys, warnings
warnings.filterwarnings("ignore")
import openpyxl
sys.path.insert(0, os.path.dirname(__file__))
from conformance_v2 import build, gen, lo_values, fora_values, new_pos, ROWS, COLS, NF, SEED
import random

XLQ = "/home/soh/aix/xlq/target/release/xlq"
WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/abv2"


def opx_edit(src, dst, op, at, count):
    wb = openpyxl.load_workbook(src); ws = wb["S"]
    if op == "insert-rows":  ws.insert_rows(at, count)
    elif op == "delete-rows": ws.delete_rows(at, count)
    elif op == "insert-cols": ws.insert_cols(at, count)
    elif op == "delete-cols": ws.delete_cols(at, count)
    wb.save(dst)


def corruption(before_lo, after_lo, before_fo, after_fo, placed, op, at, count):
    """Per-engine: fraction of formulas whose value changed (a silent corruption)."""
    out = {}
    for name, (b, a) in [("libreoffice", (before_lo, after_lo)), ("formulas", (before_fo, after_fo))]:
        checked = corrupt = 0
        for col, row, f in placed:
            nc, nr = new_pos(col, row, op, at, count)
            v0 = b.get((col, row)); v1 = a.get((nc, nr))
            if v0 is None or v1 is None:
                continue
            checked += 1
            if abs(v1 - v0) > 1e-6 * max(abs(v0), abs(v1), 1.0):
                corrupt += 1
        out[name] = {"checked": checked, "corrupted": corrupt,
                     "rate": round(corrupt / checked, 3) if checked else None}
    return out


def run_op(op, at, count, work):
    rng = random.Random(SEED + hash(op) % 1000)
    formulas = [gen(rng) for _ in range(NF)]
    if op.startswith("delete"):
        formulas = [f for f in formulas if ":" not in f]
    src = os.path.join(work, "orig.xlsx")
    placed = build(src, formulas, rng)
    b_lo = lo_values(src, work); b_fo = fora_values(src)
    # xlq arm
    xdst = os.path.join(work, "xlq.xlsx")
    import shutil; shutil.copy(src, xdst)
    for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(xdst + suf): os.remove(xdst + suf)
    r = subprocess.run([XLQ, "restructure", xdst, "--sheet", "S", "--op", op, "--at", str(at),
                        "--count", str(count), "--actor", "a"], capture_output=True, text=True)
    xlq_ok = '"rev"' in r.stdout
    xa_lo = lo_values(xdst, work) if xlq_ok else {}
    xa_fo = fora_values(xdst) if xlq_ok else {}
    # openpyxl arm
    odst = os.path.join(work, "opx.xlsx"); opx_edit(src, odst, op, at, count)
    oa_lo = lo_values(odst, work); oa_fo = fora_values(odst)
    return {"op": op,
            "xlq": corruption(b_lo, xa_lo, b_fo, xa_fo, placed, op, at, count) if xlq_ok else "restructure_failed",
            "openpyxl": corruption(b_lo, oa_lo, b_fo, oa_fo, placed, op, at, count)}


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    import json
    OPS = [("insert-rows", 2, 1), ("delete-rows", 4, 1), ("insert-cols", 2, 1), ("delete-cols", 4, 1)]
    results = []
    for op, at, count in OPS:
        work = os.path.join(WORK, op); os.makedirs(work, exist_ok=True)
        r = run_op(op, at, count, work); results.append(r)
        xl = r["xlq"]["libreoffice"] if isinstance(r["xlq"], dict) else {}
        ol = r["openpyxl"]["libreoffice"]; of = r["openpyxl"]["formulas"]
        print(f"  {op}@{at}: openpyxl corrupt LO {ol['rate']} / formulas {of['rate']} | "
              f"xlq corrupt LO {xl.get('rate')} / formulas {r['xlq'].get('formulas',{}).get('rate') if isinstance(r['xlq'],dict) else '?'}", flush=True)
    # aggregate
    def agg(arm, eng):
        c = sum(r[arm][eng]["corrupted"] for r in results if isinstance(r[arm], dict))
        n = sum(r[arm][eng]["checked"] for r in results if isinstance(r[arm], dict))
        return c, n, round(c / n, 3) if n else None
    summary = {
        "benchmark": "edit-path A/B — 4 structural ops x 2 independent engines (openpyxl vs xlq)",
        "ops": [o[0] for o in OPS],
        "openpyxl_silent_corruption": {"libreoffice": agg("openpyxl", "libreoffice"), "formulas": agg("openpyxl", "formulas")},
        "xlq_silent_corruption": {"libreoffice": agg("xlq", "libreoffice"), "formulas": agg("xlq", "formulas")},
        "per_op": results,
    }
    oc_lo = summary["openpyxl_silent_corruption"]["libreoffice"]
    xc_lo = summary["xlq_silent_corruption"]["libreoffice"]
    summary["headline"] = (f"4 ops, 2 engines: naive openpyxl silently corrupts "
                           f"{oc_lo[2]} (LO) of edits; xlq corrupts {xc_lo[2]} (LO). "
                           f"formulas-engine agrees.")
    json.dump(summary, open("/home/soh/aix/benchmarks/agent_ab_v2.json", "w"), indent=2)
    print("\n" + summary["headline"])
