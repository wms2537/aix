# Adversarial PC Review (mock top SE-venue)

Six independent reviewers (ICSE/FSE calibration, ~18% accept, adversarial default) read only the paper; four kill-shot deep-dives; an honest author rebuttal that could consult the real repo; a PC-chair meta-review.

**Scores:** empirical-se -2, pl-testing -2, llm-agents -2, systems-security -2, hci-humanfactors -1, pc-generalist -2 | mean -1.83 | unanimous reject band.

---

# META-REVIEW AND DECISION
**Paper:** *An Enforcement Boundary for LLM Agents Operating on Spreadsheet Artifacts* (xlq / AXLE-bench)
**Scores:** empirical-se −2, pl-testing −2, llm-agents −2, systems-security −2, hci-humanfactors −1, pc-generalist −2 · **mean −1.83** · unanimous reject band

---

## 1. META-REVIEW (committee consensus)

**What the paper is.** A measurement-plus-tool submission built around a real, well-chosen observation: when an LLM agent re-saves an `.xlsx`, mainstream substrates silently damage untouched content — openpyxl blanks ~101,961 cached formula values, LibreOffice drifts ~90,448 across a five-workbook corpus — and no agent benchmark scores this "fidelity" dimension. It delivers (a) that fidelity-gap characterization; (b) a read-only CLI (`xlq`) over a vendored IronCalc engine offering inspect/diff/calc, a privacy-safe structural "census," and a three-number "coverage honesty" taxonomy; (c) a documentation-arbitered differential oracle (1,659 hand-authored cases vs. LibreOffice, 85 disagreements, 24 faulting its own engine); and (d) AXLE-bench, which adds a fidelity axis. The write-mediation layer that would make it an actual *enforcement boundary* — typed patches, dry-run, hash-chained receipts — is specified but unbuilt (v0.2).

**Real strengths the committee credits (unanimously).**
- **Exceptional candor.** Every structural weakness is disclosed in-line rather than buried: provenance bias, single-adjudicator triage, the missing Excel oracle, the unmeasured T1 loss, the unbuilt write path. All six reviewers independently praised this. It is rare and genuine.
- **The core observation is correct and underappreciated.** Byte- and cache-level integrity checks are useless across substrates because every substrate rewrites ~100% of the OOXML container; integrity must therefore be *semantic and recompute-aware*. Naming fidelity as a first-class evaluation axis is a legitimate conceptual contribution.
- **The self-caught diff bug** (tool reported "1 change" while 442 formula results silently vanished) is a compelling, concrete motivation for recompute-aware comparison.
- **Concrete, externally checkable bugs in both engines** (CONVERT F→C, POWER(0,0), T-bill day-count) with published reproducers. Bug-finding value is real and survives all methodological objections.
- **24 self-indictments of the authors' own engine** are credible anti-stacking evidence the harness is not tuned to flatter IronCalc.

**Decisive weaknesses that survived rebuttal.**

1. **The titular mechanism is unbuilt (fatal, all six reviewers + author concession).** The paper is titled "An Enforcement Boundary," but v0.1 has *no write path*. "Enforcement by absence of a write path" is a property of `cat`, not a security guarantee. The author rebuttal **explicitly concedes this** and proposes retitling. When the load-bearing noun of the title is conceded to be unbuilt, the paper as framed cannot stand at a top venue.

2. **No threat model / non-bypassability (fatal, systems-security).** Even a completed v0.2 is opt-in: nothing forces an adversarial or careless agent through xlq — it can call openpyxl directly, which is *exactly* the #22044 failure the paper opens with. A boundary that interposes on nothing is not an enforcement boundary. Unaddressed in rebuttal.

3. **No agent is ever run (fatal, llm-agents; major, pc-generalist, hci).** The thesis is interventional ("insert this boundary, prevent corruption, still complete the task"), but every number is observational — measured on static files and static function catalogs. There is no agent-with-xlq vs. agent-without comparison on any editing task. The rebuttal does not contest this.

4. **The motivating catastrophe is never measured (major, unanimous).** #22044 is irreversible T1 loss (charts, pivots, VBA). The corpus contains none of these, so T1 is "not measured, not disproven." The headline 101,961 is the *recoverable* tier (T2.5), and **99,800 of 101,961 blanks live in a single synthetic file (perf-large.xlsx)** the tool itself authored. The scary number is one self-authored file, and it is not the harm the intro dramatizes. Author confirms these facts.

