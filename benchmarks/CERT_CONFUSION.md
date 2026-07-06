# `xlq certify` validated against an INDEPENDENT oracle (non-circular)

The live-agent slice was circular (guard predicate == ground-truth predicate). This is
the non-circular test the reviewer asked for: `xlq certify`'s CERTIFY/REFUSE verdicts
adjudicated by an engine independent of xlq — LibreOffice recompute vs Excel's cache.

## Setup (`cert_confusion.py`)
For each real workbook, two edits of insert-row@2:
- **FAITHFUL arm** = `xlq restructure` (xlq's proven transform).
- **CORRUPTED arm** = `openpyxl.insert_rows` (leaves references unshifted).

Each edit is judged by `xlq certify` **and** independently labeled by LibreOffice
(recompute vs the original Excel cache). Positive class = "truly corrupted" (guard
should REFUSE). **Oracle-reliability gate:** a file is scored only if LibreOffice
reproduces Excel's cached values on the *unedited* original — otherwise LO's engine
disagrees with Excel (ACCRINT, BESSEL, …) and cannot adjudicate; such files are skipped
so engine disagreement is never miscounted as a certify failure.

## Confusion matrix (29 edits over 16 oracle-reliable workbooks)

| | oracle: corrupted | oracle: faithful |
|---|---:|---:|
| **certify REFUSED**   | TP = **14** | FP = 2 |
| **certify CERTIFIED** | **FN = 0** | TN = 13 |

- **False-certification rate = 0/14 = 0.0** — the soundness-critical cell. `xlq certify`
  never certified a corrupted edit, judged by an **independent** engine. This is the
  non-circular evidence the live-agent slice could not provide.
- **Recall on corruption = 14/14 = 1.0** — every openpyxl-corrupted edit was refused.
- **False-refusal rate = 2/15 = 0.13** — two faithful edits were refused
  (over-conservative: openpyxl introduced incidental non-shift changes certify caught).
  Sound (fail-closed) but imprecise; a utility cost, not a soundness one.

## Honest scope
- The FAITHFUL arm is xlq-produced, so its CERTIFY side is not fully independent (xlq
  certifies its own transform); the **soundness-critical FN cell is fully independent**
  (corrupted arm = openpyxl, truth = LibreOffice, verdict = xlq certify).
- One op (insert-row@2), one oracle engine, 16 oracle-reliable workbooks (14 excluded
  for LO≠Excel engine disagreement — a limitation of the *oracle*, not the certifier).
- Together with the fuzz-validated tokenizer (`tokenizer_fuzz`: 175 pairs, 0
  disagreements) and `foreign_certify` (147/147 engine-free), this gives the certifier
  soundness evidence on both a validated TCB and an independent oracle.
