#!/usr/bin/env python3
"""COINCIDENCE BOUND, empirical q — value-collision rates of the error models.

Context: the probabilistic tier of certify-or-refuse checks a claimed
value-preserving edit against the SELF-ORACLE (Excel-cached <v> values): the
edit PASSES iff every checked cell's expected value equals its cached value.
If the edit is WRONG — a reference reads the wrong cell — the check misses iff
the wrong cell coincidentally holds a value that leaves the output unchanged.
For formulas injective in the misread input (linear/affine: SUM, +, -, *const),
output equality <=> input equality, so the per-cell miss probability reduces to
q = P(the error model's cell pair holds equal values): the VALUE-COLLISION rate.

This script MEASURES q on the real corpus (231 vendored .xlsx, first sheet each):

  M2v  off-by-one ROW shift: adjacent vertical pairs (r,c)-(r+1,c).
       This is EXACTLY openpyxl's insert_rows failure (agent_ab.json: data
       shifts down, references don't, so every read lands one row above its
       intended target).
  M2h  off-by-one COLUMN shift: adjacent horizontal pairs.
  M1c  uniform mis-target, same column: random pair of distinct cells within
       the column's used span (computed EXACTLY from value multisets, no
       sampling).
  M1a  uniform mis-target, anywhere in the used range: random pair from the
       whole file's in-span cells (exact).

Value semantics (what a formula READ actually sees, per Excel):
  excel  — pairs with >=1 nonempty cell; EMPTY collides with 0 and with ""
           (an empty cell reads as 0 in numeric context, "" in text context).
           This is the headline: it is what the recompute-vs-cache check
           actually compares.
  strict — both cells nonempty, typed equality only. Reported as the
           conservative variant.

Outputs (coincidence_q.json):
  per model x variant: pooled q (pair-weighted), per-file distribution
  (median/mean/p10/p90, fraction of check-blind files with rate ~ 1),
  detection tables 1 - miss(k) for k=1..10 under
    (a) the NAIVE INDEPENDENT bound  miss = q_pooled^k, and
    (b) the honest MIXTURE bound     miss = mean_f(rate_f^k)  [>= (a) by
        Jensen: within-file value repetition positively correlates the
        collision events, so independence is optimistic],
  and the k needed for 99% / 99.9% detection under each — including
  "unachievable" when the mixture has mass at rate_f = 1 (constant/degenerate
  columns make the check blind at ANY k).

Caveats (also in COINCIDENCE_BOUND.md): q is INPUT-level; non-injective
formulas (MAX, IF, COUNT, ROUND, ...) can pass with a differing input, so true
per-cell miss >= q there (coincidence_mc.py measures the output-level gap).
Numeric classing for the exact M1 counts uses exact float equality (cached <v>
values that the checker's 1e-9 tolerance would merge are virtually always
byte-identical here; the effect is negligible and only ever lowers q).
"""
import glob
import json
import math
import os
import re
import statistics
import xml.etree.ElementTree as ET
import zipfile
from collections import Counter

# Corpus-path parameterization (pre-registered as a measurement-harness change,
# research-log/016): argv[1] = corpus dir, argv[2] = output json. Defaults unchanged.
import sys as _sys
_CDIR = _sys.argv[1] if len(_sys.argv) > 1 else "/home/soh/aix/vendor/upstream/xlsx/tests"
CORPUS = sorted(glob.glob(_CDIR + "/**/*.xlsx", recursive=True))
OUT = _sys.argv[2] if len(_sys.argv) > 2 else "/home/soh/aix/benchmarks/coincidence_q.json"

EMPTY = None                      # sentinel for an in-span cell with no value
NUM_TOL = 1e-9                    # matches the checker (forward_correctness.py)
MIN_PAIRS_PER_FILE = 5            # files below this excluded from distributions
KS = list(range(1, 11))


# ---------------------------------------------------------------- parsing ---

def _local(tag):
    return tag.rsplit('}', 1)[-1]


def col_num(s):
    n = 0
    for ch in s:
        n = n * 26 + (ord(ch.upper()) - 64)
    return n


def num_col(n):
    s = ""
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


