# Semantic-Redundancy Certification — the durable reframe

**Design doc — office-hours output, 2026-07-05**
Status: PROBLEM DEFINED + THEOREM FORMALIZED. Next: prove the exact core, measure tier-coverage.

---

## The problem (bedrock)

AI agents increasingly mutate high-stakes artifacts — spreadsheets, CAD models,
legal contracts, notebooks, config — whose **correctness is defined by opaque,
unspecified software**. There is no accepted way to *verify* such a mutation is
correct, because:
- no formal specification of the format's semantics exists;
- independent reference implementations disagree with each other and with the
  authoritative one (we proved this: LibreOffice reconstructs shared formulas
  differently from Excel);
- the ground-truth engine is closed-source or otherwise inaccessible.

Today you either trust the agent (unsafe) or run the opaque engine and eyeball
the result (unscalable, and it may disagree or be unavailable). As agents do more
autonomous document work, this **verification gap** is what actually blocks safe
deployment — and it is general, not a spreadsheet problem.

## Why this is the durable moat (the future-fit filter)

Run every part of the prior work through "will a better model obsolete it?":
- **Editing correctly** (reference-shift algebra, surgical OOXML) → PERISHABLE.
  A smarter model edits better; the problem shrinks with capability. This is why
  the integration-level contribution hit a review ceiling.
- **Verifying what the agent did** → DURABLE, and it *grows* with capability.

> **The moat, as a law.** In the agentic era the scarce resource is not
> capability — it is *verifiability*. Capability compounds with every model
> release; verifiability does not. It must be engineered, and for artifacts whose
> semantics is defined by opaque software it can only be grounded in the artifact
> itself.

Correctness is a property of the agent; verifiability is a property you engineer.
A superintelligent agent editing Excel *still* cannot prove its edit matches
Excel's semantics — that is an access/information problem, not a reasoning
problem, and intelligence cannot close an information gap it has no access to.
Worse, verifiability is *anti-correlated* with capability: more capable, more
autonomous agents touch higher stakes, so the need for independent verification
rises while "just trust the smarter model" becomes less acceptable (regulation,
liability, audit). Stress-tested against futures (open Excel API, perfect OSS
clone, agent self-verification loops, formal methods) the moat holds: only the
artifact's own embedded ground truth is authoritative, always-available,
agent-independent, and engine-free.

## The mechanism: the self-oracle

**Semantic redundancy.** An artifact `A = (I, O)` carries input content `I` and
embedded output `O` with the guarantee `O = ⟦I⟧` — the outputs were produced by
the authoritative opaque semantics `⟦·⟧`. We hold `(I, O)`; we do NOT hold `⟦·⟧`.

**The self-oracle** is `O`: authoritative input→output observations of `⟦·⟧`,
sampled at `I`, carried inside the artifact for free. Excel already ran and left
its answers in the file. You never needed to run Excel.

Most computational formats have this: spreadsheets (a cached value per formula),
notebooks (cell outputs), PDF forms (field values + calc scripts), CAD
(evaluated geometry + parametric history), build artifacts.

## The three-tier certification theorem (correct-or-refuse, at the theory level)

An **edit** `e: I ↦ I'` admits an **output-transformer** `τ_e` if any faithful
realization satisfies `⟦I'⟧ = τ_e(⟦I⟧)`. **Available engines** `{E_j}` are
individually unreliable (`E_j ≠ ⟦·⟧`); the **trusted support** of `E_j` is
`T_j = {p : E_j(I)[p] = O[p]}` (positions where the weak engine reproduces the
self-oracle on the original).

### Tier 1 — EXACT (the durable core)
> **Theorem 1 (Exact structural certification).** For a relabeling edit `e` with
> position bijection `σ_e`, call `I'` *structurally faithful* if its
> reference-dependency graph is isomorphic to that of `I` under `σ_e`. Then
> structural faithfulness is (a) decidable **exactly and engine-free** by
> syntactic graph comparison, and (b) **sufficient for value-faithfulness under
> any deterministic semantics** `⟦·⟧`: `⟦I'⟧[σ_e p] = ⟦I⟧[p]` for all `p`.

