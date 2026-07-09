#!/usr/bin/env python3
"""Scorer for the guarded-vs-unguarded LIVE-AGENT STUDY (op = insert-row@2).

Input: agent_outputs.json = {file(rel-path): {orig_cell_A1: corrected_formula}}.
Each agent's edit is materialized as a real shipped artifact:

  STRUCTURE  openpyxl insert_rows(k) on the ORIGINAL file — NOT xlq, so nothing
             of the guard's reference implementation leaks into the artifact.
  FORMULAS   zip surgery replaces ONLY the <f> bodies at each task cell's
             post-insert position with the AGENT's formula. Task cells the
             agent did not answer keep openpyxl's behavior (formula text left
             UNSHIFTED at its new position) — that IS the shipped artifact.

GROUND TRUTH (independent of the guard): shift_correctness_real.ref_shift — the
reference shifter validated against two engines (LibreOffice + `formulas`) in
conformance_v2. A task cell is correct iff the BUILT ARTIFACT's formula at the
shifted position == ref_shift(original formula), normalized. Cells outside
ref_shift's grammar are EXCLUDED from truth (skipped, counted — no guessing).
agent_correct(task) := every truth-evaluated cell matches AND >= 1 evaluated.

ARMS (same artifact, two shipping policies):
  GUARDED    gate through foreign_certify.certify_foreign — the direct
             graph-hypothesis checker (engine-free; NOT equality-to-xlq).
             CERTIFIED -> ships; REFUSED / unparseable -> blocked (fail closed).
  UNGUARDED  ships as-is.

Per-task outcomes:
  guarded   in {shipped_correct, shipped_CORRUPT_false_cert,
                refused_correct (= COST), refused_incorrect (= SAVE)}
  unguarded in {shipped_correct, shipped_CORRUPT}

usage: score.py agent_outputs.json [results.json]
       env TASKS_FILE overrides the default tasks.json next to this script.
"""
import json, os, re, shutil, sys, zipfile
from collections import Counter

BENCH = "/home/soh/aix/benchmarks"
sys.path.insert(0, BENCH)
from foreign_certify import certify_foreign                    # noqa: E402  (the GUARD)
from shift_correctness_real import ref_shift, norm             # noqa: E402  (the TRUTH)
from live3way_truth import _esc                                # noqa: E402  (zip surgery)

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = "/home/soh/aix/vendor/upstream/xlsx/tests"
WORK = os.environ.get("AGENT_STUDY_WORK",
                      "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d"
                      "/scratchpad/agent_study_work")
FCELL = re.compile(rb'<c r="([A-Z]+)(\d+)"(?:(?!</c>).)*?<f[^>]*>([^<]*)</f>', re.S)


def _unesc(s):
    return (s.replace("&lt;", "<").replace("&gt;", ">")
             .replace("&quot;", '"').replace("&apos;", "'").replace("&amp;", "&"))


def sheet_part_by_name(z, sheet):
    """Sheet NAME -> worksheet part path, robust to XML attribute ORDER (openpyxl
    writes r:id before/after name and Target before Id; live3way_truth's regex
    assumed Excel's order and returns None on openpyxl output)."""
    wb = z.read("xl/workbook.xml").decode("utf-8", "replace")
    rid = None
    for m in re.finditer(r"<sheet\b[^>]*?/?>", wb):
        tag = m.group(0)
        nm = re.search(r'\bname="([^"]*)"', tag)
        ri = re.search(r'\br:id="([^"]*)"', tag)
        if nm and ri and nm.group(1) == sheet:
            rid = ri.group(1)
            break
    if rid is None:
        return None
    rels = z.read("xl/_rels/workbook.xml.rels").decode("utf-8", "replace")
    for m in re.finditer(r"<Relationship\b[^>]*?/?>", rels):
        tag = m.group(0)
        idm = re.search(r'\bId="([^"]*)"', tag)
        tm = re.search(r'\bTarget="([^"]*)"', tag)
        if idm and tm and idm.group(1) == rid:
            tgt = tm.group(1).lstrip("/")
            return tgt if tgt.startswith("xl/") else "xl/" + tgt
    return None


def shifted_a1(a1, k):
    col = "".join(ch for ch in a1 if ch.isalpha())
    row = int("".join(ch for ch in a1 if ch.isdigit()))
    return f"{col}{row + 1 if row >= k else row}"


def openpyxl_insert(src, sheet, k, dst):
    """The foreign structural edit: openpyxl inserts a blank row at k. It moves
    cells but shifts NO formula references — exactly the naive-tool baseline."""
    import openpyxl
    wb = openpyxl.load_workbook(src)
    wb[sheet].insert_rows(k, 1)
    wb.save(dst)
    return dst


