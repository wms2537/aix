/-
  Checker.lean — the sound-by-construction certifier (the keystone).

  SelfOracle.lean proves Theorem 1: evaluation is invariant under a
  function-and-dependency-preserving isomorphism. But the shipped certifier so far
  checked *equality to the tool's own transform* — the theorem licensed acceptance,
  yet the system never checked the theorem's hypothesis. This file closes that gap:

    * `check` is a DECIDABLE, EXECUTABLE decision procedure for exactly the
      hypothesis of Theorem 1, stated over SYNTAX ONLY: the edited artifact carries
      the same opaque function skeleton at every relabeled node, its dependency
      lists are the σ-image of the original's, and the checked domain is closed
      under dependencies.
    * `check_sound` proves: if `check` returns `true`, then for EVERY possible
      engine (every interpretation `I` of the opaque skeletons), every computed
      value transports across the edit — `eval` on the edited artifact at `σ n`
      equals `eval` on the original at `n`. Engine-free and semantics-agnostic:
      the engine is universally quantified, never run.
    * `check_transports_oracle` transports the artifact's embedded ground truth
      (the self-oracle) to the edited positions.

  Consequence for the system: a certifier that implements `check` accepts ANY
  producer's edit whose reference graph satisfies the premise — regardless of
  byte-level differences, caches, or which tool made it — and its soundness is
  this theorem, not equality to an unverified reference transform. The trusted
  base shrinks to the byte→(skeleton, deps) parse.

  What `check` does NOT claim: it certifies value transport for the checked
  domain. "No other node was silently added" is a separate, purely syntactic
  containment condition (edited nodes ⊆ σ-image ∪ declared) enforced by the
  implementation; it needs no semantic theorem.

  Self-contained (defs duplicated from SelfOracle.lean for standalone checking
  with `lean Checker.lean`). No Mathlib. No `sorry`.
-/

namespace Checker

variable {Node Skel Value : Type}

/-- A computation with SEMANTIC functions (as in SelfOracle.lean). -/
structure Computation (Node Value : Type) where
  fn   : Node → List Value → Value
  deps : Node → List Node

/-- Fuel-bounded evaluator (identical to SelfOracle.eval). -/
def eval [Inhabited Value] (C : Computation Node Value) : Nat → Node → Value
  | 0,     _ => default
  | (k+1), n => C.fn n ((C.deps n).map (eval C k))

/-- A SYNTACTIC computation: what a certifier can actually read from an artifact.
    `skel n` is the opaque function skeleton at node `n` (a formula with its
    references abstracted to ordered slots — the certifier never interprets it);
    `deps n` is the ordered reference list; `nodes` is the checked domain. -/
structure SynComp (Node Skel : Type) where
  nodes : List Node
  skel  : Node → Skel
  deps  : Node → List Node

/-- Interpret a syntactic computation under an engine `I` (opaque to the checker). -/
def toComp (I : Skel → List Value → Value) (S : SynComp Node Skel) :
    Computation Node Value :=
  { fn := fun n => I (S.skel n), deps := S.deps }

/-- **The decision procedure.** Checks, over syntax alone, exactly the hypothesis
    of the invariance theorem:
      (1) skeleton preserved:   S1.skel (σ n) = S0.skel n
      (2) deps relabeled:       S1.deps (σ n) = (S0.deps n).map σ
      (3) domain deps-closed:   every dependency of a checked node is checked.
    Executable: see the `#eval` demos at the bottom. -/
def check [DecidableEq Node] [DecidableEq Skel]
    (S0 S1 : SynComp Node Skel) (σ : Node → Node) : Bool :=
  S0.nodes.all fun n =>
    decide (S1.skel (σ n) = S0.skel n)
    && decide (S1.deps (σ n) = (S0.deps n).map σ)
    && ((S0.deps n).all fun m => decide (m ∈ S0.nodes))

/-- Unpack the three checked conditions from `check … = true`. -/
theorem check_spec [DecidableEq Node] [DecidableEq Skel]
    {S0 S1 : SynComp Node Skel} {σ : Node → Node}
    (h : check S0 S1 σ = true) :
    (∀ n ∈ S0.nodes, S1.skel (σ n) = S0.skel n)
    ∧ (∀ n ∈ S0.nodes, S1.deps (σ n) = (S0.deps n).map σ)
    ∧ (∀ n ∈ S0.nodes, ∀ m ∈ S0.deps n, m ∈ S0.nodes) := by
  have hall := List.all_eq_true.mp h
  refine ⟨?_, ?_, ?_⟩
  · intro n hn
    have hb := hall n hn
    have h1 := (Bool.and_eq_true _ _).mp ((Bool.and_eq_true _ _).mp hb).1
    exact of_decide_eq_true h1.1
  · intro n hn
    have hb := hall n hn
    have h1 := (Bool.and_eq_true _ _).mp ((Bool.and_eq_true _ _).mp hb).1
    exact of_decide_eq_true h1.2
  · intro n hn m hm
    have hb := hall n hn
    have h2 := ((Bool.and_eq_true _ _).mp hb).2
    have := List.all_eq_true.mp h2 m hm
    exact of_decide_eq_true this

