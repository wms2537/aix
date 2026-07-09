#!/usr/bin/env python3
"""SYNTHETIC agent outputs for smoke-testing score.py end-to-end (no LLM).

  perfect  every task cell gets the CORRECT shifted formula:
             - in-grammar cells   -> ref_shift(formula) (the reference shift);
             - out-of-grammar     -> xlq's proven transform for that cell (xlq
               plays the AGENT here, nothing else: it is not the truth and not
               the guard, so no circularity — score.py never sees xlq);
             - if xlq refuses the file -> original formula unchanged (counted).
  sloppy   start from perfect, then leave ~10% of the IN-GRAMMAR cells WHOSE
           CORRECT SHIFT DIFFERS from the original (so an unshifted copy is a
           truth-visible corruption) unshifted — seeded RNG (42), whole task set.

This smoke-tests the HARNESS, not any agent: perfect must yield 0 corruption in
both arms (and near-0 guard refusals); sloppy must show unguarded corruption
that the guarded arm refuses (saves), with 0 false certs.

usage: synthetic_agents.py perfect|sloppy [tasks.json] [out.json]
"""
import json, os, random, shutil, subprocess, sys, tempfile

BENCH = "/home/soh/aix/benchmarks"
sys.path.insert(0, BENCH)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shift_correctness_real import ref_shift, norm   # noqa: E402
from score import file_formulas, shifted_a1          # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = "/home/soh/aix/vendor/upstream/xlsx/tests"
XLQ = "/home/soh/aix/xlq/target/release/xlq"
SLOPPY_RATE = 0.10
SEED = 42


def xlq_shift_map(rel, sheet, k):
    """{new_A1: formula} of xlq's restructured file, or None if xlq refused."""
    src = os.path.join(CORPUS, rel)
    with tempfile.TemporaryDirectory() as td:
        dst = os.path.join(td, "x.xlsx")
        shutil.copy(src, dst)
        r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op",
                            "insert-rows", "--at", str(k), "--count", "1",
                            "--actor", "syn"], capture_output=True, text=True)
        if '"rev"' not in r.stdout:
            return None
        return file_formulas(dst, sheet)


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "perfect"
    tasks_file = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "tasks.json")
    out_file = sys.argv[3] if len(sys.argv) > 3 else os.path.join(
        HERE, f"agent_outputs_{mode}.json")
    assert mode in ("perfect", "sloppy"), mode
    tasks = json.load(open(tasks_file))

    outputs, shiftable = {}, []
    n_xlq = n_fallback = 0
    for t in tasks:
        k = t.get("k", 2)
        xmap = None
        cells = {}
        for c in t["cells"]:
            exp = ref_shift(c["formula"], "row", "insert-rows", k, 1)
            if exp is not None:
                cells[c["cell"]] = exp
                if norm(exp) != norm(c["formula"]):
                    shiftable.append((t["file"], c["cell"], c["formula"]))
                continue
            # out of the reference grammar: the synthetic agent uses xlq's
            # proven transform (agent-side only; scoring never sees xlq)
            if xmap is None:
                xmap = xlq_shift_map(t["file"], t["sheet"], k) or {}
            xf = xmap.get(shifted_a1(c["cell"], k))
            if xf is not None:
                cells[c["cell"]] = xf
                n_xlq += 1
            else:
                cells[c["cell"]] = c["formula"]          # xlq refused: unchanged
                n_fallback += 1
        outputs[t["file"]] = cells

    n_slop = 0
    if mode == "sloppy":
        rng = random.Random(SEED)
        n_slop = max(1, round(SLOPPY_RATE * len(shiftable)))
        for f, a1, orig in rng.sample(shiftable, n_slop):
            outputs[f][a1] = orig                        # left UNSHIFTED -> corrupt

    with open(out_file, "w") as fh:
        json.dump(outputs, fh, indent=2)
    print(f"{mode}: {len(outputs)} tasks, {sum(len(v) for v in outputs.values())} cells "
          f"({n_xlq} via xlq for out-of-grammar, {n_fallback} fallback-unchanged), "
          f"{len(shiftable)} truth-visible shift cells, {n_slop} left unshifted "
          f"-> {out_file}")
