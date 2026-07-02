# Research Setup

**Date:** 2026-07-03
**Phase:** 0
**Iteration:** 1
**Status:** completed

## Context

Systems work completed 2026-07-02/03 (commits 8be59f7..150fb66): xlq v0.1,
vendored IronCalc master engine, 522/522 catalog coverage with three-number
accounting, 1,659-case LibreOffice differential oracle, AXLE-bench suite.
This project prepares the publication-quality write-up + artifact package.
User directive: "make technical contribution to the field, create impact...
refer to sciagent to prepare the academia artifacts."

## Idea DNA

- **Problem:** LLM agents operating on spreadsheet artifacts corrupt files,
  flood context windows, and produce unverifiable results. Measured, not
  asserted: Anthropic's own xlsx skill corrupts financial models via
  openpyxl (issue #22044); openpyxl strips 101,961 cached formula values on
  a single re-save of our corpus; the capability matrix across 8 tool
  families has three empty columns — dry-run, receipts, revisioning.
- **Assumption (explicit, from the founder's memo §11.2):** "The Skill is
  guidance. The runtime is the enforcement boundary." — safety properties
  for agent-file interaction must be enforced by the execution surface, not
  requested via prompts. (Inferred corollary: the boundary must also be
  self-honest — a tool that cannot evaluate something must say so, because
  the agent cannot tell otherwise.)
- **Novelty claim:**
  1. The enforcement-boundary pattern for agents operating on legacy file
     formats: read-only-by-construction operations, typed patches with
     dry-run (spec'd), hash-chained receipts (spec'd), and
     coverage-honesty (the runtime reports its own blind spots per file).
  2. Privacy-safe workbook census as a shareable compatibility artifact
     (structure without content; UDF names classified as user data).
  3. Three-number coverage taxonomy (recognized / locally evaluable /
     policy-limited-with-documented-literals) replacing the binary
     supported/unsupported claim — policy-limited functions return the
     exact error literal desktop Excel produces in the same situation.
  4. Cross-engine differential testing (CSmith/EMI lineage) applied to
     spreadsheet formula engines with a three-way design: engine-under-test
     vs reference engine (LibreOffice) with documentation-as-arbiter
     (Excel) triage — surfaced real bugs in BOTH engines.
  5. AXLE-bench: five-axis comparative benchmark for agent-spreadsheet
     tooling, every cell artifact-traced or cited-not-run.
- **Domain:** systems / software engineering for LLM agents.
- **Success criteria:** paper draft passing three independent role-scoped
  reviewers (editor synthesis PUBLISH_READY); reproducible artifact package
  (repo + AXLE-bench + oracle + fixtures); venue-shaped for an SE/systems
  venue tools/artifact track (final venue framing decided after Phase 1).
- **Scope constraints:** all measurement artifacts frozen at commit 150fb66
  (evaluation contract); new experiments only additive with predictions;
  local compute only; research intensity Deep (30–50 papers) per the
  publication-grade goal.

## Setup decisions

- Literature sources: arXiv MCP (alphaXiv) + WebSearch/WebFetch. Scholar
  Gateway available but requires interactive auth — will use if reachable.
- Research intensity: **Deep** (auto-set: user asked for academia-grade
  artifacts; user's session pattern is terse directives + autonomous
  execution, so checkpoints present recommendations and proceed on the
  recommended option when unanswered).
- Compute: local machine only (see experiments/configs/environment.md).
- Output format: Markdown primary + LaTeX (paper/), figures in
  paper/figures/.
- Retrospective honesty rule: this project writes up completed systems
  work. The prediction ledger (results.tsv) records only genuinely-made
  predictions as such; exploratory measurements are labeled RETROSPECTIVE.
  The paper's Discussion will state this methodology plainly.

## Quick validation scan (Phase 0 gate)

Not trivially solved: the BASELINE.md capability survey (2026-07-02, cited)
found no tool offering dry-run/receipts/revisioning for agent-spreadsheet
operations across openpyxl, xlwings, LibreOffice, excel-mcp-server (4k
stars), OfficeCLI (8.3k stars), COM, HyperFormula/GRID. Not fundamentally
flawed: enforcement-over-guidance is established direction in agent
sandboxing (to be positioned properly in Phase 1); differential testing is
a proven methodology (CSmith ~200 miscompilations found; EMI) not yet
applied to spreadsheet engines in the literature we know — Phase 1 must
verify that negative claim carefully before the paper asserts it.

## Decision

Proceed to Phase 1 literature review, Deep intensity, four parallel
searcher tracks: (1) LLM-agent safety/sandboxing/tool-use enforcement;
(2) differential testing lineage; (3) spreadsheet research (corpora,
errors, SE-for-spreadsheets, engines); (4) agent benchmarks + provenance/
audit-trail systems + file-format fidelity.

## Next Steps

Dispatch literature searchers; build literature map + decision archaeology
+ Exemplar Move Tables; baseline-strength audit of claimed comparisons;
venue decision; user checkpoint on direction.