def _shared_strings(z):
    try:
        data = z.read("xl/sharedStrings.xml")
    except KeyError:
        return []
    root = ET.fromstring(data)
    out = []
    for si in root:
        if _local(si.tag) != "si":
            continue
        parts = []
        for el in si.iter():
            tag = _local(el.tag)
            if tag == "rPh":            # skip phonetic runs (crude: their <t>
                continue                # children still iterate; rare, accept)
            if tag == "t" and el.text is not None:
                parts.append(el.text)
        out.append("".join(parts))
    return out


def _norm(t_attr, vtext, inline_text):
    """Normalize a cell to a typed value tuple (or EMPTY)."""
    if t_attr == "inlineStr":
        return ("s", inline_text or "")
    if vtext is None:
        return EMPTY
    if t_attr == "b":
        return ("b", vtext.strip() == "1")
    if t_attr == "e":
        return ("e", vtext.strip())
    if t_attr in ("str",):
        return ("s", vtext)
    if t_attr in (None, "n", "d"):
        try:
            return ("n", float(vtext))
        except ValueError:
            return ("s", vtext)         # e.g. ISO dates under t="d"
    return ("s", vtext)                 # t="s" resolved by caller


# A1-style reference token in a formula: optional $, letters, optional $, row.
# Guards: not preceded by ref-ish chars (letters/digits/$/!/:), not followed by
# "(" (function names like LOG10) or by more letters/digits.
_REF = re.compile(r"(?<![A-Za-z0-9_$!.])(\$?)([A-Za-z]{1,3})(\$?)([0-9]{1,7})"
                  r"(?![0-9A-Za-z_(])")


def _strip_strings(f):
    return re.sub(r'"(?:[^"]|"")*"', '""', f)


def _translate_formula(text, dr, dc):
    """Shift RELATIVE refs of a shared-formula master by (dr, dc)."""
    def sub(m):
        cabs, cs, rabs, rs = m.group(1), m.group(2), m.group(3), m.group(4)
        c = col_num(cs) + (0 if cabs else dc)
        r = int(rs) + (0 if rabs else dr)
        if c < 1 or r < 1:
            return "#REF!"
        return f"{cabs}{num_col(c)}{rabs}{r}"
    # translate only outside string literals: split on quoted spans
    out, i = [], 0
    for m in re.finditer(r'"(?:[^"]|"")*"', text):
        out.append(_REF.sub(sub, text[i:m.start()]))
        out.append(m.group(0))
        i = m.end()
    out.append(_REF.sub(sub, text[i:]))
    return "".join(out)


def parse_sheet(path):
    """First worksheet -> {'grid': {(r,c): typedval}, 'formulas': [...]}.

    grid holds NONEMPTY cells only. formulas: list of dicts
    {row, col, text (may be None), value (typed cached value or EMPTY)}.
    Shared formulas are expanded (master text translated to each child).
    """
    z = zipfile.ZipFile(path)
    names = sorted(n for n in z.namelist()
                   if re.match(r"xl/worksheets/sheet\d+\.xml$", n))
    if not names:
        names = sorted(n for n in z.namelist()
                       if re.match(r"xl/worksheets/[^/]+\.xml$", n))
    if not names:
        return None
    sst = _shared_strings(z)
    root = ET.fromstring(z.read(names[0]))
    sheetdata = None
    for el in root:
        if _local(el.tag) == "sheetData":
            sheetdata = el
            break
    grid, formulas = {}, []
    shared_masters = {}     # si -> (master_row, master_col, text)
    pending_shared = []     # children seen before/with masters
    row_cursor = 0
    if sheetdata is None:
        return {"grid": grid, "formulas": formulas}
    for rowel in sheetdata:
        if _local(rowel.tag) != "row":
            continue
        rattr = rowel.get("r")
        row = int(rattr) if rattr else row_cursor + 1
        row_cursor = row
        col_cursor = 0
        for cel in rowel:
            if _local(cel.tag) != "c":
                continue
            ref = cel.get("r")
            if ref:
                m = re.match(r"([A-Za-z]+)(\d+)", ref)
                col = col_num(m.group(1))
                row = int(m.group(2))
            else:
                col = col_cursor + 1
            col_cursor = col
            t = cel.get("t")
            vtext = inline = ftext = None
            fel = None
            for ch in cel:
                lt = _local(ch.tag)
                if lt == "v":
                    vtext = ch.text if ch.text is not None else ""
                elif lt == "is":
                    inline = "".join(e.text or "" for e in ch.iter()
                                     if _local(e.tag) == "t")
                elif lt == "f":
                    fel = ch
                    ftext = ch.text
            if t == "s" and vtext is not None:
                try:
                    val = ("s", sst[int(vtext)])
                except (ValueError, IndexError):
                    val = ("s", vtext)
            else:
                val = _norm(t, vtext, inline)
            if val is not EMPTY:
                grid[(row, col)] = val
            if fel is not None:
                si = fel.get("si")
                if fel.get("t") == "shared" and si is not None:
                    if ftext:
                        shared_masters[si] = (row, col, ftext)
                        formulas.append({"row": row, "col": col,
                                         "text": ftext, "value": val})
                    else:
                        pending_shared.append((row, col, si, val))
                else:
                    formulas.append({"row": row, "col": col,
                                     "text": ftext, "value": val})
    for (r, c, si, val) in pending_shared:
        mast = shared_masters.get(si)
        text = None
        if mast:
            mr, mc, mtext = mast
            text = _translate_formula(mtext, r - mr, c - mc)
        formulas.append({"row": r, "col": c, "text": text, "value": val})
    return {"grid": grid, "formulas": formulas}


