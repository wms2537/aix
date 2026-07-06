#!/usr/bin/env python3
"""Prep for the LIVE-AGENT 3-way slice. Selects real workbooks whose formulas are
fully modeled by the A1 grammar (so xlq/the certifier can rule on them) AND that
have at least one formula referencing a row >= 2 (so inserting a blank row at row 2
is a NON-TRIVIAL reference-shift task the agent can get right or wrong). Emits one
task per workbook: the formula cells with their ORIGINAL positions, formulas, and
Excel-cached values — the agent must return the corrected formulas after the insert.
"""
import glob, json, os, sys, zipfile
sys.path.insert(0, os.path.dirname(__file__))
from foreign_certify import (extract, uncertifiable_formula, first_sheet_part,
                             FTAG, CELLTAG, col_num, parse_refs)

CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))
K = 2


def zip_first_sheet_name(path):
    import re
    try:
        data = zipfile.ZipFile(path).read("xl/workbook.xml").decode("utf-8", "replace")
        m = re.search(r'<sheet\b[^>]*\bname="([^"]*)"', data)
        return m.group(1) if m else None
    except Exception:
        return None


def formula_cells(path):
    """[(a1, row, col, formula, cached_value)] over the first sheet's formula cells."""
    part = first_sheet_part(path)
    if not part:
        return []
    data = zipfile.ZipFile(path).read(part)
    out = []
    for m in CELLTAG.finditer(data):
        col, row, body = m.group(1).decode(), int(m.group(2)), m.group(3)
        fm = FTAG.search(body)
        if not fm:
            continue
        ftext = fm.group(1).decode("utf-8", "replace")
        import re as _re
        vm = _re.search(rb"<v>([^<]*)</v>", body)
        val = vm.group(1).decode() if vm else ""
        out.append((f"{col}{row}", row, col_num(col), ftext, val))
    return out


def refs_below_k(cells, k):
    """True if any formula references a row >= k (task is non-trivial)."""
    for _, _, _, f, _ in cells:
        _, deps = parse_refs(f)
        for d in deps:
            if d[0] == "C" and d[2] >= k:
                return True
            if d[0] == "R" and (d[2] >= k or d[4] >= k):
                return True
    return False


if __name__ == "__main__":
    want = int(sys.argv[1]) if len(sys.argv) > 1 else 14
    tasks = []
    for p in CORPUS:
        if len(tasks) >= want:
            break
        sheet = zip_first_sheet_name(p)
        if not sheet:
            continue
        A = extract(p)
        if A is None:
            continue
        # require EVERY formula fully modeled (so the certifier can rule cleanly)
        part = first_sheet_part(p)
        data = zipfile.ZipFile(p).read(part)
        if any(uncertifiable_formula(m.group(1).decode("utf-8", "replace"))
               for m in FTAG.finditer(data)):
            continue
        lo = int(os.environ.get("CELL_LO", "2"))
        hi = int(os.environ.get("CELL_HI", "40"))
        cells = formula_cells(p)
        if not (lo <= len(cells) <= hi) or not refs_below_k(cells, K):
            continue                       # non-trivial ref-shift task in the size band
        rel = os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests")
        tasks.append({
            "file": rel, "sheet": sheet, "k": K,
            "cells": [{"cell": a1, "row": r, "col": c, "formula": f, "cached_value": v}
                      for a1, r, c, f, v in cells],
        })
    with open("/home/soh/aix/benchmarks/live3way_tasks.json", "w") as f:
        json.dump(tasks, f, indent=2)
    print(f"selected {len(tasks)} workbooks with non-trivial, fully-modeled ref-shift tasks")
    for t in tasks:
        print(f"  {t['file'][:46]:46} {len(t['cells'])} formula cells")