/-- **Soundness (the keystone).** If the decision procedure accepts, then under
    EVERY engine `I` — the checker never runs one — every checked node's value
    transports across the edit at every fuel level:
        eval ⟦S1⟧_I k (σ n) = eval ⟦S0⟧_I k n.
    The system may therefore certify any edit `check` accepts; Theorem-1-style
    invariance is no longer an assumption about the artifact but a *checked*
    property of it. -/
theorem check_sound [DecidableEq Node] [DecidableEq Skel] [Inhabited Value]
    (S0 S1 : SynComp Node Skel) (σ : Node → Node)
    (h : check S0 S1 σ = true)
    (I : Skel → List Value → Value) :
    ∀ k, ∀ n ∈ S0.nodes,
      eval (toComp I S1) k (σ n) = eval (toComp I S0 : Computation Node Value) k n := by
  obtain ⟨hskel, hdeps, hclosed⟩ := check_spec h
  intro k
  induction k with
  | zero => intro n _; rfl
  | succ k ih =>
    intro n hn
    show (toComp I S1).fn (σ n) (((toComp I S1).deps (σ n)).map (eval (toComp I S1) k))
       = (toComp I S0).fn n (((toComp I S0).deps n).map (eval (toComp I S0) k))
    have hfn : (toComp I S1 : Computation Node Value).fn (σ n)
             = (toComp I S0 : Computation Node Value).fn n := by
      show I (S1.skel (σ n)) = I (S0.skel n)
      rw [hskel n hn]
    have hd : (toComp I S1 : Computation Node Value).deps (σ n)
            = (S0.deps n).map σ := hdeps n hn
    rw [hfn, hd, List.map_map]
    have hargs : (S0.deps n).map (eval (toComp I S1) k ∘ σ)
               = (S0.deps n).map (eval (toComp I S0) k) := by
      apply List.map_congr_left
      intro m hm
      exact ih m (hclosed n hn m hm)
    rw [hargs]
    rfl

/-- **Self-oracle transport.** If the original's evaluation realizes the embedded
    ground truth `O` (the cached values), an edit accepted by `check` reproduces
    `O` at the relabeled positions — with no engine and no recomputation. -/
theorem check_transports_oracle [DecidableEq Node] [DecidableEq Skel] [Inhabited Value]
    (S0 S1 : SynComp Node Skel) (σ : Node → Node) (O : Node → Value) (k : Nat)
    (h : check S0 S1 σ = true)
    (I : Skel → List Value → Value)
    (hO : ∀ n ∈ S0.nodes, eval (toComp I S0 : Computation Node Value) k n = O n) :
    ∀ n ∈ S0.nodes, eval (toComp I S1) k (σ n) = O n := by
  intro n hn
  rw [check_sound S0 S1 σ h I k n hn, hO n hn]

/-! ## Executable demos — the decision procedure runs.

A three-node artifact (two data cells feeding an ADD), relabeled by σ = (+10):
faithful edit accepted; a reversed dependency list (argument-order botch) and a
changed skeleton (operator botch) both rejected. -/

private def S0ex : SynComp Nat String :=
  { nodes := [1, 2, 3]
    skel := fun n => if n = 3 then "ADD(#0,#1)" else "DATA"
    deps := fun n => if n = 3 then [1, 2] else [] }

private def σex : Nat → Nat := fun n => n + 10

private def S1good : SynComp Nat String :=
  { nodes := [11, 12, 13]
    skel := fun n => if n = 13 then "ADD(#0,#1)" else "DATA"
    deps := fun n => if n = 13 then [11, 12] else [] }

private def S1reversedDeps : SynComp Nat String :=
  { S1good with deps := fun n => if n = 13 then [12, 11] else [] }

private def S1wrongOp : SynComp Nat String :=
  { S1good with skel := fun n => if n = 13 then "MUL(#0,#1)" else "DATA" }

#eval check S0ex S1good σex          -- expected: true  (faithful → certifiable)
#eval check S0ex S1reversedDeps σex  -- expected: false (argument order botched)
#eval check S0ex S1wrongOp σex       -- expected: false (operator botched)

end Checker
