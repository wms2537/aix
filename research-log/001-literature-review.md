# Literature Review

**Date:** 2026-07-03
**Phase:** 1
**Iteration:** 1
**Status:** completed

## Context

Four parallel searchers (Deep intensity) returned ~50 papers across: (A)
LLM-agent safety via execution surfaces, (B) differential-testing lineage,
(C) spreadsheet research, (D) agent benchmarks + provenance. Full per-paper
extractions preserved in the searcher transcripts; this entry is the
synthesized map. All four searchers returned DONE_WITH_CONCERNS; every
concern is addressed in "Novelty-boundary corrections" below.

## Literature map

### Group 1 — Runtime enforcement for LLM agents (2024–2026 wave)
Progent (0% ASR deterministic policies), AgentSpec (DSL runtime rules),
MiniScope (ILP least-privilege over OAuth scopes), SkillScope, IsolateGPT
(process isolation), Agent-Sentry (provenance graphs), Runtime Governance
(path-dependent policies), OpenClaw PRISM, transactional lines:
Fault-Tolerant Sandboxing (ACID snapshots, 100% rollback), Cordon (semantic
transactions), SAFEFLOW (compensating transactions). Receipts prior art:
**Notarized Agents** (receiver-attested cryptographic receipts, 2026);
survey: From Agent Traces to Trust (2026).
**What works:** deterministic runtime enforcement consistently beats
prompt/heuristic defenses (Progent 0% vs 39.9% ASR); transactional
rollback is practical (100% on filesystem snapshots).
**What's missing:** every system is format-agnostic — none understands the
artifact it protects. None can answer "will this edit corrupt the file's
semantics?" or "can this runtime even evaluate this artifact?" — the
format-aware fidelity + coverage-honesty layer does not exist in this wave.

### Group 2 — Differential testing lineage
CSmith (325+ compiler bugs; generate-in-well-defined-subset then vote), EMI
(147 bugs; arbiter-free metamorphic self-differential), PQS/SQLancer (121
bugs; tool-built reference evaluator AS the arbiter = documentation-as-
arbiter precedent), NoREC/TLP (optimizer metamorphic relations), JEST
(mechanized-spec-as-N+1th-implementation; 44 engine + 27 spec bugs),
feature-sensitive coverage (PLDI'23), Jit-Picking (intra-engine JIT
on/off), CRADLE (float tolerance + "inconsistency ≠ verdict" gap),
queryFuzz (Datalog metamorphic), Liu et al. 2024 (Prolog SQL-spec arbiter;
three-way verdict bug/inconsistency/underspecified), **SmartOracle 2026**
(LLM agents querying the ECMAScript spec to triage JS-engine
disagreements — the closest realization of documentation-as-arbiter).
**What works:** the oracle-construction menu is mature: cross-vote,
metamorphic, reference-evaluator, mechanized-spec, LLM-triage.
**What's missing (verified by dedicated search):** no published systematic
generative differential testing of spreadsheet formula ENGINES. Near-
misses handled precisely: HyperFormula 3.1.0's internal industrial
framework (unpublished, benchmark-suite, one-directional); Rob Weir's 2009
manual ODF formula tables (dozens of cases, non-archival); McCullough/
Almiron NIST-certified numerical-accuracy line (fixed workloads, accuracy
not semantics); metamorphic testing OF user spreadsheets (engine trusted);
OSS-Fuzz on LibreOffice (parser crashes, not formula semantics). All spec-
as-arbiter precedents presuppose a mechanized/normative spec — spreadsheets
have none (OpenFormula covers ODF, Excel is documented in informal vendor
pages + ECMA-376 prose). Approved claim wording recorded below.

