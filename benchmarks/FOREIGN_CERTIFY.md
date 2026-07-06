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

## Result (172 foreign edits)

| outcome | count |
|---|---:|
| **FALSE CERTIFICATIONS** (corrupted edit wrongly certified) | **0** |
| corrupted foreign edits CAUGHT (refused, engine-free) | **146 / 146 = recall 1.0** |
| faithful foreign edits certified | 3 |
| faithful foreign edits refused (conservative) | 20 |
| no-ref oracle disagreements (router provably correct) | 3 |

**Zero false certifications and 100% recall on genuine corruption** — the router,
without any engine, correctly refuses every foreign edit that mis-shifts a
reference, and never certifies a corrupted one. This is the certifier working as a
checker of untrusted work, on the anchor domain.

## Two things this run forced me to get right (both real)
1. **Extraction completeness is the TCB — demonstrated, not asserted.** The first
   run had **3 false certifications**. One (`tables.xlsx`) was genuine: its formulas
   use STRUCTURED TABLE references (`SUM(Table1[[#This Row],…])`) that my A1 regex
   cannot see, so a mangled table ref went undetected → wrongly certified. The sound
   fix is to REFUSE files with table references (exactly what xlq does via its
   residual gate). **A reference the extractor cannot see is a mis-shift it cannot
   catch** — so engine-free foreign certification is only as sound as the extractor
   is complete. This Python A1 extractor is a *proxy*; the production certifier must
   use xlq's full formula parser as the trusted base.
2. **Oracle disagreements ≠ router errors.** The other 2 (`ACCRINT`/`ACCRINTM`) have
   formulas with **no cell references** (pure constants) → shift-invariant → the
   router's CERTIFY is provably correct. The oracle flagged them only because
   LibreOffice computes ACCRINT differently than Excel's cache — an engine
   disagreement (same class as the earlier ROW/COLUMN oracle gap), not a router
   false certification. Separated out and reported as such.

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
