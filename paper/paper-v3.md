# Certify-or-Refuse: A Machine-Checked Soundness Boundary for Untrusted Agent Edits to Opaque-Semantics Artifacts

## Abstract

An LLM agent that edits a spreadsheet, a database schema, or a dbt project produces
an artifact that *opens fine* but may be silently wrong: a structural edit that fails
to propagate references corrupts computed values with no visible symptom, and neither
the agent nor a human reviewer can see it without the defining engine. We ask which
such edits can be certified **engine-free** — offline, against the artifact's own
structure, trusting neither the editor nor a recomputation — and we answer by
machine-checking **both sides of the boundary** (Lean 4, no `sorry`, axioms
`propext` and `Quot.sound` only) — and the middle ground between them (copy edits
reusing witnessed formulas) is structured by an argument-value witnessing criterion
with theorems on both of its sides. On the certifiable side: the premise of our
invariance theorem — the edit's reference-dependency graph is a function-preserving
relabeling of the original's — is *decidable from syntax*, and we ship an executable
decision procedure `check` with a machine-checked soundness theorem: if `check`
accepts, every computed value transports across the edit under **every possible
engine** (the engine is universally quantified, never run), carrying the artifact's
embedded cached values as a self-oracle. On the uncertifiable side: an impossibility
theorem — an edit introducing function semantics unwitnessed in the original admits
two engines that are pointwise indistinguishable through the original artifact yet
disagree on the edited value, so *no* engine-free checker can certify it. Certify-or-
refuse is therefore not a design choice but the only sound shape, and our checker's
accept class is exactly the *proven-certifiable* relabeling class — every accept is
certifiable; the certifiable class provably extends to witnessed copy edits
(Theorem 6), with a fuel-graded residual between the two sides stated plainly. A companion locality theorem factors
mixed edits into a certified structural scaffold plus a bounded value-fill audit cone.
The theory is load-bearing in the running system: the deployed checker agrees with
the Lean decision procedure on a randomized battery, the trusted byte→token layer
itself carries a **verified reference tokenizer** (losslessness, literal opacity,
σ-image — machine-checked) whose corpus-scale differential against the production
tokenizer (1.81M comparisons on 452k real formulas) agrees everywhere on the model
surface after finding a real defect, a production certifier (`xlq certify`) covers
five structural operations, and the same format-parametric core certifies dbt model
refactors engine-free — catching dangling
references and silent logic changes that re-materialization confirms, while certifying
faithful renames with no SQL executed. We are deliberate about what is *proof* and
what is *corroboration*: the machine-checked boundary is the result; the empirical harness
(147/147 corrupted foreign edits refused; a naive edit path silently corrupts 85.5%
of development-tier workbooks — confirmed-genuine after our own study corrected two
mislabels — while the certified path corrupts none; and TWO pre-registered, run-once
**locked tests on external corpora** — EUSES, Enron, and production dbt projects —
in which the guard made **zero false certifications across 1,370 foreign-edit calls**
(v1: 518, surfacing the first of three real defects, all found by our own
verification machinery in the trusted byte layer and since fixed; v2: 852 further; and zero
mismatches across 1,006,997 cell-checks over five operations), measured
fail-closed cost of 19.6–34.3%, and value-collision evidence that sampling-based
checking needs up to 237 cells per file for 99.9% confidence on one real corpus)
corroborates it, and we report
each measurement with its confound — including two false certifications our own adversarial reviews
found in earlier versions of the system, which we closed and report as fixed defects.

## 1. Introduction

The scarce resource in the agentic era is not capability but **verifiability**. A
capable model will, with high probability, perform a structural spreadsheet edit
correctly; but "high probability" is exactly the wrong guarantee for a financial
model, a payroll sheet, or a regulatory filing, where a single unpropagated
reference silently changes a computed total and ships. The failure is invisible by
construction: the edited file is a valid `.xlsx`, it opens, and the wrong numbers sit
in cells whose formulas *look* plausible. You cannot see the corruption without the
engine, and asking the engine is asking the very component whose edit you are trying
to check.

This paper asks a narrower, more durable question than "can the model do the edit?":
**can we certify that a given edit is value-faithful, offline, against the artifact's
own structure, without trusting the editor and without running the defining engine?**
The answer we prove is yes for the *structural* fragment — the coordinate-relabeling
edits (insert/delete rows and columns) that are simultaneously the most common
agent operations and the ones whose damage is most invisible. The certifier accepts a
structural edit only when its reference-dependency graph is the relabeling of the
original's, and refuses otherwise. Because the guarantee is grounded in the artifact
rather than the model, it does not decay as models improve — it is the boundary a
capable agent should be *required* to pass, not a crutch for a weak one.

Our claims, in order of strength:

1. **(The boundary, both sides machine-checked.)** We *bracket* engine-free
   certification of agent edits: function-preserving relabelings of the
   reference-dependency graph are certifiable — witnessed by an executable decision
   procedure `check` for that premise with a machine-checked soundness theorem
   quantifying over **all** engines — while edits introducing function semantics
   unwitnessed in the original are uncertifiable by *any* engine-free checker
   (impossibility: indistinguishable engines disagree), so *certify-or-refuse is the
   only sound shape* (§3, Lean 4, no `sorry`). The middle ground (edits reusing
   witnessed semantics) is structured by argument-value witnessing with theorems on
   both sides and a stated fuel-graded residual (Theorem 6).
2. **(Composition.)** A locality theorem factors a mixed edit into a certified
   structural scaffold plus value fills whose effect is provably contained in their
   downstream cone — collapsing the audit surface from the whole artifact to a
   bounded set (§3, measured in §5).
3. **(Mechanism, theory-linked down to bytes.)** A certify-or-refuse router whose
   deployed checker implements `check` and agrees with the Lean decider on a
   randomized battery; a production certifier (`xlq certify`, five structural ops)
   in translation-validation mode; and — where all three real defects lived — a
   **verified reference tokenizer** for the trusted byte→token layer (losslessness,
   opacity, σ-image; Theorem 7) differentially enforced against the production
   tokenizer at corpus scale (1,810,796 comparisons: 50.3% on the model surface —
   901,946 in-surface + 8,392 guard agreements, zero disagreements; the remainder
   counted and classified: 45.2% ASCII-sheet-qualified formulas, which production
   shifts as DISCLOSED-UNVERIFIED surface outside the model, plus whole-column/row
   and delete-clamp classes), with fail-closed refusal for the classes the
   production tokenizer cannot parse (§4).
4. **(Generality + corroboration, reported with confounds.)** The same
   format-parametric core certifies dbt model refactors engine-free on
   adapter-covered subgraphs (§5.6) — coverage that reached 40% on one production
   project and 0–14% on two macro-heavy ones, a non-transfer we report plainly
   (§5.10); an
   engine-free foreign-edit battery, an independent-oracle A/B, and a
   diverse-corruptor confusion matrix support the theory on real artifacts, each
   reported with its limitation, not as independent proof (§5).

A recurring, honest theme: three rounds of adversarial review of our own artifacts
found real defects — silent-corruption bugs in the trusted tokenizer, a circular
experiment, and an unimplemented soundness boundary — each of which we fixed and
report as a fixed defect, because the discipline of finding them is part of the
contribution (§6).

## 2. The problem: opaque-semantics artifacts and invisible structural damage