### Group 3 — Spreadsheet research
Motivation bedrock: Panko (94% of 88 audited sheets contain errors; 5.2%
cell error rate), Hermans & Murphy-Hill Enron corpus (24% of formula-
bearing corporate sheets contain Excel errors; 75% of sheets use only
top-15 functions — independently validates our workload-weighted coverage
approach), EUSES corpus, Hermans smells, Jannach QA survey, ExceLint
(dependence-vector structural fingerprints = classical ancestor of our
census). LLM-agent-on-spreadsheet systems: SheetCopilot (NeurIPS'23),
SheetAgent (WWW'25), SpreadsheetBench (NeurIPS'24 D&B; exact-match on
answer cells only), SpreadsheetBench 2 (2026; ~594 cell modifications/task,
best model 34.89%), Finch (2025; 38.4% on real finance workflows).
**Verified load-bearing gap:** none of these systems or benchmarks measures
file corruption, preservation of untouched content, or side effects;
SheetCopilot's checker skips unflagged properties BY DESIGN. Closest
safety prior: **Pista** (2026, incl. Gulwani) — step-level HUMAN auditing/
control of spreadsheet agents; interactive oversight, no automated
dry-run/receipts/fidelity verification, doesn't scale unattended.
Representations: TUTA (model-side encoders) vs SpreadsheetLLM
(prompt-side lossy compression, 25×/96% token reduction, understanding
tasks) vs our census (tool-side structural summary for mutation safety) —
clean three-way design space; SheetAgent's SQL sub-views are partial
precedent for view-based context reduction.

### Group 4 — Benchmarks & provenance ancestry
OSWorld, AgentBench, WebArena, SWE-bench, τ-bench: task-success metrics
only; **only OS-Harm (NeurIPS'25) measures side effects at all** (LLM-judge
harm categories; 21–29% unsafe rates in frontier models), and none measures
file fidelity. Receipt-journal ancestry to cite: forward-secure MAC logging
(Nitro CCS'25), Certificate Transparency Merkle proofs (SoK IEEE S&P'22),
W3C PROV + PROV-AGENT, BlockAudit. Our journal = domain instantiation
(single-writer local sidecar, hash chain, external-edit adoption
semantics), NOT a new cryptographic construction — cite honestly.

## Bedrock vs. convention audit

- Bedrock: spreadsheet error pervasiveness (Panko, Enron — replicated);
  differential testing finds real bugs (CSmith lineage — replicated);
  deterministic runtime enforcement beats prompting (Progent et al.).
- Convention (challengeable): "agent benchmark success = task-cell match"
  — an unexamined convention our fidelity axis directly challenges;
  "coverage = binary supported/unsupported" — convention our three-number
  taxonomy replaces; "safety = block/allow" — convention; format-aware
  fidelity is a third dimension the enforcement wave ignores.

## Novelty-boundary corrections (from searcher concerns)

1. Generic "enforcement > prompts" is CROWDED (Group 1) → headline novelty
   is the format-aware enforcement layer: fidelity guarantees + per-file
   coverage-honesty (runtime declares its own evaluability blind spots) —
   absent from all 20 Group-1 papers.
2. Receipts: Notarized Agents is prior art for agent-action receipts →
   position ours as a format-level, local-first instantiation with
   different threat model (file custody + out-of-band edit detection, not
   receiver attestation); cite Nitro/CT/PROV ancestry.
3. Transactions: ACID-snapshot/compensating-transaction prior art exists →
   our dry-run-with-semantic-prediction (affected cells, new formula
   errors, watch values) is the differentiator, not atomicity itself.
4. Differential testing: use the approved claim wording — "While
   cross-engine comparisons of spreadsheet numerical accuracy against
   certified reference values exist [Almiron 2010; McCullough], and
   vendors privately benchmark formula compatibility [HyperFormula 3.1.0],
   no prior work has applied generative differential testing to
   spreadsheet formula engines with an automated, documentation-grounded
   triage of disagreements; existing spec-as-arbiter techniques [JEST; Liu
   et al. 2024; SmartOracle] presuppose a mechanized or normative
   specification that the spreadsheet domain lacks." Adopt Liu et al.'s
   three-way verdict vocabulary (bug / inconsistency / underspecified).
5. Census: SpreadsheetLLM's 25× is for understanding tasks; our 7,502× is
   structure-only for mutation safety — never compare the numbers head-to-
   head as if same task.

## Baselines to beat — strength audit

- openpyxl 3.1.5: **strong** (current release, the de-facto agent-skill
  substrate — literally what Claude Code ships; capability baseline, no
  tuning applicable).
- LibreOffice 24.8 headless: **strong** (current stable; documented
  methodology caveats — process startup, no-recalc-on-load — already in
  BENCHMARKS.md).
- excel-mcp-server / OfficeCLI: **unverified** as run-here baselines
  (documented-capability rows only, cited; marked NOT-RUN-HERE in
  AXLE-bench — honest handling already in place).
- SheetCopilot/SpreadsheetBench agents: not baselines for our claims (we
  make no task-success claims); cited as the systems whose evaluation
  blind spot we measure.
- IronCalc-as-engine: the engine under test, not a baseline.

## Research directions

- **D1 (recommended): the systems paper.** "An enforcement boundary for
  LLM agents operating on spreadsheet artifacts" — xlq design (census,
  read-only trio, receipts spec, coverage-honesty) + the differential
  oracle as the correctness-validation methodology + AXLE-bench as the
  evaluation, with the fidelity gap (Group 3/4 verification) as the
  motivating wedge. Venue shape: SE conference technical/tools track
  (ICSE/FSE/ASE) or arXiv-first; artifact badge material is already built.
- **D2: the methodology paper.** Documentation-arbitered generative
  differential testing of spreadsheet engines (deepest single novelty;
  narrower; would want a larger generative campaign than our 1,659 curated
  cases to be maximally convincing — a real limitation to state if D1
  carries it instead).
- **D3: the benchmark paper.** AXLE-bench as a D&B-track artifact with the
  fidelity axis as headline (fastest to venue-fit but leaves the system
  contribution on the table).

## Decision

Recommend **D1 with D2 as its evaluation core** (single paper; the oracle
is both validation of xlq's engine honesty and a standalone methodological
contribution), D3 released as the artifact package. Checkpoint with user
before Phase 2.

## Next Steps

- Decision archaeology + Exemplar Move Tables (001b) — dispatched.
- User checkpoint on direction (D1/D2/D3).
- Phase 2: formalize the falsifiable claims for the paper's evaluation.