5. **Circular arbitration on the strongest quantitative result (major, unanimous).** The largest fault asymmetry (13 LibreOffice-wrong vs. 3 IronCalc-wrong, financial functions) rests "entirely" on the authors' own Python reimplementation — a third same-provenance implementation voting with the vendored engine against a third-party one. No second blind rater, no κ, no independent financial library, no Excel oracle. The rebuttal **concedes the directional claim** and keeps only the (valid) bug-finding value.

6. **Thin, borrowed, or reframed contributions (major, pl-testing, systems-security, pc-generalist).** The engine is vendored (coverage rose 66%→94% by "reading the upstream diff"; authors implemented ~8 functions). The differential method is generation-free and single-rater — "less than CSmith in a new domain." "Coverage honesty" is a reporting convention, and its *consumer* value is never tested (hci). AXLE-bench's two differentiator columns are "Yes" for xlq / "No" for everyone by construction, and two of four competitors are cited, not run.

**Net:** The rebuttal is honest and concedes the two most damaging points (unbuilt enforcement; circular financial claim). Conceding a fatal objection resolves it in the objector's favor. Nothing in the rebuttal converts a conceded gap into a result.

---

## 2. DECISION AT ICSE/FSE AS-IS

**REJECT.** (Not major revision — the fatal gaps require *new artifacts and new experiments*, not editing, which is out of scope for a revision cycle.)

**Vote rationale.** Top venues reject ~82%, and this paper sits below the line on the axis that matters most: the central claimed system does not exist, and the one experiment the thesis demands (agent-in-the-loop intervention) was never run. Three independent reviewers filed *fatal* flags, and they are non-overlapping (unbuilt mechanism / no threat model / no agent), meaning even fixing one leaves two standing. Candor is correctly credited but, as every reviewer noted verbatim, "honesty about a gap does not convert it into a result." A −2 mean with unanimous consensus and multiple orthogonal fatals is a clear reject; there is no committee member willing to champion.

---

## 3. VENUE STRATEGY (ranked)

1. **arXiv + the upstream bug reports as the real impact (do this now, regardless).** The concrete, externally verifiable bugs (CONVERT, POWER(0,0), T-bill day-count, the openpyxl cache-blanking behavior) are the paper's most durable output. File them upstream against IronCalc, LibreOffice, and openpyxl/agent frameworks; post the preprint. This delivers value immediately and cannot be "rejected."

2. **★ TOP PICK: a measurement/benchmark or empirical short-paper venue** (e.g., an empirical/benchmark track, MSR-style, or an agent-tooling workshop). **Justification:** the paper's genuine, defensible contribution is the *fidelity-gap characterization* — a correct, underappreciated measurement finding with a clean instrument (recompute-aware semantic diff). Reframed honestly as a measurement paper (drop "enforcement," drop the agent-safety framing), it is a credible, even valuable, contribution at a venue whose bar is "is this a real, well-measured phenomenon?" rather than "is this a novel enforced system?"

3. **ICSE-SEIP / industry track.** The practitioner motivation (a shipped agent destroyed a customer workbook) is strong, and SEIP tolerates vendored components and engineering-flavored contributions. A read-only inspector plus a fidelity benchmark is a reasonable SEIP story *if* reframed away from research-novelty claims.

4. **Tools/artifact or demo track.** xlq is a real, reproducible, commit-pinned artifact. This is an honest home for the CLI itself, decoupled from the overclaimed research framing.

5. **ICSE/FSE full paper — only after building and measuring v0.2** (see §4b). Not viable as-is; viable only if the intervention is actually run.

**Why #2 over #3/#5:** the fidelity finding is the part that is *true, novel-enough, and already measured*. Everything the top venues rejected is scaffolding bolted around it (unbuilt enforcement, unrun agent, vendored engine). Strip the scaffolding and the measurement paper is submittable *this cycle* with high acceptance odds; the full-paper route requires months of new implementation and still faces the threat-model and agent-evaluation objections.

---

## 4. MUST-FIX-BEFORE-SUBMISSION