# ------------------------------------------------------------- collision ---

def collide(a, b):
    """Do two READ values leave a check comparison equal? Excel semantics:
    an empty cell reads as 0 in numeric context and "" in text context."""
    if a is EMPTY and b is EMPTY:
        return True
    if a is EMPTY:
        a, b = b, a
    if b is EMPTY:
        return (a[0] == "n" and a[1] == 0.0) or (a[0] == "s" and a[1] == "")
    if a[0] != b[0]:
        return False
    if a[0] == "n":
        x, y = a[1], b[1]
        return abs(x - y) <= NUM_TOL * max(abs(x), abs(y), 1.0)
    return a[1] == b[1]


def _c2(n):
    return n * (n - 1) // 2


def column_spans(grid):
    """{col: (min_row, max_row)} over nonempty cells."""
    spans = {}
    for (r, c) in grid:
        lo, hi = spans.get(c, (r, r))
        spans[c] = (min(lo, r), max(hi, r))
    return spans


def row_spans(grid):
    spans = {}
    for (r, c) in grid:
        lo, hi = spans.get(r, (c, c))
        spans[r] = (min(lo, c), max(hi, c))
    return spans


def adjacent_rates(grid, vertical=True):
    """(excel_pairs, excel_coll, strict_pairs, strict_coll) for off-by-one
    pairs within each column (vertical) / row (horizontal) span."""
    ep = ec = sp = sc = 0
    spans = column_spans(grid) if vertical else row_spans(grid)
    for key, (lo, hi) in spans.items():
        for i in range(lo, hi):
            a = grid.get((i, key) if vertical else (key, i))
            b = grid.get((i + 1, key) if vertical else (key, i + 1))
            if a is EMPTY and b is EMPTY:
                continue
            ep += 1
            hit = collide(a, b)
            ec += hit
            if a is not EMPTY and b is not EMPTY:
                sp += 1
                sc += hit
    return ep, ec, sp, sc


def _multiset_pairs(values, span_len):
    """Exact uniform-pair collision counts for one cell population.
    values: list of typed values (nonempty); span_len: population size
    including EMPTY in-span cells. Returns (excel_pairs, excel_coll,
    strict_pairs, strict_coll)."""
    cnt = Counter(values)
    n_nonempty = len(values)
    n_empty = span_len - n_nonempty
    strict_pairs = _c2(n_nonempty)
    strict_coll = sum(_c2(k) for k in cnt.values())
    excel_pairs = _c2(span_len) - _c2(n_empty)   # >=1 nonempty
    excel_coll = (strict_coll
                  + n_empty * cnt.get(("n", 0.0), 0)
                  + n_empty * cnt.get(("s", ""), 0))
    return excel_pairs, excel_coll, strict_pairs, strict_coll


