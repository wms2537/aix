# Expanded guarded-vs-unguarded study v3

Date: 2026-08-24. Protocol date: 2026-08-23.

## Scope

This is the expanded multi-operation harness validation requested by the live-agent
benchmark issue. It uses 100 real first-sheet workbooks drawn deterministically from
EUSES and Enron `converted_v2`, one structural operation per task. It does **not**
add a new live-model arm. The two agents below are deterministic synthetic probes of
the scorer, truth grammar, and guard.

The task protocol is recorded in [`tasks_v3.json`](tasks_v3.json) with seed
`20260823`. Its rules exclude shared-formula followers, volatile functions, empty or
uncertifiable formulas, and workbooks outside the 2-40 formula band. Duplicate source
files are allowed across distinct operations, but each `(file, operation, position)`
tuple is unique. Deterministic generation preceded smoke scoring; the task file was
not separately committed before scoring, so this is not an external pre-registration
claim.

From 3,091 qualifying candidates, the selector targeted 20 tasks for each of five
operation variants and retained 100 after shuffling. The resulting mix is 40
`insert-rows` (two positions), 20 `delete-rows`, 20 `insert-cols`, and 20
`delete-cols`; 89 tasks are EUSES and 11 are Enron. Eligibility skipped 4,728 unique
files: no formulas 3,467, size band 784, uncertifiable formula 346, volatile function
91, empty formula body 32, and shared-formula follower 8.

## Instruments

- **Artifact:** openpyxl performs the structural edit; zip surgery splices only the
  agent-supplied formula bodies.
- **Truth:** the independent reference grammar decides whether each moved formula is
  correct. Cells outside its grammar are counted, not guessed. Deleted hosts are
  excluded from truth.
- **Guard:** the engine-free graph checker certifies the observed dependency
  transformation or refuses. A deleted dependency, parse failure, or checker exception
  refuses fail-closed.

The scorer never treats agreement with xlq as ground truth.

## Aggregate results

| Metric | Perfect agent | Sloppy agent |
|---|---:|---:|
| Tasks scored | 100 | 100 |
| Agent errors | 3 | 17 |
| Unguarded corrupt shipments | 3 | 17 |
| Guarded false certifications | **0** | **0** |
| Guarded incorrect saves | 3 | 17 |
| Guarded refusals of correct work | 77 | 69 |

Across both 100-task arms, all 20 incorrect artifacts were refused and no certified
artifact contained a truth-visible corruption.

## Per-operation results

### Perfect agent

| Operation | Tasks | Unguarded corrupt | False certs | Incorrect saves | Correct refusals |
|---|---:|---:|---:|---:|---:|
| Insert rows | 40 | 1 | 0 | 1 | 29 |
| Delete rows | 20 | 0 | 0 | 0 | 17 |
| Insert cols | 20 | 0 | 0 | 0 | 18 |
| Delete cols | 20 | 2 | 0 | 2 | 13 |

### Sloppy agent

| Operation | Tasks | Unguarded corrupt | False certs | Incorrect saves | Correct refusals |
|---|---:|---:|---:|---:|---:|
| Insert rows | 40 | 8 | 0 | 8 | 25 |
| Delete rows | 20 | 4 | 0 | 4 | 15 |
| Insert cols | 20 | 1 | 0 | 1 | 17 |
| Delete cols | 20 | 4 | 0 | 4 | 12 |

The sloppy agent leaves 10% of truth-visible shifts unchanged using seed 42. Error
counts therefore measure the harness response to known corruption, not a natural LLM
error distribution.

## Difficulty slices

Agent-error counts by binary difficulty flag were:

| Slice | Perfect errors / tasks | Sloppy errors / tasks |
|---|---:|---:|
| Absolute references present | 0 / 12 | 2 / 12 |
| Ranges present | 3 / 73 | 9 / 73 |
| EUSES | 2 / 89 | 13 / 89 |
| Enron | 1 / 11 | 4 / 11 |

These slices are observational. The sample was balanced by operation, not by corpus,
absolute-reference use, or range use.

## Honest limits

1. **No expanded live arm.** The existing careful/hasty arms remain 21-task,
   single-operation studies. New live runs require fresh explicit model/API
   authorization.
2. **Truth-grammar coverage bounds the false-certification claim.** As in prior
   versions, zero false certifications means zero on cells the reference grammar can
   rule on. Certified tasks may contain out-of-grammar cells invisible to truth.
3. **Selection is guard-compatible.** Excluding constructs the guard cannot model
   makes measured refusal cost conservative relative to unrestricted real workbooks;
   it also means these 100 tasks do not estimate population-wide certification rate.
4. **Text equality is strict.** Normalized formula text must match the independently
   shifted reference; semantically equivalent rewrites score as wrong.
5. **openpyxl is part of the artifact path.** Builder round-trip behavior affects both
   arms equally, but may affect absolute scores.
6. **Deleted bands are asymmetric by design.** Deleted host cells are skipped by truth,
   while deleted dependencies make the guard refuse. That distinction prevents silent
   deletion semantics from being scored as correct.

## Reproduction

```bash
python3 benchmarks/agent_study/synthetic_v3.py perfect
python3 benchmarks/agent_study/score_v3.py \
  benchmarks/agent_study/outputs_v3_perfect.json \
  benchmarks/agent_study/results_v3_smoke_perfect.json

python3 benchmarks/agent_study/synthetic_v3.py sloppy
python3 benchmarks/agent_study/score_v3.py \
  benchmarks/agent_study/outputs_v3_sloppy.json \
  benchmarks/agent_study/results_v3_smoke_sloppy.json
```

Task regeneration scans the full local corpus and can take substantial time because
each candidate undergoes an openpyxl host-preservation probe. Do not regenerate before
scoring unless intentionally creating a new protocol version.
