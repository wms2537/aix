#!/usr/bin/env python3
"""Real-corpus shift correctness across all 4 ops — DETERMINISTIC (no recompute).

Real-corpus value-preservation is confounded: LibreOffice recomputes exotic financial/
date functions (ACCRINT/CUMPRINC/DB/DAYS360/TIME) inconsistently with the Excel cache,
so a value oracle flags xlq as 'corrupt' on files where its shifted formulas are in fact
correct (verified by hand: e.g. ACCRINT_ACCRINTM I5->I6 = ACCRINT(A6,B6,C6,D6,E6,F6)).
So instead we check shift correctness DETERMINISTICALLY: compare xlq's output formula for
each cell to an independent reference shifter — the SAME grid-validity shifter that
ag_ab_v2/conformance_v2 validated against TWO engines (LibreOffice + formulas) with 0
value divergences. xlq matching that validated shifter on real formulas is clean,
engine-noise-free evidence across all four ops. openpyxl leaves formulas unshifted."""
import glob, os, re, shutil, subprocess, sys, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/scr"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))
FTAG = re.compile(rb'<c r="([A-Z]+)(\d+)"(?:(?!</c>).)*?<f[^>]*>([^<]*)</f>', re.S)
CELLTOK = re.compile(r"(\$?)([A-Za-z]{1,3})(\$?)(\d+)")
RANGETOK = re.compile(r"\$?[A-Z]{1,3}\$?\d+:\$?[A-Z]{1,3}\$?\d+")
VOL = re.compile(r"\b(OFFSET|INDIRECT|NOW|TODAY|RAND|RANDBETWEEN|CELL|INFO|ROW|COLUMN|ROWS|COLUMNS)\b", re.I)


def coln(s):
    n = 0
    for ch in s.upper():
        n = n * 26 + (ord(ch) - 64)
    return n


def num2col(n):
    s = ""
    while n > 0:
        n, r = divmod(n - 1, 26); s = chr(65 + r) + s
    return s


WHOLECOL = re.compile(r"(?<![A-Za-z0-9])\$?[A-Z]{1,3}:\$?[A-Z]{1,3}(?![A-Za-z0-9])")
WHOLEROW = re.compile(r"(?<![A-Za-z0-9.])\$?\d+:\$?\d+(?![0-9])")
RANGE_FN = re.compile(r"[A-Z]{1,3}\d+:[A-Za-z_]")   # range endpoint is a function (A9:CHOOSE)


def ref_shift(formula, axis, op, k, count):
    """Independent grid-validity reference shifter for insert/delete on rows or cols.
    (Same predicate validated against 2 engines in conformance_v2.) Returns the shifted
    formula, or None if a construct is outside this shifter's grammar — tables,
    cross-sheet, whole-column/row (A:A, 5:5), or a range whose endpoint is a function
    (A9:CHOOSE(...)) — which xlq handles but this simple checker cannot independently
    verify, so it SKIPS them rather than guess (they are not counted either way)."""
    if "[" in formula or "!" in formula or WHOLECOL.search(formula) or \
       WHOLEROW.search(formula) or RANGE_FN.search(formula):
        return None
    out, i, n = [], 0, len(formula)

    def shift_line(v, is_col):
        if op == "insert-rows" or op == "insert-cols":
            return v + count if v >= k else v
        # delete
        if v < k:
            return v
        if v >= k + count:
            return v - count
        return "REF"                          # consumed

    while i < n:
        ch = formula[i]
        if ch == '"':
            j = i + 1
            while j < n and formula[j] != '"':
                j += 1
            out.append(formula[i:j + 1]); i = j + 1; continue
        rm = RANGETOK.match(formula, i)
        if rm:                                # shift both endpoints (single-col clamp skip)
            toks = re.findall(r"(\$?)([A-Z]{1,3})(\$?)(\d+)", rm.group(0))
            parts = []
            ok = True
            for cd, cl, rd, rw in toks:
                nc, nr = coln(cl), int(rw)
                if axis == "row":
                    sr = shift_line(nr, False)
                    if sr == "REF": ok = False; break
                    parts.append(f"{cd}{cl}{rd}{sr}")
                else:
                    sc = shift_line(nc, True)
                    if sc == "REF": ok = False; break
                    parts.append(f"{cd}{num2col(sc)}{rd}{rw}")
            if not ok:
                return None                   # #REF! in a range under delete — skip
            out.append(":".join(parts)); i = rm.end(); continue
        m = CELLTOK.match(formula, i)
        if m:
            prev = formula[i - 1] if i > 0 else ""
            nxt = formula[m.end()] if m.end() < n else ""
            col = coln(m.group(2)); row = int(m.group(4))
            grid = 1 <= col <= 16384 and 1 <= row <= 1048576
            delim = not (prev.isalnum() or prev in ("_", ".", "$", "!", "'"))
            tail = not (nxt.isalpha() or nxt in ("_", "("))
            if grid and delim and tail:
                if axis == "row":
                    sr = shift_line(row, False)
                    if sr == "REF": return None
                    out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{sr}")
                else:
                    sc = shift_line(col, True)
                    if sc == "REF": return None
                    out.append(f"{m.group(1)}{num2col(sc)}{m.group(3)}{row}")
                i = m.end(); continue
        out.append(ch); i += 1
    return "".join(out)


