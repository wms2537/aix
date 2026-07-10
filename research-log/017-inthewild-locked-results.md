# Locked in-the-wild test — results and Phase 5 analysis

**Date:** 2026-07-10 · **Phase:** 5 (Path C step 1 complete) · **Cycle:** 1 · **Iteration:** 3 · **Status:** completed

## Context

Pre-registered in 016 (protocol + 9 prediction rows committed before download).
Corpora: EUSES (796 converted, CC-BY-4.0, MD5-verified vs Zenodo canonical), Enron
(786 converted, CC0-1.0), Mattermost dbt (real production, substitution from
GitLab-went-private disclosed). Run once; harness artifacts fixed WITH DISCLOSURE
mid-run (three: self-closing-cell misattribution; XML entity decoding; per-file
watchdog after a 2h hang); the systems under test were never modified. Dev-tier
numbers re-verified IDENTICAL after each harness fix (1651/1608/1648/1076, 0
mismatches).

## Results vs pre-registered predictions (results.tsv rows filled)

| Row | Predicted | Actual | Signal |
|---|---|---|---|
| itw-shift-euses | 0 mismatches (high) | 38/113,164 (0.034%) — ALL one real xlq defect: UTF-8 double-encoding of non-ASCII string literals (1 file, 4 ops); ZERO reference-shift errors | **disconfirm** — real defect found |
| itw-shift-enron | 0 mismatches (high) | **0/170,796** | **confirm** |
| itw-residual-rate | 20–50% (low) | transform-level: EUSES 1.8%, Enron 10.5%; full guarded pipeline (transform refused ∪ own-certify not CERTIFIED): EUSES 19.6%, Enron 32.0% | **partial** (definition-dependent; both recorded) |
| itw-prevalence | 80–95% (med) | EUSES 69.3%; Enron 89.2% | **partial** (Enron in range, EUSES below) |
| itw-guard-falsecert | 0 (high) | **0** across 503 certify(openpyxl-edit) calls (158 EUSES + 345 Enron REFUSED) | **confirm** — the central claim |
| itw-guard-ownrefuse | 0 (high) | EUSES 20 REFUSED (`unverified_reference_part` denylist) + 9 ERROR (loader); Enron 75 + 3 | **disconfirm** — fail-closed COST 17.8%/21.5%, not unsoundness |
| itw-q-m2v | ≥ 0.178 (low-med) | EUSES pooled 0.478 (k999=12); Enron 0.566, median 0.245, **k999=20** | **confirm**, decisively |
| itw-dbt-coverage | <30% (low-med) | 40.2% parse / 31.1% closed (254 models) | **disconfirm** (favorable) |
| itw-dbt-verdicts | both-correct (high) | faithful rename CERTIFIED; dangling botch REFUSED | **confirm** |

## The seven Phase 5 questions

1. **Did it work?** Yes at the stated scope. The central soundness claim — zero false
   certifications — held on 503 foreign edits across two corpora development never
   touched. xlq's reference-shift arithmetic: 0 errors in 283,960 checked cells. One
   real defect found (below) — in the trusted byte layer, not the proven core.
2. **Why did it work?** The fail-closed architecture: everything the checker cannot
   verify becomes a refusal, so soundness survives even hostile-weird real files —
   at measured cost, which is where all the disconfirmations landed.
3. **What contributed most?** The graph-premise check + enumerated denylist. The
   encoding defect confirms the paper's trusted-base scoping was load-bearing: the
   ONLY failure class in 283,960 cells sits exactly in the byte→token layer the
   proofs explicitly do not cover (§3 "the explicitly trusted surface is the
   byte→token parse").
4. **How robust / where does it fail?** Robust across two very different corpora
   (academic/web EUSES incl. CJK; business Enron). Failure surfaces: (a) THE REAL
   DEFECT — xlq double-encodes non-ASCII formula string literals (silent literal
   corruption; frozen during the test per protocol; to be fixed post-analysis with a
   regression test; own-certify cannot catch it — both sides share the encoder — but
   for FOREIGN edits it over-refuses, never falsely certifies); (b) fail-closed cost
   17.8–21.5% of eligible files (denylist parts + loader limits on LO-converted
   exotica); (c) conversion caveat: corpora are .xls→.xlsx via LibreOffice.
5. **What was surprising?** The defect class itself (encoding, not arithmetic); q̂
   2.7–3.2× fixtures with Enron k999=20 (the probabilistic tier is far weaker in the
   wild — the strongest new argument FOR the exact tier); dbt coverage above
   prediction; prevalence split (EUSES below range — more data-only sheets among
   web-collected files).
6. **Literature?** Freshness check (015): no direct competitor; the 2026 benchmark
   wave (Spreadsheet-RL, MBABench, BlueFin) documents this failure mode but verifies
   engine-in-the-loop or by LLM judge.
7. **Does it solve the problem?** PROBLEM.md's proxy caveat is discharged: the
   fixture-proxy guarantee transferred to in-the-wild artifacts (0 false certs), the
   costs are now measured facts, and the one failure found is precisely in the layer
   the theory honestly labeled trusted-not-proven. The metric advanced the real
   thing, not just the proxy.

## Budget check

research_iterations 3/5 (Path C consumes none); paper_review_rounds 2/2 exhausted
(paper integration proceeds as revision presented to user, no new panel without
grant); test set run ONCE (this entry logs it as irreversible — reruns were harness-
bug reruns, disclosed above, never a second look at accepted results).

## Problem alignment

Every headline number is now test-tier evidence about real artifacts — the exact gap
both PROBLEM.md and the final review named as the single most valuable missing item.

## Decision

Path C step 2 (publish decision) goes to the user with the no-publish option
steelmanned. Recommendation: contribution paper — the test strengthened every claim
it touched and the two failures it found are disclosed scope facts (a real defect in
the trusted layer; measured fail-closed cost), which the paper's honesty architecture
is built to carry.

## Next Steps

1. User checkpoint: publish decision (a/b/c).
2. On (a): fold test-tier results into the paper (new §5.9 + abstract/scope updates,
   fixture corpus relabeled development-tier), fix the encoding defect POST-analysis
   with regression, present final draft.

## Post-review corrections (granted panel round, appended — original text above stands as history)

The granted review round (research-log/progress 2026-07-10) corrected this entry's
overclaims and found a live sibling defect:
1. "never falsely certifies" (line ~48) was too strong: over-refusal under the
   encoding defect is guaranteed only for CORRECTLY-ENCODING foreign tools; a foreign
   tool with the identical Latin-1 misread would produce byte-identical corrupted
   output and be certified. Paper §5.9 now carries the scoped statement.
2. SIBLING DEFECT (live, reproduced by the panel): unquoted non-ASCII sheet
   qualifiers (集計01!CI3) mis-tokenize under the ASCII-only qualifier grammar —
   silent stale references; present ~4,110 times in the locked corpus; invisible to
   the locked harness (truth grammar skips cross-sheet formulas — now disclosed in
   §5.9). Fixed fail-closed (non_ascii_sheet_qualifier residual), 198 tests green,
   end-to-end refusal verified. Locked numbers unchanged.
3. EUSES sampling: the acquisition prefix (791/796 files = database category) was
   deterministic and pre-inspection but NOT pre-registered; §5.9 now disclosed it and
   tempers cross-corpus claims. dbt-leg provenance correctly restated as weaker
   (single-commit attestation; fallback bypassed; 4th harness fix disclosed).
4. Denominator fix: 0.034% is of the EUSES leg (113,164), not the combined 283,960.
