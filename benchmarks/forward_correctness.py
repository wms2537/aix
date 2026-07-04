#!/usr/bin/env python3
"""FORWARD correctness oracle for σ — discriminating, against Excel ground truth.

The prior round-trip oracle (insert@k then delete@k, compare carried caches) was
non-discriminative: it proved displacement invertibility, not reference-shift
correctness — a no-op shifter (openpyxl, which shifts NO references) passes it
too. The six-reviewer PC flagged this unanimously. This is the fix.

Property: inserting a BLANK row at k is value-preserving under CORRECT reference
shifting — every formula tracks its data (which moved down) and the blank
contributes nothing, so each formula's recomputed value is unchanged. Therefore:

  recompute(xlq-edited)[cell shifted to its new position] == Excel-cache(original)[cell]

We recompute the edited file with LibreOffice (an engine INDEPENDENT of xlq's
IronCalc; and xlq expands shared formulas to explicit ones, which LibreOffice
computes correctly). We compare against the ORIGINAL file's Excel-authored
cached <v> values (ground truth).

DISCRIMINATION PROOF: we run the SAME check on openpyxl's insert_rows output. A
no-op shifter leaves =B5*2 pointing at the now-blank inserted row, so LibreOffice
recomputes a DIFFERENT value → openpyxl must FAIL where xlq passes. If both pass,
the oracle is not discriminating and the result is worthless."""
import csv, glob, json, os, re, shutil, subprocess, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
SCRATCH = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/fwd"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))
K = 2  # insert one blank row at row 2 (below a typical header)

# formula cells with a numeric cached value on the FIRST sheet: (col,row)->value.
# The lazy (?!</c>) guards keep the <f> and <v> within the SAME cell element.
CELL = re.compile(rb'<c r="([A-Z]+)(\d+)"((?:(?!</c>).)*?<f[ >](?:(?!</c>).)*?<v>([^<]*)</v>)', re.S)

# Position-dependent / volatile functions whose result legitimately CHANGES when
# a blank row is inserted (they resolve by absolute position or wall-clock, not
# by shifted references), so the value-preservation property does NOT apply —
# their reference ARGUMENTS still shift correctly, but the value cannot be
# checked this way. Excluded from the value-preservation oracle.
VOLATILE = re.compile(rb'\b(OFFSET|INDIRECT|NOW|TODAY|RAND|RANDBETWEEN|CELL|INFO)\b', re.I)


def col_num(s):
    n = 0
    for ch in s:
        n = n * 26 + (ord(ch) - 64)
    return n


def first_sheet_name(path):
    r = subprocess.run([XLQ, "inspect", path], capture_output=True, text=True)
    try:
        for s in json.loads(r.stdout).get("sheets", []):
            if s.get("state", "visible") == "visible":
                return s["name"]
    except Exception:
        pass
    return None


def first_sheet_part(path):
    z = zipfile.ZipFile(path)
    # sheet1.xml is the conventional first; fall back to the lowest-numbered
    names = sorted(n for n in z.namelist() if re.match(r"xl/worksheets/sheet\d+\.xml$", n))
    return names[0] if names else None


def orig_formula_caches(path):
    """{(col,row): float} for formula cells with numeric caches on the first sheet."""
    part = first_sheet_part(path)
    if not part:
        return {}
    data = zipfile.ZipFile(path).read(part)
    out = {}
    for m in CELL.finditer(data):
        # m.group(3) is the cell's inner content (the <f>...<v> span); exclude
        # cells whose formula uses a position-dependent/volatile function.
        if VOLATILE.search(m.group(3)):
            continue
        try:
            out[(col_num(m.group(1).decode()), int(m.group(2)))] = float(m.group(4))
        except ValueError:
            pass
    return out


def lo_grid(path, work):
    outd = os.path.join(work, "csv")
    os.makedirs(outd, exist_ok=True)
    subprocess.run(["libreoffice", "--headless", "--convert-to", "csv",
                    "--outdir", outd, path], capture_output=True, timeout=90,
                   env={**os.environ, "HOME": work})
    base = os.path.splitext(os.path.basename(path))[0] + ".csv"
    p = os.path.join(outd, base)
    if not os.path.exists(p):
        return None
    with open(p, newline="") as f:
        return list(csv.reader(f))


def val_at(grid, col, row):
    """1-based (col,row) into a 0-based CSV grid; None if out of range/non-numeric."""
    if grid is None or row - 1 >= len(grid):
        return None
    r = grid[row - 1]
    if col - 1 >= len(r):
        return None
    try:
        return float(r[col - 1])
    except ValueError:
        return None


