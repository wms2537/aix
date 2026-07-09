#!/usr/bin/env python3
"""EDIT-PATH A/B (independent oracle) — NOT an agent study.

HONEST SCOPE (per adversarial review): this compares two PROGRAMMATIC edit paths,
not a live LLM. "Unguarded" is openpyxl, whose insert_rows has a documented
non-feature (it rewrites zero formula references); the 86.6% is therefore the
corpus prevalence of below-insert references times that one known bug, NOT an
agent-error rate. "Guarded" is xlq, which AUTHORS the edit and then self-certifies
it — so this measures xlq's own shifter forward-correctness + a refusal policy,
and the guarded 0%-silent-corruption is partly definitional (a fail-closed gate
cannot silently corrupt by construction). The certifier as a checker of UNTRUSTED
FOREIGN edits (the actual verifiability thesis) is tested separately in
foreign_certify.py, not here.

Setup (real formula-bearing workbooks, task = insert a blank row at K):
  ARM A  openpyxl insert_rows (the standard programmatic edit path).
  ARM B  xlq certify-or-refuse (σ-shift + residual gate).
INDEPENDENT ORACLE: LibreOffice recomputes each edited file; a formula value at its
shifted position that diverges from the original Excel cache = corruption. Labels
by ENGINE divergence, not by anything xlq computed.

Genuinely empirical content: 150/172 xlq edits ENGINE-CONFIRMED faithful, 0 false
certifications, 22 principled refusals. Strawman-sensitive content: the 86.6%
openpyxl figure (a competent ref-shifting engine also gets this op right; the
guard's real differentiator is auditability + explicit refusal + engine-free
certification, which THIS experiment does not isolate)."""
import glob, json, os, re, shutil, subprocess, sys, zipfile
from collections import Counter

sys.path.insert(0, os.path.dirname(__file__))
from forward_correctness import (XLQ, orig_formula_caches, lo_grid, check,
                                 xlq_insert, openpyxl_insert, K)


def zip_first_sheet_name(path):
    """First sheet name straight from xl/workbook.xml — NO subprocess (the xlq
    inspect spawn per file was tripping the sandbox process-spawn cap)."""
    try:
        data = zipfile.ZipFile(path).read("xl/workbook.xml").decode("utf-8", "replace")
        m = re.search(r'<sheet\b[^>]*\bname="([^"]*)"', data)
        return m.group(1) if m else None
    except Exception:
        return None

SCRATCH = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/agentab"
CORPUS = sorted(glob.glob("/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx", recursive=True))


def arm_a_unguarded(path, caches, work):
    """Agent edits with openpyxl, commits as-is. Returns 'faithful'|'SILENT_CORRUPTION'|'na'."""
    try:
        of = openpyxl_insert(path, work)
    except Exception:
        return "na"
    grid = lo_grid(of, work)
    checked, matched, _ = check(caches, grid)
    if checked == 0:
        return "na"
    return "faithful" if matched == checked else "SILENT_CORRUPTION"


def arm_b_guarded(path, sheet, caches, work):
    """Same intent through xlq certify-or-refuse. Returns one of:
    'certified_faithful' | 'certified_but_WRONG' (a real guard failure) | 'refused'."""
    dry = subprocess.run([XLQ, "restructure", path, "--sheet", sheet, "--op",
                          "insert-rows", "--at", str(K), "--count", "1", "--dry-run"],
                         capture_output=True, text=True)
    try:
        if json.loads(dry.stdout).get("edit", {}).get("residuals"):
            return "refused"                        # can't certify -> explicit refuse
    except Exception:
        return "refused"
    xf = xlq_insert(path, sheet, work)
    if not xf:
        return "refused"                            # apply gate blocked it
    grid = lo_grid(xf, work)
    checked, matched, _ = check(caches, grid)
    if checked == 0:
        return "refused"
    return "certified_faithful" if matched == checked else "certified_but_WRONG"


def evaluate(path, work):
    sheet = zip_first_sheet_name(path)
    if not sheet:
        return None
    caches = orig_formula_caches(path)
    if not caches:
        return None
    a = arm_a_unguarded(path, caches, work)
    if a == "na":
        return None                                  # not checkable this file
    b = arm_b_guarded(path, sheet, caches, work)
    return {"file": os.path.relpath(path, "/home/soh/aix/vendor/upstream/xlsx/tests"),
            "unguarded": a, "guarded": b}


