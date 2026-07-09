# The Coincidence Bound — how much confidence a k-cell value check can honestly buy

**Question.** The probabilistic tier of certify-or-refuse checks a claimed
value-preserving edit against the *self-oracle* (the Excel-cached `<v>` next to
each formula): pick k checked cells, compare each expected value to its cache,
PASS iff all match. If the edit is actually **wrong** — a reference points at
the wrong cell — what is the probability it passes anyway, because the wrong
cell coincidentally holds a value that leaves the output unchanged?

**Honest verdict up front (mixed, mostly good news for the tier design):**

1. The per-check coincidence rate is **substantial**: for the empirically
   dominant error (off-by-one row shift) a single checked read passes by
   coincidence **q̂ ≈ 0.18** of the time on the real corpus (adjacent cells
   equal in value ~1 time in 5.6). Spreadsheets really are full of repeats.
2. The naive independence bound `q^k` is **optimistic by orders of magnitude**
   at useful k (30× at k=5, ~20,000× at k=10 vs. the Monte Carlo on real
   formulas). The *mixture* (dependence-honest) bound derived below tracks the
   MC within ~25% at every k — that is the bound to use.
3. Under the honest bound, **systemic** errors (every reference off by one)
   are still detectable with modest k: 99% at k≈5, 99.9% at k≈9 — roughly
   double the naive prescription. This part of the tier is *not* vibes.
4. But two structural limits are now quantified, and they are the real case
   for the exact tier:
   - **Localized errors are coverage-limited**: one wrong reference corrupts
     one cell; a k-of-N sampled check detects it with probability ≤ k/N no
     matter how good q is. And even **k = N caps at 1 − q ≈ 86–89%**
     detection per single mis-reference — 99% is unachievable by value
     checking alone.
   - **Even k = N has a floor**: by independent-engine ground truth
     (`agent_ab.json` + this study), **19/161 = 11.8%** of really-corrupted
     real workbooks (conservatively 12/161 = 7.5%, see §7) recompute to
     values identical to every compared cache. The reference graph is wrong;
     the values coincide; *no* value check at *any* k can see it. Only
     graph-level (exact-tier) certification can.

Artifacts: `coincidence_q.py` → `coincidence_q.json` (measured q),
`coincidence_mc.py` → `coincidence_mc.json` (Monte Carlo + engine anchor).
Corpus: `/home/soh/aix/vendor/upstream/xlsx/tests/**/*.xlsx` (231 files, 230
first sheets measured; stdlib-only parsing of cached `<v>`, shared strings
resolved, shared formulas expanded).

---

## 1. Setting

A worksheet has cells `A`, cached values `w: A → V` (numbers, strings, bools,
errors; absent = blank). An edit claims value-preservation on a set `S` of
cells. The verifier selects `K ⊆ S`, `|K| = k`, computes each checked cell's
expected value under the claim, and compares with the cache under the check
equality `≡`: relative tolerance 1e-9 for numbers, exact for strings, and —
because that is what a formula read actually sees — **blank ≡ 0 and blank ≡
""**. PASS iff all k match.

"Wrong edit" means: at least one formula reference now resolves to the wrong
cell. Note the category distinction that drives everything below: the error is
a **graph** error (wrong dependency edge); the check observes only **values**.
When the wrong cell's value coincides with the right cell's value, the values
are fine *today* and wrong the moment the referenced data changes — latent
corruption.

## 2. Error models

**M2 — off-by-one shift (empirically dominant).** Every same-sheet reference
reads the cell one row (column) away from its intended target. This is exactly
the standard-tool failure documented in `EDIT_PATH_AB.md`/`agent_ab.json`:
`openpyxl.insert_rows(2)` moves the data down but rewrites **zero**
references, so every reference to row ≥ 2 reads one row above its data —
149/172 real workbooks flagged (86.6% as originally measured; 147/172 = 85.5% confirmed-genuine after this study's own label correction, §7) silently corrupted by the default programmatic
edit path. The Monte Carlo simulates this exact read map: a reference to row 1
is unaffected; to row 2 reads the inserted blank; to row j ≥ 3 reads old row
j−1.

**M1 — localized uniform mis-target.** A single reference occurrence in a
single formula is retargeted to another cell drawn uniformly from the same
column's used span (row-shift bugs preserve the column, and same-column is the
conservative choice — columns are type-homogeneous, so they collide more).
Variant M1a: uniform over the whole used range.