A spreadsheet is a program whose semantics live in an engine (Excel, LibreOffice
Calc, IronCalc) that most tools do not reimplement. An edit tool that manipulates the
file's bytes therefore edits a program it cannot evaluate. The dominant failure mode
is the **unpropagated reference**: insert a row above a formula's input and, unless
every reference below the insertion point is shifted, the formula now reads the wrong
cell. The popular `openpyxl` library's `insert_rows` does exactly this — it moves cell
contents but rewrites zero references — so on any workbook with a below-insertion
reference, it produces a file that opens cleanly and computes wrong (§5.2).

The artifact carries its own partial ground truth: Excel writes a cached value `<v>`
next to each formula. This *self-oracle* is what a certifier can check against — but
only for cells the artifact chose to cache, and only if the checker can recompute,
which returns us to the engine. Our theorem removes the recompute: for the structural
fragment, value-faithfulness follows from structure alone.

## 3. The formal core (the contribution)

We model a computation as `C = (fn, deps)`: `fn n` maps a node's ordered dependency
*values* to its value, and `deps n` is its ordered dependency *nodes*. This is exactly
what a formula carries — what it reads and how it combines what it reads — with `fn`
left an arbitrary (opaque) function. Evaluation is fuel-bounded: `eval C 0 _ = default`
and `eval C (k+1) n = fn n (map (eval C k) (deps n))`. The theorem holds at every fuel
level, hence for the true evaluation of any acyclic computation.

**Theorem 1 (exact structural certification).** Let `σ` map every node of `C` to a
node of `C'` with the same function and dependency list relabeled by `σ`
(a function- and edge-preserving graph isomorphism): `C'.fn (σ n) = C.fn n` and
`C'.deps (σ n) = map σ (C.deps n)`. Then `eval C' k (σ n) = eval C k n` for all `k, n`.

*Consequence.* If the original's evaluation is the artifact's embedded ground truth
`O` (`eval C k = O`, the self-oracle), then a structurally-faithful edit reproduces
that ground truth at the relabeled positions, `eval C' k (σ n) = O n`, established
without the defining engine and without recomputing `O`. This is the exact tier of
certify-or-refuse.

**Theorem 2 (locality / audit-surface bound).** Define `agree_upto C C' k n`: the two
computations have identical `fn` and `deps` at every node within `k` dependency-steps
of `n`. Then `eval C k n = eval C' k n` — a node's value depends only on its
dependency cone. Hence a value edit changes nothing outside its downstream cone: a
mixed edit factors into a certified structural scaffold plus value fills whose effect
is provably contained, collapsing the audit surface from "did this corrupt anything
anywhere?" to "are these N cells right?".

Both theorems are machine-checked in Lean 4, self-contained (no Mathlib), with
`#print axioms` reporting only `[propext, Quot.sound]` and no `sorry`
(`formal/SelfOracle.lean`). The reference-shift algebra's arithmetic laws
(insert∘delete = identity, monotonicity, the six-case delete clamp against the
set-theoretic truth) are separately proved for all inputs by the Z3 SMT solver
(`formal/shift_laws.py`).

**Theorem 3 (graph preservation — the shift discharges the hypothesis, for single
cells and range endpoints).** Theorems 1 and 2 *assume* the edited graph is the
`σ`-relabeling of the original's; the tool does not assume it, it produces it by
shifting references. We model a formula as a list of tokens — each an opaque literal
(number, string, function name), a single cell reference, or a range carrying its two
endpoints — and machine-check that the reference-shift produces exactly the relabeled
graph: `refs (shiftF σ f) = (refs f).map σ`, with literals provably untouched
(`formal/RefShift.lean`). This is *verbatim* the `hdeps` premise of Theorem 1, so for
these token shapes the graph isomorphism is discharged **constructively by the tool's
operation**, not assumed. We also machine-check `delete∘insert = id` on the cell maps.
All `sorry`-free, axioms `[propext, Quot.sound]`.

**What Theorem 3 does and does not give.** It is a fusion law parametric in an
*arbitrary total* map `σ : Cell → Cell`: it proves the shift *transports* the reference
graph by whatever `σ` it is given — it does **not** verify that `σ` is Excel's shift
arithmetic (that is the separate Z3-proved algebra), nor does the total-map model
express the asymmetric six-case delete *clamp* (head-in-band → k, tail-in-band → k−1),
the `#REF!` a fully-consumed reference produces, or the true multi-cell interior of a
range (we model a range by its endpoints). So the honest statement is: **the
token→graph→value chain is machine-checked for single-cell references and
range-endpoint pairs under a value-preserving relabeling**; the explicitly *trusted*
surface is the byte→token parse (§4, §5.1), the correctness of `σ` itself, the
asymmetric clamp / `#REF!` algebra, range-interior dependencies, and
sheet-qualified/3D/table/external references. This is a real narrowing of the trusted
base — not its elimination. Fidelity of value edits remains out of the exact tier by
construction.

**Theorem 4 (the premise is decidable, and the decider is proven sound —
`formal/Checker.lean`).** Theorems 1–3 would still be decorative if the running system
never checked their hypothesis. We therefore state the hypothesis of Theorem 1 as an
**executable decision procedure** over syntax alone: `check S₀ S₁ σ` verifies that
every checked node's opaque function *skeleton* is preserved (`S₁.skel (σ n) = S₀.skel
n`), that its dependency list is the `σ`-image (`S₁.deps (σ n) = map σ (S₀.deps n)`),
and that the checked domain is closed under dependencies. The soundness theorem,
`check_sound`, then proves: if `check` returns `true`, every checked node's value
transports across the edit **for every interpretation `I` of the skeletons** — the
engine is universally quantified and never run — and `check_transports_oracle`
carries the artifact's embedded cached values to the edited positions. The procedure
*executes* (`#eval` in the development runs it: a faithful relabeling returns `true`;
an argument-order botch and an operator botch return `false`). The consequence for the
system is architectural: a certifier implementing `check` accepts **any producer's
faithful edit** — regardless of byte-level differences, cache handling, or which tool
made it — and its soundness is this theorem, not equality to a reference transform.

**Theorem 5 (impossibility — the other side of the boundary,
`formal/Impossibility.lean`).** An edit that introduces a function skeleton *not
witnessed anywhere in the original artifact* cannot be value-certified by any
engine-free checker. The proof is indistinguishability: overriding an engine at an
unwitnessed skeleton changes no evaluation of the original artifact
(`eval_override_fresh` — the two worlds agree at every node and every fuel, hence
realize every cached observation identically), yet the edited node's value can be
made to differ from *any* value a checker might commit to
(`fresh_skeleton_uncertifiable`), and two such worlds disagree with each other
(`two_worlds_disagree`) — and the quantification over checkers is itself a
machine-checked object: `no_engine_free_predictor` models a checker's value
commitment as *any* function of the two syntactic artifacts and proves every such
predictor is refuted by an engine consistent with the original. Sound checkers
**must refuse**. One scope note, stated plainly: the impossibility is for
*semantics-agnostic* checkers — a checker carrying a trusted specification of
particular function semantics (say, documented Excel built-ins) has a partial
engine and escapes the hypothesis; that is an oracle assumption, exactly what
"engine-free" excludes. Within that scope, certify-or-refuse is the only sound
shape — proven, not a design preference.

**The boundary, stated.** Together, Theorems 4 and 5 bracket engine-free
certification: edits whose reference graph is a skeleton-preserving relabeling are
certifiable, with an executable proven-sound decider (Theorem 4); edits introducing
unwitnessed semantics are uncertifiable by any engine-free checker (Theorem 5).

