#!/usr/bin/env python3
"""LOCKED IN-THE-WILD TEST harness (pre-registered: research-log/016).

Runs the three deterministic xlsx legs on a converted corpus directory:
  Leg 1  4-op shift correctness: xlq output formulas vs the independent
         grid-validity reference shifter (reuses shift_correctness_real verbatim).
  Leg 2  would-corrupt prevalence: share of eligible files with >=1 in-grammar
         formula whose references require a shift under insert-row@2.
  Leg 3  guard verdicts, engine-free production `xlq certify`:
         (a) certify(orig, xlq's own transform)   -> false-refusal count
         (b) certify(orig, openpyxl insert_rows)  -> false-cert count on
             would-corrupt files (the central soundness claim).

Eligibility (pre-registered): first sheet has >= 2 formula cells; zip+openpyxl
parse OK. Cap 500 eligible files in sorted-filename order. No other filtering.
Usage: inthewild_run.py <corpus_dir> <out_json> [cap]
"""
import glob, json, os, shutil, subprocess, sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shift_correctness_real import (XLQ, ref_shift, formulas_of, zip_sheet_name,
                                    new_pos, xlq_edit, norm)

WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/itw"
OPS = [("insert-rows", "row", 2), ("delete-rows", "row", 4),
       ("insert-cols", "col", 2), ("delete-cols", "col", 4)]


def eligible_files(corpus_dir, cap):
    """Sorted-order eligibility scan; returns (kept, counts)."""
    import openpyxl
    out, c = [], Counter()
    for p in sorted(glob.glob(corpus_dir + "/**/*.xlsx", recursive=True)):
        if len(out) >= cap:
            c["beyond_cap"] += 1
            continue
        try:
            sheet = zip_sheet_name(p)
            forms = formulas_of(p)
            if not sheet or len(forms) < 2:
                c["ineligible_lt2_formulas"] += 1
                continue
            wb = openpyxl.load_workbook(p, read_only=True)
            wb.close()
        except Exception:
            c["parse_failed"] += 1
            continue
        out.append((p, sheet, forms))
        c["eligible"] += 1
    return out, c


def would_corrupt(forms):
    """>=1 in-grammar formula whose refs change under insert-rows@2."""
    for f in forms.values():
        exp = ref_shift(f, "row", "insert-rows", 2, 1)
        if exp is not None and norm(exp) != norm(f):
            return True
    return False


def certify(orig, edited, sheet, op="insert-rows", at=2):
    r = subprocess.run([XLQ, "certify", orig, edited, "--sheet", sheet, "--op", op,
                        "--at", str(at), "--count", "1"], capture_output=True, text=True)
    try:
        return json.loads(r.stdout).get("status", "ERROR")
    except Exception:
        return "ERROR"


def opx_edit(src, work):
    import openpyxl
    dst = os.path.join(work, "opx.xlsx")
    shutil.copy(src, dst)
    wb = openpyxl.load_workbook(dst)
    wb.worksheets[0].insert_rows(2, 1)
    wb.save(dst)
    return dst


if __name__ == "__main__":
    corpus_dir, out_json = sys.argv[1], sys.argv[2]
    cap = int(sys.argv[3]) if len(sys.argv) > 3 else 500
    os.makedirs(WORK, exist_ok=True)
    files, counts = eligible_files(corpus_dir, cap)
    print(f"eligible: {counts['eligible']}  parse_failed: {counts['parse_failed']}  "
          f"lt2: {counts['ineligible_lt2_formulas']}  beyond_cap: {counts['beyond_cap']}", flush=True)

    tally = {op: Counter() for op, _, _ in OPS}
    guard = Counter()
    mismatch_log, falsecert_log = [], []

    for i, (p, sheet, forms) in enumerate(files):
        work = os.path.join(WORK, str(i)); os.makedirs(work, exist_ok=True)
        wc = would_corrupt(forms)
        guard["would_corrupt" if wc else "no_shift_needed"] += 1

        # Leg 1 — 4-op shift correctness (identical method to the committed dev-tier run)
        for op, axis, at in OPS:
            xf = xlq_edit(p, sheet, op, at, work)
            if xf is None:
                tally[op]["refused"] += 1
                continue
            tally[op]["applied"] += 1
            xout = formulas_of(xf)
            for (col, row), f in forms.items():
                exp = ref_shift(f, axis, op, at, 1)
                if exp is None:
                    tally[op]["skipped_out_of_grammar"] += 1
                    continue
                np_ = new_pos(col, row, axis, op, at, 1)
                if np_ is None:
                    continue
                got = xout.get(np_)
                if got is None:
                    continue
                if norm(got) != norm(exp):
                    tally[op]["xlq_MISMATCH"] += 1
                    if len(mismatch_log) < 10:
                        mismatch_log.append({"file": os.path.basename(p), "op": op,
                                             "cell": f"{col}{row}", "orig": f,
                                             "expected": exp, "xlq": got})
                else:
                    tally[op]["xlq_match"] += 1
                tally[op]["opx_unshifted" if norm(f) != norm(exp) else "opx_ok"] += 1

        # Leg 3a — certify xlq's own transform (insert-rows@2 leg)
        xf2 = xlq_edit(p, sheet, "insert-rows", 2, work)
        if xf2 is not None:
            v = certify(p, xf2, sheet)
            guard[f"own_{v}"] += 1
        # Leg 3b — certify openpyxl's edit
        try:
            of = opx_edit(p, work)
            v = certify(p, of, sheet)
            guard[f"opx_{v}"] += 1
            if wc and v == "CERTIFIED":
                guard["FALSE_CERT_on_would_corrupt"] += 1
                if len(falsecert_log) < 10:
                    falsecert_log.append(os.path.basename(p))
        except Exception:
            guard["opx_edit_failed"] += 1
        shutil.rmtree(work, ignore_errors=True)
        if (i + 1) % 50 == 0:
            print(f"  ...{i+1}/{len(files)}", flush=True)

    n = len(files)
    summary = {
        "benchmark": "LOCKED in-the-wild test (pre-registered research-log/016)",
        "corpus_dir": corpus_dir, "eligibility": dict(counts), "files_run": n,
        "leg1_shift_correctness": {op: {
            "applied_workbooks": tally[op]["applied"], "refused_workbooks": tally[op]["refused"],
            "cells_checked": tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"],
            "xlq_mismatch": tally[op]["xlq_MISMATCH"],
            "xlq_correct_rate": (round(tally[op]["xlq_match"] /
                                 (tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"]), 4)
                                 if (tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"]) else None),
            "openpyxl_unshifted_cells": tally[op]["opx_unshifted"],
            "skipped_out_of_grammar_cells": tally[op]["skipped_out_of_grammar"],
        } for op, _, _ in OPS},
        "leg2_prevalence": {"would_corrupt_files": guard["would_corrupt"],
                            "no_shift_needed_files": guard["no_shift_needed"],
                            "prevalence": round(guard["would_corrupt"] / n, 4) if n else None},
        "leg3_guard": {k: v for k, v in guard.items() if k.startswith(("own_", "opx_", "FALSE"))},
        "mismatch_samples": mismatch_log, "false_cert_samples": falsecert_log,
    }
    json.dump(summary, open(out_json, "w"), indent=2)
    print(json.dumps(summary, indent=2)[:2000])