def uniform_rates(grid, same_column=True):
    """Exact collision counts for uniform random pairs: within each column's
    span (same_column) or across the whole file's in-span cells."""
    spans = column_spans(grid)
    if same_column:
        tot = [0, 0, 0, 0]
        for c, (lo, hi) in spans.items():
            vals = [grid[(r, c)] for r in range(lo, hi + 1) if (r, c) in grid]
            for i, x in enumerate(_multiset_pairs(vals, hi - lo + 1)):
                tot[i] += x
        return tuple(tot)
    vals, span_len = [], 0
    for c, (lo, hi) in spans.items():
        span_len += hi - lo + 1
        vals.extend(grid[(r, c)] for r in range(lo, hi + 1) if (r, c) in grid)
    return _multiset_pairs(vals, span_len)


# ------------------------------------------------------------ aggregation ---

def detection_tables(file_rates, pooled_q):
    """Naive vs mixture miss/detection for k=1..10 + k for 99/99.9%."""
    def naive_k_for(target_miss):
        if pooled_q <= 0:
            return 1
        if pooled_q >= 1:
            return None
        return math.ceil(math.log(target_miss) / math.log(pooled_q))

    def mixture_miss(k):
        return sum(r ** k for r in file_rates) / len(file_rates)

    def mixture_k_for(target_miss):
        asymptote = sum(1 for r in file_rates if r >= 1.0) / len(file_rates)
        if asymptote > target_miss:
            return None
        hi = 1
        while hi <= 10**6 and mixture_miss(hi) > target_miss:
            hi *= 2
        if hi > 10**6:
            return None
        lo = hi // 2  # mixture_miss is monotone decreasing in k
        while lo + 1 < hi:
            mid = (lo + hi) // 2
            if mixture_miss(mid) <= target_miss:
                hi = mid
            else:
                lo = mid
        return hi

    return {
        "naive_independent": {
            "miss_k": {k: pooled_q ** k for k in KS},
            "detection_k": {k: 1 - pooled_q ** k for k in KS},
            "k_for_99pct": naive_k_for(0.01),
            "k_for_99.9pct": naive_k_for(0.001),
        },
        "mixture_dependent": {
            "miss_k": {k: mixture_miss(k) for k in KS},
            "detection_k": {k: 1 - mixture_miss(k) for k in KS},
            "k_for_99pct": mixture_k_for(0.01),
            "k_for_99.9pct": mixture_k_for(0.001),
            "asymptotic_miss_floor(frac files rate==1)":
                sum(1 for r in file_rates if r >= 1.0) / len(file_rates),
        },
    }


def summarize(per_file, model, variant):
    pk, ck = f"{variant}_pairs", f"{variant}_coll"
    rows = [f for f in per_file if f[model][pk] >= MIN_PAIRS_PER_FILE]
    rates = [f[model][ck] / f[model][pk] for f in rows]
    pooled_pairs = sum(f[model][pk] for f in per_file)
    pooled_coll = sum(f[model][ck] for f in per_file)
    pooled_q = pooled_coll / pooled_pairs if pooled_pairs else None
    if not rates or pooled_q is None:
        return {"files": len(rows), "pooled_q": pooled_q}
    return {
        "files_in_distribution": len(rows),
        "pooled_pairs": pooled_pairs,
        "pooled_q": round(pooled_q, 6),
        "file_rate_mean": round(statistics.mean(rates), 6),
        "file_rate_median": round(statistics.median(rates), 6),
        "file_rate_p10": round(sorted(rates)[int(0.10 * (len(rates) - 1))], 6),
        "file_rate_p90": round(sorted(rates)[int(0.90 * (len(rates) - 1))], 6),
        "files_check_blind_rate>=0.99": sum(1 for r in rates if r >= 0.99),
        "files_rate==0": sum(1 for r in rates if r == 0.0),
        "detection": detection_tables(rates, pooled_q),
    }