**Theorem 6 (the middle ground, structured — `formal/CopyEdits.lean`).** The
remaining class — edits that *reuse* witnessed skeletons at new positions
(copy-pasting an existing formula shape onto new inputs) — is now governed by an
**argument-value witnessing criterion with machine-checked theorems on both sides**.
`copy_value_forced` (axiom-free): a copied node with the same skeleton and equal
argument values as a witnessed application takes the same value under *every* engine;
`copy_certifiable` gives the engine-free checkable premise — a scaffold-transport
hypothesis whose shape is exactly `check_sound`'s conclusion (the composition is
stated on paper; the two developments are self-contained files, so the glue is not
itself a Lean object), plus dependency **oracle values** pointwise equal to the
witnessed application's — forcing the copied value to that application's cached
output under the standing cache-realization assumption. And
`copy_unwitnessed_uncertifiable`: an argument tuple unwitnessed at every fuel up to
the oracle fuel admits an engine override, pointwise indistinguishable through the
original, that refutes any committed value — *given the copied node's own inputs are
pinned* (they evaluate to the same tuple under the overridden engine, e.g. as
transported oracle values; the `hargs` hypothesis) — so unwitnessed copies must be
refused.
Honest residual, stated: the criterion is fuel-graded (a tuple witnessed only below
the oracle fuel falls between the two theorems).

**Theorem 7 (the trusted byte→token layer, verified — `formal/Tokenizer.lean`).**
Every real defect our campaigns found (§6) lived in the byte→token parse the earlier
theorems scope out. We therefore place a **reference tokenizer for that layer inside
the machine-checked core**, at the byte level: **T1 losslessness** (`render ∘
tokenize = id` — the tokenizer never invents, drops, or rewrites a byte, making the
double-encoding defect class impossible by construction), **T2 opacity** (the shift
rewrites only reference segments — precisely the property the mojibake defect
violated), and **T3 σ-image**, proved for **total** relabelings (the insert-class σ; the
partial delete/`#REF!` path has no σ-image theorem and remains Z3-plus-differential
territory): the reference list of the shifted segments is the σ-image of the
input's — the invariance theorem's premise discharged from bytes at segment level.
The model refuses all sheet-qualified formulas by construction, so the
qualifier-defect class cannot mis-tokenize. The remaining trusted link — *the
production Rust tokenizer implements this reference on the model surface* — is
discharged empirically: a corpus-scale differential (452,384 unique real corpus
formulas + 315 synthetic battery formulas, × four edits = **1,810,796 comparisons**)
between the executable Lean reference and the production tokenizer agrees
**901,946 / 901,946 in-surface and 8,392 / 8,392 on guard refusals — zero
disagreements**, after itself finding a third real production defect (§6). The
compared surface is 50.3% of all comparisons; the remainder is counted and
classified, not compared: 818,640 (45.2%) ASCII-sheet-qualified formulas — which
production shifts, an *unverified-by-this-model* surface we disclose rather than
claim — plus 79,084 delete-clamp range cases and 2,734 whole-column/row references.

## 4. The certify-or-refuse router and its trusted base

**The router.** Given an original artifact and an untrusted foreign edit plus a
declared structural op, the router computes the op's node relabeling `σ`, and CERTIFIES
iff the foreign edit's reference-dependency graph is exactly the `σ`-image of the
original's; otherwise it REFUSES. Two properties make this sound as a check of
*untrusted* work: under-declaring a change is caught (an unaccounted graph difference
→ refuse), and a wrong `σ` yields a mismatch → refuse. The router is fail-closed: any
condition it cannot certify becomes a refusal.

**Two certifier modes, honestly labeled.** The system ships two implementations of
the router, and we are precise about which carries which guarantee. *(i)
Direct-premise mode* implements Theorem 4's `check` literally: extract both artifacts
into `(skeleton, deps, oracle)` triples and decide the premise — skeletons preserved,
dependency lists the `σ`-image, containment (no unaccounted nodes), oracle values
preserved outside any declared fill cone. Its accept class is exactly the proven-certifiable
relabeling class of §3 — any tool's faithful *relabeling*, regardless of bytes —
and its soundness is
`check_sound`. The deployed implementation agrees with the Lean decision procedure
(run via `#eval`) on a randomized differential battery of faithful edits and four
botch classes — skeleton change, dependency reorder, retarget, dropped dependency —
at 30/30 (`formal/differential_check.py`); we state plainly that this differential
link is evidence of implementation fidelity, not a proof of it. *(ii)
Translation-validation mode* is the production Rust path (`xlq certify`): apply the
tool's own proven structural transform to the original and diff the foreign edit
against it, certifying iff the only differences are stripped caches and number
formats. It is stricter than mode (i) (byte-for-formula identity to the canonical
transform), engine-free, and multi-sheet-safe; its marginal value over *doing* the
edit is trust-topology — the untrusted producer's artifact ships, gated fail-closed,
with the tool off the write path — not a verifier-cheaper-than-prover asymmetry
(there is none in this mode; it recomputes the transform). Mode (i) is where the
theory is load-bearing; mode (ii) is the hardened production gate.

**The trusted base: the reference-shift tokenizer.** Every certificate is only as
sound as the predicate deciding *which tokens in a formula are cell references*. We
initially used syntactic proxies (a 1–3-letter column, an unbounded row) and
adversarial review found these were wrong in three ways that caused *silent
corruption*: a function name whose prefix looks like a cell (`BIN2DEC` → `BIN3DEC`
under a row insert), a function name ending in digits before a paren (`LOG10` →
`LOG11`), and out-of-grid tokens (`XFE9`, `A2000000`) whose column or row exceeds the
sheet limits. We replaced the proxies with the exact **grid-validity** predicate — a
token is a cell reference iff its column is in `A..XFD` (1..16384) and its row is in
`1..1048576`, boundary-gated so an identifier or function call is never mistaken for a
reference. This one rule subsumes all four bug classes.

We validate the tokenizer at two levels. First, **differential cross-checking**
against an independent A1 shifter over 19 digit-bearing Excel functions, out-of-grid
tokens, `$`-absolutes, and ranges, across insert-rows and delete-rows: 175 (formula,
op) pairs, 0 disagreements (`tokenizer_fuzz.py`). This establishes *mechanization
consistency* between the Rust scanner and the reference — but the reference is a
second implementation of *our own* grid-validity spec, so it cannot establish
conformance to a spreadsheet engine's semantics. Second, therefore, **differential value-preservation against an independent engine**:
a blank-row insert is value-preserving under a correct reference shift, so recomputing
the same file with LibreOffice *before and after* the edit must leave every formula's
value unchanged (`tokenizer_conformance.py`, seeded property-based generation over
evaluable formulas — digit-bearing function names, single/mixed/absolute refs, ranges,
strictly-distinct cell values so a mis-shift onto another cell changes the value). We
run this across **all four structural ops** (insert/delete × rows/columns) and against
**two independent engines** — LibreOffice *and* the pure-Python `formulas` engine,
neither of which is xlq's IronCalc (`conformance_v2.py`): **465 formulas checked per
engine, 0 value divergences, with the two engines agreeing exactly.** (The earlier
single-op/one-engine run, `tokenizer_conformance.py`, gave 264 formulas / 0
divergences.) We are precise about what this does and does not establish.
Value-preservation is *necessary but not sufficient* for reference-graph correctness: a
mis-shift of a reference that the formula's value does not depend on (a zero-weighted
term, a non-max argument of `MAX`) would pass undetected, so the count over-counts
references actually *witnessed* by the value. Two independent engines agreeing rules out
a single-engine artifact, but both are open-source engines, not Excel; and
sheet-qualified/3D/table/R1C1 constructs are outside the generator. The
honest label is *differential value-preservation against LibreOffice — a
necessary-not-sufficient corroboration of the trusted parse, complementing the
same-spec fuzzer* — not full conformance to an engine's reference semantics.

