# Reproduction package for `paper/paper-v3.md`

One command re-derives every quantitative claim in the paper from the
**committed artifacts** — fast, deterministic, no experiments re-run:

```
python3 repro/verify_claims.py
```

Exit code 0 iff every claim PASSes. The script prints a PASS/FAIL/SKIP table
of **102 claims** (94 artifact claims spanning §4, every §5 subsection, and
§7, plus 8 formal-proof checks), each row showing the paper's number, the
value extracted from the committed artifact, and the artifact path.

## What "verify" means here — two tiers

1. **Artifact verification (94 claims, always runs).** Every number in
   §5.1–§5.9, the §4 tokenizer-validation counts, and the §7 coverage figures
   is re-extracted from the committed JSON/TSV/MD artifact it came from
   (`benchmarks/*.json`, `benchmarks/agent_study/results_*.json`,
   `experiments/generality/composition_coverage.json`,
   `experiments/dbt/dbt_results.json`, `benchmarks/inthewild_*.json`,
   `benchmarks/euses_38_mismatch_classification.json`, `results.tsv` itw-*
   rows, `benchmarks/EDIT_PATH_AB.md`). Derived figures (e.g. 283,960 locked
   cells; the 19.6%/32.0% fail-closed cost; the 30×/2×10⁴ mixture-vs-naive
   amplification; the 518 = 503+15 certify-call accounting) are recomputed
   from raw artifact fields, not read off a headline string.
2. **Formal re-checking (8 claims, live when the toolchain is present).**
   These do not trust a committed output — they re-run the proof checker:
   - The four Lean files (`formal/SelfOracle.lean`, `RefShift.lean`,
     `Checker.lean`, `Impossibility.lean`) are re-compiled with
     `#print axioms` appended for each named theorem; the check asserts exit
     0, no `sorry`/`sorryAx`, and axioms ⊆ `[propext, Quot.sound]`. The
     `Checker.lean` `#eval` demos must print `true / false / false`.
     Needs `lean` (elan; the paper used Lean 4.31.0). SKIP if absent.
   - `formal/differential_check.py` (seeded, deterministic) re-runs the
     Lean-vs-deployed-checker battery and must report `agreement: 30/30`.
     Needs `lean`. SKIP if absent.
   - `formal/shift_laws.py` re-proves the Z3 shift-algebra laws (including
     the §5.2b move-σ bijection) if the `z3-solver` Python package is
     importable; otherwise the six base laws are verified against the
     committed `formal/shift_laws.out.txt` and the move-σ claim is SKIPped
     (that committed output predates the move laws).

SKIP means "not verified on this machine", never "assumed true"; the exit
code is 0 only when there are no FAILs (SKIPs are listed separately).

## Environment

- Python 3.9+ (stdlib only) for the artifact tier.
- Optional, to eliminate the SKIPs:
  - Lean 4 via elan (`~/.elan/bin` is added to PATH automatically):
    `curl https://elan.lean-lang.org/elan-init.sh -sSf | sh`
  - `pip install z3-solver` (a venv is fine; run the script with that python).
- Run from anywhere; paths resolve relative to the repo root
  (`repro/..`). Note `formal/differential_check.py` imports
  `experiments/generality/{core,router}.py` via an absolute repo path
  (`/home/soh/aix`) — on a differently-located checkout that one check needs
  the path at the top of that script adjusted.

Verified state at package creation: **102/102 PASS** with lean + z3
available; **101 PASS / 1 SKIP** (the move-σ Z3 claim) without z3. No paper
number failed to reproduce. A mutation test (deliberately corrupting an
expected value) was run to confirm the harness fails loudly (exit 1) rather
than passing vacuously.

## What this package does NOT re-run (and the exact commands that would)

The committed artifacts were produced by experiments with heavier
requirements. Re-generating them is deliberately out of scope for
`verify_claims.py`; the commands below re-run them (writing the same JSON
paths, which `verify_claims.py` then re-verifies).

Requirements legend: **[xlq]** release build (`cd xlq && cargo build
--release`), **[LO]** LibreOffice `soffice` headless, **[opx]** python3 +
`openpyxl`, **[fml]** the pure-Python `formulas` engine, **[LLM]** live
agent access, **[corpus]** external data download.

