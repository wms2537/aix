/-
  RefShift.lean — closing the proof↔extraction gap (Lever 2).

  SelfOracle.lean proves value-fidelity of a structural edit GIVEN that the edited
  computation's dependency graph is the σ-relabeling of the original's (Theorem 1's
  `hdeps` hypothesis). But the tool does not assume that graph — it PRODUCES it, by
  shifting the references in each formula. This file machine-checks that the
  reference-shift, modeled on a token-level formula, produces EXACTLY the σ-relabeled
  reference graph — so the hypothesis Theorem 1 assumes is discharged constructively
  by the operation the tool performs. It also verifies the grid-shift is invertible
  (delete-after-insert is the identity) at the cell-model level.

  Self-contained: no Mathlib. Checked by `lean RefShift.lean`.
-/

namespace RefShift

/-! ## The cell model and grid validity -/

/-- A spreadsheet cell as (column, row), 1-based. -/
structure Cell where
  col : Nat
  row : Nat
deriving DecidableEq, Repr

/-- Grid validity: a real Excel cell has column in A..XFD (1..16384) and row in
    1..1048576. This is exactly the predicate the tokenizer uses to decide whether
    a token is a reference at all. -/
def Cell.valid (c : Cell) : Prop :=
  1 ≤ c.col ∧ c.col ≤ 16384 ∧ 1 ≤ c.row ∧ c.row ≤ 1048576

/-! ## The token-level formula model

A formula is a list of tokens; each token is either an opaque literal (a number, a
string, a function name — anything that is NOT a reference) or a cell reference. This
is the abstraction the byte-level tokenizer targets: its whole job is to decide, for
each maximal token, whether it is a `ref` or a `lit`. Everything downstream is proved. -/

inductive Tok (V : Type) where
  | lit : V → Tok V
  | ref : Cell → Tok V
  | rng : Cell → Cell → Tok V     -- a range A1:B2, carrying both endpoints

abbrev Form (V : Type) := List (Tok V)

/-- The reference-dependency graph of a formula: the cells it reads, in order. A range
    contributes both endpoints (the extent is determined by them); literals contribute
    nothing. This is the `deps` a computation carries. -/
def refs {V : Type} : Form V → List Cell
  | []               => []
  | (.ref c) :: t    => c :: refs t
  | (.rng a b) :: t  => a :: b :: refs t
  | (.lit _) :: t    => refs t

/-- Apply a coordinate map to every reference (single cell AND both range endpoints),
    leaving literals untouched. This is exactly what the reference-shift does — and it
    holds for ANY cell map `σ`, including the 6-case delete CLAMP that maps a range
    endpoint landing on a deleted row (the clamp's arithmetic is separately Z3-proved;
    here we prove that whatever `σ` is, the graph is its image). -/
def shiftF {V : Type} (σ : Cell → Cell) : Form V → Form V
  | []               => []
  | (.ref c) :: t    => (.ref (σ c)) :: shiftF σ t
  | (.rng a b) :: t  => (.rng (σ a) (σ b)) :: shiftF σ t
  | (.lit v) :: t    => (.lit v)     :: shiftF σ t

/-! ## The key theorem: the shift produces the σ-relabeled reference graph -/

/-- **Graph preservation.** Shifting a formula's references by `σ` produces a formula
    whose reference graph is exactly the `σ`-image of the original's:
    `refs (shiftF σ f) = (refs f).map σ`. This is precisely the `hdeps` hypothesis of
    SelfOracle.Theorem 1 (`C'.deps (σ n) = (C.deps n).map σ`) — so the tool's shift
    operation CONSTRUCTS the graph isomorphism that the value-fidelity theorem assumes,
    rather than the theorem assuming it for free. Literals (function names, constants)
    are provably untouched. -/
theorem refs_shiftF {V : Type} (σ : Cell → Cell) :
    ∀ f : Form V, refs (shiftF σ f) = (refs f).map σ
  | []               => rfl
  | (.ref c) :: t    => by simp [refs, shiftF, refs_shiftF σ t]
  | (.rng a b) :: t  => by simp [refs, shiftF, refs_shiftF σ t]
  | (.lit v) :: t    => by simp [refs, shiftF, refs_shiftF σ t]

