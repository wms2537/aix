/-
  CopyEdits.lean — the middle ground, structured: argument-value witnessing.

  Checker.lean proves relabeling edits certifiable; Impossibility.lean proves
  fresh-skeleton edits uncertifiable. The stated-open middle ground is edits that
  REUSE a witnessed skeleton at a new position (copy-paste of an existing formula
  shape onto new inputs). This file proves both sides of a witnessing criterion
  that structures that middle ground at the oracle fuel:

    * `copy_value_forced` (+ the engine-free corollary `copy_certifiable`):
      if the copied node carries a witnessed skeleton AND its argument values
      equal a witnessed application's argument values, then its value is FORCED —
      equal to that application's cached output — under EVERY engine consistent
      with the original. Engine-free checkable premise: skeleton equality +
      pointwise ORACLE equality of dependency values, with the scaffold checked
      by `Checker.check`.
    * `copy_unwitnessed_uncertifiable`: if the copied node's argument tuple is
      unwitnessed — no g-skeleton node of the original evaluates on that tuple at
      any fuel up to the oracle fuel — then two engines exist that are
      indistinguishable through the original (identical evaluation at every node
      and every fuel up to the oracle fuel, hence realizing every cached
      observation) yet disagree on the copied node. No engine-free checker may
      certify it: refuse.

  Honest scope: the criterion is fuel-graded. The certifiable side matches at the
  oracle fuel; the impossibility side requires unwitnessed-at-all-fuels-≤-k. A
  tuple witnessed only at a strictly lower fuel falls between the two theorems —
  the bracket is tight at the oracle fuel but not pointwise closed. Stated in the
  paper as such.

  Self-contained; no Mathlib; no `sorry`. Value type ℕ for concreteness.
-/

namespace CopyEdits

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

/-! ## Side 1: witnessed argument values force the copied value -/

/-- **Core lemma (value forcing).** If the edited node `b` carries the same
    skeleton as a witnessed node `m` of the original, and `b`'s dependency
    values in the edited artifact equal `m`'s dependency values in the original
    (both at fuel `k`), then `b`'s value at fuel `k+1` equals `m`'s — under the
    SAME engine `I`, whatever it is. If `I` realizes the cached observation
    `O m`, the copied value is forced to `O m`. -/
theorem copy_value_forced {Node Skel Value : Type} [Inhabited Value]
    (S0 S1 : SynComp Node Skel)
    (I : Skel → List Value → Value)
    (b m : Node) (k : Nat)
    (hskel : S1.skel b = S0.skel m)
    (hargs : (S1.deps b).map (eval (toComp I S1) k)
           = (S0.deps m).map (eval (toComp I S0 : Computation Node Value) k)) :
    eval (toComp I S1) (k+1) b = eval (toComp I S0 : Computation Node Value) (k+1) m := by
  show I (S1.skel b) ((S1.deps b).map (eval (toComp I S1) k))
     = I (S0.skel m) ((S0.deps m).map (eval (toComp I S0) k))
  rw [hskel, hargs]

/-- **Engine-free corollary (the checkable premise).** Suppose the copied node's
    dependencies are σ-images of original nodes `ds` whose values transport (the
    scaffold condition — discharged in the system by `Checker.check_sound`), the
    original realizes its cached values `O` at fuel `k` on `ds` and on `m`'s
    dependencies, and the CACHED values match pointwise:
    `ds.map O = (S0.deps m).map O`. Then the copied value is forced to the
    witnessed output `O m` — every hypothesis here is checkable from syntax and
    the embedded oracle, with no engine run. -/
theorem copy_certifiable {Node Skel : Type} [Inhabited Nat]
    (S0 S1 : SynComp Node Skel)
    (I : Skel → List Nat → Nat)
    (O : Node → Nat)
    (b m : Node) (σ : Node → Node) (ds : List Node) (k : Nat)
    (hskel : S1.skel b = S0.skel m)
    (hdeps : S1.deps b = ds.map σ)
    -- scaffold transport (from check_sound applied to the relabeled part):
    (htrans : ∀ d ∈ ds, eval (toComp I S1) k (σ d)
                       = eval (toComp I S0 : Computation Node Nat) k d)
    -- the original realizes its cached values at fuel k on the relevant nodes:
    (hOds : ∀ d ∈ ds, eval (toComp I S0 : Computation Node Nat) k d = O d)
    (hOdm : ∀ d ∈ S0.deps m, eval (toComp I S0 : Computation Node Nat) k d = O d)
    -- the ENGINE-FREE witnessing check: cached dep values match pointwise:
    (hOeq : ds.map O = (S0.deps m).map O)
    -- and the witnessed output is cached:
    (hOm : eval (toComp I S0 : Computation Node Nat) (k+1) m = O m) :
    eval (toComp I S1) (k+1) b = O m := by
  have hargs : (S1.deps b).map (eval (toComp I S1) k)
             = (S0.deps m).map (eval (toComp I S0 : Computation Node Nat) k) := by
    rw [hdeps, List.map_map]
    have h1 : ds.map (eval (toComp I S1) k ∘ σ) = ds.map O := by
      apply List.map_congr_left
      intro d hd
      show eval (toComp I S1) k (σ d) = O d
      rw [htrans d hd, hOds d hd]
    have h2 : (S0.deps m).map (eval (toComp I S0 : Computation Node Nat) k)
            = (S0.deps m).map O :=
      List.map_congr_left (fun d hd => hOdm d hd)
    rw [h1, h2, hOeq]
  rw [copy_value_forced S0 S1 I b m k hskel hargs, hOm]

