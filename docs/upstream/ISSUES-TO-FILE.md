# IronCalc — ready-to-file issues

These are draft GitHub issues for `ironcalc/IronCalc`, one section per bug.
Copy the **Title** line into the issue title and the **Body** into the issue
body. They are written as gifts to the maintainers, not complaints — each
includes a minimal repro, the Excel-verified expected value, IronCalc's actual
output, and a one-line hypothesis about the cause.

**How these were found.** We differential-tested IronCalc master (`e50ccea8`)
against LibreOffice 24.8 over 1,634 formula cases / 484 functions from one
shared workbook, then arbitrated every value-vs-value disagreement against
desktop Excel (M365 / documented behavior). Financial day-count and pricing
verdicts were re-derived by reimplementing Excel's documented formulas in
Python. This surfaced **~11 IronCalc issue clusters** (below) and, symmetrically,
41 LibreOffice bugs — so the method is not tuned to flatter either engine. Raw
data (both engines' values per case) lives in `benchmarks/agreement.json` in our
tree if you want to check our work.

The last issue (**coverage gaps**) collects several functions that error on
valid input into a single checklist rather than filing eleven micro-issues.

---

## 1. CONVERT(68,"F","C") returns 19.65 instead of 20 — offset/scale ordering in the temperature path

**Body:**

```
=CONVERT(68,"F","C")
```

- **Expected (Excel):** `20` — 68 °F is exactly 20 °C.
- **Actual (IronCalc):** `19.650000000000034`
- **Likely cause:** Temperature units need an affine transform (scale *and*
  additive offset). The result suggests the F→C path applies the scale factor
  and the 32°/273.15 offsets in the wrong order (or via an intermediate
  absolute scale without re-applying the offset), so the additive term is lost
  or mis-weighted. `=CONVERT(1,"lbm","kg")` and other pure-ratio conversions
  are correct, which points specifically at the offset-carrying temperature
  units (F/C/K/Rank).

---

## 2. TRIM does not collapse internal whitespace runs

**Body:**

```
=TRIM("  a   b  ")
```

- **Expected (Excel):** `a b` — TRIM strips leading/trailing spaces **and**
  collapses each interior run of spaces to a single space.
- **Actual (IronCalc):** `a   b` (leading/trailing removed, but the 3 interior
  spaces are preserved).
- **Likely cause:** The implementation only trims the ends (e.g. a
  `trim()`-equivalent) and never squeezes interior runs. Excel's TRIM is
  "trim ends + collapse interior runs to one space each."

---

## 3. ROW over a multi-row range returns a scalar, not an array

**Body:**

```
=SUM(ROW(C1:C5))
```

- **Expected (Excel):** `15` — `ROW(C1:C5)` yields the vertical array
  `{1;2;3;4;5}`, which SUM adds to 15.
- **Actual (IronCalc):** `1` (only the first row's number).
- **Likely cause:** `ROW(range)` collapses to the first cell's row index
  instead of spilling an array spanning the range's rows. `COLUMN(range)`
  is worth checking for the mirror-image bug.

---

## 4. SUMPRODUCT does not coerce a boolean comparison array

**Body:**

```
=SUMPRODUCT(--(A1:A10>5))     ' where 5 of the 10 values exceed 5
```

- **Expected (Excel):** `5` — the double-unary coerces the `{TRUE;FALSE;…}`
  comparison array to `{1;0;…}`, and SUMPRODUCT sums it.
- **Actual (IronCalc):** `0`
- **Likely cause:** The boolean array produced by `>` (or the `--` unary over
  it) is not coerced to numeric 1/0 inside SUMPRODUCT — it appears to be
  treated as an all-zero / non-numeric array. This is the single most common
  SUMPRODUCT idiom (`SUMPRODUCT(--(cond))`), so it is worth prioritizing.

---

## 5. MAXA / MINA / STDEVA (the "A" family) ignore text and boolean cells

**Body:**

The `*A`-suffixed statistical functions are defined to include logical and text
values referenced from cells: `TRUE`→1, `FALSE`→0, text→0. IronCalc appears to
skip such cells as if blank.

```
=MAXA(A6,E1)          ' A6 = -3, E1 = TRUE   → Excel 1,   IronCalc -3
=MINA(C1:C5,E2)       ' E2 = FALSE           → Excel 0,   IronCalc 1
=STDEVA(A1:A2,B9)     ' B9 = "100" (text)    → Excel 2,   IronCalc 1.4142135623730951
```

- **Expected (Excel):** MAXA counts `TRUE` as 1 → `1`; MINA counts `FALSE`
  as 0 → `0`; STDEVA counts text-in-reference as 0 (numeric-looking text is
  **not** coerced), giving the population/sample stat over `{A1, A2, 0}` → `2`.
- **Actual (IronCalc):** `-3`, `1`, `1.4142135623730951` respectively — in each
  case the boolean/text cell was dropped instead of contributing 1 / 0 / 0.
- **Likely cause:** The `*A` variants reuse the plain (MAX/MIN/STDEV)
  numeric-cell filter, which excludes booleans and text, instead of the
  "A"-rule coercion. Likely also affects AVERAGEA / VARA / VARPA / STDEVPA.

---

## 6. SECOND (and likely MINUTE/HOUR) truncates sub-second time instead of rounding

**Body:**

```
=SECOND(0.999999)
```

- **Expected (Excel):** `0` — 0.999999 of a day is 23:59:59.9136; Excel rounds
  the time serial to the nearest second first, which rolls over to 00:00:00.
- **Actual (IronCalc):** `59` (truncates the fractional second).
- **Likely cause:** Time-component extraction truncates the fractional seconds
  rather than rounding the serial to the nearest second before decomposing.
  Worth checking MINUTE/HOUR for the same rounding-vs-truncation boundary.

---

## 7. PRICE with basis 3 (actual/365) returns exact par at coupon == yield

**Body:**

```
=PRICE(DATE(2026,3,15),DATE(2031,3,15),0.05,0.05,100,4,3)
```

- **Expected (Excel):** `99.9897902308227` (≈ 99.98979) — with basis 3
  (actual/365) the quasi-coupon periods are not uniform in length, so a bond
  whose coupon equals its yield does **not** price at exactly 100.
- **Actual (IronCalc):** `100.00000000000007` (exact par).
- **Likely cause:** The basis-3 branch appears to discount over uniform
  periods (as if basis 0), ignoring the actual/365 day-count fraction that
  makes the price deviate slightly from par. Re-derived independently in
  Python; high confidence.

---

## 8. Complex functions leak inf/NaN text at singularities instead of returning #NUM!

**Body:**

At their poles these complex functions emit IEEE inf/NaN straight into the
string formatter rather than the `#NUM!` Excel returns.

```
=IMLN("0")     → IronCalc "-inf"      Excel #NUM!
=IMCOT("0")    → IronCalc "inf"       Excel #NUM!
=IMCSC("0")    → IronCalc "NaNNaNi"   Excel #NUM!
=IMCSCH("0")   → IronCalc "NaNNaNi"   Excel #NUM!
```

- **Expected (Excel):** `#NUM!` for all four (ln, cot, csc, csch are singular
  at 0).
- **Actual (IronCalc):** the textual strings `"-inf"`, `"inf"`, `"NaNNaNi"`.
- **Likely cause:** No singularity guard before formatting the complex result;
  non-finite intermediate values (`ln 0`, `1/sin 0`, `1/tan 0`) propagate into
  the `a+bi` string builder. Guarding non-finite real/imaginary parts and
  returning `#NUM!` would fix the whole family at once.

---

## 9. ODDFPRICE / ODDLPRICE accept invalid date orderings

**Body:**

Excel enforces `issue < settlement < first_coupon < maturity` (and the
last-period analogue for ODDLPRICE) and returns `#NUM!` when the ordering is
violated. IronCalc computes a price anyway.

```
' settlement (2026-02-01) precedes issue (2026-03-01):
=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2026,3,1),DATE(2026,9,1),0.05,0.06,100,2)
' last_interest (2026-03-01) is after settlement (2026-02-01):
=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2026,3,1),0.05,0.06,100,2)
```

- **Expected (Excel):** `#NUM!` for both (invalid date ordering).
- **Actual (IronCalc):** `95.2644236004297` and `99.71603320227172`
  respectively — a numeric price for an ill-formed bond.
- **Likely cause:** The odd-period pricing paths validate individual dates but
  skip the pairwise-ordering precondition. (Note: for *valid* odd-coupon bonds
  IronCalc's prices look correct and LibreOffice is the one erroring — so this
  is only about the missing domain guard, not the pricing math.)

---

## 10. PERCENTRANK.EXC truncates to decimal places, not significant digits

**Body:**

```
=PERCENTRANK.EXC(A1:A10,-3)
```

- **Expected (Excel):** `0.0909` — PERCENTRANK(.INC/.EXC) truncates the result
  to the requested number of **significant digits** (default 3), so a small
  rank like 0.0909… keeps four decimals worth of significant figures.
- **Actual (IronCalc):** `0.09` (truncated to 3 **decimal places**).
- **Likely cause:** The rounding step truncates to N decimal places rather than
  N significant digits. Note IronCalc already matches Excel (and beats LO's
  rounding) for ranks ≥ 0.1 where the two rules coincide — the divergence only
  shows up for ranks below 0.1. Medium confidence; worth a spot-check against
  live Excel.

---

## 11. Coverage gaps — functions that error on valid input (checklist)

**Body:**

A cluster of functions return an error (or lose array typing) on inputs Excel
accepts. Grouping them here rather than filing separately; each is independent
and could be picked off one at a time. Verified expected values are from Excel.

Reference-shape / missing-mode gaps:

- [ ] **AREAS over a union reference** — `=AREAS((A1:A5,C1:C5))` → IronCalc
  `#ERROR!`, Excel `2`. Union (comma) reference arguments not accepted.
- [ ] **GROWTH with array-literal known/new_x** —
  `=INDEX(GROWTH({2,4,8,16,32},{1,2,3,4,5},6),1,1)` → IronCalc `#REF!`,
  Excel `64`. `#REF!` looks like a reference-shape assumption on `new_x`.
- [ ] **SHEET with a reference argument** — `=SHEET(A1)` → IronCalc `#N/A`,
  Excel `1`.
- [ ] **SHEETS with a reference argument** — `=SHEETS(A1:A10)` → IronCalc
  `#N/IMPL!`, Excel `1`.
- [ ] **INDIRECT R1C1 mode** — `=INDIRECT("R3C1",FALSE)` → IronCalc
  `#N/IMPL!`, Excel `6`. R1C1-style refs (second arg FALSE) unimplemented.
- [ ] **IMPRODUCT with >2 scalar args** — `=IMPRODUCT("2","3","4")` → IronCalc
  `#ERROR!`, Excel `24`. Variadic arity seems capped at 2.

Dynamic-array typing (these may share one root cause — an array result losing
numeric typing when consumed by COUNT/ROWS):

- [ ] **FILTER through COUNT** — `=COUNT(FILTER(C1:C5,D1:D5>25))` (3 rows match)
  → IronCalc `0`, Excel `3`.
- [ ] **MODE.MULT through COUNT** — `=COUNT(MODE.MULT({1,1,2,2,3}))` (two modes)
  → IronCalc `0`, Excel `2`.
- [ ] **UNIQUE over an array literal** — `=ROWS(UNIQUE({1;2;2;3}))` → IronCalc
  `#VALUE!`, Excel `3`. (FILTER/MODE.MULT/UNIQUE over array literals all
  smell like the same array-typing/plumbing issue.)

Financial domain guards that are too strict:

- [ ] **AMORDEGRC** — `=AMORDEGRC(10000,DATE(2025,1,1),DATE(2025,12,31),1000,2,0.2,0)`
  → IronCalc `#NUM!`, Excel `1440`. Rate 0.2 with basis 0 rejected as
  out-of-domain when Excel computes it.
- [ ] **PRICEMAT with settlement == issue** —
  `=PRICEMAT(DATE(2026,1,1),DATE(2027,1,1),DATE(2026,1,1),0.04,0.05,1)` →
  IronCalc `#NUM!`, Excel `99.0476…`. A zero-length accrued-interest period
  (settlement on the issue date) is valid in Excel.