**The one undecidable case, made a fail-closed boundary.** A defined name spelled like
a grid-valid cell (`FY2021` = column FY, row 2021; `Q1`; `Tax2020`) is
indistinguishable from a reference *in the formula text*, so the tokenizer would shift
its uses and the edited file would still equal the tool's own (wrong) transform — the
single place `certified ⇒ correct` could be false on a real workbook. It is, however,
decidable from the workbook's defined-names table, which the certifier already holds.
We close it fail-closed: a structural edit on a workbook containing a defined name
that collides with a cell-reference spelling emits a `defined_name_ref_collision`
residual, and the certifier refuses. A workbook defining `FY2021` and using it in a
formula routes to REFUSE; a benign name (`TaxRate`) proceeds. This converts the last
hidden hole into a demonstrated boundary and scopes the verified surface explicitly:
**single sheet, in-grid coordinates, row/column shifts, no name collision.**

## 5. Corroboration on the development-tier corpus (reported with confounds)

Sections 5.1–5.8 measure on the vendored fixture corpus — real-provenance files, but
our de-facto *development tier* (the locked test tier is §5.9).

None of the following is the soundness argument — that is §3. Each supports the
theorem on real files and each carries a limitation we state.

### 5.1 Engine-free certification of untrusted foreign edits

Running the router (format-parametric core) on `openpyxl`'s edits of 172 real
formula-bearing workbooks, engine-free, cross-checked against an independent
LibreOffice oracle: **0 false certifications and 147/147 corrupted foreign edits
refused** (`foreign_certify`). The load-bearing caveat, always reported alongside:
this soundness is bought with heavy refusal — the fail-closed A1 proxy certifies only
about **1 in 23** *faithful* foreign edits, because it refuses every reference form it
cannot fully model (whole-row, whole-column, cross-sheet, table, defined-name). Useful
soundness — a high certify rate on faithful edits — requires the complete parser of
§4, not the proxy. The value is the boundary, not the throughput.

### 5.2 Independent-oracle A/B: the corruption the boundary prevents

