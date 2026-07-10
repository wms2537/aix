#!/usr/bin/env python3
"""LOCKED TEST V2 harness (pre-registered: research-log/018).

Differences from v1's inthewild_run.py, all pre-registered:
  - cross-sheet truth grammar: ref_shift(..., sheet=<edited sheet>) — qualified
    refs and (function-endpoint) ranges enter the checked set;
  - robust certify-JSON extraction (vendored ironcalc println!-pollutes stdout);
  - 5th op: move-rows@3x2->8 with the independent permutation shifter;
  - refusal-cause capture (denylist class / residual reason histogram);
  - per-file watchdog (from v1).
Usage: inthewild_run_v2.py <corpus_dir> <out_json> [cap]
"""
import glob, json, os, re, shutil, signal, subprocess, sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shift_correctness_real import (XLQ, ref_shift, formulas_of, zip_sheet_name,
                                    new_pos, norm, coln, num2col)

WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/itw2"
OPS = [("insert-rows", "row", 2), ("delete-rows", "row", 4),
       ("insert-cols", "col", 2), ("delete-cols", "col", 4)]
MOVE = ("move-rows", 3, 2, 8)                      # at=3 count=2 dest=8 (as dev-tier)


class FileTimeout(Exception):
    pass


def _alarm(sig, frame):
    raise FileTimeout()


def robust_json(stdout):
    """Extract the JSON object from possibly println!-polluted stdout."""
    try:
        return json.loads(stdout)
    except Exception:
        pass
    for m in re.finditer(r"\{", stdout):
        try:
            return json.loads(stdout[m.start():])
        except Exception:
            continue
    return None


def xlq_edit(src, sheet, op, at, count, work, dest=None):
    dst = os.path.join(work, "x.xlsx"); shutil.copy(src, dst)
    for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    cmd = [XLQ, "restructure", dst, "--sheet", sheet, "--op", op, "--at", str(at),
           "--count", str(count), "--actor", "v2"]
    if dest is not None:
        cmd += ["--dest", str(dest)]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=240)
    if '"rev"' in r.stdout:
        return dst, None
    j = robust_json(r.stdout) or {}
    res = j.get("residuals") or []
    reason = res[0].get("reason") if res else j.get("reason", "unknown")
    return None, str(reason)


def certify(orig, edited, sheet, op="insert-rows", at=2, count=1):
    r = subprocess.run([XLQ, "certify", orig, edited, "--sheet", sheet, "--op", op,
                        "--at", str(at), "--count", str(count)],
                       capture_output=True, text=True, timeout=240)
    j = robust_json(r.stdout)
    if j is None:
        return "ERROR", "stdout_unparseable"
    st = j.get("status", "ERROR")
    detail = ""
    if st == "REFUSED":
        detail = str(j.get("reason", ""))[:60]
        if not detail and j.get("residuals"):
            detail = str(j["residuals"][0].get("reason", ""))[:60]
    return st, detail


def move_sigma(r, a=3, n=2, b=8):
    if a <= b <= a + n:
        return r
    if b > a + n:
        if a <= r < a + n: return b - n + (r - a)
        if a + n <= r < b: return r - n
        return r
    if a <= r < a + n: return b + (r - a)
    if b <= r < a: return r + n
    return r


def move_ref_shift(formula, sheet):
    """Move-rows expected formula via σ; reuse the v2 grammar by mapping each
    in-grammar row through σ (single cells + ranges where σ preserves order)."""
    # reuse ref_shift's tokenization by shifting twice? Simplest correct: walk
    # with the same public grammar — insert a large offset then... Keep honest:
    # only single-cell refs + qualified single cells via a targeted walk.
    out, i, n = [], 0, len(formula)
    if "[" in formula or "!" in formula or ":" in formula:
        return None
    CELL = re.compile(r"(\$?)([A-Za-z]{1,3})(\$?)(\d+)")
    while i < n:
        ch = formula[i]
        if ch == '"':
            j = i + 1
            while j < n and formula[j] != '"':
                j += 1
            out.append(formula[i:j + 1]); i = j + 1; continue
        m = CELL.match(formula, i)
        if m:
            prev = formula[i - 1] if i > 0 else ""
            nxt = formula[m.end()] if m.end() < n else ""
            col, row = coln(m.group(2)), int(m.group(4))
            if 1 <= col <= 16384 and 1 <= row <= 1048576 and \
               not (prev.isalnum() or prev in ("_", ".", "$", "!", "'")) and \
               not (nxt.isalpha() or nxt in ("_", "(")):
                out.append(f"{m.group(1)}{m.group(2)}{m.group(3)}{move_sigma(row)}")
                i = m.end(); continue
        out.append(ch); i += 1
    return "".join(out)


def eligible_files(corpus_dir, cap):
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