## 3. Reduction: pass probability = value-collision rate q

Let checked cell `s` read misread input `x'` in place of intended `x`. Define
the **collision event** `C(x,x') = { w(x') ≡ w(x) }`.

**Lemma 1 (injective formulas).** If `s`'s formula is injective in the misread
coordinate given its other inputs — every affine formula with a nonzero
coefficient is: `SUM`, `+`, `−`, scaling; also `SUM` over a shifted window,
which telescopes to a two-cell condition — then `s` passes iff `C(x,x')`
holds. So the per-cell pass probability reduces to

> **q = P( w(x') ≡ w(x) )** under the error model's pair distribution
> — the value-collision rate. This is what `coincidence_q.py` measures.

**Lemma 2 (non-injective formulas raise q).** In general
`P(s passes) ≥ P(all of s's misread inputs collide)`: `MAX/MIN/COUNT/IF/
ROUND/MODE/D*`-style formulas can return the same output from different
inputs. Input-level q is therefore a **lower bound** on the per-cell miss
probability; the output-level excess is measured in §6 (factor 2.4–3.5×).

**Multi-read formulas lower input-level q.** A formula reading m misread cells
passes at input level only if all m collide (measured: real formulas read a
median ≈ 2.4 cells, and per-formula input-collision is ~0.02–0.05 vs. 0.18 per
single pair). The two effects pull in opposite directions; the Monte Carlo
(§6) measures their net on real formulas.

## 4. The naive bound, and three honest corrections

**Naive independent bound.** If the k checked cells miss via k *distinct*
collision events, *independent*, homogeneous rate q:

> P(miss) = q^k  detection = 1 − q^k  k(ε) = ⌈ln ε / ln q⌉

All three assumptions fail in practice, each in the direction of *more*
misses:

**(D1) Shared misread pairs — only d ≤ k distinct pairs count.** If several
checked formulas read the *same* misread cell, their collision events are the
same event. With d distinct pairs, P(miss) = Π_{j≤d} q_j ≥ q^d ≥ q^k. Worst
case d = 1 (a column of formulas all reading one header cell): **P(miss) = q
regardless of k**. Evidence is counted in distinct misread pairs, not checked
cells.

**(D2) Value-repetition dependence — the mixture bound (the big one).**
Collision indicators inside a sheet are positively correlated: they are all
driven by that sheet's value distribution (constant columns, repeated labels,
default zeros). Model the file as carrying a latent collision rate `p_F`
(measured per file), pairs conditionally independent given the file. Then

> miss(k) = E[p_F^k] ≥ (E p_F)^k = q̄^k  (Jensen; strict unless degenerate)

The measurable, defensible dependent bound is the plug-in mixture
`mean_f(p̂_f^k)`. Two consequences: (i) required k roughly doubles (below);
(ii) the k → ∞ limit is `P(p_F = 1)` — **files with degenerate value
distributions are check-blind at any k**. On this corpus that floor is 0 for
M2v/M1 under Excel read semantics, but not universally: under strict
both-nonempty pairing, `dynamic_arrays.xlsx` has 7/7 adjacent horizontal
pairs equal (M2h-strict floor 0.005 → 99.9% detection unachievable at any k).

**(D3) Output non-injectivity** (Lemma 2): replace q_in with q_out ≥ q_in.
Measured: ×2.4 (M2), ×3.5 (M1) on the evaluator-validated subset (§6) — and
engine-verified at file level in §7.

**(D4) Coverage against localized errors.** Sampling k of N candidate cells
against an error that corrupts A of them: P(hit ≥ 1) = 1 − C(N−A,k)/C(N,k).
For M1, A ≈ 1 (the one wrong formula, plus dependents):

> detection ≤ (k/N)·(1 − q) — **coverage, not collision, binds**: it rises
> only linearly in k, and even k = N caps at 1 − q. With measured q̂ (§6),
> the cap is ≈ 0.89 input-level / ≈ 0.86 output-level: **99% detection of a
> single mis-reference is unachievable by value checking at any k.**

**Proposition (full-check floor).** If *every* formula's *every* read collides
in value, then recomputing the corrupted sheet reproduces every cache
(induction over dependency order; requires deterministic, non-volatile
formulas — volatile/position-dependent functions are excluded exactly as in
`forward_correctness.py`). So sheet-wide input collision ⟹ the edit passes at
k = N; with (D3), the true k = N miss is even larger. Value checking has an
irreducible floor measured in §7.

## 5. Measured q̂ (coincidence_q.json)

Populations: pairs with ≥ 1 nonempty cell within each column/row's used span;
Excel read semantics (blank ≡ 0 ≡ ""). M1 rates computed **exactly** from
value multisets (no sampling; 5.1e7 same-column and 3.3e8 any-pair
comparisons). Strict both-nonempty variants in the JSON differ little
(e.g. M2v 0.184 vs 0.178 pooled).

| model | pooled q̂ (pairs) | file mean | file median | file p90 | worst file |
|---|---|---|---|---|---|
| M2v off-by-one row | **0.178** (58,557) | 0.163 | 0.125 | 0.398 | 0.726 |
| M2h off-by-one col | 0.404 (51,592) | 0.108 | 0.061 | 0.303 | 0.745 |
| M1c uniform same-column | 0.007 (5.06e7) | 0.099 | 0.059 | 0.245 | — |
| M1a uniform used-range | 0.008 (3.28e8) | 0.050 | 0.034 | — | — |

Weighting honesty: pooled rates are pair-weighted and can be dominated by one
giant file — `ARABIC_ROMAN.xlsx` alone is 39.7% of all M2h pairs at rate
0.745 (pooled 0.404 vs file-median 0.061), and ~92% of M1a pairs (its
diversity drags pooled M1 far below the file median). **Use the file-level
distribution**; the pooled number answers "a random pair from the corpus",
not "a random workbook".

Notable: the two most collision-prone files under M2v are *real-world
templates*, not calc tables — `templates/mortgage_calculator.xlsx` (0.726)
and `templates/travel_expenses_tracker.xlsx` (0.663). Business-style sheets
(repeated rates, zeros, labels) collide *more* than dense test tables. 48/217
files have M2v rate 0; none reach 1.0 under Excel semantics.

**Detection tables (M2v, Excel semantics).** miss(k) = P(wrong edit passes):

| k | naive q̂^k | mixture mean_f(p̂_f^k) | MC on real formulas (§6) |
|---|---|---|---|
| 1 | 0.178 | 0.163 | 0.158 |
| 2 | 0.0318 | 0.0531 | 0.0443 |
| 3 | 0.0057 | 0.0223 | 0.0176 |
| 5 | 0.00018 | 0.0059 | 0.0054 |
| 10 | 3.2e-8 | 0.00056 | 0.00066 |

**k needed for 99% / 99.9% detection** (headline deliverable):

| model | naive k₉₉/k₉₉₉ | honest mixture k₉₉/k₉₉₉ |
|---|---|---|
| M2v off-by-one row | 3 / 5 | **5 / 9** |
| M2h off-by-one col | 6 / 8 | 4 / 8 (strict variant: 5 / **unachievable**) |
| M1c same-column (per-pair only) | 1 / 2 | 3 / 6 |
| M1a used-range (per-pair only) | 1 / 2 | 2 / 5 |

The M1 rows are per-collision-event only — end-to-end M1 detection is
coverage-limited (D4): with the MC's median N = 45 checkable cells per file,
even k = 10 detects only ~19% (measured, §6), and k = N = 45 caps at
1 − q̂ ≈ 89% (86% output-level) — **99% is out of reach at any k**. For M2h,
naive > mixture: the pooled q̂ is inflated by the one giant file — an
instance of the weighting trap, not of mixture optimism.

## 6. Monte Carlo validation (coincidence_mc.json)

30 real formula-bearing workbooks (147 eligible with ≥10 analyzable formulas,
evenly strided; seed 20260709), 3,184/3,226 formulas analyzable after shared-
formula expansion; volatile/position-dependent functions excluded as in the
A/B oracle.

**M2 (exact openpyxl insert@2 read map, input level).** One deterministic
error per file; randomness = which k cells the verifier samples; per-file
miss computed *exactly* (hypergeometric over real per-formula indicators —
real shared-input and value-repetition dependence, no MC noise), then
averaged over the 29 error-present files (1 file has no affected read — the
edit is accidentally correct there and is excluded rather than counted as a
"miss"). Result: the table in §5. **Agreement:** the pair-based mixture bound
tracks the real-formula MC within ~25% at every k; the naive bound is 30×
too optimistic at k=5 and ~2×10⁴× at k=10. (Two dependence effects partially
cancel: multi-read ANDs push per-formula pass *down* — mean 0.047 among
affected formulas — while within-file correlation pushes joint miss *up*.
The mixture happens to net them out well here; that cancellation is measured,
not guaranteed.)

**M1 (single retargeted reference, input level).** 10,750 trials over 27
files: q̂ = 0.107 pooled / 0.045 median — target-weighted on *real* reference
targets, agreeing with the analytic uniform-pair file-mean 0.099 (M1c). The
sampled k-of-N check detects: 1.8% (k=1) → 19.2% (k=10), matching the
coverage bound (k/N)·(1−q) = 2.2% → 19.7%. **Coverage binds, exactly as
derived.**

**Output level (the injectivity gap).** A validation-gated evaluator
(arithmetic + SUM/AVERAGE/MIN/MAX/COUNT/COUNTA/PRODUCT/ABS/SQRT/ROUND/INT/
POWER/MOD/PI/IF/AND/OR/NOT; a formula is used only if evaluating its *correct*
reads reproduces its Excel cache) measures true output collision on the same
formulas: **M2: 0.103 input → 0.252 output (×2.4)** over 352 affected
validated formulas; **M1: 0.040 → 0.140 (×3.5)** over 4,363 trials. Small
subset (613 validated formulas, 7–11 files) — treat as an order-of-magnitude
estimate of how optimistic input-level q is.

## 7. Engine ground-truth anchor (agent_ab.json × this model)

`agent_ab.json` recorded, per file, whether LibreOffice's recompute of the
openpyxl-corrupted file matched the original Excel caches (numeric cells).
Comparing the input-level sheet-wide prediction against that verdict on all
172 A/B files: **agreement 146/168 = 86.9%** (4 indeterminate). The 22
disagreements decompose — and both directions are informative:

- **3 predicted-pass / engine-corrupt.** `tables.xlsx`: structured-table
  references are unanalyzable to this parser (coverage gap, disclosed).
  `ACCRINT.xlsx`, `ACCRINTM.xlsx`: **verified oracle label noise, not check
  physics** — their formulas take only literal arguments (zero cell reads;
  the shift *cannot* change their values, they drop out of the error-present
  set). Reproduced mechanism: openpyxl writes `<v/>` (drops caches), forcing
  LibreOffice to compute `ACCRINT` itself, which disagrees with Excel's cache
  (116.667/`Err:504` vs cached 116.944) on *byte-identical formula text*; the
  original file converts using its caches. Flag for the A/B numbers: a few of
  the 149 "corrupt" verdicts on ACCRINT-family files are partly
  LO-vs-Excel function gaps, not reference corruption.
- **19 predicted-fail / engine-faithful — the latent-corruption set.** These
  are files where references *do* read different cells (error present), yet
  the engine's recompute matched every compared cache.

**The floor, quantified.** Of 161 files where the error is genuinely present
(≥1 reference whose target changed): **19 (11.8%) pass a full — k = N —
value check** by engine ground truth. Decomposition (in the JSON):

- 13 `output_noninjective`: some numeric formula demonstrably reads a
  *different* value, yet every compared numeric cell recomputed identically —
  `DCOUNT/DMIN/DVAR` families, `RATE`, `IFERROR`, `SORT`, `MMULT`,
  `SERIESSUM`, `MIN_MAX`, `MODE`, `STDEV`, `dynamic_arrays`, plus
  `ARABIC_ROMAN`* (asterisk: only 3 numeric cells were comparable; its text
  outputs certainly differ, so a text-aware checker would catch that one —
  conservative count **12/161 = 7.5%**).
- 6 `oracle_blind`: outputs are text/boolean, which the numeric-only A/B
  oracle never compared; output-level status unknown (a text-aware full check
  might catch some).
- 0 `input_coincidence` at sheet-wide scope on this corpus (no file where
  *every* misread value coincided) — consistent with the M2v mixture floor of
  0 in §5.

So the honest floor statement: **on real workbooks under the dominant real
error, a full value check over every comparable cell silently certifies a
wrong reference graph 7.5–11.8% of the time.** These files are exactly the
non-injective-function families the derivation predicts (D3). No choice of k
fixes this; it is a category limit of value checking.

## 8. Implications for the tier design

1. **The probabilistic tier is a refuter, not a certifier.** It can offer
   quantified refutation power against *systemic* errors: with the mixture
   bound, k = 5 checked cells → 99%, k = 9 → 99.9% (M2v). Use the mixture
   numbers, never q^k. Better: compute p̂_f from the *target file's own*
   value multiset (both scripts show per-file rates vary 0 → 0.75) and issue
   a per-file confidence, refusing to quote confidence when p̂_f is high.
2. **Against localized errors, sampling is near-worthless** (detection ≤
   k/N), and even exhaustive checking caps at 1 − q ≈ 86–89% per single
   mis-reference. If the claim localizes the edit (it does — S is known),
   check *all* of S plus its dependents, and treat the residual ~11–14%
   per-error miss as irreducible for this tier.
3. **Even exhaustive value checking has a measured ~8–12% blind spot on
   really-wrong workbooks** (§7) — and 100% blindness on the *graph* itself:
   a coincidence pass leaves a workbook that is wrong tomorrow. This is the
   quantified reason the exact tier (graph-isomorphism certification,
   engine-free) is not a luxury: it decides the property the value tier
   constitutionally cannot see.
4. Where the numbers are *good* news, say so: for dense numeric sheets with
   diverse values (48/217 files have zero adjacent collisions), a handful of
   checked cells is genuinely strong evidence against systemic shifts. The
   tier earns its keep — as a bounded-confidence instrument.

## 9. Caveats (all load-bearing ones)

- **Corpus**: engine calc-test workbooks + a few templates; dense formula
  tables. The two real templates measured are *worse* (more collision-prone)
  than the corpus average, so this likely does not overstate the risk for
  business files — but corpus transfer is unproven.
- **Input-level primacy**: q and the MC k-curves are input-level; output-level
  misses are strictly more frequent (×2.4–3.5 measured on a small validated
  subset; engine-verified at file level in §7). Detection numbers here are
  therefore *upper* bounds on value-check power.
- **Propagation**: per-cell indicators ignore corruption flowing into a
  checked cell from an unchecked upstream formula (understates detection
  somewhat); the sheet-wide prediction and the engine anchor do not have this
  gap.
- The A/B engine oracle compared **numeric cells only**; 6/19 latent files
  are "unknown at output level", hence the 7.5–11.8% range rather than a
  point.
- Exact M1 counting classes numbers by exact float equality; the checker's
  1e-9 tolerance could only merge near-equal values and *raise* q̂ slightly.
- First sheets only; structured-table references and row-ranges unanalyzable
  (counted per file in the JSONs); shared formulas expanded (79% of corpus
  formulas), array formulas carried as text.
- M1's "uniform other cell" is a modeling choice; the MC's target-weighted
  version on real references agreed with it (0.107 vs 0.099), which is why we
  trust the analytic M1 rates at all.

## 10. Reproduce

```
python3 benchmarks/coincidence_q.py    # ~30 s  → coincidence_q.json
python3 benchmarks/coincidence_mc.py   # ~1 min → coincidence_mc.json (reads coincidence_q.json + agent_ab.json)
```

Stdlib-only (zipfile + ElementTree + regex over sheet XML, per
`forward_correctness.py` precedent). Seed 20260709 fixed; the M2 k-curves are
exact (hypergeometric), not sampled.
