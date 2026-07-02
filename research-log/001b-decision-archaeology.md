# Decision Archaeology + Exemplar Move Tables

**Date:** 2026-07-03 · **Phase:** 1 · **Status:** completed

Four exemplars read in full for the systems paper "An Enforcement Boundary
for LLM Agents Operating on Spreadsheet Artifacts." Full per-paragraph
tables in the task transcript; this file records the load-bearing
archaeology and the reusable moves the section writers must transfer.

## 1. CSmith (PLDI 2011) — tool + empirical-study paper
- **Motivation (real):** silent wrong-code bugs, not crashes; making random
  inputs *meaningful* despite absent ground truth; being believed by the
  compiler maintainers.
- **Load-bearing assumption:** that CSmith's own unverified analyses are
  correct (every generated program truly UB-free). Evidence chain bottoms
  out in a *social* check — maintainers agreed the reduced cases were valid.
- **Reusable moves:**
  - **Catastrophic-artifact-on-page-1** — a tiny, eye-verifiable failure
    from a *shipped* system before any generality.
  - **Victim-validated metrics** — the tested systems' maintainers supply
    the importance judgment (fixes, severity labels) when no oracle exists.
  - **Enumerate the hazard space, then a coverage table** — "191+52
    behaviors" up front; a table mapping each hazard to its defense.
  - **The CompCert contrast** — evaluate the strongest competing paradigm
    inside your own evaluation; report where your method *loses*, with
    effort quantified. Most-cited paragraph in the paper.
  - **Decision-log methodology** — named trade-off subsections, each with
    the rejected alternative and a measured reason.
  - **Kept negative result** — run the experiment your hypothesis predicts
    will succeed; if it fails (coverage barely moved), print and interpret.
  - **Adversarial self-questioning discussion headers** ("Are we finding
    bugs that matter?") answered with un-manufacturable evidence.

## 2. PQS/SQLancer (OSDI 2020) — testing-methodology paper
- **Motivation (real):** the test-*oracle* problem for logic bugs; escaping
  differential testing's intersection trap (only the shared dialect is
  testable); a technique *others reimplement* (PingCAP/TiDB).
- **Load-bearing assumption:** that errors in the hand-written,
  documentation-derived interpreter surface as noisy false positives, not
  silent false negatives — and that its blind spots don't correlate with
  where the DBMS bugs live.
- **Reusable moves (this is our oracle chapter's template):**
  - **Motivating bug as triple-duty artifact** — one real, vendor-critical,
    years-latent example that demonstrates the mechanism, proves severity,
    and re-proves the gap ("prior art couldn't even express this test").
  - **Oracle soundness in three installments, never one defensive block** —
    design-time radical simplicity, invariant-preserving feature admission,
    and the false-positive-loop + mutual-testing flip placed LAST in the
    discussion, after the results have earned patience.
  - **Defended null baseline** — dismiss each candidate baseline with a
    distinct checkable reason rather than benchmarking a straw man.
  - **Vendor as validator** — fixed/verified counts, third-party severity,
    bug ages, a CVE, substitute for an impossible benchmark.
  - **Contribution fencing** — explicitly disclaim non-novel components
    (generation, state creation) so the claimed core is small and defended.
  - **De-confound before the reader computes** — explain why headline
    numbers vary before showing the table that invites the wrong inference.
  - **Publish the full denominator** — rejected reports, unflattering
    coverage numbers, voluntarily, each with its interpretation.
  - **Mechanism-derived limitations that seed follow-up** — every
    limitation traces to the core design choice.
  - **Three-way verdict vocabulary** (from Liu et al. 2024): bug /
    inconsistency / underspecified — adopt this for oracle triage.

## 3. OS-Harm (NeurIPS 2025 D&B) — benchmark paper
- **Motivation (real):** be the reference safety benchmark for the
  computer-use-agent *category*; the finding that chatbot-tuned safety
  doesn't transfer to agents; a *cheap, reusable* harness ($53/run,
  published as a first-class metric).
- **Load-bearing assumption:** that judge accuracy validated on ONE agent's
  traces transfers across the other four and across categories — the
  model *ranking* may not be identified.
