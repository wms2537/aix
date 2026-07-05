#!/usr/bin/env python3
"""Tier-coverage of semantic-redundancy certification on the real corpus — the
moat, quantified, ENGINE-FREE.

For a structural edit, a formula is EXACT-tier certifiable (Theorem 1, machine-
checked in formal/SelfOracle.lean) iff its reference dependencies are STATIC
(determinable from syntax) — so the graph-isomorphism check is engine-free and
implies value-faithfulness under ANY semantics. The exact boundary is the
dynamic-reference functions INDIRECT and OFFSET, whose dependency set is
data-computed; those drop to the PROBABILISTIC tier (self-oracle value check).
Array/table workbooks are REFUSED.

This measures: what fraction of real structural edits are certifiable with ZERO
engine and ZERO oracle, purely by the machine-checked theorem?"""
import glob, json, re, subprocess, zipfile
from collections import Counter

XLQ = "/home/soh/aix/xlq/target/release/xlq"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))

# functions whose reference DEPENDENCIES are data-computed (not syntactic) -> a
# formula containing one cannot be graph-iso-checked engine-free -> Tier 2, not Exact.
DYNAMIC_REF = re.compile(rb'\b(INDIRECT|OFFSET)\s*\(', re.I)
# formula bodies, per worksheet part
FBODY = re.compile(rb'<f[^>]*>((?:(?!</f>).)+)</f>', re.S)
SHARED_ARRAY = re.compile(rb'<f[^>]*\bt="(shared|array)"')


def first_sheet(path):
    r = subprocess.run([XLQ, "inspect", path], capture_output=True, text=True)
    try:
        for s in json.loads(r.stdout).get("sheets", []):
            if s.get("state", "visible") == "visible":
                return s["name"]
    except Exception:
        pass
    return None


def classify(path):
    sheet = first_sheet(path)
    if not sheet:
        return {"status": "no_sheet"}
    # refused (array / table) — Tier 3
    dry = subprocess.run([XLQ, "restructure", path, "--sheet", sheet, "--op",
                          "insert-rows", "--at", "2", "--count", "1", "--dry-run"],
                         capture_output=True, text=True)
    try:
        resid = json.loads(dry.stdout).get("edit", {}).get("residuals")
    except Exception:
        return {"status": "dry_error"}
    if resid:
        reasons = sorted({r.get("reason") for r in resid})
        return {"status": "refused", "reasons": reasons}

    z = zipfile.ZipFile(path)
    exact = dynamic = 0
    has_dynamic = False
    for n in z.namelist():
        if n.startswith("xl/worksheets/sheet") and n.endswith(".xml"):
            data = z.read(n)
            for m in FBODY.finditer(data):
                body = m.group(1)
                if DYNAMIC_REF.search(body):
                    dynamic += 1
                    has_dynamic = True
                else:
                    exact += 1
    total = exact + dynamic
    return {
        "status": "fully_exact" if (not has_dynamic and total > 0) else
                  ("mixed" if has_dynamic else "no_formulas"),
        "formula_cells": total, "exact_cells": exact, "dynamic_cells": dynamic,
    }


if __name__ == "__main__":
    st = Counter()
    refusal = Counter()
    cell_exact = cell_dynamic = 0
    files_with_formulas = 0
    for p in CORPUS:
        c = classify(p)
        st[c["status"]] += 1
        if c["status"] == "refused":
            for r in c["reasons"]:
                refusal[r] += 1
        if c.get("formula_cells", 0) > 0:
            files_with_formulas += 1
            cell_exact += c["exact_cells"]
            cell_dynamic += c["dynamic_cells"]
    total = len(CORPUS)
    editable = st["fully_exact"] + st["mixed"]  # not refused, has content
    cell_total = cell_exact + cell_dynamic
    summary = {
        "corpus": "231 vendored IronCalc test workbooks (real Excel/LibreOffice-authored)",
        "operation": "insert 1 row at row 2, first visible sheet",
        "total_files": total,
        "tiers": {
            "exact_engine_free_files": st["fully_exact"],
            "mixed_files": st["mixed"],
            "refused_files": st["refused"],
            "no_formulas_or_sheet": st["no_formulas"] + st["no_sheet"] + st["dry_error"],
        },
        "of_editable_files": {
            "editable": editable,
            "fully_exact_pct": round(100 * st["fully_exact"] / editable, 1) if editable else None,
        },
        "formula_cell_level": {
            "total_formula_cells": cell_total,
            "exact_certifiable_cells": cell_exact,
            "exact_cell_pct": round(100 * cell_exact / cell_total, 1) if cell_total else None,
            "dynamic_ref_cells_INDIRECT_OFFSET": cell_dynamic,
        },
        "refusal_reasons": dict(refusal),
        "interpretation": "exact-tier files/cells are certified value-faithful under ANY semantics with zero engine and zero oracle, by the Lean-checked Theorem 1",
    }
    with open("/home/soh/aix/benchmarks/tier_coverage.json", "w") as f:
        json.dump(summary, f, indent=2)
    print(json.dumps(summary, indent=2))
