/-
  Impossibility.lean — the boundary of engine-free certification.

  Checker.lean proves the relabeling class IS certifiable engine-free. This file
  proves the other side of the boundary: an edit that introduces a function
  skeleton NOT witnessed anywhere in the original artifact CANNOT be value-
  certified by any engine-free checker.

  The argument is indistinguishability. An engine-free checker computes its
  verdict from syntax alone (skeletons, dependency lists, cached observations) —
  the engine is not among its inputs. We construct, for any engine `I0`
  consistent with the original artifact and any value `predict` a checker might
  commit to for the edited node, an engine `I` that is *pointwise
  indistinguishable from `I0` on the original artifact* — every node of the
  original evaluates identically at every fuel, so every cached observation is
  equally realized — yet the edited node's value differs from `predict`.
  Since the checker's inputs are identical in both worlds, its verdict is fixed
  while the ground truth varies: no engine-free checker is simultaneously sound
  and complete for fresh-skeleton edits.

  Together with Checker.check_sound this characterizes the boundary:
    · edits whose graph is a skeleton-preserving relabeling → certifiable,
      with an executable decision procedure proven sound (Checker.lean);
    · edits introducing unwitnessed semantics → uncertifiable without an engine
      or additional oracle assumptions (this file). The honest middle ground —
      edits that REUSE witnessed skeletons in new positions (copy-paste of an
      existing formula shape) — is future work, noted in the paper.

  Self-contained; no Mathlib; no `sorry`. Value type is ℕ for concreteness (any
  type with two distinct elements works).
-/

namespace Impossibility

/-- Syntactic computation (as in Checker.lean). -/
structure SynComp (Node Skel : Type) where
  nodes : List Node
  skel  : Node → Skel
  deps  : Node → List Node

structure Computation (Node Value : Type) where
  fn   : Node → List Value → Value
  deps : Node → List Node

def eval {Node Value : Type} [Inhabited Value]
    (C : Computation Node Value) : Nat → Node → Value
  | 0,     _ => default
  | (k+1), n => C.fn n ((C.deps n).map (eval C k))

def toComp {Node Skel Value : Type}
    (I : Skel → List Value → Value) (S : SynComp Node Skel) :
    Computation Node Value :=
  { fn := fun n => I (S.skel n), deps := S.deps }

/-- Override an engine at a single skeleton `g` with the constant function `v`. -/
def override (I : String → List Nat → Nat) (g : String) (v : Nat) :
    String → List Nat → Nat :=
  fun s => if s = g then (fun _ => v) else I s

/-- If the skeleton `g` is never used by the artifact `S`, overriding the engine
    at `g` changes NO evaluation — the two engines are indistinguishable through
    the original artifact, at every node and every fuel. -/
theorem eval_override_fresh {Node : Type}
    (S : SynComp Node String) (I : String → List Nat → Nat)
    (g : String) (v : Nat)
    (hfresh : ∀ n, S.skel n ≠ g) :
    ∀ k n, eval (toComp (override I g v) S) k n = eval (toComp I S) k n := by
  intro k
  induction k with
  | zero => intro n; rfl
  | succ k ih =>
    intro n
    show override I g v (S.skel n) ((S.deps n).map (eval (toComp (override I g v) S) k))
       = I (S.skel n) ((S.deps n).map (eval (toComp I S) k))
    have hargs : (S.deps n).map (eval (toComp (override I g v) S) k)
               = (S.deps n).map (eval (toComp I S) k) := by
      apply List.map_congr_left
      intro m _
      exact ih m
    rw [hargs]
    show (if S.skel n = g then (fun _ => v) else I (S.skel n))
           ((S.deps n).map (eval (toComp I S) k))
       = I (S.skel n) ((S.deps n).map (eval (toComp I S) k))
    rw [if_neg (hfresh n)]

/-- **Impossibility (fresh-skeleton edits are engine-free uncertifiable).**
    Let `S0` be any original artifact whose syntax never uses skeleton `g`, and
    let the edited artifact `S1` carry `g` at some node `b`. Then for ANY engine
    `I0` (in particular, one realizing every cached observation of `S0`) and ANY
    value `predict` an engine-free checker might certify for `b`, there is an
    engine `I` that:
      (1) evaluates the ORIGINAL artifact identically to `I0` at every node and
          fuel — no syntactic or observational input distinguishes the worlds —
      (2) yet gives the edited node a value ≠ `predict`.
    An engine-free checker's verdict is a function of inputs that are equal in
    both worlds; committing to any value is unsound in one of them. Hence sound
    engine-free certifiers must REFUSE fresh-skeleton edits: certify-or-refuse
    is not a design choice but the only sound shape. -/
