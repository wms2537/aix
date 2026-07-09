# PROBLEM.md — pinned problem formulation

**Core question (one sentence):** Can we certify that an untrusted agent's edit to an
opaque-semantics artifact (a spreadsheet, a dbt project) is value-faithful — offline,
against the artifact's own structure, without trusting the editor and without running
the defining engine?

**Who has this problem and why it matters:** Anyone letting LLM agents edit files
whose correctness lives in an engine the editor does not run — financial models,
payroll sheets, regulatory filings, analytics DAGs. A structural edit that fails to
propagate references produces a file that opens fine and computes wrong, with no
visible symptom; "high probability correct" is the wrong guarantee for these
artifacts, and the guarantee must not decay as models improve.

**Why current approaches fall short:** Naive programmatic edit paths (openpyxl-class)
silently corrupt; recompute-and-compare requires the engine and trusts its
recomputation; LLM self-checks are the component under test. Translation validation
exists for compilers but assumes the semantics it validates against — here the engine
is absent by construction.

**What success looks like (beyond the metric):**
1. A machine-checked account of *which* edits are certifiable engine-free (both sides
   of the boundary), with the running system checking the theorem's premise — theory
   load-bearing, not decorative.
2. A working certify-or-refuse guard that blocks real agents' real errors with zero
   false certifications and measured, acceptable refusal cost.
3. Claims that survive hostile review with no overclaim — every number traceable to a
   committed artifact.

**Explicit non-goals:** certifying value/semantic edits in the exact tier (proven
impossible engine-free for unwitnessed semantics); column-level dbt lineage;
Excel-engine reimplementation; agent capability improvement.

**Proxy caveat:** Corruption/refusal rates measured on the vendored fixture corpus
(engine calc-test suites + templates) are our proxy for preventing silent corruption
of in-the-wild artifacts. Improving fixture numbers without in-the-wild transfer is
failure — this is exactly the gap the final review named (fixture corpora behind
every headline number).