| Paper | Artifact | Re-run command | Needs |
|---|---|---|---|
| §5.1 | `benchmarks/foreign_certify.json` | `python3 benchmarks/foreign_certify.py` | xlq, LO, opx |
| §5.2 A/B | `benchmarks/agent_ab.json` | `python3 benchmarks/agent_ab.py` (sharded; see `agent_ab_measure.py`) | xlq, LO, opx |
| §5.2 4-op×2-engine | `benchmarks/agent_ab_v2.json` | `python3 benchmarks/agent_ab_v2.py` | xlq, LO, opx, fml |
| §5.2 real-corpus shift | `benchmarks/shift_correctness_real.json` | `python3 benchmarks/shift_correctness_real.py` | xlq, opx (deterministic) |
| §5.2b move-rows | `benchmarks/move_correctness_real.json` | `python3 benchmarks/move_correctness_real.py` | xlq (deterministic) |
| §5.3 confusion matrix | `benchmarks/cert_confusion_v2.json` | `python3 benchmarks/cert_confusion_v2.py` | xlq, LO, opx |
| §5.3 non-cell exploit | (behavioral; no JSON) | `python3 benchmarks/cert_noncell_test.py` | xlq, opx |
| §5.4 agent outputs | `benchmarks/agent_outputs_all.json` | live-agent harness (see `benchmarks/LIVE_3WAY.md`) | LLM |
| §5.5 composition | `experiments/generality/composition_coverage.json` | `python3 experiments/generality/composition_coverage.py` | python3 only |
| §5.6 dbt demo | `experiments/dbt/dbt_results.json` | `python experiments/dbt/demo_dbt.py` | duckdb (oracle materialization) |
| §5.7 agent study | `benchmarks/agent_study/results_*.json` | `agent_study/prep.py` → agent runs → `agent_study/score.py` (see its README) | LLM, xlq, opx |
| §5.8 q + bounds | `benchmarks/coincidence_q.json` | `python3 benchmarks/coincidence_q.py` | opx (deterministic) |
| §5.8 Monte-Carlo | `benchmarks/coincidence_mc.json` | `python3 benchmarks/coincidence_mc.py` | opx (seeded) |
| §5.8 in-the-wild q | `benchmarks/coincidence_q_{euses,enron}.json` | `python3 benchmarks/coincidence_q.py <corpus_dir> <out.json>` | corpus |
| §5.9 locked xlsx legs | `benchmarks/inthewild_{euses,enron}.json` | `python3 benchmarks/inthewild_run.py <corpus_dir> <out.json>` | xlq, opx, corpus |
| §5.9 locked dbt leg | `benchmarks/inthewild_dbt.json` | `python3 experiments/dbt/inthewild_dbt.py` | Mattermost repo checkout |
| §4 tokenizer fuzz | `benchmarks/tokenizer_fuzz.json` | `python3 benchmarks/tokenizer_fuzz.py` | xlq |
| §4 conformance v1/v2 | `benchmarks/tokenizer_conformance.json`, `conformance_v2.json` | `python3 benchmarks/tokenizer_conformance.py`; `python3 benchmarks/conformance_v2.py` | xlq, LO, opx, fml |
| §7 edit distribution | `benchmarks/edit_distribution.json` | committed study input (task-coding; no script re-run) | — |
| §3 Lean theorems | `formal/*.lean` | `lean formal/<file>.lean` (re-run live by this package) | lean |
| §3 Z3 laws | `formal/shift_laws.out.txt` | `python3 formal/shift_laws.py` (re-run live when z3 present) | z3-solver |
| §4.i differential | (stdout 30/30) | `python3 formal/differential_check.py` (re-run live by this package) | lean |

Two caveats stated plainly:

- **The §5.9 locked test is pre-registered, run-once** (research-log/016).
  Re-running `inthewild_run.py` is an *audit*, not a fresh locked test — the
  systems under test have since been fixed (the UTF-8 double-encoding defect
  and the CJK-qualifier fail-close of §6), so a re-run on current binaries
  will legitimately differ from the frozen committed numbers (fewer than 38
  EUSES mismatches, more refusals).
- **Anything oracle-adjudicated by LibreOffice** (`agent_ab*`,
  `cert_confusion*`, `conformance*`, `foreign_certify`) depends on the local
  LibreOffice version; the committed numbers were produced with the version
  frozen during the study. The deterministic checks
  (`shift_correctness_real`, `move_correctness_real`, `coincidence_q`,
  `inthewild_run` legs 1–2) have no engine in the loop and reproduce exactly.

## Claim coverage by paper section

| Section | Claims | Artifacts |
|---|---|---|
| §5.1 | 4 | `foreign_certify.json` |
| §5.2 | 19 | `agent_ab.json`, `EDIT_PATH_AB.md`, `agent_ab_v2.json`, `shift_correctness_real.json` |
| §5.2b | 2 (+1 formal) | `move_correctness_real.json`, `shift_laws.py` |
| §5.3 | 5 | `cert_confusion_v2.json`, `cert_confusion.json` |
| §5.4 | 1 | `agent_outputs_all.json` |
| §5.5 | 4 | `composition_coverage.json` |
| §5.6 | 5 | `dbt_results.json` |
| §5.7 | 11 | `agent_study/results_{live_careful,live_hasty,smoke_perfect,smoke_sloppy}.json` |
| §5.8 | 19 | `coincidence_q.json`, `coincidence_mc.json`, `coincidence_q_{euses,enron}.json` |
| §5.9 | 19 | `inthewild_{euses,enron,dbt}.json`, `euses_38_mismatch_classification.json`, `results.tsv` |
| §4 (TCB) | 3 | `tokenizer_fuzz.json`, `tokenizer_conformance.json`, `conformance_v2.json` |
| §7 | 2 | `edit_distribution.json`, `composition_coverage.json` |
| §3 formal | 8 | four `formal/*.lean` (live), `shift_laws.py`/`.out.txt`, `differential_check.py` (live) |