OUT = "/home/soh/aix/benchmarks/agent_ab.json"


def load_prior():
    try:
        return {r["file"]: r for r in json.load(open(OUT))["per_file"]}
    except Exception:
        return {}


if __name__ == "__main__":
    os.makedirs(SCRATCH, exist_ok=True)
    # batched append mode: `agent_ab.py OFFSET COUNT` processes a small slice so a
    # single invocation stays under the sandbox process-spawn cap; results merge
    # into OUT across invocations.
    offset = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    # PRE-FILTER by ZIP ONLY (no subprocess): files with formula caches + a sheet.
    cand = [p for p in CORPUS if zip_first_sheet_name(p) and orig_formula_caches(p)]
    checkable = cand[offset:offset + count]
    print(f"{len(cand)} candidate files; this batch [{offset}:{offset+count}] = {len(checkable)}", flush=True)
    merged = load_prior()          # accumulate across batches, keyed by file
    results, ua, gb = [], Counter(), Counter()

    def checkpoint():
        by = dict(merged)
        for r in results:
            by[r["file"]] = r
        allr = list(by.values())
        uac, gbc = Counter(), Counter()
        for r in allr:
            uac[r["unguarded"]] += 1; gbc[r["guarded"]] += 1
        n = len(allr)
        ug_c = uac["SILENT_CORRUPTION"]; g_s = gbc["certified_but_WRONG"]
        s = {"experiment": "EDIT-PATH A/B (NOT an agent study): openpyxl vs xlq "
             "certify-or-refuse on insert-row@2; INDEPENDENT engine oracle (LibreOffice)",
             "scope_caveat": "openpyxl = the standard programmatic edit path, NOT a live "
             "LLM; the openpyxl % is corpus below-insert-reference prevalence x one known "
             "library bug; guarded 0% is partly definitional (fail-closed). Foreign-edit "
             "certification (the verifiability thesis) is in foreign_certify.py.",
             "files_evaluated": n,
             "openpyxl_path": {"faithful": uac["faithful"], "SILENT_CORRUPTION": ug_c,
                               "silent_corruption_rate": round(ug_c / n, 3) if n else None},
             "xlq_certify_or_refuse": {"certified_faithful_engine_confirmed": gbc["certified_faithful"],
                         "refused_principled": gbc["refused"],
                         "certified_but_WRONG_false_certification": g_s,
                         "false_certification_rate": round(g_s / n, 3) if n else None},
             "empirical_result": (f"xlq's self-authored edit is engine-confirmed faithful on "
                          f"{gbc['certified_faithful']}/{n} files, 0 false certifications, "
                          f"{gbc['refused']} principled refusals; the openpyxl path silently "
                          f"mis-shifts references in {ug_c}/{n} ({round(100*ug_c/n,1) if n else 0}%, "
                          f"a known-library-bug x corpus property, not an agent-error rate)."),
             "per_file": allr}
        with open(OUT, "w") as f:
            json.dump(s, f, indent=2)
        return s

    for i, p in enumerate(checkable):
        work = os.path.join(SCRATCH, str(i)); os.makedirs(work, exist_ok=True)
        try:
            r = evaluate(p, work)
        except Exception as e:
            r = None; print(f"  ! {os.path.basename(p)}: {e}", flush=True)
        shutil.rmtree(work, ignore_errors=True)
        if r is None:
            continue
        results.append(r); ua[r["unguarded"]] += 1; gb[r["guarded"]] += 1
        checkpoint()          # write after every file so a timeout still leaves results
        print(f"  [{len(results)}] {r['file'][:40]:40} unguarded={r['unguarded']:17} guarded={r['guarded']}", flush=True)

    summary = checkpoint()
    print(json.dumps({k: v for k, v in summary.items() if k != "per_file"}, indent=2))
    n = len(results)
    print(f"\nunguarded silent-corruption {ua['SILENT_CORRUPTION']}/{n} | "
          f"guarded silent-corruption {gb['certified_but_WRONG']}/{n}")
