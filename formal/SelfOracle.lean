/-
  Machine-checked proof of Theorem 1 (the EXACT core of semantic-redundancy
  certification): evaluation of a computation is invariant under a
  function-and-dependency-preserving isomorphism.

  Consequence: for a relabeling (structural) edit whose implementation I' has a
  reference-dependency graph isomorphic to I's under the position bijection σ,
  every computed value is preserved — ⟦I'⟧(σ n) = ⟦I⟧(n) — under ANY deterministic
  semantics `fn`, with NO evaluation of that semantics on I' (engine-free),
  and independent of what the semantics actually computes (model-free).

  This is the durable moat: structural-edit correctness certified offline,
  against the artifact's own structure, forever.

  Self-contained: no Mathlib. Checked by `lean SelfOracle.lean`.
-/

namespace SelfOracle

variable {Node Value : Type} [Inhabited Value]

/-- A `Computation` assigns to each node a function of its dependency values
    (`fn`) and an ordered list of dependency nodes (`deps`). This is exactly the
    information a formula carries: what it reads, and how it combines what it
    reads. The semantics `fn` is arbitrary (opaque). -/
structure Computation (Node Value : Type) where
  fn   : Node → List Value → Value
  deps : Node → List Node

/-- Fuel-bounded evaluator. At fuel `k+1`, a node's value is its function applied
    to the values of its dependencies at fuel `k`. For an acyclic computation of
    depth ≤ k this equals the true evaluation ⟦·⟧; the theorem holds at every
    fuel level, so it holds for the true evaluation. -/
def eval (C : Computation Node Value) : Nat → Node → Value
  | 0,     _ => default
  | (k+1), n => C.fn n ((C.deps n).map (eval C k))

/-- **Theorem 1 (Exact structural certification, evaluation core).**
    Let `σ` map every node of `C` to a node of `C'` with the SAME function and
    with the dependency list relabeled by `σ` (a graph isomorphism preserving
    functions and edges). Then the evaluation is invariant:
        eval C' k (σ n) = eval C k n
    at every fuel level `k`, for every node `n`, under ANY semantics.
    No hypothesis on `C.fn` beyond that it is a (deterministic) function. -/
theorem eval_iso_invariant
    (C C' : Computation Node Value) (σ : Node → Node)
    (hfn   : ∀ n, C'.fn (σ n) = C.fn n)
    (hdeps : ∀ n, C'.deps (σ n) = (C.deps n).map σ) :
    ∀ k n, eval C' k (σ n) = eval C k n := by
  intro k
  induction k with
  | zero => intro n; rfl
  | succ k ih =>
    intro n
    -- unfold one step of eval on both sides
    show C'.fn (σ n) ((C'.deps (σ n)).map (eval C' k))
       = C.fn n ((C.deps n).map (eval C k))
    -- the recursive value maps agree pointwise by the induction hypothesis
    have hmap : (eval C' k) ∘ σ = eval C k := by
      funext m; exact ih m
    rw [hfn, hdeps, List.map_map, hmap]

/-- **Corollary (self-oracle transfer).** If the original computation's
    evaluation is the artifact's embedded ground truth `O` (the self-oracle:
    `eval C k = O`), then a structurally-faithful edit `I' = C'` reproduces the
    ground truth at the shifted positions — `eval C' k (σ n) = O n` — established
    without the defining engine and without recomputing `O`. This is the exact
    tier of correct-or-refuse certification. -/
theorem self_oracle_transfer
    (C C' : Computation Node Value) (σ : Node → Node) (O : Node → Value) (k : Nat)
    (hfn   : ∀ n, C'.fn (σ n) = C.fn n)
    (hdeps : ∀ n, C'.deps (σ n) = (C.deps n).map σ)
    (hO    : ∀ n, eval C k n = O n) :
    ∀ n, eval C' k (σ n) = O n := by
  intro n
  rw [eval_iso_invariant C C' σ hfn hdeps k n, hO n]

/-- Sanity: an edit that only PERMUTES positions but keeps each node reading the
    same (relabeled) neighbours with the same function preserves all values.
    (A degenerate check that the hypotheses are satisfiable and non-vacuous.) -/
example (C : Computation Node Value) :
    ∀ k n, eval C k (id n) = eval C k n := by
  intro k n
  exact eval_iso_invariant C C id (fun _ => rfl) (fun n => by simp) k n

end SelfOracle
