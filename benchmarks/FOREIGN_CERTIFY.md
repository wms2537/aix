# The verifiability thesis, tested: certifying UNTRUSTED FOREIGN edits (engine-free)

The chair's deepest hole in the edit-path A/B: xlq **authored and self-certified**
its edits, so the certifier was never tested as a checker of *untrusted foreign*
work — which is the entire moat claim. This closes that gap on the spreadsheet path.

## Setup (`foreign_certify.py`)
- Build the reference-dependency graph (formula skeleton + ordered ref tokens +
  cache) of the ORIGINAL and of a **foreign-edited** file — openpyxl's `insert_rows`
  output, which **xlq did not produce**.
- Run the SAME router (`experiments/generality/router.certify_edit`) with
  σ = insert-row@2. CERTIFY iff the foreign graph is the σ-relabeling of the
  original; REFUSE any unaccounted difference. **No engine is run.**
- Cross-check every verdict against the INDEPENDENT LibreOffice oracle labels from
  the edit-path A/B (`agent_ab.json`).

## Result (172 foreign edits, after the fail-closed hardening below)

| outcome | count |
|---|---:|
| **FALSE CERTIFICATIONS** (corrupted edit wrongly certified) | **0 (robust)** |
| corrupted foreign edits CAUGHT (refused, engine-free) | **147 / 147 = recall 1.0** |
| faithful foreign edits certified | 1 |
| faithful foreign edits refused (conservative) | 22 |
| no-ref oracle disagreements (router provably correct, ref-free) | 2 |

**Zero false certifications and 100% recall on genuine corruption** — the router,
without any engine, never certifies a corrupted foreign edit. This is the certifier
working as a sound checker of untrusted work on the anchor domain. But read the
utility caveat: it now certifies almost nothing (1 of 23 faithful foreign edits) —
soundness here is bought with heavy refusal, and *useful* soundness needs a complete
parser (below).

## Two adversarial rounds this survived (the interesting part)
**Round 1** had 3 apparent false certifications: 1 genuine (`tables.xlsx`, structured
table refs my A1 regex can't see) and 2 oracle noise (`ACCRINT`, ref-free constants
where LibreOffice ≠ Excel). I refused table refs and separated the oracle noise.

**Round 2 (an adversarial reviewer) broke that fix and found the real hole.** The
`[`-only table gate was far too narrow: **whole-row (`SUM(6:6)`), whole-column
(`A:A`), cross-sheet (`Sheet2!A5`), and defined-name (`=NC_1+NC_2`) references are
ALL invisible to the A1 regex** — so a mis-shifted one is silently CERTIFIED. Worse,
when such a ref was the only one, `has_any_cell_ref` returned false and the false
certification was *laundered* into the "no-ref → router provably correct" bucket.
The reviewer proved it on `defined_names.xlsx` (`=NC_1+NC_2` has references; the
router was blind to them, not proving them invariant) and gave the exact trigger
`=A1 + SUM(6:6)`.

**Fix (fail-closed completeness gate):** REFUSE any file whose formulas contain a
reference form the extractor cannot fully model — whole-row, whole-column,
cross-sheet, table, or a bare defined-name identifier. After it: 0 false
certifications is **robust** (the trigger and all named files now REFUSE), and
`defined_names.xlsx` is a sound refusal, not a laundered "correct." The 2 remaining
no-ref cases are genuinely ref-free `ACCRINT` constants (router provably correct).

## The real lesson: soundness is cheap, USEFUL soundness needs the complete TCB
The fail-closed gate keeps the certifier sound, but it now certifies only **1 of 23**
faithful foreign edits — it refuses almost everything with a non-trivial reference
form. That is the point, made empirical: **a reference the extractor cannot see is a
mis-shift it cannot catch, so it must refuse it.** This A1 proxy stays sound only by
refusing the reference forms it can't model; converting those refusals into faithful
certifications requires a *complete* reference parser that models and shifts
whole-row/whole-column/cross-sheet/defined-name/table refs — i.e. xlq's real formula
engine, which is the actual trusted base (TCB). The gap between "refuses everything"
and "certifies faithful edits" is exactly the completeness of that trusted parser.

## Honest scope
- The foreign edits are **openpyxl's**, not a live LLM's varied mistakes — but they
  ARE genuinely foreign (xlq did not author them), which is the point the chair
  raised: the certifier now rules on edits it did not make, with 0 false
  certifications. A live-LLM slice (varied errors + task-completion scoring) remains
  the stronger, still-open step.
- Utility caveat (not soundness): the router refuses 20 *faithful* foreign edits
  conservatively (mostly table refs + incidental openpyxl rewrites), so its precision
  on foreign faithful edits is lower than on xlq's own clean edits (87% in the A/B).
  Soundness (0 false certifications) is what the thesis needs; precision improves with
  a complete parser and cleaner edits.
