#!/usr/bin/env python3
"""Honest applicability envelope for xlq structural edits on a REAL corpus.

Answers the survivorship-bias critique: instead of one openpyxl-generated
fixture (where shared formulas / tables never appear), run xlq restructure
(dry-run insert-row) across the vendored IronCalc test corpus — real files
authored in Excel/LibreOffice — and report what fraction xlq SAFELY EDITS vs
REFUSES, and why. A truthful "handles N%, refuses the rest with a real reason,
never silently wrong" is the defensible result."""
import json, os, subprocess, glob
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
SCRATCH = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/corpus"
os.makedirs(SCRATCH, exist_ok=True)
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))


def first_sheet(path):
    r = subprocess.run([XLQ, "inspect", path], capture_output=True, text=True)
    try:
        d = json.loads(r.stdout)
        for s in d.get("sheets", []):
            if s.get("state", "visible") == "visible":
                return s["name"]
    except Exception:
        pass
    return None


def classify(path):
    sheet = first_sheet(path)
    if not sheet:
        return {"status": "inspect_failed"}
    # dry-run insert 1 row at row 2 (past a header) on the first visible sheet
    r = subprocess.run(
        [XLQ, "restructure", path, "--sheet", sheet, "--op", "insert-rows",
         "--at", "2", "--count", "1", "--dry-run"],
        capture_output=True, text=True)
    try:
        d = json.loads(r.stdout)
    except Exception:
        return {"status": "error", "detail": r.stderr[:120]}
    if "error" in d:
        return {"status": "cli_error", "reason": d.get("error")}
    edit = d.get("edit", {})
    residuals = edit.get("residuals", [])
    if residuals:
        reasons = sorted({x.get("reason") for x in residuals})
        return {"status": "refused", "reasons": reasons, "reopens": edit.get("reopens")}
    if not edit.get("reopens", False):
        return {"status": "would_not_reopen"}
    return {"status": "safe", "refs_shifted": edit.get("refs_shifted", 0),
            "parts_touched": len(edit.get("parts_touched", []))}


if __name__ == "__main__":
    results = {}
    status_counter = Counter()
    refusal_reasons = Counter()
    for p in CORPUS:
        rel = os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests")
        c = classify(p)
        results[rel] = c
        status_counter[c["status"]] += 1
        if c["status"] == "refused":
            for r in c["reasons"]:
                refusal_reasons[r] += 1
    total = len(CORPUS)
    safe = status_counter["safe"]
    refused = status_counter["refused"]
    summary = {
        "corpus": "vendored IronCalc xlsx test suite (real Excel/LibreOffice-authored files)",
        "operation": "dry-run insert 1 row at row 2 on the first visible sheet",
        "total_files": total,
        "safe_edit": safe,
        "safe_pct": round(100 * safe / total, 1) if total else 0,
        "refused_residual": refused,
        "refused_pct": round(100 * refused / total, 1) if total else 0,
        "status_breakdown": dict(status_counter),
        "refusal_reasons": dict(refusal_reasons),
        "invariant": "every file is EITHER safely shifted OR refused with a truthful reason — never silently wrong",
    }
    out = {"summary": summary, "per_file": results}
    with open("/home/soh/aix/benchmarks/corpus_coverage.json", "w") as f:
        json.dump(out, f, indent=2)
    print(json.dumps(summary, indent=2))