- **Reusable moves (our AXLE-bench chapter's template):**
  - **Taxonomy-before-benchmark** — present a *complete partition* of the
    space as the intellectual contribution before the mechanism; the
    artifact then instantiates it symmetrically.
  - **Instrument validation as co-headline** — when the metric is itself
    fallible, report its accuracy against ground truth in the main body,
    next to the results it produced, with a named failure-modes section.
  - **Receipts-ledger appendix** — every main-text default paired with an
    ablation showing the discarded alternative; main text carries pointers.
  - **Piggyback + publish the adoption cost** — build on the substrate the
    field reports against; state dollar/wall-clock/install as design metrics.
  - **Declare your adversary a floor** — when tests are simple by
    construction, say so and structure code so stronger ones slot in.
  - **Safe-because-incapable pre-emption** — if a low violation rate could
    reflect incompetence rather than the property claimed, raise the
    confound yourself and derive a consequence (benchmark longevity).
  - **Ethics/scope by construction, documented** — external-authority
    definitions, explicit exclusions, each stated as a reasoned decision.

## 4. Pista (2026, incl. Gulwani) — the closest prior; HCI systems paper
- **Motivation (real):** accountability under artifact/process
  inseparability — in spreadsheets the agent's decisions *are* the cells the
  user owns; kill post-hoc review as a paradigm; make Copilot-class agents
  overseeable (product-adjacent).
- **Load-bearing assumption:** trail faithfulness — that model-generated
  step explanations faithfully describe what the executed snippet did (no
  verification of explanation-to-action correspondence); and that
  participation-accrued trust is *calibrated* (equal success but higher
  confidence → possible automation-complacency machine).
- **THIS IS THE PAPER WE DIFFERENTIATE AGAINST.** Pista = synchronous human
  oversight with model-generated explanations, prompt-only constraints, no
  automated verification, no fidelity measurement, doesn't scale unattended.
  Its own Limitations mark the successor problem: guarantees were "solely
  system prompt instructions" and need "programmatic controls." **We walk
  through that door.** Our enforcement boundary is machine-checked
  (coverage-honesty, differential-validated engine, receipts), format-aware
  (fidelity), and scales to unattended operation.
- **Reusable moves we adopt:**
  - **Name the primitive** — Pista promoted one feature into "semantic
    diff." We promote *coverage-honesty* and the *three-number coverage
    taxonomy* into named, domain-general primitives in the Discussion.
  - **Mark the successor problem** — but we're on the *answering* side of
    Pista's open door, which is the cleanest possible positioning.
  - **Lead with the tie / relocate success** — our task-success story is
    not the point; fidelity + coverage-honesty + differential agreement are.

## Cross-exemplar synthesis for our paper
- Open with a **catastrophic artifact** (CSmith): the openpyxl re-save that
  silently stripped 101,961 cached values, or issue #22044, verifiable in
  seconds, before any generality.
- Motivate with the **verified gap as a shared assumption** (Pista P4 move):
  every LLM-spreadsheet benchmark scores only task cells — a convention, not
  a law — and none measures fidelity.
- Present the enforcement boundary with **contribution fencing** (PQS): we
  did NOT invent receipts/transactions/differential-testing; the claimed
  core is the *format-aware* layer (fidelity + coverage-honesty) and the
  *documentation-arbitered* oracle for a domain with no mechanized spec.
- The oracle chapter follows **PQS's three-installment soundness** and
  adopts the **three-way verdict**; LibreOffice is a reference, Excel docs
  the arbiter, disagreements triaged not auto-blamed.
- AXLE-bench follows **OS-Harm's taxonomy-before-benchmark** and
  **instrument-validation-as-co-headline** (our instrument is the oracle +
  coverage probe; validate and state its blind spots).
- Discussion uses **CSmith's adversarial self-questioning** and **PQS's
  mechanism-derived limitations**; state the load-bearing assumptions
  plainly (our own: the vendored engine's differential-validated honesty;
  the census's structure-only privacy invariant; retrospective-prediction
  methodology).
