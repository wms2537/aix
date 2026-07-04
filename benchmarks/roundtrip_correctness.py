#!/usr/bin/env python3
"""Correctness oracle for σ on the REAL corpus — engine-free, against Excel's
own ground truth.

For each safely-editable file: insert a blank row at k, then delete row k. The
net effect is identity, so every cell returns to its original position. xlq
carries each cell's cached value (`<v>`, computed by the ORIGINAL authoring
tool — Excel) along the shift, so the round-tripped file's caches must equal the
original's, cell-for-cell. Any wrong shift (on insert OR delete) would move a
value to the wrong cell and the caches would diverge. We read `<v>` directly
from the sheet XML — no recompute engine, so this compares against Excel's
ground truth, not a possibly-inconsistent reimplementation. (We learned
LibreOffice reconstructs SHARED formulas differently from Excel, so an
LO-recompute reference is unsound here; the cached values are authoritative.)"""
import glob, json, os, re, shutil, subprocess, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
SCRATCH = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/rt2"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))

CELL_V = re.compile(rb'<c r="([A-Z]+\d+)"[^>]*?>(?:(?!</c>).)*?<v>([^<]*)</v>', re.S)


def first_sheet(path):
    r = subprocess.run([XLQ, "inspect", path], capture_output=True, text=True)
    try:
        for s in json.loads(r.stdout).get("sheets", []):
            if s.get("state", "visible") == "visible":
                return s["name"]
    except Exception:
        pass
    return None


def cached_values(path):
    """{cell -> float} for every cell with a numeric cached value, across all
    worksheet parts."""
    out = {}
    try:
        z = zipfile.ZipFile(path)
    except Exception:
        return None
    for n in z.namelist():
        if n.startswith("xl/worksheets/sheet") and n.endswith(".xml"):
            data = z.read(n)
            for m in CELL_V.finditer(data):
                cell = m.group(1).decode()
                try:
                    out[(n, cell)] = float(m.group(2))
                except ValueError:
                    pass
    return out


def values_match(a, b, rtol=1e-9, atol=1e-12):
    if a is None or b is None:
        return False, "read_failed"
    # compare on the intersection of cells present in both (identity round-trip
    # keeps the same cells; a divergence in the set is itself a failure)
    if set(a) != set(b):
        return False, f"cell_set_differs({len(set(a)^set(b))})"
    for k in a:
        fa, fb = a[k], b[k]
        if abs(fa - fb) > atol + rtol * max(abs(fa), abs(fb), 1.0):
            return False, f"value_differs@{k[1]}:{fa}!={fb}"
    return True, "ok"


def xlq_op(dst, sheet, op, at):
    for suf in ("", ".xlq.jsonl", ".rev-1.xlsx", ".rev-2.xlsx", ".xlq.lock"):
        p = dst + suf
        if os.path.exists(p):
            os.remove(p)
    return dst


def roundtrip(path, work):
    sheet = first_sheet(path)
    if not sheet:
        return {"status": "no_sheet"}
    dry = subprocess.run([XLQ, "restructure", path, "--sheet", sheet, "--op",
                          "insert-rows", "--at", "2", "--count", "1", "--dry-run"],
                         capture_output=True, text=True)
    try:
        if json.loads(dry.stdout).get("edit", {}).get("residuals"):
            return {"status": "refused_skip"}
    except Exception:
        return {"status": "dry_error"}

    before = cached_values(path)
    dst = os.path.join(work, "b.xlsx")
    for suf in ("", ".xlq.jsonl", ".rev-1.xlsx", ".rev-2.xlsx"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    shutil.copy(path, dst)
    r1 = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op",
                         "insert-rows", "--at", "2", "--count", "1", "--actor", "rt"],
                        capture_output=True, text=True)
    if '"rev"' not in r1.stdout:
        return {"status": "insert_failed"}
    r2 = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op",
                         "delete-rows", "--at", "2", "--count", "1", "--actor", "rt"],
                        capture_output=True, text=True)
    if '"rev"' not in r2.stdout:
        return {"status": "delete_failed"}
    after = cached_values(dst)
    ok, why = values_match(before, after)
    return {"status": "match" if ok else "MISMATCH", "why": why}


if __name__ == "__main__":
    os.makedirs(SCRATCH, exist_ok=True)
    outcomes = Counter()
    mismatches = []
    for i, p in enumerate(CORPUS):
        work = os.path.join(SCRATCH, str(i))
        os.makedirs(work, exist_ok=True)
        r = roundtrip(p, work)
        outcomes[r["status"]] += 1
        if r["status"] == "MISMATCH":
            mismatches.append((os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests"), r.get("why")))
        shutil.rmtree(work, ignore_errors=True)
    checked = outcomes["match"] + outcomes["MISMATCH"]
    summary = {
        "oracle": "insert@2 then delete@2 is identity; xlq-carried Excel cached values (<v>) must equal the original's, cell-for-cell (engine-free, Excel ground truth)",
        "total": len(CORPUS),
        "differentially_checked": checked,
        "match": outcomes["match"], "mismatch": outcomes["MISMATCH"],
        "correctness_pct": round(100 * outcomes["match"] / checked, 1) if checked else None,
        "refused_skip": outcomes["refused_skip"],
        "mismatched_files": mismatches,
        "outcome_breakdown": dict(outcomes),
    }
    with open("/home/soh/aix/benchmarks/roundtrip_correctness.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps({k: v for k, v in summary.items() if k != "mismatched_files"}, indent=2))
    if mismatches:
        print("MISMATCHES:", json.dumps(mismatches[:10], indent=2))