def main():
    per_file, skipped = [], []
    for path in CORPUS:
        rel = os.path.relpath(path, "/home/soh/aix/vendor/upstream/xlsx/tests")
        try:
            sheet = parse_sheet(path)
        except Exception as e:
            skipped.append({"file": rel, "error": repr(e)[:120]})
            continue
        if not sheet or not sheet["grid"]:
            skipped.append({"file": rel, "error": "empty_first_sheet"})
            continue
        grid = sheet["grid"]
        rec = {"file": rel, "nonempty_cells": len(grid)}
        for model, args in (("M2v", ("adj", True)), ("M2h", ("adj", False)),
                            ("M1c", ("uni", True)), ("M1a", ("uni", False))):
            kind, flag = args
            ep, ec, sp, sc = (adjacent_rates(grid, flag) if kind == "adj"
                              else uniform_rates(grid, flag))
            rec[model] = {"excel_pairs": ep, "excel_coll": ec,
                          "strict_pairs": sp, "strict_coll": sc}
        per_file.append(rec)

    models = {
        "M2v_offbyone_row": "M2v", "M2h_offbyone_col": "M2h",
        "M1c_uniform_same_column": "M1c", "M1a_uniform_used_range": "M1a",
    }
    results = {}
    for label, key in models.items():
        results[label] = {
            "excel_semantics": summarize(per_file, key, "excel"),
            "strict_both_nonempty": summarize(per_file, key, "strict"),
        }

    # illustrative extremes for the writeup (headline model/variant)
    ranked = sorted(
        ((f["M2v"]["excel_coll"] / f["M2v"]["excel_pairs"], f["file"],
          f["M2v"]["excel_pairs"]) for f in per_file
         if f["M2v"]["excel_pairs"] >= MIN_PAIRS_PER_FILE), reverse=True)
    out = {
        "experiment": ("COINCIDENCE q: value-collision rate of error-model "
                       "cell pairs on the real corpus (first sheets), from "
                       "Excel-cached <v> values"),
        "corpus_files": len(CORPUS),
        "files_measured": len(per_file),
        "files_skipped": len(skipped),
        "semantics": {
            "excel": ("pairs with >=1 nonempty cell; EMPTY collides with 0 "
                      "and \"\" (what a formula read actually sees) — headline"),
            "strict": "both cells nonempty, typed equality only",
        },
        "models": results,
        "reading_the_tables": (
            "miss_k = P(a WRONG edit passes a k-cell value check). "
            "naive_independent assumes k distinct, independent collision "
            "events (optimistic). mixture_dependent = mean_f(rate_f^k): "
            "conditionally-iid within file — still optimistic within "
            "file, but captures the dominant between-file dependence; "
            ">= naive by Jensen. k_for_*: smallest k reaching that "
            "detection; null = unachievable (files with rate 1 are "
            "check-blind at any k)."),
        "top10_most_collision_prone_files_M2v_excel":
            [{"rate": round(r, 4), "file": f, "pairs": p}
             for r, f, p in ranked[:10]],
        "bottom5_least_collision_prone_M2v_excel":
            [{"rate": round(r, 4), "file": f, "pairs": p}
             for r, f, p in ranked[-5:]],
        "caveats": [
            "q is INPUT-level; non-injective formulas (MAX/IF/COUNT/...) can "
            "pass with a differing input, so true miss >= these numbers "
            "there (output-level gap measured in coincidence_mc.py).",
            "M1 uniform-pair rates weight pairs uniformly; real references "
            "target data cells non-uniformly (coincidence_mc.py measures the "
            "target-weighted version on real formulas).",
            "Exact M1 counting classes numbers by exact float equality; the "
            "checker's 1e-9 tolerance could only merge near-equal values and "
            "RAISE q slightly.",
            "Corpus is calc-test workbooks (dense formula tables); collision "
            "structure of business workbooks may differ in either direction.",
        ],
        "skipped": skipped[:10],
    }
    with open(OUT, "w") as f:
        json.dump(out, f, indent=1)
    print(json.dumps({k: out[k] for k in
                      ("files_measured", "files_skipped")}, indent=1))
    for label in models:
        s = results[label]["excel_semantics"]
        print(label, "pooled q =", s.get("pooled_q"),
              "| file median =", s.get("file_rate_median"),
              "| check-blind files =", s.get("files_check_blind_rate>=0.99"))


if __name__ == "__main__":
    main()
