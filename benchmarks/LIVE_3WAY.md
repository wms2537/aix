# Live-agent 3-way: a real agent's edits through `xlq certify` (the interventional slice)

The chair's gate: not openpyxl's one deterministic bug, but a REAL agent's own varied
edits, routed through the certifier, scored for task-completion / safe-refusal /
silent-corruption. This is that experiment.

## Setup
- **Agent:** a fast model (Claude Haiku) is given a workbook's formulas and the task
  "insert a blank row at row 2; rewrite each formula so it still computes the same
  value." It returns its own corrected formulas — its own mistakes, not a library's.
- **20 real workbooks** (vendored calc-tests: DATE, ENGINEERING, FINANCIAL, DATABASE,
  MATH, TEXT), spanning easy (`DATE(A2,B2,C2)`) to hard (`DCOUNT($A$1:$H$11,$F$1,A14:A15)`
  — 75 mixed absolute/relative refs across many rows).
- **Guard:** the real `xlq certify` (new subcommand) — applies xlq's proven structural
  transform to the original and diffs it against the agent's file; CERTIFY iff they
  agree, else REFUSE. Engine-free.
- **Ground truth:** xlq's transform, independently validated (Theorem 1 + forward-
  correctness on 150 real workbooks). No LibreOffice (it disagrees with Excel on
  BESSEL/ERF/ACCRINT — an oracle we deliberately avoid here).

## The headline finding: the experiment hardened the TCB
Comparing the agent's output to xlq's surfaced **two real reference-shift bugs in xlq
itself** — the certifier's trusted base:
1. **`BIN2DEC` → `BIN3DEC`.** xlq's tokenizer read the `BIN2` in the function name
   `BIN2DEC` as a cell ref (col BIN, row 2) and a row insert rewrote it — silent
   corruption of every `BIN2*`/`OCT2*`/`HEX2*` formula. The agent got it right; xlq was
   wrong. Fixed: a ref followed by a letter is an identifier head, not a ref.
2. **`Sales2020` (defined name) shifted.** A >3-letter "column" scanned as a ref. Fixed:
   a valid column is 1–3 letters.
Both now have regression tests; full suite 192/192. **The interventional test found bugs
the 150-file forward-correctness sweep did not** — because those corpora lacked
digit-bearing function names.

## The 3-way result (after fixing xlq)

| | count |
|---|---:|
| Workbooks | 20 |
| Agent correct (matches xlq's proven shift) | **19 / 20** |
| **GUARDED — task completed & certified** | **19** |
| **GUARDED — safely refused (agent erred → caught)** | **1** |
| **GUARDED — false certification (silent corruption)** | **0** |
| UNGUARDED — would ship the error silently | 1 |

The one agent error (`COMPLEXs.xlsx`): the agent dropped a `=""` empty-string formula,
returning an empty formula. Minor, but a genuine corruption — and the guard **refused
it** rather than let it ship. Every correct edit was certified; the one wrong edit was
refused; **nothing was silently certified wrong.**

## Honest scope
- Haiku is a strong reference-shifter (19/20), so the error sample is thin (n=1) — the
  guard's error-catching at scale is better evidenced by `foreign_certify` (147/147
  corrupted foreign edits refused, 0 false certs). This slice's larger contribution is
  (a) confirming the certifier certifies real correct agent edits and (b) **finding two
  real xlq bugs** — the interventional test earned its keep by hardening the TCB.
- Single op (insert-row@2); the agent edits a curated subset of each sheet's formulas;
  ground truth is xlq's transform (a proven reference, not an independent engine).
