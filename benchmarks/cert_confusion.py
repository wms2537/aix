#!/usr/bin/env python3
"""Non-circular confusion matrix for the REAL `xlq certify`, adjudicated by an
INDEPENDENT engine (LibreOffice) — the validation the circular live-agent slice
could not give.

For each real workbook, two edits of the SAME structural op (insert-row@2):
  FAITHFUL arm  = xlq restructure (xlq's own proven transform)
  CORRUPTED arm = openpyxl insert_rows (leaves references unshifted)
Each edit is (a) judged by `xlq certify` (CERTIFIED / REFUSED) and (b) independently
labeled by LibreOffice recompute vs the original Excel cache (faithful / corrupted).

Positive class = "edit is truly corrupted" (guard should REFUSE):
  TP certify REFUSED  & oracle corrupted      (correctly caught)
  FN certify CERTIFIED & oracle corrupted      <- FALSE CERTIFICATION (must be 0)
  TN certify CERTIFIED & oracle faithful       (correctly certified)
  FP certify REFUSED  & oracle faithful        (over-conservative refusal)
The corrupted arm's FN cell is the soundness-critical, fully xlq-independent test."""
import glob, json, os, re, shutil, subprocess, sys, zipfile
from collections import Counter
sys.path.insert(0, os.path.dirname(__file__))
from forward_correctness import (XLQ, orig_formula_caches, lo_grid, check,
                                 openpyxl_insert, first_sheet_part, K)

WORK = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/certconf"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))


def zip_first_sheet_name(path):
    try:
        d = zipfile.ZipFile(path).read("xl/workbook.xml").decode("utf-8", "replace")
        m = re.search(r'<sheet\b[^>]*\bname="([^"]*)"', d)
        return m.group(1) if m else None
    except Exception:
        return None


def xlq_restructure(src, sheet, work):
    dst = os.path.join(work, "xlq.xlsx"); shutil.copy(src, dst)
    for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op", "insert-rows",
                        "--at", str(K), "--count", "1", "--actor", "c"], capture_output=True, text=True)
    return dst if '"rev"' in r.stdout else None


def xlq_certify(src, edited, sheet):
    r = subprocess.run([XLQ, "certify", src, edited, "--sheet", sheet, "--op",
                        "insert-rows", "--at", str(K), "--count", "1"], capture_output=True, text=True)
    try:
        return json.loads(r.stdout).get("status", "REFUSED")
    except Exception:
        return "REFUSED"


def oracle_label(orig, edited, caches, work):
    grid = lo_grid(edited, work)
    checked, matched, _ = check(caches, grid)
    if checked == 0:
        return "na"
    return "faithful" if matched == checked else "corrupted"


def main():
    os.makedirs(WORK, exist_ok=True)
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 30
    cm = Counter()
    fn_files, fp_files = [], []
    rows, done = [], 0
    for p in CORPUS:
        if done >= limit:
            break
        sheet = zip_first_sheet_name(p)
        caches = orig_formula_caches(p)
        if not sheet or not caches:
            continue
        work = os.path.join(WORK, str(done)); os.makedirs(work, exist_ok=True)
        rel = os.path.relpath(p, "/home/soh/aix/vendor/upstream/xlsx/tests")
        # ORACLE RELIABILITY GATE: LibreOffice must reproduce Excel's cached values on
        # the UNEDITED original. If it doesn't (e.g. ACCRINT/BESSEL, where LO's engine
        # disagrees with Excel), the oracle cannot adjudicate this file — skip it, so
        # engine disagreement is never miscounted as a certify false-certification.
        og = lo_grid(p, work)
        oc, om, _ = check(caches, og)
        if oc == 0 or om != oc:
            shutil.rmtree(work, ignore_errors=True); continue
        arms = []
        # FAITHFUL arm
        xo = xlq_restructure(p, sheet, work)
        if xo:
            arms.append(("faithful", xlq_certify(p, xo, sheet), oracle_label(p, xo, caches, work)))
        # CORRUPTED arm
        try:
            oo = openpyxl_insert(p, work)
            arms.append(("corrupt", xlq_certify(p, oo, sheet), oracle_label(p, oo, caches, work)))
        except Exception:
            pass
        counted = False
        for name, verdict, truth in arms:
            if truth == "na":
                continue
            counted = True
            pos = (truth == "corrupted")
            refused = (verdict == "REFUSED")
            if pos and refused:      cm["TP"] += 1
            elif pos and not refused: cm["FN"] += 1; fn_files.append(f"{rel}:{name}")
            elif not pos and not refused: cm["TN"] += 1
            else: cm["FP"] += 1; fp_files.append(f"{rel}:{name}")
            rows.append({"file": rel, "arm": name, "certify": verdict, "oracle": truth})
        shutil.rmtree(work, ignore_errors=True)
        if counted:
            done += 1
            print(f"  [{done}] {rel[:40]:40} " +
                  " ".join(f"{n}:{v}/{t}" for n, v, t in arms if t != 'na'), flush=True)

    tp, fn, tn, fp = cm["TP"], cm["FN"], cm["TN"], cm["FP"]
    summary = {
        "validation": "xlq certify vs INDEPENDENT LibreOffice oracle; positive=truly corrupted",
        "workbooks": done, "edits_scored": tp + fn + tn + fp,
        "confusion_matrix": {"TP_refused_corrupt": tp, "FN_certified_corrupt_FALSE_CERT": fn,
                             "TN_certified_faithful": tn, "FP_refused_faithful": fp},
        "false_certification_rate": round(fn / (tp + fn), 4) if (tp + fn) else None,
        "recall_on_corruption": round(tp / (tp + fn), 4) if (tp + fn) else None,
        "false_refusal_rate": round(fp / (fp + tn), 4) if (fp + tn) else None,
        "FN_files": fn_files, "FP_files": fp_files[:10],
        "headline": (f"{tp+fn+tn+fp} real edits, independent oracle: {fn} FALSE "
                     f"CERTIFICATIONS, caught {tp}/{tp+fn} corrupted, {fp} false refusals."),
    }
    with open("/home/soh/aix/benchmarks/cert_confusion.json", "w") as f:
        json.dump(summary, f, indent=2)
    print("\n" + json.dumps({k: v for k, v in summary.items() if k not in ("FN_files", "FP_files")}, indent=2))


if __name__ == "__main__":
    main()
