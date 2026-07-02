# Writing Rationale Matrix — execution plan for section writers

Row 1 justifies the whole-work framework; subsequent rows split the paper
into the smallest useful units. Columns: Unit | Function | Idea-DNA link |
Exemplar pattern | Venue norm | Evidence anchor | Operation.

**Row 1 — Whole-work framework.** Controlling structure: a systems paper
built as *problem (fidelity gap) → mechanism (format-aware boundary) →
earned trust (differential oracle) → honest coverage (3-number taxonomy) →
evaluation (AXLE-bench) → what-we-didn't-invent (related work placed late,
PQS-style contribution fencing)*. Exemplar arc: PQS (oracle-soundness in
installments; contribution fencing) fused with OS-Harm (taxonomy-before-
benchmark) and CSmith (catastrophic artifact; adversarial discussion). Idea
DNA: the enforcement boundary must be format-aware and self-honest.
Structural pivot: related work comes AFTER the mechanism, so the reader
meets our contribution before the crowded enforcement-wave — the fencing
then reads as precision, not defense.

| Unit | Function | DNA link | Exemplar | Venue norm | Evidence anchor | Operation |
|---|---|---|---|---|---|---|
| Abstract | fire→contribution in 200w | all | CSmith P4 numbers | 250w | 101,961; 522/505/17; 97.1%; 7,502× | WRITE |
| Intro P1 | catastrophic artifact | C1 | CSmith Fig1 | concrete-first | issue #22044 / 101,961 strip | WRITE |
| Intro P2 | gap as shared assumption | C1 | Pista P4 | — | benchmarks score only task cells (5 cited) | WRITE |
| Intro P3 | why runtime not prompt/agent/human | C4 | Pista P5 | — | enforcement-wave crowded but format-blind | WRITE |
| Intro P4 | 4 numbered contributions + fencing | all | PQS P8 | numbered | "did not invent receipts/DT" | WRITE |
| §2 Fidelity Gap | measure the omitted dimension | C1 | OS-Harm 3.1 | table | part-survival + cache matrix (results.json) | WRITE |
| §2 fig | OOXML part survival across tools | C1 | CSmith Fig1 | figure | fixtures re-save matrix | FIGURE |
| §3 Boundary design | read-only + census + coverage-honesty + patch/receipt spec | C4 | PQS §3 (radical-simplicity) | design | census-spec, receipt-journal-spec | WRITE |
| §3 census | structure-only; UDF=user data; token ratio | C4 | Pista "name primitive" | — | 965B vs 7.24MB; privacy regression test | WRITE |
| §4 Coverage honesty | 3-number taxonomy; policy literals | C2 | OS-Harm taxonomy-first | table | coverage.json; semantics spec | WRITE |
| §5 Oracle | 3-installment soundness; 3-way verdict; LO=reference not truth | C3 | PQS oracle chapter | methodology | agreement.json; AGREEMENT.md | WRITE |
| §5 bug stories | real bugs both directions | C3 | CSmith §3.7 sidebars | vignette | CONVERT/ROW (IronCalc); POWER(0,0)/PERCENTRANK (LO) | WRITE |
| §6 AXLE-bench | 5-axis; instrument validation; adoption cost | C1,C3 | OS-Harm co-headline | matrix | benchmarks/README matrix | WRITE |
| §7 Related Work | contribution fencing, placed late | all | PQS defended-null | fair positioning | 50-paper map; Pista differentiation | WRITE |
| §8 Discussion | self-question; load-bearing assumptions; named primitives; Pista door | all | CSmith + Pista §7 | journey | the diff/cached_value self-bug; ledger honesty | WRITE |
| §9 Limitations | mechanism-derived; retrospective ledger | all | PQS §5 | honest | 1,659 curated (not generative-scale); single-machine | WRITE |
| §10 Conclusion | contributions + successor work | all | — | evidence-based | 3 named primitives | WRITE |

**Anti-shallow-revision note:** on any v2, apply the metrics in the sciagent
cross-cutting concerns (near-identical paragraph ratio <35%, dominant op not
ADD, 0 numbers without a source anchor). Every number above traces to a
frozen artifact in benchmarks/ or docs/.