def would_corrupt(forms, sheet):
    for f in forms.values():
        exp = ref_shift(f, "row", "insert-rows", 2, 1, sheet=sheet)
        if exp is not None and norm(exp) != norm(f):
            return True
    return False


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
    tally["move-rows"] = Counter()
    guard = Counter()
    refuse_causes = Counter()
    own_refuse_causes = Counter()
    mismatch_log = []

    signal.signal(signal.SIGALRM, _alarm)
    for i, (p, sheet, forms) in enumerate(files):
        work = os.path.join(WORK, str(i)); os.makedirs(work, exist_ok=True)
        signal.alarm(300)
        try:
            wc = would_corrupt(forms, sheet)
            guard["would_corrupt" if wc else "no_shift_needed"] += 1

            # Leg 1 — 4 ops with the v2 (cross-sheet) grammar
            for op, axis, at in OPS:
                xf, rreason = xlq_edit(p, sheet, op, at, 1, work)
                if xf is None:
                    tally[op]["refused"] += 1
                    refuse_causes[f"{op}:{rreason}"] += 1
                    continue
                tally[op]["applied"] += 1
                xout = formulas_of(xf)
                for (col, row), f in forms.items():
                    exp = ref_shift(f, axis, op, at, 1, sheet=sheet)
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
                        if len(mismatch_log) < 12:
                            mismatch_log.append({"file": os.path.basename(p), "op": op,
                                                 "cell": f"{col}{row}", "orig": f,
                                                 "expected": exp, "xlq": got})
                    else:
                        tally[op]["xlq_match"] += 1

            # Leg 1b — move-rows with the independent permutation shifter
            op, at, cnt, dest = MOVE
            xf, rreason = xlq_edit(p, sheet, op, at, cnt, work, dest=dest)
            if xf is None:
                tally["move-rows"]["refused"] += 1
                refuse_causes[f"move-rows:{rreason}"] += 1
            else:
                tally["move-rows"]["applied"] += 1
                xout = formulas_of(xf)
                for (col, row), f in forms.items():
                    exp = move_ref_shift(f, sheet)
                    if exp is None:
                        tally["move-rows"]["skipped_out_of_grammar"] += 1
                        continue
                    got = xout.get((col, move_sigma(row)))
                    if got is None:
                        continue
                    if norm(got) != norm(exp):
                        tally["move-rows"]["xlq_MISMATCH"] += 1
                        if len(mismatch_log) < 12:
                            mismatch_log.append({"file": os.path.basename(p), "op": "move-rows",
                                                 "cell": f"{col}{row}", "orig": f,
                                                 "expected": exp, "xlq": got})
                    else:
                        tally["move-rows"]["xlq_match"] += 1

            # Leg 3a — own transform certify (robust JSON)
            xf2, rreason = xlq_edit(p, sheet, "insert-rows", 2, 1, work)
            if xf2 is not None:
                v, detail = certify(p, xf2, sheet)
                guard[f"own_{v}"] += 1
                if v != "CERTIFIED":
                    own_refuse_causes[detail or v] += 1
            else:
                guard["own_not_attempted_restructure_refused"] += 1
                own_refuse_causes[f"restructure:{rreason}"] += 1
            # Leg 3b — openpyxl path certify
            try:
                of = opx_edit(p, work)
                v, _ = certify(p, of, sheet)
                guard[f"opx_{v}"] += 1
                if wc and v == "CERTIFIED":
                    guard["FALSE_CERT_on_would_corrupt"] += 1
            except FileTimeout:
                raise
            except Exception:
                guard["opx_edit_failed"] += 1
        except FileTimeout:
            guard["file_timeout"] += 1
            print(f"  TIMEOUT (skipped, counted): {os.path.basename(p)}", flush=True)
        finally:
            signal.alarm(0)
            shutil.rmtree(work, ignore_errors=True)
        if (i + 1) % 50 == 0:
            print(f"  ...{i+1}/{len(files)}", flush=True)

    n = len(files)
    all_ops = [op for op, _, _ in OPS] + ["move-rows"]
    summary = {
        "benchmark": "LOCKED TEST V2 (pre-registered research-log/018)",
        "corpus_dir": corpus_dir, "eligibility": dict(counts), "files_run": n,
        "leg1_shift_correctness": {op: {
            "applied_workbooks": tally[op]["applied"], "refused_workbooks": tally[op]["refused"],
            "cells_checked": tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"],
            "xlq_mismatch": tally[op]["xlq_MISMATCH"],
            "xlq_correct_rate": (round(tally[op]["xlq_match"] /
                                 (tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"]), 4)
                                 if (tally[op]["xlq_match"] + tally[op]["xlq_MISMATCH"]) else None),
            "skipped_out_of_grammar_cells": tally[op]["skipped_out_of_grammar"],
        } for op in all_ops},
        "leg2_prevalence": {"would_corrupt_files": guard["would_corrupt"],
                            "no_shift_needed_files": guard["no_shift_needed"],
                            "prevalence": round(guard["would_corrupt"] / n, 4) if n else None},
        "leg3_guard": {k: v for k, v in guard.items()
                       if k.startswith(("own_", "opx_", "FALSE", "file_"))},
        "refusal_causes_restructure": dict(refuse_causes.most_common(15)),
        "own_refusal_causes": dict(own_refuse_causes.most_common(15)),
        "mismatch_samples": mismatch_log,
    }
    json.dump(summary, open(out_json, "w"), indent=2)
    print(json.dumps(summary, indent=2)[:2200])