/-! ## Functoriality and invertibility of the shift -/

/-- The shift is functorial: shifting by `τ` then `σ` equals shifting by `σ ∘ τ`. -/
theorem shiftF_comp {V : Type} (σ τ : Cell → Cell) :
    ∀ f : Form V, shiftF σ (shiftF τ f) = shiftF (σ ∘ τ) f
  | []               => rfl
  | (.ref c) :: t    => by simp [shiftF, shiftF_comp σ τ t]
  | (.rng a b) :: t  => by simp [shiftF, shiftF_comp σ τ t]
  | (.lit v) :: t    => by simp [shiftF, shiftF_comp σ τ t]

/-- Shifting by the identity map is the identity. -/
theorem shiftF_id {V : Type} : ∀ f : Form V, shiftF id f = f
  | []               => rfl
  | (.ref c) :: t    => by simp [shiftF, shiftF_id t]
  | (.rng a b) :: t  => by simp [shiftF, shiftF_id t]
  | (.lit v) :: t    => by simp [shiftF, shiftF_id t]

/-- If the cell maps compose to the identity, the formula shifts round-trip to the
    original — so an insert followed by the matching delete restores every formula
    exactly (no reference is lost or mangled). -/
theorem shiftF_roundtrip {V : Type} (σ τ : Cell → Cell) (h : ∀ c, σ (τ c) = c) :
    ∀ f : Form V, shiftF σ (shiftF τ f) = f := by
  intro f
  rw [shiftF_comp]
  have : (σ ∘ τ) = (id : Cell → Cell) := by funext c; exact h c
  rw [this, shiftF_id]

/-! ## The concrete row-insert / row-delete cell maps and their inverse law -/

/-- Insert `1` blank row at index `k`: every cell at row ≥ k moves down by one. -/
def insertRow (k : Nat) (c : Cell) : Cell :=
  { c with row := if k ≤ c.row then c.row + 1 else c.row }

/-- Delete the row at index `k`: every cell at row > k moves up by one. (Cells AT row
    k are handled by the residual/#REF! layer, not this map.) -/
def deleteRow (k : Nat) (c : Cell) : Cell :=
  { c with row := if k < c.row then c.row - 1 else c.row }

/-- **Insert∘delete is the identity on cells.** Deleting row `k` right after inserting
    a blank row at `k` restores every cell's coordinate — the structural counterpart
    of the Z3-proved arithmetic law, now on the cell model. -/
theorem delete_insert_id (k : Nat) (c : Cell) : deleteRow k (insertRow k c) = c := by
  simp only [insertRow, deleteRow]
  by_cases h : k ≤ c.row
  · -- row ≥ k: insert → row+1, which is > k, so delete → row+1-1 = row
    have h1 : k < c.row + 1 := Nat.lt_succ_of_le h
    simp [h, h1]
  · -- row < k: insert leaves it, and it is not > k, so delete leaves it
    have h2 : ¬ k < c.row := fun hlt => h (Nat.le_of_lt hlt)
    simp [h, h2]

/-- Hence a formula shifted by an insert-then-delete at the same index is unchanged:
    the composed edit loses no reference and mangles none. -/
theorem insert_delete_form_id {V : Type} (k : Nat) (f : Form V) :
    shiftF (deleteRow k) (shiftF (insertRow k) f) = f :=
  shiftF_roundtrip (deleteRow k) (insertRow k) (delete_insert_id k) f

/-! ## Bridge to value-fidelity

`refs_shiftF` states that the reference-shift produces `deps' = map σ deps`. That is
verbatim the `hdeps` premise of `SelfOracle.eval_iso_invariant`. Composing the two:
if the edit keeps each cell's function and shifts its references by `σ`, then — because
`refs_shiftF` proves the resulting dependency graph is the `σ`-image — Theorem 1 gives
`eval C' (σ n) = eval C n` for every node, i.e. every computed value is preserved.
The extraction the theorem assumed is now discharged by the operation the tool runs;
the only remaining trusted step is the byte-level parse into this token model. -/

end RefShift
