# Locked test v2 — results and analysis (iteration 5)

**Date:** 2026-07-10 · **Phase:** 5 · **Cycle:** 1 · **Iteration:** 5 · **Status:** completed

## Ledger scoring (10 pre-registered predictions: 5 confirm / 3 disconfirm / 2 partial)

| Row | Predicted | Actual | Signal |
|---|---|---|---|
| v2-shift-both | 0 MM (high) | **0 / 1,006,997 cells, 5 ops** (EUSES-full 316,746; Enron-random 690,251; cross-sheet grammar; post-fix binary) | **confirm** |
| v2-prevalence-euses | <0.693 (low) | **0.946** | **disconfirm** — full corpus is MORE formula-rich than the database category |
| v2-prevalence-enron | 0.79–0.99 (low-med) | 0.9116 | **confirm** |
| v2-guard-falsecert | 0 (high) | **0** (opx: 496+356 REFUSED; 7 edit-fails; 3 timeouts) | **confirm** |
| v2-cost-corrected | EUSES≤14%, Enron≤29% (med) | EUSES 21.2% (106/500), Enron 34.3% (124/362) | **disconfirm** — artifact classes DID vanish (0 ERROR; robust JSON + name decode worked) but the fuller samples carry more denylist parts |
| v2-extlink-share | 40–60% (med) | 77.7% of refusals contain externalLinks; 64% sole-cause | **partial** — direction strongly confirmed, magnitude above range |
| v2-q-euses | 0.478±0.15 (low) | 0.361 (4,432 files), k999=18 | **confirm** |
| v2-q-enron | 0.566±0.10 (low) | 0.635 (761 files), **k999=237** | **confirm** (edge of band; the tail finding is the headline) |
| v2-agents | 0 false certs; mid errs less (med) | 0 false certs ✓; mid 1 > fast 0 ✗; refused_correct 5→0 | **partial** |
| v2-dbt-extras | 20–60% each (low) | 0.0% / 13.7%; certify legs unevaluable | **disconfirm** |

## Headline findings

1. **One million cells, zero errors.** After the three verification-driven fixes
   (encoding, qualifier guard, range-head), xlq's shift is correct on 1,006,997 real
   formula cells across five ops and two corpora — including the widened cross-sheet
   grammar (Enron skips fell from >checked to ~15% of checked).
2. **The central claim held again**: 0 false certifications on 852 fresh foreign
   edits; the new fail-closed guard refused 3 real non-ASCII-qualifier EUSES files.
3. **The probabilistic tier collapses in the tail**: Enron-random k999 = 237 checked
   cells (near-check-blind files exist in the wild) — sampling-based value checking
   cannot reach high confidence on real business data; the exact tier is not an
   optimization but a necessity.
4. **The cost is real, structural, and levered**: 21.2%/34.3% fail-closed;
   measurement artifacts eliminated (prediction confirmed mechanically: ERROR class
   = 0); externalLinks are the single dominant cause (64% sole-cause on Enron) —
   verifying that one part class would roughly halve Enron's cost.
5. **A verification-driven fix reduced measured cost**: agent-study refusals of
   correct work fell 5 → 0 after the range-head fix found by the Lean differential.
6. **dbt coverage does not transfer** to macro-heavy production projects (0%/13.7%)
   — the format-parametric claim holds at the theory level; the current adapter
   subset does not. Reported plainly.

## Seven questions (delta from 017)

1. Did it work? Yes — every soundness prediction confirmed at larger scale on the
   fixed binary. 2. Why? Fail-closed architecture + the verified trusted layer.
3. What contributed most? The v2 grammar (4× checked cells) + the three fixes.
4. Robust/where fails? Cost concentrated in externalLinks/charts; dbt adapter
   subset. 5. Surprising? k999=237; prevalence disconfirm; dbt collapse; sonnet's
   invented-argument error class. 6. Literature? unchanged (015). 7. Problem?
   PROBLEM.md's proxy caveat now discharged twice, at two scales, with predictions.

## Budget

Iterations 5/5 spent. Diminishing returns: not applicable (v2 targeted validity,
not metric). CONCLUDE: no further iterations without user grant.

## Decision

Fold v1+v2 into the paper as the two-locked-tests evidence architecture; update §3
(CopyEdits, Tokenizer), §4 (verified reference + differential), §5 (v2 section),
§6 (third defect + cost-reduction), abstract + claims. Then final draft to user.