*Proof of (b).* Graph preserved under `σ_e` ⇒ each formula computes the same
function of the same relabeled inputs ⇒ by induction over the DAG (base:
data/constants preserved) the value is invariant, for any `⟦·⟧`. No engine, no
probability, no ground truth. ∎

Rests only on the reference *grammar* (documented, stable), never on the opaque
function semantics. Model-proof and engine-proof. xlq's minimal-patch invariant
was an unwitting syntactic proxy for this.

### Tier 2 — PROBABILISTIC (honest reach)
For transformer-admitting *value* edits (not pure relabelings): certify `I'` when
`E_j(I') = τ_e(O)` on `T_j`. Sound modulo the coincidence that a wrong `I'`
evaluates under `E_j` to the authoritative `O` at a trusted cell — a probability
driven to 0 by the number of independent trusted ground-truth points, the
independence of the engines, and algebraic edit-composition laws (e.g.
insert∘delete = id as a second independent constraint). Same rigor basis as
differential/metamorphic testing, but anchored to *authoritative* ground truth
(`O`) rather than a chosen reference engine. **Derive the explicit bound** as
part of the research plan.

### Tier 3 — REFUSE (the forced boundary)
Edits with no computable transformer, or positions outside `⋃_j T_j` — cells the
self-oracle cannot corroborate with any available engine. This is the exact
informational limit of offline verification: a *consequence*, not a design choice.

## Why it is not vacuous (survives the grilling)
- *vs metamorphic testing:* metamorphic testing has no ground truth (relates two
  runs of the same available system, finds bugs, never certifies). Here `O` is
  authoritative and we *transfer* it across the edit; the target is a *mutation*,
  not the engine. Certification-against-ground-truth ≠ bug-finding.
- *stale/absent cache:* self-detecting (falls out of every `T_j` → refused);
  degrades to trusted support, never certifies on ungrounded cells.
- *smarter model:* correctness ≠ verifiability; the theorem gives offline,
  agent-independent, engine-independent certification whose demand grows with
  autonomy.

## The tell: it re-explains the entire four-round journey
- Forward oracle = a Tier-2 instance (`τ_e` = positional shift, checked vs `O`).
- LibreOffice shared-formula divergence = cells `∉ T_LibreOffice`, correctly
  excluded — not a bug.
- Round-trip oracle worthless = never touched `O` (compared carried caches to
  themselves). The theory says exactly why: no authoritative ground truth was
  transferred.
- "Never silently wrong" = certify-on-trusted-support / refuse-on-uncorroborated,
  now a theorem with a forced boundary.

Everything built for four rounds was an unwitting special case. Strongest
evidence this is the real, durable core.

## The reframed contribution
> **Semantic-redundancy certification** — a theory and method for verifying agent
> mutations of opaque-semantics artifacts against the artifact's own embedded
> ground truth, offline, independent of both the agent and the defining engine,
> with an EXACT core for structural edits (graph-iso ⇒ value-faithfulness) and a
> bounded-confidence frontier, and a forced refusal boundary. xlq is the
> demonstrator that discovered it.

## Machine-checked status (formal/)
- **Theorem 1 is PROVEN in Lean 4** (`formal/SelfOracle.lean`): evaluation is
  invariant under a function-and-dependency-preserving isomorphism, under ANY
  semantics; plus the self-oracle-transfer corollary. `#print axioms` = only
  `[propext, Quot.sound]`, no `sorry` — a complete, constructive machine proof.
  The exact core of the moat is formally verified.
- **The shift algebra is VERIFIED in Z3** (`formal/shift_laws.py`) for all
  positions/ranges/(k,n): insert∘delete = identity (the Tier-2 composition
  constraint), monotonicity, and the 6-case delete clamp correct against
  set-theoretic truth (the path an early theory-review had failed).

## Research plan (next)
1. ✅ **Prove Theorem 1 rigorously** — DONE (Lean, machine-checked). Remaining:
   pin the reference-grammar dependency of the graph-iso check for absolute/mixed
   refs, whole-row/col, cross-sheet, 3D, and prove the check is fully engine-free
   (does resolving names/3D smuggle in semantics? — the load-bearing open question).