def splice_formulas(path, sheet, by_a1):
    """SURGICAL: replace ONLY the <f>...</f> body of each cell r=a1 with the
    agent's formula (XML-escaped). Every other byte of the openpyxl-built file
    stays as openpyxl wrote it. Returns the list of cells whose <f> element was
    not found (splice missed -> the artifact keeps openpyxl's unshifted text)."""
    z = zipfile.ZipFile(path)
    part = sheet_part_by_name(z, sheet)
    names = z.namelist()
    data = z.read(part).decode("utf-8", "replace")
    missed = []
    for a1, f in by_a1.items():
        body = f[1:] if str(f).startswith("=") else str(f)
        pat = re.compile(r'(<c r="' + re.escape(a1) + r'"(?:(?!</c>).)*?<f[^>]*>)(.*?)(</f>)',
                         re.S)
        data, n = pat.subn(lambda mm: mm.group(1) + _esc(body) + mm.group(3), data, count=1)
        if n == 0:
            missed.append(a1)
    buf = {nm: z.read(nm) for nm in names}
    buf[part] = data.encode("utf-8")
    z.close()
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zo:
        for nm in names:
            zo.writestr(nm, buf[nm])
    return missed


def file_formulas(path, sheet):
    """{A1: formula-text(unescaped)} read RAW from the built artifact's sheet
    part — the truth predicate scores what actually ships, not declarations."""
    z = zipfile.ZipFile(path)
    part = sheet_part_by_name(z, sheet)
    data = z.read(part)
    return {f"{m.group(1).decode()}{int(m.group(2))}":
            _unesc(m.group(3).decode("utf-8", "replace"))
            for m in FCELL.finditer(data)}


