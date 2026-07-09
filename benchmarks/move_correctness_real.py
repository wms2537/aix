#!/usr/bin/env python3
"""Real-corpus move-rows shift correctness — DETERMINISTIC (no recompute).

Same method as shift_correctness_real, for the new move-rows op: relocate rows
[at, at+count) to before `dest`, and compare xlq's output formula for each cell to an
independent permutation shifter σ (single-cell refs only; ranges/whole-col/cross-sheet/
straddle are skipped, not guessed — xlq handles them but this simple checker cannot
independently verify them). Files xlq refuses (straddle/residual) are skipped."""
import glob, os, re, shutil, subprocess, sys, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))
WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/mcr"
AT, COUNT, DEST = 3, 2, 8          # move rows 3-4 to before row 8 (move-down)
FTAG = re.compile(rb'<c r="([A-Z]+)(\d+)"(?:(?!</c>).)*?<f[^>]*>([^<]*)</f>', re.S)
CELLTOK = re.compile(r"(\$?)([A-Za-z]{1,3})(\$?)(\d+)")
VOL = re.compile(r"\b(OFFSET|INDIRECT|NOW|TODAY|RAND|RANDBETWEEN|CELL|INFO|ROW|COLUMN|ROWS|COLUMNS)\b", re.I)


def sigma(r, a=AT, n=COUNT, b=DEST):
    if a <= b <= a + n:
        return r
    if b > a + n:
        if a <= r < a + n: return b - n + (r - a)
        if a + n <= r < b: return r - n
        return r
    if a <= r < a + n: return b + (r - a)
    if b <= r < a: return r + n
    return r


def coln(s):
    n = 0
    for ch in s.upper():
        n = n * 26 + (ord(ch) - 64)
    return n


def ref_shift_move(formula):
    """Shift single-cell row refs by σ. Skip (None) any construct outside the simple
    grammar: ranges, whole-col/row, cross-sheet, tables, function-endpoint ranges."""
    if "[" in formula or "!" in formula or ":" in formula:
        return None
    out, i, n = [], 0, len(formula)
    while i < n:
        ch = formula[i]
        if ch == '"':
            j = i + 1
            while j < n and formula[j] != '"':
                j += 1
            out.append(formula[i:j + 1]); i = j + 1; continue
        m = CELLTOK.match(formula, i)
        if m:
            prev = formula[i - 1] if i > 0 else ""
            nxt = formula[m.end()] if m.end() < n else ""
            col, row = coln(m.group(2)), int(m.group(4))
            if 1 <= col <= 16384 and 1 <= row <= 1048576 and \
               not (prev.isalnum() or prev in ("_", ".", "$", "!", "'")) and \
               not (nxt.isalpha() or nxt in ("_", "(")):
                out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{sigma(row)}"); i = m.end(); continue
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
    out = {}
    for m in FTAG.finditer(z.read(part)):
        f = m.group(3).decode("utf-8", "replace")
        if not VOL.search(f):
            out[(m.group(1).decode(), int(m.group(2)))] = f
    return out


def zip_sheet_name(path):
    d = zipfile.ZipFile(path).read("xl/workbook.xml").decode("utf-8", "replace")
    m = re.search(r'<sheet\b[^>]*\bname="([^"]*)"', d)
    return m.group(1) if m else None


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 60
    t = Counter(); done = 0
    for p in CORPUS:
        if done >= limit:
            break
        sheet = zip_sheet_name(p); orig = formulas_of(p)
        if not sheet or len(orig) < 2:
            continue
        work = os.path.join(WORK, str(done)); os.makedirs(work, exist_ok=True)
        dst = os.path.join(work, "x.xlsx"); shutil.copy(p, dst)
        for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
            if os.path.exists(dst + suf):
                os.remove(dst + suf)
        r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op", "move-rows",
                            "--at", str(AT), "--count", str(COUNT), "--dest", str(DEST), "--actor", "m"],
                           capture_output=True, text=True)
        if '"rev"' not in r.stdout:
            t["refused_or_residual"] += 1; shutil.rmtree(work, ignore_errors=True); continue
        xout = formulas_of(dst)
        used = False
        for (col, row), f in orig.items():
            exp = ref_shift_move(f)
            if exp is None:
                continue
            got = xout.get((col, sigma(row)))
            if got is None:
                continue
            used = True
            if norm(got) == norm(exp):
                t["xlq_match"] += 1
            else:
                t["xlq_MISMATCH"] += 1
                if t["_log"] < 4:
                    t["_log"] += 1
                    print(f"  MISMATCH {os.path.basename(p)} {col}{row}->{col}{sigma(row)}: "
                          f"orig={f!r} exp={exp!r} xlq={got!r}", flush=True)
        shutil.rmtree(work, ignore_errors=True)
        if used:
            done += 1
    import json
    xm, xM = t["xlq_match"], t["xlq_MISMATCH"]
    summary = {"benchmark": f"real-corpus move-rows shift correctness (move [{AT},{AT+COUNT}) -> before {DEST}), deterministic",
               "workbooks_checked": done, "workbooks_refused_or_residual": t["refused_or_residual"],
               "cells_checked": xm + xM, "xlq_mismatch": xM,
               "xlq_correct_rate": round(xm / (xm + xM), 4) if (xm + xM) else None}
    json.dump(summary, open("/home/soh/aix/benchmarks/move_correctness_real.json", "w"), indent=2)
    print(json.dumps(summary, indent=2))