On 172 development-tier workbooks, insert-row@2, with LibreOffice (independent of both `openpyxl`
and the tool's engine) recomputing each edit against Excel's cache: the naive
`openpyxl` edit path silently corrupts 149/172 workbooks *as originally measured* —
of which **147/172 = 85.5% is confirmed-genuine reference corruption** after our own
coincidence-bound study (§5.8) surfaced two label mislabels (files whose formulas
carry *zero references of any kind*, where the flagged divergence is LibreOffice
recomputing `ACCRINT` differently from Excel's cache, not corruption; three further
files carry only non-A1 reference classes where corruption is plausible but not
attributable by this oracle). The tool's structural edit is engine-confirmed faithful
on 150/172 and explicitly refuses the remaining 22 — **0 silent corruptions**
(`agent_ab`). We extend this across **all
four structural ops and both independent engines** on generated distinct-value
workbooks (`agent_ab_v2.py`): the naive path silently corrupts insert-rows 100% /
delete-rows 98.8% / insert-cols 94.3% / delete-cols 80.2% (**93.8% overall**), the tool
**0% on every op**, and the `formulas` engine agrees with LibreOffice *exactly* — so the
protection is not a LibreOffice artifact and holds across the op space, not just
insert-row@2. We flag the confound that remains: the openpyxl figure is a corpus/tool
property (openpyxl shifts no references), not an agent-error distribution, and a
competent reference-shifting engine also gets these ops right; the tool's differentiator
is auditability and explicit refusal, which the A/B does not isolate. We found and fixed an oracle false-positive during this study —
`ROW()`/`COLUMN()` are position-dependent and legitimately change on a row insert —
which is why the oracle excludes position-dependent functions.

On the **real** corpus across all four ops we check shift correctness
*deterministically* rather than by recompute, because real-corpus value-preservation is
confounded — LibreOffice recomputes exotic financial/date functions (ACCRINT, CUMPRINC,
DB, DAYS360, TIME) inconsistently with the Excel cache, flagging as "corrupt" files
whose shifted formulas we verified correct by hand. So we compare xlq's output formulas
to the two-engine-validated reference shifter over ~6,000 real formula cells
(`shift_correctness_real.py`): **xlq is 100% correct on every op** (insert-rows 1651
cells, delete-rows 1608, insert-cols 1648, delete-cols 1076; 0 mismatches), while the
naive path leaves 17–72% of shift-requiring cells wrong. Constructs the simple checker
cannot independently verify (whole-column/row, cross-sheet, tables, range-with-function
endpoint) are skipped, not guessed; every early "mismatch" was a checker bug (comparing a
deleted-band cell to the wrong output cell; the reference shifter not handling `F:F`,
which xlq correctly shifts to `G:G`), and xlq had zero real errors.

### 5.2b A fifth op — move-rows — shows σ is not special to insert/delete

Theorem 1 holds for *any* function-and-dependency-preserving isomorphism `σ`, so the
certifiable class is not limited to the monotonic insert/delete shift. We add **move-rows**
(relocate a contiguous block of rows), whose `σ` is a *permutation* — proved a bijection
in Z3 (injective + surjective on `[1, maxrow]`, both directions) — and whose physical
edit reorders rows (a buffered, non-streaming rewrite). A range whose endpoints reorder
under the permutation cannot be a shifted rectangle, so it fail-closes as a
`move_straddles_range` residual. On the same real corpus, deterministic move-rows shift
correctness is **100% on 1,538 real formula cells** (25/60 workbooks fail-closed as
residual/straddle), and the certify-or-refuse contract holds: `certify(orig, correct
move)` CERTIFIES while a botched move (one reference reverted) is REFUSED. This is a
concrete demonstration that the exact tier extends to a genuinely non-monotonic
structural edit by supplying the right `σ`, with no change to the theorem.

### 5.3 Independent-oracle confusion matrix for the production certifier

We adjudicate `xlq certify`'s verdicts *independently of the tool* over **diverse**
corruption. An initial matrix used a single corruptor — `openpyxl`'s deterministic
no-op-shift — and the Excel cache with a reliability gate; its `FN=0` on 14 edits was
a point estimate (rule-of-three ≈ 21%) confounded with editor identity. We diversified
to three corruptor types — `openpyxl` (no-op), `unshift_one` (revert one shifted
reference), and `wrong_delta` (over-shift one reference) — with ground truth **by
construction** (we inject the corruption, so its label is independent of any engine)
and a LibreOffice *self-consistent* oracle as cross-check (recompute before/after; no
reliability gate). All 45 injected corruptions were refused (each type 15/15) and all 15 faithful
tool-produced edits certified (`cert_confusion_v2`). We are careful about what this
does *not* show: because the certifier accepts iff the edit equals the tool's transform,
and all three corruptors are *defined* as deviations from that transform, `FN=0` here is
**by construction**, not a sampled bound — we do not attach a confidence interval to a
structurally-guaranteed zero, and `unshift_one`/`wrong_delta` differ only in the sign of
a perturbation the certifier does not weigh. The one *reachable* false certification is
not in this family at all: the cell diff compares only sheet cells, so a foreign edit
that shifts every cell formula correctly but leaves a **non-cell reference** (a defined
name's target, a data-validation or conditional-formatting formula, a chart series)
unshifted is invisible to it. We built exactly that edit (`cert_noncell_test.py`): with
a defined name `Rate = $A$10` left unshifted while all cells moved, an earlier `xlq
certify` **CERTIFIED it with all-zero diffs — a genuine false certification.** We closed
it: certify now checks that defined names match the tool's transform (refusing the
unshifted one, certifying the correct one) and *fail-closes* on data-validation,
conditional-formatting, and chart parts it does not compare — a stated coverage
boundary. Separately, 6 of the cell-formula corruptions were **value-preserving** (a
reference error that does not change the current value); the value oracle called them
faithful but certify refused all 6 — its structural equality is *stricter* than a value
check. The symmetric cost, stated plainly: certify equally refuses a *faithful but
non-identical* rewrite (a commuted `A1+B1` → `B1+A1`), so its accept class is
"byte-for-formula identical to the tool's transform," narrower than "value-faithful."

### 5.4 The interventional finding: differential testing hardened the trusted base

We ran a live fast-model agent on 20 real workbooks, tasking it to rewrite formula
references for insert-row@2, and compared its output to the tool's transform. As a
guard-soundness experiment this is **circular** — the "agent correct" label and the
`xlq certify` verdict are the same predicate applied to a file grafted onto the tool's
own output — and we do not report it as such. Its genuine, non-circular yield is
**differential testing that surfaced real silent-corruption bugs in the tool's own
tokenizer** (`BIN2DEC`, `Sales2020`, and on follow-up `LOG10` and the out-of-grid
class), each now fixed and covered by §4's cross-check. A validation method that finds
real bugs in its own trusted base is exactly what a certifier's TCB needs; the honest
framing is that the method proved its worth, not that it produced a soundness number.

### 5.5 Composition on mixed edits, measured

By Theorem 2 a mixed task factors into a certified structural scaffold plus value
fills contained in their downstream cone. We measure this on realistic mixed edits
(`composition_coverage.py`): every scaffold is certified, and the audit surface
collapses to the fill cone — 100/84/68/52% of the artifact certified untouched as the
number of fills grows (mean 76%) — so on a realistic edit-distribution study the
certifiable *component* rises from 27% of tasks fully certified to **87% with a
certified scaffold**.

### 5.6 A second real domain: dbt model refactors, certified engine-free

The theory is format-parametric — nothing in §3 mentions spreadsheets — and we cash
that on a practitioner-relevant second domain: **dbt projects**, where models
reference each other via `{{ ref(…) }}`, the everyday edit is a rename/refactor, and
the warehouse's materialized tables from the pre-edit state are the self-oracle. The
adapter extracts the same `(skeleton, deps, oracle)` triple the checker consumes:
skeletons are SQL with references abstracted to ordered slots (case and whitespace
folded *outside* string literals — `'ABC'` is never conflated with `'abc'`; any
unmodeled Jinja marks the node dynamic and out of the exact tier, refused rather than
guessed); the edited artifact is built with **no SQL executed**. Four scenarios on a
ten-node staging→marts project: a faithful rename with all references updated is
**CERTIFIED** (100% of the artifact untouched, no engine run); a rename leaving one
downstream reference dangling is **REFUSED**; a rename with a silently changed
aggregation (`SUM`→`AVG`) is **REFUSED**, and independent re-materialization confirms
the values really differ; a rename plus a *declared* logic change is certified as a
scaffold with the audit surface exactly the declared model plus its downstream cone
(collapse 0.8), and re-materialization confirms outside-cone values are preserved.
Honest scope: model-level granularity (the cone is whole models, not columns); a
mini-dbt subset (single-argument `ref`/`source`; no macros); and fail-closed
normalization — a comment-only reformat is refused, never wrongly certified.

### 5.7 The interventional study: live agents, guarded vs unguarded, with cost

Finally, the experiment every earlier version of this work lacked: **live agents'
own errors**, caught by the direct-premise checker, with completion cost measured.
Design, with the independence structure stated: 21 real-workbook tasks (196 formula
cells; task prep excludes constructs the guard cannot see, so measured cost
*understates* real-world refusal cost — stated, not hidden); the agent's artifact is
built via `openpyxl` plus surgical formula splicing with the reference tool nowhere
in it; ground truth is the two-engine-validated reference shifter (cells outside its
grammar are excluded and counted, never guessed); the guard is the direct-premise
checker of §4.i; and two live agent conditions (a careful and a hasty prompt, same
fast model) make the error distribution the agent's own rather than injected. The
harness was first validated on synthetic perfect/sloppy agents (perfect: zero
corruption in both arms; sloppy: all seven injected corruptions blocked).

Results: the careful agent erred on 2/21 tasks, the hasty agent on 4/21 (six saves
total across the two arms — which share tasks and model, so they are not independent
runs — spanning four distinct workbooks; with n this small we report counts, not
rates: 6/6 blocked has a 95% binomial lower bound of 0.54). Unguarded, all six
erroneous artifacts ship as silent corruption; four are content errors and two are
protocol-keying errors (the agent's formulas were right but returned under shifted
addresses — interface-dependent corruption, still corruption as shipped). Guarded,
**zero ship — all six are refused, with zero false certifications on the 137/196
truth-visible cells and zero unambiguous completion cost**. The symmetric caveats,
both stated: certified artifacts contain truth-blind cells (11 of 19 certified tasks
carry cells outside the truth grammar), where corruption would be invisible to the
truth instrument just as hidden saves are — the instrument cannot contradict the
guard there in either direction; and the two refusals of "correct" hasty work are on
truth-partial tasks whose unverifiable cells may themselves be wrong — possible
hidden saves, split-reported. Two findings deserve emphasis. First, the live error
distribution *naturally* contained the adversarial classes our earlier synthetic
corruptors were criticized for omitting: partial shifts (`F.INV(F2,B3,C3)` — two
references shifted, one missed), a protocol misread (formulas returned under
already-shifted addresses), and dropped cells. Second, one careful-agent error was a
**semantics-changing edit with correctly shifted references** — `TEXTSPLIT(A26,,"")`
rewritten as `TEXTSPLIT(A27,"")`, silently turning a row-delimiter argument into a
column-delimiter — and the graph check caught it because the function *skeleton*
changed even though every reference moved correctly: precisely the class a
value-spot-check or a reference-only linter would miss.

### 5.8 The probabilistic tier, quantified: the coincidence bound

Everything outside the certifiable class routes to probabilistic checking against the
self-oracle — and until now "probabilistic" was unquantified. We derive the
**coincidence bound**: the probability that a *wrong* edit passes a k-cell value
check because the misread cells coincidentally carry colliding values. The naive
independent bound `q^k` is badly optimistic: on the real corpus the honest,
dependence-aware mixture bound (within-file value repetition dominates) is **30×**
higher at k=5 and **~2×10⁴×** at k=10, and tracks a Monte-Carlo simulation on real
formulas within ~25% at every k. Measured on 230 real first sheets under Excel read
semantics, the off-by-one-row collision rate — exactly the naive edit path's error —
is q̂ = 0.178 pooled (file median 0.125, p90 0.40; spreadsheets are full of repeated
values). Detection therefore needs **k = 5 checked cells for 99% and k = 9–10 for
99.9%** (the mixture bound is itself still mildly optimistic within-file — the MC
sits just above it at k = 10) against *systemic* errors — double the naive prescription. Against
*localized* errors the tier is coverage-bound, not collision-bound: a single
mis-routed reference corrupts one cell plus its cone, so a k-of-N sampled check
detects it with probability at most k/N regardless of q. And the hard limit,
engine-verified **on this corpus** (engine calc-test suites plus templates — not
in-the-wild business workbooks): **of 161 workbooks where the naive path's error is
genuinely present, 19 (11.8%; conservatively 12 = 7.5%) pass a full k = N value
check** — LibreOffice recomputes identical values on a *wrong reference graph*
(aggregates, `MIN`/`MAX`, `MODE`-class functions absorb the misread). The
passing files are dominated by test suites for exotic non-injective functions,
over-represented here by construction; the floor's magnitude plausibly *shrinks* on
arithmetic-dominated business sheets, and we state that direction rather than claim
transfer. This is the quantified case
for the exact tier: value checking, even exhaustive, cannot close the gap that the
graph check closes by construction.

**In-the-wild transfer, measured (locked test, §5.9):** on 761 EUSES and 777 Enron
first sheets the off-by-one collision rate is *higher* than this corpus — pooled q̂ =
0.478 and 0.566 respectively (file-median 0.104/0.245), pushing 99.9%-detection to
k = 12 and **k = 20** checked cells. The direction we pre-stated held: the
probabilistic tier is substantially *weaker* on real business sheets, which
strengthens, not weakens, the case for the exact tier.

### 5.9 The locked in-the-wild test: pre-registered, run once

Every number above §5.9 is measured on the vendored fixture corpus — our de-facto
development tier. To answer the ecological-validity question, we ran a **locked test**:
protocol and nine predictions committed to the repository *before* any test data was
downloaded (research-log/016); corpora untouched by development — **EUSES** (796
converted workbooks, CC-BY-4.0, checksums matched to the canonical Zenodo record) and
**Enron** (786, CC0, FERC-released business spreadsheets), both `.xls`→`.xlsx` via
LibreOffice (disclosed caveat: cached values are engine-regenerated). One sampling
caveat the pre-registration did not anticipate: conversion capacity forced a
deterministic *lexicographic-prefix* subset at acquisition (first 800 sorted paths per
corpus; Enron additionally a first-1,500-of-20,872 archive prefix for disk budget) —
applied before any content inspection but *not* pre-registered, and for EUSES the
prefix collapses to essentially one category (791 of 796 files from the `database`
folder, 9 from `cs101`, out of 4,652 files in 11 categories). EUSES-leg numbers
should therefore be read as the *database category*, not the full corpus, and
cross-corpus generality claims are correspondingly tempered; plus one **real
production dbt project** (Mattermost's data warehouse, 254 models). The dbt leg's
provenance is honestly *weaker* than the xlsx legs': the pre-registered GitLab target
went private, the pre-registered fallback (a demo project) was bypassed for the
better real-production substitute, and the substitution rationale, leg harness, one
harness fix (an identical source-oracle sentinel on both sides — a rename does not
touch the warehouse), and results all attest in a single commit rather than a
pre-inspection commit. Rules: run once; measurement-harness bugs fixable with
disclosure (four were: the dbt sentinel above, a cell-association regex artifact, XML
entity decoding, a per-file watchdog — after each xlsx-side fix the development-tier
results re-verified byte-identical, and the two mismatch-reducing fixes are further
justified by XML semantics independent of results: a self-closing `<c/>` cannot
contain an `<f>` child, and `&quot;` denotes the same formula as a literal quote);
the systems under test frozen.

Results against the pre-registered predictions (all scored in the repository's
prediction ledger — 4 confirm, 3 disconfirm, 2 partial):

- **The central claim held on data development never touched:** **zero false
  certifications** across all 518 completed `certify(openpyxl edit)` calls — 503
  refusals (158 EUSES + 345 Enron) plus 15 fail-closed errors — and 7 further edits
  failed before certification; **no would-corrupt edit was certified**.
- **Reference-shift arithmetic: 0 errors in 283,960 real formula cells** across the
  four ops (Enron: 170,796 cells, 100%). The checked-cell denominator is bounded by
  the truth grammar: cells outside it are skipped, never guessed — and the largest
  skipped class is *cross-sheet formulas* (on Enron, skipped cells exceed checked
  cells per op), which is exactly where the post-review qualifier defect below lives. The *only* defect the test found — 38 cells
  (0.034% of the 113,164 EUSES-leg cells), all in one Japanese workbook — was **not** a shift error: xlq
  double-encoded non-ASCII string literals (a Latin-1 byte misread in the rewrite
  path), silently corrupting literal *text* while shifting references correctly.
  This landed **exactly in the layer §3 explicitly labels trusted-not-proven** (the
  byte→token surface) — the scoping was load-bearing, not decorative. No foreign edit
  in the corpus was falsely certified because of it; we note precisely that
  over-refusal is guaranteed only for *correctly-encoding* foreign tools — a foreign
  tool sharing the identical byte-misread would produce byte-identical corrupted
  literals and be certified, since the mojibake transformation is deterministic and
  tool-independent. It also means the tool's own gates passed over the 38 corrupted
  cells (both certify sides share the encoder). The defect was frozen during the test
  per protocol and is fixed with regression tests since (§6). Predictions of zero
  mismatches: *disconfirmed* on EUSES, confirmed on Enron — reported as scored.
- **Fail-closed cost, measured:** the full guarded pipeline (transform residual ∪
  own-certify not certified) refuses 19.6% (EUSES) and 32.0% (Enron) of eligible
  files — dominated by the `unverified_reference_part` denylist and loader limits on
  converted exotica. This is the honest price of soundness on real artifacts, and we
  report it as such rather than tuning it away post-hoc.
- **Would-corrupt prevalence:** 69.3% (EUSES, below our predicted range) and 89.2%
  (Enron, within it) of eligible files carry at least one reference a naive edit
  path would silently corrupt.
- **dbt (production project):** mini-adapter parse coverage 40.2% (*above* the
  predicted <30% — a favorable disconfirmation; 152 models with unmodeled Jinja fail
  closed), and on the 79-model closed subgraph the certify legs behaved exactly as
  the theory demands: a faithful rename of a staging model with four real dependents
  CERTIFIED with no SQL executed; a dangling-reference botch REFUSED.

The test converts the fixture-tier headline into a test-tier statement: *the guard's
soundness transferred to in-the-wild artifacts unchanged; its costs are real and now
quantified; and its one failure was found by our own protocol, in the layer our own
theory declared unproven.*

### 5.10 Locked test v2: full corpora, five ops, the fixed system

A second pre-registered, run-once test (10 predictions committed before acquisition)
widened v1's scope: EUSES converted in full (4,648 workbooks, 11 categories) with the
shift/guard legs running on the pre-registered **first-500-eligible cap** — which in
sorted order spans **six** categories (cs101 4, database 162, filby 30, financial
270, forms3 21, grades 13 — recomputed against the locked eligibility counters; only
the value-collision leg used the full corpus: 4,497 files measured, 4,432 in the
off-by-one model's distribution) — plus a
**seeded-random Enron sample** (replacing v1's lexicographic prefix), all **five**
structural ops, a cross-sheet-capable truth grammar, the measurement artifacts v1's
post-hoc analysis attributed (both eliminated exactly as predicted — the error class
went to zero), and the system under test at the thrice-fixed binary.

Two disclosed protocol deviations. *(i)* The third fix (the range-head defect) landed
after the pre-registration commit but before the run; we originally described its
discovery as development-tier only — **that was wrong**: the differential's formula
corpus included the v1 locked corpora, and 163 of the 500 EUSES shift-leg files are
byte-identical v1 copies. The Enron leg is the **less-contaminated headline** (690,251 cell-checks, zero
mismatches): 27 of its 362 eligible files (7.5%) are re-conversions of source
workbooks whose v1 conversions fed the discovery corpus — zero byte-identical, the
overlap is at source-document level — versus 163/500 (33%) byte-identical v1 copies
on the EUSES leg; the fix also cannot manufacture agreement against the independent,
engine-validated truth grammar — but the provenance is reported as it is. *(ii)* The
pre-registration said function-endpoint ranges "stay out-of-grammar"; the shipped v2
grammar admits them with semantics written to match production's fixed behavior — an
unregistered, anti-conservative widening whose truth semantics are co-constructed
with the code under test, breaking independence on exactly that class (6 of 452,384
corpus formulas); the class's correctness rests on Excel semantics + the Lean
reference, not on the independent shifter, and we disclose it as such. Scored: **5 confirm / 3 disconfirm / 2
partial.**

- **Shift correctness: 1,006,997 cell-checks across five ops, zero mismatches**
  (EUSES 316,746; Enron 690,251; a cell checked under several ops counts once per
  op — distinct physical cells ≤ ~227,879). Checked volume grew 4.0× on Enron
  and 2.8× on EUSES — both confounded: Enron v2 is a seeded-random *resample*
  (zero byte overlap between the converted corpora), and EUSES has 3.1× more files, on
  top of the widened grammar and the fifth op. The new fail-closed guard refused three
  real non-ASCII-qualifier files in the wild.
- **Zero false certifications on 852 further foreign edits** (496 EUSES + 356
  Enron; 163 EUSES files repeat v1's deterministic edit byte-identically, the
  rest are fresh) — with v1, 1,370 foreign-edit calls have produced no false
  certification (fail-closed errors and timeouts counted separately and disclosed
  per leg).
- **The probabilistic tier collapses in the tail**: the random Enron sample
  contains near-check-blind files (2 of 761 drive the tail); 99.9% detection
  requires **k = 237** checked cells there (EUSES corpus: 18). Together with the
  dev-tier full-check blind-spot floor (§5.8), this grounds the claim that
  sampling-based value checking cannot certify real business data at high
  confidence — on these corpora the exact tier is a necessity, not an
  optimization.
- **The fail-closed cost is structural and levered**: 21.2% (EUSES cap sample) and 34.3%
  (Enron-random) — both *above* our artifact-corrected predictions (disconfirmed:
  the fuller samples simply carry more denylist parts), with **externalLinks the
  sole cause of 64% of Enron's denylist refusals** (zip-grounded attribution,
  generator committed) — verifying that one part class would roughly halve the
  cost. Prevalence on the EUSES cap sample: 94.6%, *above* v1's database-category
  69.3% (our "lower" prediction disconfirmed — the cap's database+financial mix is
  richer in formulas, not poorer).
- **Two-model agent study** (21 tasks — the corpus's honest ceiling — × fast/mid
  tiers): the mid-tier model made the study's one error — three corruption modes in
  a single file (unshifted references, dropped `$` absolutes, and an *invented*
  function argument) — refused by the guard; zero false certifications; refusals of
  correct work were 0 in both live arms (v1 live: 0 careful, 2 hasty truth-partial).
  A like-for-like measurement of the range-head fix's cost effect uses the
  deterministic synthetic perfect-agent arm: its refusals fell **5 → 4** pre-fix to
  post-fix — the fix eliminated exactly the one refusal class it addressed, no
  more. The pre-registered "mid-tier errs less" prediction was disconfirmed (1 vs 0
  — counts at trivial error rates).
- **dbt does not transfer**: two further production projects yield 0.0% (the
  2,484-model `daily_spellbook` subproject of the 7.5k-model spellbook repo — every
  one of its models opens with a `{{ config() }}` macro the fail-closed adapter
  treats as dynamic) and 13.7% (cal-itp, 619 models) adapter coverage. The Mattermost
  40% is not representative of macro-heavy production dbt; the format-parametric
  claim holds at the theory level, the current adapter subset does not, and we say
  so plainly (config-stripping, semantically inert for the dependency graph, is the
  disclosed future-work lever).

## 6. On finding our own defects

Successive adversarial reviews of these artifacts each landed a real hole: the
tokenizer's syntactic proxies (silent corruption); the circular live-agent experiment
(a tautological "0 false certifications"); the unimplemented defined-name *aliasing*
boundary; and — most consequentially — a **reachable false certification** in the
production certifier itself, where a foreign edit that shifts every cell formula
correctly but leaves a defined name's target unshifted was certified with all-zero
diffs, because the cell diff never compared non-cell references. We built the exploit,
confirmed it, and closed it (defined names now compared against the tool's transform;
data-validation, conditional-formatting, charts, pivots, and external links
fail-closed). A subsequent review then falsified a *universal* phrasing of that fix by
the same pattern — a foreign edit that reverts a `mergeCell`/`hyperlink`/`autoFilter`
reference, which the transform shifts but the cell diff never compared, was certified —
so we extended the net to those semantic structural references and reframed the boundary
as an explicitly *enumerated denylist* whose completeness we do not claim to have proven.
The coincidence-bound study (§5.8) surfaced label noise in our own
headline A/B: two "corrupted" files carry zero references of any kind, so their flagged
divergence is engine disagreement, not corruption — we corrected the headline from
86.6% to 85.5% confirmed-genuine and record the mislabel here. Most recently, the
locked in-the-wild test (§5.9) found a **real silent-corruption defect in the tool
itself**: the formula rewrite walked bytes and copied non-ASCII scalars as Latin-1
codepoints, double-encoding string literals — references shifted correctly, literal
text corrupted, on exactly the trusted byte→token surface §3 scopes out of the
proofs. Per the pre-registered protocol the tool stayed frozen for the test (the 38
affected cells are reported as measured); the defect is since fixed with regression
tests covering the exact failing shape and all UTF-8 planes — and we state plainly
that the tool's own certify gate passed over those 38 corrupted cells (both sides of
an own-transform comparison share the encoder), so the defect was caught by the
locked test's independent truth instrument, not by the guard. A granted post-test
review round then found a **live sibling** by attacking the fix's boundary: the
tokenizer's unquoted-sheet-qualifier grammar is ASCII-only, so a CJK-named sheet's
unquoted qualifier (`集計01!CI3`) mis-tokenizes and the shift silently leaves the
reference stale — a class present thousands of times in the locked corpus yet
structurally invisible to the locked harness, whose truth grammar skips all
cross-sheet formulas. Rather than extend the grammar (new trusted surface), the fix
is fail-closed: a detector refuses any edit whose formulas carry an unquoted
non-ASCII qualifier, with regression tests and an end-to-end refusal check; the
locked numbers stay as measured. Finally, the **verified-reference differential**
(Theorem 7) found a third production defect in its first full-corpus run: a failed
range-kind parse (`A2:CHOOSE(...)` — a range whose tail is a function call)
*swallowed* the valid head reference, leaving it unshifted — value-preserving for
`SUM` by accident, wrong for `COUNT`-class functions (3 disagreements in 1,810,796
comparisons, all this shape). Fixed with regressions; the post-fix differential
agrees everywhere on the model surface; and the fix *measurably reduced the guard's
cost*: the like-for-like synthetic arm's refusals of correct work fell 5 → 4 —
exactly the one refusal class the fix addressed, because the previously-documented
guard-vs-tool divergence on this construct disappeared (§5.10). The live arms also
went 0/2 (v1) → 0/0 (v2), but across different agent conditions and with v1's two
refusals on unrelated constructs — confounded corroboration, not part of the
measured effect. Three defects, three layers of the same trusted surface, each found by
the project's own verification machinery.
We report each as a fixed defect. This is part of the contribution: a certify-or-refuse
claim is only credible if its authors have tried hardest to break it — the record of
what broke, and what the fix was, is the evidence that the remaining boundary is real,
and it is why we are conservative about calling the corroboration anything more than
corroboration, and the denylist anything more than a denylist.

## 7. Scope and limitations

The **verified surface** the certifier certifies (rather than refuses) is: row/column
insert/delete *and row-block move* (§5.2b), single-sheet, in-grid coordinates, cell
formulas over single-cell and range-endpoint references, with no defined-name/cell
collision and (for move) no move-straddling range. On top of the sheet-cell
diff, certify explicitly compares the reference-bearing constructs the transform shifts
that a cell diff cannot see — defined-name targets and mergeCell/hyperlink/autoFilter
`ref`s — refusing any that differ from the transform, and fail-closes on
data-validation, conditional-formatting, sparklines, charts, pivot tables, and external
links. Two honesty caveats, both learned from this artifact's own failures. First, this
compare-and-fail-close net is an **enumerated denylist**, not a machine-checked
allowlist: its completeness over value-affecting non-cell references is *asserted, not
proven*, so an un-enumerated value-bearing construct would be silently certified — the
exact failure the defined-name case exhibited until a reviewer built the exploit. (We
keep certify's compare surface a superset of the transform's *value-bearing* write
surface — every reference-bearing construct except the deliberately-excluded view-state
`dimension`/`selection`/`pane`/`brk` — which makes the enumeration mechanically checkable
against `has_ref_attr`, but that superset relation is a code invariant, not a proof.) Second, the guarantee is over **computed values**:
pure view-state the transform also shifts — the used-range `dimension`, `selection`,
frozen `pane`, and page `brk`s — is deliberately *not* compared, because it is
non-value-bearing and foreign tools legitimately vary it; a certified file may differ
from the transform there without affecting any value. Value edits are out of the exact
tier by construction. The **trusted base** (narrowed by Theorems 3 and 7, not eliminated) is:
the *production* byte→token parse — now reduced to its conformance with the verified
reference tokenizer on the model surface (empirical: the 1.81M-comparison
differential) plus the ASCII-sheet-qualified surface the model does not cover; the
correctness of the shift map `σ` itself (the Z3-proved arithmetic, not re-proved in
the graph layer); the asymmetric six-case delete clamp and its `#REF!` outcomes; the
multi-cell interior of ranges (modeled by endpoints); the constructs absent from the
token type; and the completeness of the non-cell denylist. The
corroboration is bounded accordingly:
value-preservation is necessary-not-sufficient (though now confirmed across all four
structural ops and by two independent engines that agree, ruling out a single-engine
artifact, and still not Excel); the confusion matrix's `FN=0` is by-construction over
deviations-from-transform, so the informative test is the non-cell corruptor of §5.3,
which found and closed a real false certification. The exact tier certifies a measured
37.5% of operations and 27% of whole tasks on a realistic edit-distribution study; the
majority of real tasks are mixed and need the probabilistic tier — but by Theorem 2 a
mixed task factors into a *certified structural scaffold* plus a bounded value-fill cone,
and we measure this (`composition_coverage.py`): on mixed edits the scaffold is always
certified and the audit surface collapses to the value-fill cone (100/84/68/52% of the
artifact certified untouched as fills grow, mean 76%), so the certifiable *component*
rises from **27% fully certified to 87% with a certified scaffold**. Theorem 7
delivers the verified byte-level *reference* tokenizer; verifying the *production*
implementation itself (extraction, or Rust verification tooling) and a model faithful
to the clamp/`#REF!` algebra are the natural next steps.

## 8. Related work

The 2026 wave of LLM-spreadsheet-agent benchmarks documents precisely our failure
class — Spreadsheet-RL [arXiv:2605.22642] names index-shift and reference-translation
hazards and enforces an inspect–modify–verify loop; MBABench [arXiv:2605.22664] and
BlueFin [arXiv:2605.30907] score structure and formula transparency — but all three
verify **engine-in-the-loop** (recalculate and read) or by rubric/LLM judge, an
evaluation MBABench's own authors call "inherently ambiguous, difficult to verify"
(BlueFin similarly concedes theirs is "difficult to verify programmatically"); none
certifies offline. Spreadsheet analysis and
smell detection, structural-edit tools that reshape files without a fidelity
property, and record/replay or unit-test approaches to spreadsheet correctness all
differ from this work in the same way: they establish behavior on
tested inputs, whereas we prove value-fidelity of the structural fragment for all
inputs and all semantics, and gate untrusted edits on that proof. **Translation
validation** [Pnueli, Siegel, Singerman, TACAS 1998] and verified compilation are the closest methodological
analogues, and our production mode (§4.ii) is honestly an instance of TV's weakest
form — accept iff the output equals a proven reference transform. The delta of this
work over TV is twofold. First, the *direct-premise mode* (§4.i) does what TV does
not: it decides the invariance theorem's hypothesis directly on the untrusted
artifact, so its accept class is the entire *proven-certifiable* relabeling class
(any producer's faithful relabeling, not only outputs identical to a canonical
transform; certifiability provably extends to witnessed copy edits — Theorem 6 —
with a fuel-graded residual), and its
soundness quantifies over **all** engines — TV assumes the semantics it validates
against, whereas our setting's defining engine is opaque and absent. Second, TV has
no analogue of our impossibility side: Theorem 5 shows the refusal half of
certify-or-refuse is forced, bracketing the boundary rather than picking a point
on it. Proof-carrying code shares the shape of "checkable evidence, small checker,"
and our decision procedure plays the checker's role with the certificate being the
edit descriptor `σ` itself.

## 9. Conclusion

Verifiability, not capability, is the durable constraint on agent edits to
opaque-semantics artifacts. We bracketed — machine-checked on both sides, with the middle ground structured
by argument-value witnessing (fuel-graded residual stated) — what
can be certified about such edits without the engine: the relabeling class is
certifiable, witnessed by an executable decision procedure proven sound under every
possible engine; anything introducing unwitnessed semantics is not, so
certify-or-refuse is the only sound shape. The theory is load-bearing in the running
system — the deployed checker agrees with the Lean decider, the production gate covers
five structural operations on real spreadsheets, and the same core certifies dbt
refactors engine-free where its adapter reaches (a coverage boundary we measure and
report) — and every measurement is reported with its confound. The
result is an honest, genuinely-verified boundary over an explicitly-scoped surface:
the boundary a capable agent should be *required* to pass, not a patch for a weak
one — and one that does not decay as agents improve, because it is grounded in the
artifact, not the model.
