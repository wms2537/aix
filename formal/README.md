# Formal core — machine-checked foundations of semantic-redundancy certification

The durable moat is a *verification* theory, so its core is machine-checked, not
argued. Two independent tools, two independent parts of the theory.

## Lean 4 — the exact-core theorem (`SelfOracle.lean`)

**Theorem 1 (Exact structural certification).** Evaluation of a computation is
invariant under a function-and-dependency-preserving isomorphism `σ`:
`eval C' k (σ n) = eval C k n`, under **any** deterministic semantics `fn`.

**Corollary (self-oracle transfer).** A structurally-faithful edit reproduces the
artifact's embedded ground truth `O` at the shifted positions
(`eval C' k (σ n) = O n`) — established with no evaluation of the semantics on the
edited artifact (engine-free) and independent of what the semantics computes
(model-free).

Checked by `lean SelfOracle.lean` (Lean 4.31.0). `#print axioms` reports only
`[propext, Quot.sound]` — the standard sound axioms — and **no `sorryAx`**: the
proofs are complete and constructive (not even `Classical.choice`). This is the
formal statement of the moat: structural-edit correctness is decidable offline,
against the artifact's own structure, forever — no engine, no spec, no model.

Reproduce:
```
export PATH="$HOME/.elan/bin:$PATH"
lean SelfOracle.lean          # exit 0 = verified
```

## Z3 — the reference-shift algebra laws (`shift_laws.py`)

The algebraic laws underpinning the shift map, proved for **all** positions,
ranges, and `(k, n)` (a proof, not a test — Z3 finds no counterexample to the
negation):

1. `insert(k,n)` then `delete(k,n) = identity` — the composition law that gives an
   independent Tier-2 certification constraint.
2. the inserted band always survives its matching delete.
3. insert is monotone (order-preserving); delete is monotone on survivors — so a
   range's endpoint order is preserved (the 6-case clamp is well-formed).
4. the **6-case delete clamp** (the most silent-corruption-prone path, the one an
   early theory-review FAILED) matches the set-theoretic truth — the shifted
   first/last surviving row — on both endpoints.

Reproduce: `python shift_laws.py` (all lines print `PROVED`).

## Why this matters
The four-round review ceiling was a *novelty* ceiling: the contribution read as
integration. A machine-checked exact theorem for the certifiable class, plus an
SMT-verified edit algebra, moves the core from "engineering + rigor" to a
*formally-grounded verification result* — the part that survives model progress
and cannot be dissolved as "known techniques composed." xlq becomes the
demonstrator; the verified theory is the moat.