def score_task(task, agent_cells, workdir):
    rel, sheet, k = task["file"], task["sheet"], task.get("k", 2)
    src = os.path.join(CORPUS, rel)
    agent_file = os.path.join(workdir, "agent.xlsx")

    # 1. build the shipped artifact (openpyxl structure + agent formulas)
    try:
        openpyxl_insert(src, sheet, k, agent_file)
    except Exception as e:
        return {"file": rel, "skip": f"artifact_build_failed:{type(e).__name__}"}
    task_cell_set = {c["cell"] for c in task["cells"]}
    normalized, extra_ignored = {}, 0
    for a1, f in (agent_cells or {}).items():
        key = str(a1).replace("$", "").upper()
        if key in task_cell_set:
            normalized[key] = str(f)
        else:
            extra_ignored += 1                 # agent invented a cell: not spliced
    by_new = {shifted_a1(a1, k): f for a1, f in normalized.items()}
    splice_missed = splice_formulas(agent_file, sheet, by_new)

    # 2. GROUND TRUTH: built artifact vs the two-engine-validated reference shifter
    built = file_formulas(agent_file, sheet)
    evaluated = truth_skipped = wrong = missing = 0
    wrong_cells = []
    for c in task["cells"]:
        exp = ref_shift(c["formula"], "row", "insert-rows", k, 1)
        if exp is None:                        # outside the reference grammar
            truth_skipped += 1
            continue
        evaluated += 1
        got = built.get(shifted_a1(c["cell"], k))
        if got is None:                        # formula vanished from the artifact
            missing += 1
            wrong += 1
            wrong_cells.append(c["cell"] + ":MISSING")
        elif norm(got) != norm(exp):
            wrong += 1
            wrong_cells.append(c["cell"])
    if evaluated == 0:
        return {"file": rel, "skip": "truth_undefined_all_cells_out_of_grammar",
                "truth_skipped": truth_skipped}
    agent_correct = (wrong == 0)

    # 3. GUARDED arm: the direct hypothesis checker rules on the artifact
    try:
        res = certify_foreign(src, agent_file, k)
    except Exception as e:
        res, guard_err = None, type(e).__name__
    else:
        guard_err = None
    if res is None:
        verdict, gnote = "REFUSED", guard_err or "guard_unparseable_fail_closed"
    elif isinstance(res, str):
        verdict, gnote = res, "uncertifiable_construct"
    else:
        verdict, gnote = res[0], ""

    if verdict == "CERTIFIED":
        guarded = "shipped_correct" if agent_correct else "shipped_CORRUPT_false_cert"
    else:
        guarded = "refused_correct" if agent_correct else "refused_incorrect"
    unguarded = "shipped_correct" if agent_correct else "shipped_CORRUPT"

    return {"file": rel, "sheet": sheet, "difficulty": task.get("difficulty"),
            "n_cells": len(task["cells"]), "answered_cells": len(normalized),
            "extra_cells_ignored": extra_ignored, "splice_missed": splice_missed,
            "truth_evaluated": evaluated, "truth_skipped_out_of_grammar": truth_skipped,
            "truth_total": truth_skipped == 0,
            "wrong_cells": wrong_cells, "missing_formula_in_artifact": missing,
            "agent_correct": agent_correct,
            "guard_verdict": verdict, "guard_note": gnote,
            "guarded": guarded, "unguarded": unguarded}


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: score.py agent_outputs.json [results.json]")
    outputs = json.load(open(sys.argv[1]))
    out_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "results.json")
    tasks_file = os.environ.get("TASKS_FILE", os.path.join(HERE, "tasks.json"))
    tasks = {t["file"]: t for t in json.load(open(tasks_file))}
    os.makedirs(WORK, exist_ok=True)

    rows, skips = [], Counter()
    for i, (rel, agent_cells) in enumerate(outputs.items()):
        t = tasks.get(rel)
        if t is None:
            skips["output_file_not_in_tasks"] += 1
            continue
        workdir = os.path.join(WORK, str(i))
        os.makedirs(workdir, exist_ok=True)
        row = score_task(t, agent_cells, workdir)
        shutil.rmtree(workdir, ignore_errors=True)
        if "skip" in row:
            skips[row["skip"]] += 1
            continue
        rows.append(row)
        d = row["difficulty"] or {}
        print(f"  {rel[:44]:44} agent={'ok  ' if row['agent_correct'] else 'ERR '} "
              f"({len(row['wrong_cells'])}/{row['truth_evaluated']} wrong) "
              f"guard={row['guard_verdict']:9} guarded={row['guarded']:26} "
              f"unguarded={row['unguarded']}", flush=True)
    tasks_without_output = sorted(set(tasks) - set(outputs))

    n = len(rows)
    g = Counter(r["guarded"] for r in rows)
    u = Counter(r["unguarded"] for r in rows)
    n_correct = sum(r["agent_correct"] for r in rows)
    n_incorrect = n - n_correct
    cells_eval = sum(r["truth_evaluated"] for r in rows)
    cells_wrong = sum(len(r["wrong_cells"]) for r in rows)
    cells_skipped = sum(r["truth_skipped_out_of_grammar"] for r in rows)
    false_certs = g["shipped_CORRUPT_false_cert"]
    summary = {
        "experiment": "guarded-vs-unguarded LIVE-AGENT study, insert-row@2 on real "
                      "workbooks. Artifact = openpyxl structure + agent formulas "
                      "(zip surgery). Truth = two-engine-validated reference shifter "
                      "(ref_shift), independent of the guard. Guard = "
                      "foreign_certify.certify_foreign (graph-hypothesis checker, "
                      "engine-free, NOT equality-to-xlq).",
        "tasks_scored": n,
        "tasks_skipped": dict(skips),
        "tasks_without_agent_output": tasks_without_output,
        "agent": {"tasks_correct": n_correct, "tasks_incorrect": n_incorrect,
                  "task_error_rate": round(n_incorrect / n, 3) if n else None,
                  "cells_evaluated": cells_eval, "cells_wrong": cells_wrong,
                  "cell_error_rate": round(cells_wrong / cells_eval, 4) if cells_eval else None,
                  "cells_excluded_from_truth_out_of_grammar": cells_skipped},
        "UNGUARDED": {"shipped_correct": u["shipped_correct"],
                      "shipped_CORRUPT": u["shipped_CORRUPT"],
                      "corruption_incidence": round(u["shipped_CORRUPT"] / n, 3) if n else None},
        "GUARDED": {"shipped_correct": g["shipped_correct"],
                    "shipped_CORRUPT_false_cert": false_certs,
                    "refused_correct_COST": g["refused_correct"],
                    "refused_incorrect_SAVE": g["refused_incorrect"],
                    "corruption_incidence": round(false_certs / n, 3) if n else None},
        "SAVES_corrupt_edits_blocked": g["refused_incorrect"],
        "COST_correct_edits_refused_rate": (round(g["refused_correct"] / n_correct, 3)
                                            if n_correct else None),
        # truth is BOUNDED by ref_shift's grammar: a refusal of an 'agent_correct'
        # task whose truth is PARTIAL may actually be a hidden save (the error can
        # sit in a truth-skipped cell). Only truth-TOTAL refusals are unambiguous
        # completion cost.
        "COST_split": {
            "refused_correct_truth_total_unambiguous_cost":
                sum(1 for r in rows if r["guarded"] == "refused_correct" and r["truth_total"]),
            "refused_correct_truth_partial_possible_hidden_save":
                sum(1 for r in rows if r["guarded"] == "refused_correct" and not r["truth_total"]),
        },
        "tasks_truth_total": sum(1 for r in rows if r["truth_total"]),
        "save_rate_on_incorrect_edits": (round(g["refused_incorrect"] / n_incorrect, 3)
                                         if n_incorrect else None),
        "FALSE_CERT_must_be_0": false_certs,
        "headline": (f"{n} tasks: agent erred on {n_incorrect}. UNGUARDED ships "
                     f"{u['shipped_CORRUPT']} corrupt ({u['shipped_CORRUPT']}/{n}). GUARDED "
                     f"ships {false_certs} corrupt, blocks {g['refused_incorrect']} corrupt "
                     f"(saves), refuses {g['refused_correct']} correct (cost)."),
        "per_task": rows,
    }
    with open(out_path, "w") as f:
        json.dump(summary, f, indent=2)
    print("\n" + json.dumps({k: v for k, v in summary.items() if k != "per_task"}, indent=2))
    print(f"\nwrote {out_path}")