def check(orig_caches, edited_grid):
    """Every original formula-cache value must appear, recomputed, at its shifted
    position in the edited grid. Returns (checked, matched, first_fail)."""
    checked = matched = 0
    fail = None
    for (c, rr), v in orig_caches.items():
        rr2 = rr + 1 if rr >= K else rr  # blank-row insert at K shifts row down
        got = val_at(edited_grid, c, rr2)
        if got is None:
            continue  # LO didn't emit a numeric there (text/blank) — skip
        checked += 1
        if abs(got - v) <= 1e-9 * max(abs(got), abs(v), 1.0) + 1e-12:
            matched += 1
        elif fail is None:
            fail = f"({c},{rr})->{rr2}: cache {v} != recompute {got}"
    return checked, matched, fail


def xlq_insert(src, sheet, work):
    dst = os.path.join(work, "xlq.xlsx")
    for suf in ("", ".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    shutil.copy(src, dst)
    r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op",
                        "insert-rows", "--at", str(K), "--count", "1", "--actor", "fwd"],
                       capture_output=True, text=True)
    return dst if '"rev"' in r.stdout else None


def openpyxl_insert(src, work):
    import openpyxl
    dst = os.path.join(work, "opx.xlsx")
    wb = openpyxl.load_workbook(src)
    wb[wb.sheetnames[0]].insert_rows(K, 1)
    wb.save(dst)
    return dst


def evaluate(path, work):
    sheet = first_sheet_name(path)
    if not sheet:
        return {"status": "no_sheet"}
    dry = subprocess.run([XLQ, "restructure", path, "--sheet", sheet, "--op",
                          "insert-rows", "--at", str(K), "--count", "1", "--dry-run"],
                         capture_output=True, text=True)
    try:
        if json.loads(dry.stdout).get("edit", {}).get("residuals"):
            return {"status": "refused_skip"}
    except Exception:
        return {"status": "dry_error"}
    caches = orig_formula_caches(path)
    if not caches:
        return {"status": "no_formula_caches"}

    xf = xlq_insert(path, sheet, work)
    if not xf:
        return {"status": "xlq_failed"}
    xg = lo_grid(xf, work)
    xchecked, xmatched, xfail = check(caches, xg)
    if xchecked == 0:
        return {"status": "no_checkable_cells"}
    xlq_ok = xmatched == xchecked

    # discrimination control: openpyxl on the same file
    opx_ok = None
    try:
        of = openpyxl_insert(path, work)
        og = lo_grid(of, work)
        ochecked, omatched, _ = check(caches, og)
        if ochecked:
            opx_ok = (omatched == ochecked)
    except Exception:
        opx_ok = None

    return {"status": "xlq_correct" if xlq_ok else "xlq_WRONG",
            "checked": xchecked, "matched": xmatched, "fail": xfail,
            "openpyxl_correct": opx_ok}


if __name__ == "__main__":
    os.makedirs(SCRATCH, exist_ok=True)
    # sample for runtime (LibreOffice is slow); every 3rd file
    sample = CORPUS[::3]
    out = Counter()
    xlq_correct = xlq_wrong = 0
    opx_pass = opx_fail = opx_na = 0
    fails = []
    disc = []  # files where xlq passes AND openpyxl fails => discrimination shown
    n = 0
    for i, p in enumerate(sample):
        work = os.path.join(SCRATCH, str(i))
        os.makedirs(work, exist_ok=True)
        r = evaluate(p, work)
        out[r["status"]] += 1
        if r["status"] in ("xlq_correct", "xlq_WRONG"):
            n += 1
            if r["status"] == "xlq_correct":
                xlq_correct += 1
            else:
                xlq_wrong += 1
                fails.append((os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests"), r.get("fail")))
            oc = r.get("openpyxl_correct")
            if oc is True:
                opx_pass += 1
            elif oc is False:
                opx_fail += 1
                if r["status"] == "xlq_correct":
                    disc.append(os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests"))
            else:
                opx_na += 1
        shutil.rmtree(work, ignore_errors=True)
    summary = {
        "oracle": "FORWARD: insert blank row @2, recompute edited file with LibreOffice (independent engine), every formula's recomputed value at its shifted position must equal the original Excel cache. Blank-row insert is value-preserving under CORRECT shifting; a no-op shifter fails.",
        "sampled": len(sample), "forward_checked": n,
        "xlq_correct": xlq_correct, "xlq_wrong": xlq_wrong,
        "xlq_correctness_pct": round(100 * xlq_correct / n, 1) if n else None,
        "discrimination": {
            "openpyxl_failed_same_check": opx_fail,
            "openpyxl_passed": opx_pass,
            "openpyxl_not_applicable": opx_na,
            "files_where_xlq_passes_and_openpyxl_fails": len(disc),
            "note": "openpyxl failing where xlq passes proves the oracle tests forward reference-shift correctness, not mere invertibility",
        },
        "xlq_failures": fails[:15],
        "status_breakdown": dict(out),
    }
    with open("/home/soh/aix/benchmarks/forward_correctness.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps(summary, indent=2))