2. **Derive the Tier-2 coincidence bound** as f(#trusted points, #independent
   engines, composition laws) — now with insert∘delete=id verified as one of the
   independent constraints.
3. **Characterize the certifiable class precisely** — which real edits are Tier-1
   (exact), which Tier-2, which refused.
4. **Measure tier-coverage on real artifacts** — on the real corpus + cross-part
   corpus: what fraction of edits fall in the exact tier vs probabilistic vs
   refused? This is the headline empirical result (and it is engine-free for
   Tier 1, sidestepping the Excel-oracle blocker entirely).
5. **Generality**: demonstrate the self-oracle on a second format (Jupyter
   notebooks — cells embed outputs — is the cheapest second proof) to show
   semantic redundancy is a general property, not a spreadsheet accident.
6. **Agent loop**: the certificate as a runtime gate an autonomous agent must
   pass to commit — the durable version of the safe-write boundary.

## Resolved: the graph-iso boundary is the static-reference fragment
The load-bearing question — is the graph-iso check engine-free? — is answered.
A formula's *dependencies* (which cells it reads) are what the check needs.
- **STATIC references** — plain A1/absolute/ranges, whole-row/col, cross-sheet,
  3D spans, defined names (resolve via the names table), table structured refs
  (resolve via the table definition), and `INDEX` (reads its whole range
  argument as a static dependency) — are all determinable from *syntax +
  structural metadata*. The graph-iso check is engine-free on them → EXACT tier.
- **DYNAMIC references** — `INDIRECT` (target from a runtime string) and `OFFSET`
  (target from computed offsets) — have a data-computed dependency set,
  unresolvable without the engine → they drop to the PROBABILISTIC tier by
  necessity. This is *exactly* the same boundary the forward oracle already
  needed (it excluded OFFSET/INDIRECT as non-value-invariant): same functions,
  same cause (dynamic dependencies). The theory unifies them.

## MEASURED: the moat is enormous (benchmarks/tier_coverage.json)
On the 231-workbook real corpus, insert-row, ENGINE-FREE:
- **169 of 175 editable files (96.6%) are fully EXACT-tier** — every formula
  static, so the whole structural edit is certified value-faithful under ANY
  semantics with **zero engine, zero oracle**, by the Lean-checked Theorem 1.
- **At the formula-cell level: 24,973 of 25,072 (99.6%) are exact-certifiable**;
  only 99 cells (0.4%) use INDIRECT/OFFSET → Probabilistic tier.
- 49 refused (47 array, 2 table), 6 mixed.

This is the headline: **almost all real structural edits are certified correct
with no access to Excel, no spec, and no ground-truth oracle — the exact blocker
that stopped the prior line of work — purely by a machine-checked theorem, and
durably (no model advance touches a syntactic graph-iso check).**

## GENERALITY PROVEN (experiments/generality/, census.json)
One format-parametric certifier core (`core.py`, no engine invoked) over three
domains confirms the exact tier tracks STATIC DEPENDENCY STRUCTURE (a law), not
the format and not redundancy:
- Excel .xlsx (grid, static refs): 96.6% files / 99.6% cells exact.
- SQLite STORED generated columns (relational, NON-GRID): 100% files; 25/25
  renames certified engine-free AND falsifiably loop-confirmed vs SQLite's stored
  values; a poisoned expr refused + confirmed to change values.
- Jupyter notebooks (real, implicit deps): exact tier structurally ZERO (as the
  static-ref precondition predicts); self-oracle present → probabilistic.
The honest thesis is the INTERSECTION — engine-free certified value-preserving
structural edits over formats with static references that also cache — shown
non-empty beyond the grid (SQLite) with a mechanical boundary (Jupyter). Sharpest
attack ("redundancy = memoization + static-deps welded") conceded and built on.

## Remaining
- The minimal trusted-checker TCB (the parser + graph-iso comparator) as a
  shipped `xlq certify` tool + its independent audit.
- Tier-2 coincidence bound derivation (with insert∘delete=id as a verified
  independent constraint).
- Empirical "what fraction of real agent edits are structural" study (justifies
  the exact tier's value envelope).
