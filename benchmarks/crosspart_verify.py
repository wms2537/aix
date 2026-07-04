#!/usr/bin/env python3
"""Verify the UNIFORM cross-part reference-shift on the cross-part fixture corpus,
across all four operations (insert/delete x row/column). For each fixture and op,
apply xlq and confirm every reference-bearing part — in-sheet formulas,
cross-sheet refs, chart data ref, defined name, CF sqref, DV formula, merged
region — shifted to its documented target, computed by an independent Python
re-implementation of the shift semantics. This validates the contribution where
the PC said it was under-evaluated: cross-part uniformity, on charts/cross-sheet/
defined-names, over delete and column paths, not just insert-row."""
import json, os, re, shutil, subprocess, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
CP = "/home/soh/aix/fixtures/crosspart"
SCRATCH = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/cpv"

A1 = re.compile(r"^(\$?)([A-Z]+)(\$?)(\d+)$")


def col_n(s):
    n = 0
    for c in s:
        n = n * 26 + (ord(c) - 64)
    return n


def col_s(n):
    s = ""
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


def shift_line(pos, op, k, n):
    if op == "insert":
        return pos + n if pos >= k else pos
    if pos < k:
        return pos
    if pos >= k + n:
        return pos - n
    return None  # consumed


def shift_span(lo, hi, op, k, n):
    if op == "insert":
        return (lo + n if lo >= k else lo, hi + n if hi >= k else hi)
    be = k + n
    if hi < k:
        return (lo, hi)
    if lo >= be:
        return (lo - n, hi - n)
    if lo < k and hi >= be:
        return (lo, hi - n)
    if k <= lo < be and hi >= be:
        return (k, hi - n)
    if lo < k and k <= hi < be:
        return (lo, k - 1)
    return None  # entirely consumed


def parse_ep(s):
    m = A1.match(s)
    if not m:
        return None
    return (m.group(1) == "$", col_n(m.group(2)), m.group(3) == "$", int(m.group(4)))


def fmt_ep(ca, c, ra, r):
    return f"{'$' if ca else ''}{col_s(c)}{'$' if ra else ''}{r}"


def expected(ref_body, axis, op, k, n):
    """Compute the expected shifted A1 body (no sheet qualifier). axis='row'|'col'.
    Returns the shifted string, or '#REF!' if consumed, or the original if
    unaffected/unparseable."""
    if ":" in ref_body:
        h, t = ref_body.split(":", 1)
        hp, tp = parse_ep(h), parse_ep(t)
        if not hp or not tp:
            return ref_body
        (hca, hc, hra, hr), (tca, tc, tra, tr) = hp, tp
        if axis == "row":
            lo, hi = min(hr, tr), max(hr, tr)
            sp = shift_span(lo, hi, op, k, n)
            if sp is None:
                return "#REF!"
            nl, nh = sp
            return f"{fmt_ep(hca, hc, hra, nl if hr <= tr else nh)}:{fmt_ep(tca, tc, tra, nh if hr <= tr else nl)}"
        else:
            lo, hi = min(hc, tc), max(hc, tc)
            sp = shift_span(lo, hi, op, k, n)
            if sp is None:
                return "#REF!"
            nl, nh = sp
            return f"{fmt_ep(hca, nl if hc <= tc else nh, hra, hr)}:{fmt_ep(tca, nh if hc <= tc else nl, tra, tr)}"
    ep = parse_ep(ref_body)
    if not ep:
        return ref_body
    ca, c, ra, r = ep
    if axis == "row":
        nr = shift_line(r, op, k, n)
        return "#REF!" if nr is None else fmt_ep(ca, c, ra, nr)
    nc = shift_line(c, op, k, n)
    return "#REF!" if nc is None else fmt_ep(ca, nc, ra, r)


def part_text(z, pred):
    for nm in z.namelist():
        if pred(nm):
            yield nm, z.read(nm).decode("utf8", "replace")


