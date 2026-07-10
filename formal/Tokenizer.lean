/-
  Tokenizer.lean — a verified reference tokenizer for the trusted byte→token layer.

  Both real defects the in-the-wild campaign found (UTF-8 double-encoding of
  string literals; the ASCII-only unquoted-qualifier grammar) lived in the
  byte→token layer the earlier proofs scope out. This file puts a REFERENCE
  TOKENIZER for that layer inside the machine-checked core, at the byte level,
  where both defects actually happened.

  Executable definitions:
    * `tokenize : List UInt8 → Option (List Seg)` — a fueled scanner producing
      string literals (verbatim spans, `""` escapes, unterminated → refuse),
      maximal word runs classified as grid-valid `$?COL$?ROW` references or
      opaque words (boundary- and call-paren-guarded), and passthrough bytes.
      It REFUSES (`none`) any `!` or `'` outside a literal: the model's
      verified surface is UNQUALIFIED formulas — sheet-qualified ones are the
      production denylist/differential surface, so the non-ASCII-qualifier
      defect class is refused wholesale by construction. Bytes ≥ 0x80 are word
      material: `集計A4` is ONE opaque word, never a boundary + reference.
    * `shiftSegs σ` — per-reference relabeling; `σ : Nat × Nat → Option (Nat ×
      Nat)`, `none` renders `#REF!`.

  Machine-checked theorems (no `sorry`):
    * **T1 `render_tokenize` (losslessness):** `tokenize bs = some segs →
      render segs = bs` — the tokenizer never invents, drops, or rewrites a
      byte, so the double-encoding defect class is impossible by construction
      for anything built on these spans.
    * **T2 `shift1_lit`/`shift1_word`/`shift1_other` (opacity):** the shift
      rewrites ONLY reference segments — literals, opaque words, and
      passthrough bytes are definitionally untouched. The mojibake defect
      violated exactly this.
    * **T3 `refs_shiftSegs` (σ-image, total case):** for a total relabeling the
      reference list of the shifted segments is the σ-image of the input's —
      verbatim the `hdeps` premise shape of the invariance theorem, discharged
      from the byte level.

  The remaining trusted link — "the production Rust tokenizer implements this
  reference on the model surface" — is discharged by a corpus-scale
  differential (`formal/tokenizer_differential.py`) between `main` below (via
  `lean --run`) and `xlq __shift-formula-batch`.

  Self-contained; no Mathlib; no `sorry`.
-/

namespace TokenizerModel

abbrev Byte := UInt8

inductive Seg
  | lit   (bs : List Byte)
  | word  (bs : List Byte)
  | ref   (bs : List Byte) (col row : Nat) (cAbs rAbs : Bool)
  | other (b : Byte)
deriving Repr, DecidableEq

def render1 : Seg → List Byte
  | .lit bs => bs
  | .word bs => bs
  | .ref bs _ _ _ _ => bs
  | .other b => [b]

def render (segs : List Seg) : List Byte := segs.flatMap render1

/-! ## Byte classes -/

def isUpper (b : Byte) : Bool := 65 ≤ b.toNat && b.toNat ≤ 90
def isLower (b : Byte) : Bool := 97 ≤ b.toNat && b.toNat ≤ 122
def isDigit (b : Byte) : Bool := 48 ≤ b.toNat && b.toNat ≤ 57
/-- Word-run material: letters, digits, `_ . $`, and every byte ≥ 0x80. -/
def isWordByte (b : Byte) : Bool :=
  isUpper b || isLower b || isDigit b || b == 95 || b == 46 || b == 36 ||
  b.toNat ≥ 128

def letterVal (b : Byte) : Nat := b.toNat - 64
def colVal : List Byte → Nat
  | [] => 0
  | b :: rest => letterVal b * 26 ^ rest.length + colVal rest
def rowVal : List Byte → Nat
  | [] => 0
  | b :: rest => (b.toNat - 48) * 10 ^ rest.length + rowVal rest

def gridOK (c r : Nat) : Bool :=
  1 ≤ c && c ≤ 16384 && 1 ≤ r && r ≤ 1048576

/-! ## Local list lemmas (kept core-free) -/