**(a) Cheap fixes that clearly raise the score**
1. **Retitle.** Remove "Enforcement Boundary." Use the fidelity framing (author's own proposal: *"The Fidelity Gap: How Spreadsheet Substrates Silently Corrupt Workbooks, and a Read-Only Boundary for Detecting It"*). Single highest-leverage change.
2. **Report per-file fidelity numbers, not corpus totals.** Lead with the perf-large.xlsx dominance (99,800/101,961) instead of the aggregate; the aggregate reads as inflation the moment a reviewer decomposes it.
3. **Cross-check the financial reimplementation against `numpy-financial` / an external bond-math library.** Directly retires the circularity objection on the strongest quantitative result. Feasible in days.
4. **Move all v0.2 write-path text into a clearly-marked "Proposed Design / Future Work" box.** Stop stating write-path guarantees in the present tense.
5. **Cut the repetition.** The same four figures (101,961 / 90,448 / 24-of-85 / 7,246.5×) recur in ~8 sections and read as padding over a thin core.

**(b) The ONE experiment that would most change the outcome.**
**Run a real agent-in-the-loop A/B: an LLM agent completing genuine editing tasks on real workbooks (with charts/pivots/VBA), with vs. without routing through xlq, scoring both task success AND fidelity of untouched content.** This is the experiment the thesis has always demanded and its absence is the single most-cited fatal flaw (llm-agents, pc-generalist, hci).
- **Feasibility pre-submission:** *Partially, and only if v0.2's write path is built first* — you cannot show "xlq prevents corruption" with a read-only tool. Building typed-patch + dry-run + a minimal mediated write is weeks of work; the agent harness and a real-workbook corpus (with the T1 features the corpus currently lacks) are additional weeks. **Not feasible for a near-term full-paper deadline; feasible for a next-cycle full-paper attempt.** For the recommended measurement-venue submission, this experiment is *not required* — which is precisely why that venue is the top pick.

**(c) Zero-cost reframes that preempt the strongest attacks**
- Frame the differential oracle as a **bug-finding instrument**, not a correctness oracle. Claim only the externally-checkable bugs; drop every "our engine is more correct" directional claim. (Author already concedes this — put it in the paper.)
- Frame the census/coverage-taxonomy as **producer-side reporting hygiene**, not a validated consumer-facing "primitive." Do not claim the agent "learns which cells not to trust" without a consumer study.
- State up front that this is a **substrate-characterization measurement**, not an agent-safety evaluation, so no reviewer arrives expecting an agent and finds none.
- Replace "7,246.5× smaller (token proxy)" ergonomics claim with a plain bytes-of-context statement; do not call a compression ratio "ergonomics."

**(d) Limitations to state plainly rather than fix (accept and disclose)**
- The corpus is self-authored and synthetic; the IronCalc 0-drift row is provenance-biased to tautology. Say so, and scope all fidelity claims to "substrate re-save behavior on this corpus," not to agents or to the field.
- T1 irreversible loss (the #22044 harm) is *cited, not measured*. Do not motivate with a catastrophe you cannot reproduce; either add fixtures containing charts/pivots/VBA (cheap, and worth doing) or demote the #22044 framing to related-work context.
- Single-adjudicator triage with no κ: disclose as a bounded-validity threat and publish the per-case triage (already done) — but stop calling the verdicts "reproducible measurements."

---

## 5. HONEST BOTTOM LINE

Do **not** submit this to ICSE/FSE as-is — it will be rejected again, for the same three orthogonal fatal reasons, and the authors have already conceded two of them. **File the upstream bug reports and post the preprint immediately** (that is the paper's most durable real-world impact), then **reframe it as a fidelity-gap *measurement* paper** — retitled, per-file numbers, external financial cross-check, write-path demoted to future work — and submit it to a benchmark/empirical or agent-tooling venue this cycle, where it is a credible accept. Reserve the ICSE/FSE full-paper ambition for a *next* cycle only after building v0.2 and running the agent-in-the-loop A/B on real workbooks with charts/pivots/VBA — that one experiment is the only thing that turns this from a well-measured observation into the enforcement-for-agents result the title promises. The authors' honesty is genuinely admirable, but at a top venue it is being spent to disclose gaps rather than to defend results, and that is the whole problem.