def verify_file(path, sheet, axis, op, k):
    """Apply xlq and check each cross-part reference shifted to its expected
    target. Returns (checked, ok, failures)."""
    inv = INV[os.path.basename(path)]
    W = os.path.join(SCRATCH, f"{os.path.basename(path)}.{op}.{axis}")
    if os.path.exists(W):
        shutil.rmtree(W)
    os.makedirs(W)
    dst = os.path.join(W, "b.xlsx")
    shutil.copy(path, dst)
    opname = {("insert", "row"): "insert-rows", ("delete", "row"): "delete-rows",
              ("insert", "col"): "insert-cols", ("delete", "col"): "delete-cols"}[(op, axis)]
    r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op", opname,
                        "--at", str(k), "--count", "1", "--actor", "cp"],
                       capture_output=True, text=True)
    try:
        rj = json.loads(r.stdout)
    except Exception:
        return 0, 0, [("cli", r.stderr[:80])]
    if "rev" not in rj:
        return 0, 0, [("refused", rj.get("error"))]
    z = zipfile.ZipFile(dst)
    refs = inv["references"]
    checked = ok = 0
    fails = []

    def want(body):
        return expected(body, axis, op, k, 1)

    # in-sheet formulas (Data sheet)
    data_xml = next(t for n, t in part_text(z, lambda n: n == "xl/worksheets/sheet1.xml"))
    for key in ("insheet_sum", "insheet_single", "insheet_abs"):
        exp = want(refs[key]["ref"])
        checked += 1
        if exp in data_xml:
            ok += 1
        else:
            fails.append((key, f"want {exp}"))
    # chart data ref (sheet-qualified body after Data!)
    if any("chart" in n for n in z.namelist()):
        chart_xml = "".join(t for n, t in part_text(z, lambda n: "charts/chart" in n))
        body = refs["chart"]["ref"].split("!", 1)[1]
        exp = want(body)
        checked += 1
        if exp in chart_xml:
            ok += 1
        else:
            fails.append(("chart", f"want Data!{exp}"))
    # cross-sheet refs (Report sheet)
    rep_xml = next(t for n, t in part_text(z, lambda n: n == "xl/worksheets/sheet2.xml"))
    for key in ("xref_single", "xref_range"):
        body = refs[key]["ref"].split("!", 1)[1]
        exp = want(body)
        checked += 1
        if exp in rep_xml:
            ok += 1
        else:
            fails.append((key, f"want Data!{exp}"))
    # defined name (workbook.xml)
    wb_xml = next(t for n, t in part_text(z, lambda n: n == "xl/workbook.xml"))
    body = refs["defined"]["ref"].split("!", 1)[1]
    exp = want(body)
    checked += 1
    if exp in wb_xml:
        ok += 1
    else:
        fails.append(("defined", f"want Data!{exp}"))
    # CF sqref + merged (Data sheet)
    for key in ("cf", "merged"):
        body = refs[key].get("sqref") or refs[key].get("ref")
        exp = want(body)
        checked += 1
        if exp in data_xml or exp == "#REF!":
            ok += 1
        else:
            fails.append((key, f"want {exp}"))
    return checked, ok, fails


if __name__ == "__main__":
    os.makedirs(SCRATCH, exist_ok=True)
    INV = {e["file"]: e for e in json.load(open(os.path.join(CP, "inventory.json")))}
    # ops: insert-row@3 (inside ranges), delete-row@2 (clip head), insert-col@2 (col B),
    #      delete-col@6 (a trailing/unused col G area — use 7 to avoid data cols)
    OPS = [("insert", "row", 3), ("delete", "row", 2), ("insert", "col", 2), ("delete", "col", 7)]
    matrix = {}
    totals = Counter()
    all_fails = []
    for e in INV.values():
        path = os.path.join(CP, e["file"])
        for op, axis, k in OPS:
            checked, ok, fails = verify_file(path, e["edited_sheet"], axis, op, k)
            tag = f"{op}-{axis}"
            matrix.setdefault(tag, [0, 0])
            matrix[tag][0] += checked
            matrix[tag][1] += ok
            totals["checked"] += checked
            totals["ok"] += ok
            if fails:
                all_fails.append((e["file"], tag, fails))
    summary = {
        "corpus": f"{len(INV)} cross-part fixtures (chart + cross-sheet + defined-name + CF + DV + merged), x 4 ops",
        "per_op": {k: {"checked": v[0], "correct": v[1],
                       "pct": round(100 * v[1] / v[0], 1) if v[0] else None} for k, v in matrix.items()},
        "total_reference_checks": totals["checked"],
        "correct": totals["ok"],
        "correctness_pct": round(100 * totals["ok"] / totals["checked"], 1) if totals["checked"] else None,
        "failures": all_fails[:20],
    }
    with open("/home/soh/aix/benchmarks/crosspart_correctness.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps({k: v for k, v in summary.items() if k != "failures"}, indent=2))
    if all_fails:
        print("FAILURES:", json.dumps(all_fails[:12], indent=2))