theorem takeWhile_append_dropWhile' (p : Byte → Bool) :
    ∀ l : List Byte, l.takeWhile p ++ l.dropWhile p = l
  | [] => rfl
  | b :: rest => by
      cases hp : p b <;>
        simp [List.takeWhile, List.dropWhile, hp,
              takeWhile_append_dropWhile' p rest]

theorem length_dropWhile_le' (p : Byte → Bool) :
    ∀ l : List Byte, (l.dropWhile p).length ≤ l.length
  | [] => by simp [List.dropWhile]
  | b :: rest => by
      cases hp : p b <;> simp [List.dropWhile, hp]
      exact Nat.le_trans (length_dropWhile_le' p rest) (Nat.le_succ _)

/-! ## Word classification (never splits the span) -/

/-- Match `w` against `$?[A-Z]+$?[0-9]+` exactly; return parsed coordinates. -/
def matchRef (w : List Byte) : Option (Nat × Nat × Bool × Bool) :=
  let (cAbs, w1) : Bool × List Byte := match w with
    | 36 :: r => (true, r)
    | _ => (false, w)
  let ls := w1.takeWhile isUpper
  let w2 := w1.dropWhile isUpper
  if ls.isEmpty then none else
  let (rAbs, w3) : Bool × List Byte := match w2 with
    | 36 :: r => (true, r)
    | _ => (false, w2)
  let ds := w3.takeWhile isDigit
  let w4 := w3.dropWhile isDigit
  if ds.isEmpty then none else
  if !w4.isEmpty then none else
  let c := colVal ls
  let r := rowVal ds
  if gridOK c r then some (c, r, cAbs, rAbs) else none

/-- Classify a word run: a reference iff the boundary allowed it, the pattern
    matches the whole word, and the next byte does not open a call
    (`LOG10(` protection). -/
def classify (bnd : Bool) (w : List Byte) (next : Option Byte) : Seg :=
  if bnd && next != some 40 then
    match matchRef w with
    | some (c, r, ca, ra) => .ref w c r ca ra
    | none => .word w
  else .word w

theorem render1_classify (bnd : Bool) (w : List Byte) (next : Option Byte) :
    render1 (classify bnd w next) = w := by
  unfold classify
  split
  · cases hm : matchRef w with
    | none => rfl
    | some p =>
        obtain ⟨c, r, ca, ra⟩ := p
        rfl
  · rfl

/-! ## String literals: length through the closing quote (called after the
    opening quote; `""` escapes; `none` = unterminated → refuse). -/

def scanLitAux : Nat → List Byte → Option Nat
  | 0, _ => none
  | _ + 1, [] => none
  | fuel + 1, b :: rest =>
      if b == 34 then
        match rest with
        | b2 :: rest2 =>
            if b2 == 34 then (scanLitAux fuel rest2).map (· + 2) else some 1
        | [] => some 1
      else (scanLitAux fuel rest).map (· + 1)

/-! ## The scanner -/

def tokenizeAux : Nat → List Byte → Bool → Option (List Seg)
  | 0, [], _ => some []
  | 0, _ :: _, _ => none
  | _ + 1, [], _ => some []
  | fuel + 1, b :: rest, bnd =>
    if b == 33 || b == 39 then none                        -- ! ' → refuse
    else if b == 34 then
      match scanLitAux (fuel + 1) rest with
      | none => none                                       -- unterminated
      | some n =>
          match tokenizeAux fuel (rest.drop n) true with
          | some segs => some (.lit (b :: rest.take n) :: segs)
          | none => none
    else if isWordByte b then
      match tokenizeAux fuel ((b :: rest).dropWhile isWordByte) false with
      | some segs =>
          some (classify bnd ((b :: rest).takeWhile isWordByte)
                  ((b :: rest).dropWhile isWordByte).head? :: segs)
      | none => none
    else
      match tokenizeAux fuel rest true with
      | some segs => some (.other b :: segs)
      | none => none

def tokenize (bs : List Byte) : Option (List Seg) :=
  tokenizeAux bs.length bs true

/-! ## T1: losslessness -/

theorem render_tokenizeAux (fuel : Nat) :
    ∀ (bs : List Byte) (bnd : Bool) (segs : List Seg),
      bs.length ≤ fuel → tokenizeAux fuel bs bnd = some segs →
      render segs = bs := by
  induction fuel with
  | zero =>
    intro bs bnd segs hlen h
    cases bs with
    | nil => simp only [tokenizeAux, Option.some.injEq] at h; subst h; rfl
    | cons b rest => simp at hlen
  | succ fuel ih =>
    intro bs bnd segs hlen h
    cases bs with
    | nil => simp only [tokenizeAux, Option.some.injEq] at h; subst h; rfl
    | cons b rest =>
      have hlen' : rest.length ≤ fuel := by simp at hlen; omega
      simp only [tokenizeAux] at h
      split at h
      · exact absurd h (by simp)
      · split at h
        · -- literal branch
          split at h
          · exact absurd h (by simp)
          · rename_i n hs
            split at h
            · rename_i segs' ht
              simp only [Option.some.injEq] at h
              subst h
              have hdl : (rest.drop n).length ≤ fuel := by
                simp only [List.length_drop]; omega
              have hr : render segs' = rest.drop n := ih _ true segs' hdl ht
              simp only [render, List.flatMap_cons, render1]
              rw [show List.flatMap render1 segs' = render segs' from rfl, hr]
              simp [List.take_append_drop]
            · exact absurd h (by simp)
        · split at h
          · -- word branch
            rename_i hwb
            split at h
            · rename_i segs' ht
              simp only [Option.some.injEq] at h
              subst h
              have hdw : (b :: rest).dropWhile isWordByte
                       = rest.dropWhile isWordByte := by
                simp [List.dropWhile, hwb]
              have hdl : ((b :: rest).dropWhile isWordByte).length ≤ fuel := by
                rw [hdw]
                exact Nat.le_trans (length_dropWhile_le' _ rest) hlen'
              have hr : render segs' = (b :: rest).dropWhile isWordByte :=
                ih _ false segs' hdl ht
              simp only [render, List.flatMap_cons, render1_classify]
              rw [show List.flatMap render1 segs' = render segs' from rfl, hr,
                  takeWhile_append_dropWhile']
            · exact absurd h (by simp)
          · -- passthrough branch
            split at h
            · rename_i segs' ht
              simp only [Option.some.injEq] at h
              subst h
              have hr : render segs' = rest := ih rest true segs' hlen' ht
              simp only [render, List.flatMap_cons, render1]
              rw [show List.flatMap render1 segs' = render segs' from rfl, hr]
              rfl
            · exact absurd h (by simp)

/-- **T1 (losslessness).** -/
theorem render_tokenize (bs : List Byte) (segs : List Seg)
    (h : tokenize bs = some segs) : render segs = bs :=
  render_tokenizeAux bs.length bs true segs (Nat.le_refl _) h

/-! ## The shift, and T2/T3 -/

def digitsOf (n : Nat) : List Byte :=
  (Nat.toDigits 10 n).map (fun c => UInt8.ofNat c.toNat)

def lettersOf (c : Nat) : List Byte :=
  if c = 0 then []
  else if c ≤ 26 then [UInt8.ofNat (c + 64)]
  else lettersOf ((c - 1) / 26) ++ [UInt8.ofNat ((c - 1) % 26 + 65)]
  decreasing_by
    simp_wf
    omega

def renderRef (c r : Nat) (ca ra : Bool) : List Byte :=
  (if ca then [36] else []) ++ lettersOf c ++
  (if ra then [36] else []) ++ digitsOf r

def refBytes : List Byte := [35, 82, 69, 70, 33]          -- "#REF!"

def shift1 (σ : Nat × Nat → Option (Nat × Nat)) : Seg → Seg
  | .ref _ c r ca ra =>
      match σ (c, r) with
      | some (c', r') => .ref (renderRef c' r' ca ra) c' r' ca ra
      | none => .word refBytes
  | s => s

def shiftSegs (σ : Nat × Nat → Option (Nat × Nat)) (segs : List Seg) : List Seg :=
  segs.map (shift1 σ)

/-- **T2 (opacity):** the shift touches ONLY reference segments. -/
theorem shift1_lit (σ) (bs : List Byte) : shift1 σ (.lit bs) = .lit bs := rfl
theorem shift1_word (σ) (bs : List Byte) : shift1 σ (.word bs) = .word bs := rfl
theorem shift1_other (σ) (b : Byte) : shift1 σ (.other b) = .other b := rfl

def refs : List Seg → List (Nat × Nat)
  | [] => []
  | .ref _ c r _ _ :: rest => (c, r) :: refs rest
  | .lit _ :: rest => refs rest
  | .word _ :: rest => refs rest
  | .other _ :: rest => refs rest

/-- **T3 (σ-image, total case).** -/
theorem refs_shiftSegs (f : Nat × Nat → Nat × Nat) (segs : List Seg) :
    refs (shiftSegs (fun p => some (f p)) segs) = (refs segs).map f := by
  induction segs with
  | nil => rfl
  | cons s rest ih =>
    cases s with
    | ref bs c r ca ra =>
        simp only [shiftSegs, List.map_cons, shift1, refs, List.map] at ih ⊢
        rw [ih]
    | lit bs => simpa [shiftSegs, shift1, refs] using ih
    | word bs => simpa [shiftSegs, shift1, refs] using ih
    | other b => simpa [shiftSegs, shift1, refs] using ih

/-! ## Executable surface + defect regressions -/

def strBytes (s : String) : List Byte := s.toUTF8.toList
def bytesStr (bs : List Byte) : String :=
  match String.fromUTF8? ⟨bs.toArray⟩ with
  | some s => s
  | none => "<invalid-utf8>"

def insRows (k n : Nat) : Nat × Nat → Option (Nat × Nat) :=
  fun (c, r) => some (c, if r ≥ k then r + n else r)

def shiftFormula (σ : Nat × Nat → Option (Nat × Nat)) (s : String) : Option String :=
  match tokenize (strBytes s) with
  | some segs => some (bytesStr (render (shiftSegs σ segs)))
  | none => none

-- the exact locked-test defect shape: literal preserved, refs shifted
#eval shiftFormula (insRows 2 1) "IF(C8=\"\",\"\",IF(C8=$IA$4,\"大当たり！\",\"はずれ！\"))"
-- the sibling defect shape: unquoted non-ASCII qualifier → REFUSED (none)
#eval shiftFormula (insRows 2 1) "集計01!CI3"
-- range endpoints shift per-cell; LOG10( protected by the paren guard
#eval shiftFormula (insRows 2 1) "SUM(A2:B5)+LOG10(A2)"

/-! ## Differential driver: TSV on stdin
    `formula \t axis \t op \t at \t count` → shifted | __REFUSE__ -/

def sigmaOf (axis op : String) (at_ cnt : Nat) : Nat × Nat → Option (Nat × Nat) :=
  fun (c, r) =>
    let move (v : Nat) : Option Nat :=
      match op with
      | "insert" => some (if v ≥ at_ then v + cnt else v)
      | "delete" =>
          if v < at_ then some v
          else if v ≥ at_ + cnt then some (v - cnt)
          else none
      | _ => none
    match axis with
    | "row" => (move r).map (fun r' => (c, r'))
    | "col" => (move c).map (fun c' => (c', r))
    | _ => none

def main : IO Unit := do
  let stdin ← IO.getStdin
  let stdout ← IO.getStdout
  let mut line ← stdin.getLine
  while line ≠ "" do
    let l := (line.dropRightWhile (· == '\n')).dropRightWhile (· == '\r')
    let parts := l.splitOn "\t"
    match parts with
    | [f, axis, op, atS, cntS] =>
        match atS.toNat?, cntS.toNat? with
        | some at_, some cnt =>
            match shiftFormula (sigmaOf axis op at_ cnt) f with
            | some out => stdout.putStrLn out
            | none => stdout.putStrLn "__REFUSE__"
        | _, _ => stdout.putStrLn "__BADLINE__"
    | _ => stdout.putStrLn "__BADLINE__"
    line ← stdin.getLine

end TokenizerModel

def main : IO Unit := TokenizerModel.main