theorem fresh_skeleton_uncertifiable {Node : Type}
    (S0 S1 : SynComp Node String) (g : String)
    (hfresh : ∀ n, S0.skel n ≠ g)
    (b : Node) (hb : S1.skel b = g)
    (I0 : String → List Nat → Nat)
    (k : Nat) (predict : Nat) :
    ∃ I : String → List Nat → Nat,
      (∀ j n, eval (toComp I S0) j n = eval (toComp I0 S0) j n)
      ∧ eval (toComp I S1) (k+1) b ≠ predict := by
  refine ⟨override I0 g (predict + 1),
          eval_override_fresh S0 I0 g (predict + 1) hfresh, ?_⟩
  show override I0 g (predict + 1) (S1.skel b)
         ((S1.deps b).map (eval (toComp (override I0 g (predict + 1)) S1) k))
     ≠ predict
  rw [hb]
  show (if g = g then (fun _ => predict + 1) else I0 g)
         ((S1.deps b).map (eval (toComp (override I0 g (predict + 1)) S1) k))
     ≠ predict
  rw [if_pos rfl]
  exact Nat.succ_ne_self predict

/-- **Corollary (no engine-free predictor — the checker as a formal object).**
    An engine-free checker that CERTIFIES a fresh-skeleton edit commits (implicitly
    or explicitly) to the edited node's value as a function of syntax alone. Model
    that commitment as ANY `predictor : SynComp → SynComp → ℕ` — a function of the
    two syntactic artifacts and nothing else (the engine is not among its inputs).
    Then for every predictor there is an engine consistent with the original
    artifact under which the prediction is wrong. Hence no sound checker may
    certify a value for a fresh-skeleton node: it must refuse. The quantification
    over checkers is now itself a machine-checked object, not a prose step. -/
theorem no_engine_free_predictor {Node : Type}
    (S0 S1 : SynComp Node String) (g : String)
    (hfresh : ∀ n, S0.skel n ≠ g)
    (b : Node) (hb : S1.skel b = g)
    (I0 : String → List Nat → Nat)
    (k : Nat)
    (predictor : SynComp Node String → SynComp Node String → Nat) :
    ∃ I : String → List Nat → Nat,
      (∀ j n, eval (toComp I S0) j n = eval (toComp I0 S0) j n)
      ∧ eval (toComp I S1) (k+1) b ≠ predictor S0 S1 :=
  fresh_skeleton_uncertifiable S0 S1 g hfresh b hb I0 k (predictor S0 S1)

/-- **Corollary (two indistinguishable worlds disagree).** There exist two
    engines that evaluate the original artifact identically everywhere, yet
    disagree on the edited node's value — the direct two-world form. -/
theorem two_worlds_disagree {Node : Type}
    (S0 S1 : SynComp Node String) (g : String)
    (hfresh : ∀ n, S0.skel n ≠ g)
    (b : Node) (hb : S1.skel b = g)
    (I0 : String → List Nat → Nat)
    (k : Nat) :
    ∃ I1 I2 : String → List Nat → Nat,
      (∀ j n, eval (toComp I1 S0) j n = eval (toComp I0 S0) j n)
      ∧ (∀ j n, eval (toComp I2 S0) j n = eval (toComp I0 S0) j n)
      ∧ eval (toComp I1 S1) (k+1) b ≠ eval (toComp I2 S1) (k+1) b := by
  refine ⟨override I0 g 0, override I0 g 1,
          eval_override_fresh S0 I0 g 0 hfresh,
          eval_override_fresh S0 I0 g 1 hfresh, ?_⟩
  have h1 : eval (toComp (override I0 g 0) S1) (k+1) b = 0 := by
    show (if S1.skel b = g then (fun _ => 0) else I0 (S1.skel b))
           ((S1.deps b).map (eval (toComp (override I0 g 0) S1) k)) = 0
    rw [hb, if_pos rfl]
  have h2 : eval (toComp (override I0 g 1) S1) (k+1) b = 1 := by
    show (if S1.skel b = g then (fun _ => 1) else I0 (S1.skel b))
           ((S1.deps b).map (eval (toComp (override I0 g 1) S1) k)) = 1
    rw [hb, if_pos rfl]
  rw [h1, h2]
  exact Nat.zero_ne_one

end Impossibility