/-! ## Side 2: unwitnessed argument tuples are uncertifiable -/

/-- Override an engine at one skeleton and ONE argument tuple. -/
def overrideAt (I : String → List Nat → Nat) (g : String) (v : List Nat) (w : Nat) :
    String → List Nat → Nat :=
  fun s args => if s = g ∧ args = v then w else I s args

/-- If no `g`-skeleton node of `S` evaluates on the tuple `v` at any fuel below
    the bound, overriding the engine at `(g, v)` changes NO evaluation of `S` up
    to that bound — the two engines are indistinguishable through the original,
    at every node and every fuel ≤ the oracle fuel. -/
theorem eval_overrideAt_unwitnessed {Node : Type}
    (S : SynComp Node String) (I : String → List Nat → Nat)
    (g : String) (v : List Nat) (w : Nat) (K : Nat)
    (hunwit : ∀ j, j < K → ∀ n, S.skel n = g →
        (S.deps n).map (eval (toComp I S : Computation Node Nat) j) ≠ v) :
    ∀ k, k ≤ K → ∀ n, eval (toComp (overrideAt I g v w) S) k n
                    = eval (toComp I S : Computation Node Nat) k n := by
  intro k
  induction k with
  | zero => intro _ n; rfl
  | succ k ih =>
    intro hk n
    have hk' : k ≤ K := Nat.le_of_lt hk
    have hargs : (S.deps n).map (eval (toComp (overrideAt I g v w) S) k)
               = (S.deps n).map (eval (toComp I S : Computation Node Nat) k) :=
      List.map_congr_left (fun d _ => ih hk' d)
    show overrideAt I g v w (S.skel n)
           ((S.deps n).map (eval (toComp (overrideAt I g v w) S) k))
       = I (S.skel n) ((S.deps n).map (eval (toComp I S) k))
    rw [hargs]
    unfold overrideAt
    by_cases hg : S.skel n = g
    · have hne : (S.deps n).map (eval (toComp I S : Computation Node Nat) k) ≠ v :=
        hunwit k (Nat.lt_of_lt_of_le hk (Nat.le_refl K) |> fun _ => Nat.lt_of_succ_le hk) n hg
      rw [if_neg]
      intro ⟨_, hv⟩
      exact hne hv
    · rw [if_neg]
      intro ⟨hg', _⟩
      exact hg hg'

/-- **Impossibility (unwitnessed copies).** Let the edited artifact carry, at
    node `b`, a skeleton `g` witnessed in the original — but with an argument
    tuple `v` that is UNWITNESSED: no `g`-node of the original evaluates on `v`
    at any fuel below the oracle fuel `K`, and `b`'s own dependencies evaluate to
    `v` robustly (under any engine agreeing with `I0` on the tuples below `K` —
    supplied here as the two hypotheses `hargs` and `hargs'`). Then for ANY value
    a checker might commit to, there is an engine indistinguishable from `I0`
    through the original (every node, every fuel ≤ K — hence realizing every
    cached observation) whose value at `b` differs. Sound engine-free checkers
    must refuse unwitnessed copies. -/
theorem copy_unwitnessed_uncertifiable {Node : Type}
    (S0 S1 : SynComp Node String) (g : String)
    (b : Node) (hb : S1.skel b = g)
    (I0 : String → List Nat → Nat)
    (v : List Nat) (K : Nat) (hK : 0 < K)
    (hunwit : ∀ j, j < K → ∀ n, S0.skel n = g →
        (S0.deps n).map (eval (toComp I0 S0 : Computation Node Nat) j) ≠ v)
    (predict : Nat)
    -- b's argument tuple is v under BOTH engines (the copied node's inputs are
    -- pinned — e.g. transported oracle values — so the override cannot move them):
    (hargs : (S1.deps b).map (eval (toComp (overrideAt I0 g v (predict + 1)) S1) (K - 1)) = v) :
    ∃ I : String → List Nat → Nat,
      (∀ k, k ≤ K → ∀ n, eval (toComp I S0) k n
                       = eval (toComp I0 S0 : Computation Node Nat) k n)
      ∧ eval (toComp I S1) K b ≠ predict := by
  refine ⟨overrideAt I0 g v (predict + 1),
          eval_overrideAt_unwitnessed S0 I0 g v (predict + 1) K hunwit, ?_⟩
  obtain ⟨K', hK'⟩ : ∃ K', K = K' + 1 := ⟨K - 1, (Nat.succ_pred_eq_of_pos hK).symm⟩
  subst hK'
  show overrideAt I0 g v (predict + 1) (S1.skel b)
         ((S1.deps b).map (eval (toComp (overrideAt I0 g v (predict + 1)) S1) K'))
     ≠ predict
  have hv : (S1.deps b).map (eval (toComp (overrideAt I0 g v (predict + 1)) S1) K') = v := by
    simpa using hargs
  rw [hb, hv]
  unfold overrideAt
  rw [if_pos ⟨rfl, rfl⟩]
  exact Nat.succ_ne_self predict

end CopyEdits
