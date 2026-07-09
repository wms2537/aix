# SciAgent v2 migration + Phase 5 standing

**Date:** 2026-07-09 · **Phase:** 5 · **Cycle:** 1 · **Iteration:** 3 · **Status:** in-progress

## Context

The project ran under sciagent v1 conventions (research-log/000-014, results.tsv) and
three major campaigns: (iter 1) theory + tool + paper-v1..v3 arc through repeated
adversarial review to clean-accept; (iter 2) benchmark grind — real-corpus 4-op +
move-rows shift correctness, composition coverage 27%→87%; (iter 3) the
four-contributions campaign answering the novelty audit (Checker.lean sound decision
procedure, Impossibility.lean incl. no_engine_free_predictor, coincidence bound, live
agent study, dbt domain), ending in the final panel ruling: **B+ / major-revision-
then-accept (FSE/ASE/ISSTA class)** with four gating holes, all fixed at commit
18a2209. Skill updated to sciagent v2 (deterministic loop); this entry reconstructs
loop state per the recovery rule.

## Content

Created `state.json`, `PROBLEM.md`, `research-log/progress.md` from ground truth (git
log + research-log/000-014 + paper/paper-v3.md + final panel verdict). Honest budget
reconstruction: research_iterations 3/5; paper_review_rounds 2/2 (exhausted — further
panel rounds require user grant); hypothesis_review_rounds 1/2 (log 013 theory-review
FAIL→fix round). `tried_and_failed` seeded with the two refuted/defeated approaches;
`learnings` seeded with the five recurring failure classes (claim-inflation at
boundaries, adapter normalization proxy bugs, subagent claims needing independent
re-verification, oracle label noise, working-tree drift).

Workspace integrity at ORIENT: uncommitted honest-relabel drift in
benchmarks/agent_ab.{py,json} (matches committed EDIT_PATH_AB.md framing — the .py/
.json relabel was never committed); committed deliberately with this migration.
__pycache__ noise gitignored.

**Phase 5 standing.** The final panel is this iteration's review evidence. Its ruling
identifies the single most valuable missing item: **one in-the-wild run** — every
headline number (85.5% corruption, the 7.5–11.8% full-check blind-spot floor, 6/6
saves, COST≈0) is measured on engine calc-test fixtures or author-built artifacts.
This aligns exactly with PROBLEM.md's proxy caveat.

**Data-tier framing for the path decision.** The vendored fixture corpus served as
the tuning/validation tier throughout development. An in-the-wild corpus
(EUSES/Enron spreadsheets; one real public dbt project) is untouched by development —
it is the natural **locked test tier**, to be run exactly once under Path C
(conclude), not an iteration. The dbt real-project leg may reveal adapter parse
gaps; the honest protocol is to report parse coverage and refuse-not-guess, NOT to
extend the adapter after seeing the test data (that would unlock the test set).

**Ethics/data governance (standing rule):** the Enron spreadsheet corpus derives from
FERC-released real business email attachments and contains real names and business
data (PII-adjacent). EUSES is a standard research corpus of web-collected
spreadsheets. Per the standing rule: not downloading either until the user consents
at the checkpoint.

## Gate Check

- Budget check: `state.json.budgets` — research_iterations 3/5, paper_review_rounds
  2/2 exhausted. Evidence: this entry + state.json at migration commit.
- Freshness check: dispatched (T001, in_progress) — result to be appended before the
  path is executed.
- Path decision: T002 checkpoint pending user approval (will be quoted verbatim).

## Problem alignment

The in-the-wild test run attacks PROBLEM.md's proxy caveat directly — it converts
fixture-proxy numbers into evidence about the artifacts the problem statement is
actually about.

## Decision

Recommend **Path C (conclude): one-shot locked in-the-wild test** (EUSES/Enron xlsx
through the deterministic shift-correctness + guard pipeline; one real public dbt
project through adapter_dbt with honest parse-coverage reporting), then the publish
decision. Alternative presented: conclude as-is with fixture-scoped claims (the
paper already states the scope honestly).

## Next Steps

1. T001 freshness check returns → append result here.
2. T002 user checkpoint: path + Enron/EUSES data-governance consent (+ post-hoc
   PROBLEM.md approval).
3. On approval: T003/T004 locked test run, exactly once, logged as irreversible.