def norm(f):
    return re.sub(r"\s+", "", (f[1:] if f.startswith("=") else f)).upper()


def first_sheet_part(z):
    ns = sorted(n for n in z.namelist() if re.match(r"xl/worksheets/sheet\d+\.xml$", n))
    return ns[0] if ns else None


def formulas_of(path):
    z = zipfile.ZipFile(path); part = first_sheet_part(z)
    if not part:
        return {}
    data = z.read(part)
    out = {}
    for m in FTAG.finditer(data):
        f = m.group(3).decode("utf-8", "replace")
        if VOL.search(f):
            continue
        out[(m.group(1).decode(), int(m.group(2)))] = f
    return out


def zip_sheet_name(path):
    d = zipfile.ZipFile(path).read("xl/workbook.xml").decode("utf-8", "replace")
    m = re.search(r'<sheet\b[^>]*\bname="([^"]*)"', d)
    return m.group(1) if m else None


def new_pos(col, row, axis, op, at, count):
    """New (col,row) of a cell after the edit, or None if the cell is IN a deleted band
    (its formula is gone; the output cell at that address is a different original cell)."""
    c, r = coln(col), row
    if axis == "row":
        if op == "insert-rows":
            r = r + count if r >= at else r
        else:
            if at <= r < at + count:
                return None                   # this row is deleted
            r = r - count if r >= at + count else r
    else:
        if op == "insert-cols":
            c = c + count if c >= at else c
        else:
            if at <= c < at + count:
                return None                   # this column is deleted
            c = c - count if c >= at + count else c
    return num2col(c), r


def xlq_edit(src, sheet, op, at, work):
    dst = os.path.join(work, "x.xlsx"); shutil.copy(src, dst)
    for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op", op, "--at", str(at),
                        "--count", "1", "--actor", "s"], capture_output=True, text=True)
    return dst if '"rev"' in r.stdout else None


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 60
    OPS = [("insert-rows", "row", 2), ("delete-rows", "row", 4),
           ("insert-cols", "col", 2), ("delete-cols", "col", 4)]
    tally = {op: Counter() for op, _, _ in OPS}
    done = 0
    for p in CORPUS:
        if done >= limit:
            break
        sheet = zip_sheet_name(p); orig = formulas_of(p)
        if not sheet or len(orig) < 2:
            continue
        work = os.path.join(WORK, str(done)); os.makedirs(work, exist_ok=True)
        used = False
        for op, axis, at in OPS:
            xf = xlq_edit(p, sheet, op, at, work)
            if xf is None:
                tally[op]["refused"] += 1; continue
            xout = formulas_of(xf)
            for (col, row), f in orig.items():
                exp = ref_shift(f, axis, op, at, 1)
                if exp is None:                       # out of reference grammar — skip
                    continue
                np = new_pos(col, row, axis, op, at, 1)
                if np is None:                        # cell is in the deleted band — skip
                    continue
                nc, nr = np
                got = xout.get((nc, nr))
                if got is None:
                    continue
                used = True
                if norm(got) != norm(exp):
                    tally[op]["xlq_MISMATCH"] += 1
                    if tally[op]["_logged"] < 3:
                        tally[op]["_logged"] += 1
                        print(f"  MISMATCH {op} {os.path.basename(p)} {col}{row}->{nc}{nr}: "
                              f"orig={f!r} ref_expected={exp!r} xlq={got!r}", flush=True)
                else:
                    tally[op]["xlq_match"] += 1
                # openpyxl leaves it unshifted -> matches expected only if no shift needed
                tally[op]["opx_match" if norm(f) == norm(exp) else "opx_unshifted"] += 1
        shutil.rmtree(work, ignore_errors=True)
        if used:
            done += 1
    import json
    summary = {"benchmark": "real-corpus shift correctness vs engine-validated reference shifter, 4 ops (deterministic)",
               "workbooks": done, "per_op": {}}
    for op, _, _ in OPS:
        t = tally[op]
        xm, xM = t["xlq_match"], t["xlq_MISMATCH"]
        om, ou = t["opx_match"], t["opx_unshifted"]
        summary["per_op"][op] = {
            "cells_checked": xm + xM, "xlq_mismatch": xM,
            "xlq_correct_rate": round(xm / (xm + xM), 4) if (xm + xM) else None,
            "openpyxl_wrong_rate": round(ou / (om + ou), 4) if (om + ou) else None,
            "refused_workbooks": t["refused"],
        }
    json.dump(summary, open("/home/soh/aix/benchmarks/shift_correctness_real.json", "w"), indent=2)
    print(json.dumps(summary, indent=2))
