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

## Lean 4 — closing the proof↔extraction gap (`RefShift.lean`)

Theorem 1 assumes the edited computation's dependency graph is the `σ`-relabeling of
the original's. The tool does not assume this — it PRODUCES it, by shifting the
references in each formula. `RefShift.lean` models a formula as a list of tokens
(`lit` = literal/function-name/constant, `ref` = cell reference) and machine-checks
the missing link:

**Graph preservation (`refs_shiftF`).** `refs (shiftF σ f) = (refs f).map σ` — the
reference-shift produces exactly the `σ`-image of the reference graph, with literals
provably untouched. This is verbatim the `hdeps` premise of Theorem 1, so the
hypothesis the value-fidelity theorem assumes is now *discharged constructively* by
the operation the tool performs.

**Invertibility (`delete_insert_id`, `insert_delete_form_id`).** On the cell model,
delete-after-insert at the same index is the identity, and a formula shifted by an
insert then its matching delete is unchanged — the structural counterpart of the
Z3-proved arithmetic law. `#print axioms` reports only `[propext, Quot.sound]`, no
`sorry`. The only remaining trusted step is the byte→token parse into this model,
which is validated for value-preservation against an independent engine
(`benchmarks/tokenizer_conformance.py`, 264 formulas, 0 divergences).

## Lean 4 — the sound-by-construction certifier (`Checker.lean`)

The keystone that makes the theorem load-bearing instead of decorative. `check` is a
**decidable, executable decision procedure for exactly Theorem 1's hypothesis**, stated
over syntax alone (skeleton preserved, deps = σ-image, deps-closed domain), and
`check_sound` proves: `check = true` ⟹ every checked node's value transports across
the edit **under every possible engine** (the interpretation of skeletons is
universally quantified — never run). `check_transports_oracle` carries the embedded
ground truth to the edited positions. `#eval` demos run the procedure (faithful →
`true`; argument-order and operator botches → `false`). Consequence: a certifier
implementing `check` accepts *any* producer's faithful edit — regardless of bytes,
caches, or tool — and its soundness is a theorem, not equality to a reference
transform. `differential_check.py` ties the proof to the running system: the Lean
`check` and `experiments/generality/router.certify_edit` agree on 30/30 randomized
cases (faithful + four botch classes). No `sorry`; axioms `[propext, Quot.sound]`.

## Lean 4 — the boundary theorem (`Impossibility.lean`)

The other side of the characterization: **an edit introducing a function skeleton not
witnessed in the original cannot be value-certified by any engine-free checker.**
`eval_override_fresh` shows two engines differing only at an unwitnessed skeleton are
pointwise indistinguishable through the original artifact (every node, every fuel);
`fresh_skeleton_uncertifiable` then refutes every value a checker could commit to with
a consistent engine, and `two_worlds_disagree` gives the direct two-world form. Since
an engine-free checker's verdict is a function of inputs identical in both worlds,
sound checkers must REFUSE fresh-skeleton edits — certify-or-refuse is the only sound
shape, not a design choice. Together with `Checker.lean` this characterizes the
boundary of engine-free certification. No `sorry`; axioms `[propext, Quot.sound]`.

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
