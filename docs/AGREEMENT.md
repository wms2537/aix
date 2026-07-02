# ironcalc vs LibreOffice: differential agreement report

Part of **AXLE-bench** (suite front page:
[benchmarks/README.md](../benchmarks/README.md)) — this document is the
narrative for axis 1 (Correctness).

Generated 2026-07-03 by `benchmarks/run_oracle.sh` from
`benchmarks/oracle-cases.json` (492 functions, 1,659 cases; regenerated for
the 100%-catalog milestone — 25 new cases covering the newly implemented
BAHTTEXT, DBCS, JIS, EUROCONVERT, FILTERXML, GROUPBY, PIVOTBY, PHONETIC).
Raw per-case results: `benchmarks/agreement.json`.

Coverage caveat up front: "492 functions" is the number of functions
*exercised*, not the number the oracle actually *validated*. For 36 of them
(115 cases) LibreOffice could not produce a comparable value for a single
case — `#NAME?` (function unknown) or `#VALUE!` (inline-LAMBDA invocation it
cannot call) on every row — so this report says nothing about whether
ironcalc's values for those functions are right. The full list is in the
"functions with no oracle signal" note below.

Engines under comparison:

- **ironcalc 0.7.1+e50ccea8 (vendored master)** (`vendor/upstream`, the
  engine xlq links — the master pin plus the local residual-functions patch
  and the full-catalog Tier I/Tier II implementations; see
  `docs/COVERAGE.md`)
- **LibreOffice Calc 24.8.7.2** (`/usr/bin/soffice`, headless, locale pinned
  to C.UTF-8/en-US by the harness)

## Methodology

1. **One shared workbook.** `benchmarks/gen_oracle_workbook.py` writes
   `oracle.xlsx` via openpyxl: a fixed data block on sheet `T`
   (`A1:A10` numbers incl. negatives/zero/decimals, `B1:B10` text incl.
   mixed case/whitespace/numeric-looking strings and one blank, `C1:C5`,
   `D1:D5` numbers, `E1`/`E2` booleans — the exact block is recorded under
   `_meta` in `oracle-cases.json`), then one case per row with the formula
   in column `G` (row *n* = case *n*, functions in sorted order).
2. **LibreOffice computes on convert.** openpyxl writes *no* cached formula
   values, so `soffice --headless --convert-to xlsx` must evaluate every
   formula itself; the `<v>` values LibreOffice writes into the converted
   file are the LibreOffice side of the oracle (verified: all 1,659 cells
   carry computed values). The harness pins `LC_ALL=C.UTF-8` and a throwaway
   `-env:UserInstallation` profile: several cases (TEXT format codes,
   DATEVALUE, DOLLAR, FIXED, VALUE, NUMBERVALUE) are locale-sensitive in
   LibreOffice, and an unpinned run inherits whatever the host locale is.
3. **ironcalc computes in memory.** `xlq`'s `oracle-compare` binary
   (`xlq/src/bin/oracle_compare.rs`) builds one in-memory ironcalc model
   with the identical data block, sets each case formula verbatim,
   evaluates once, and reads both the raw and the formatted value.
4. **LibreOffice values are read back through ironcalc's importer** —
   without re-evaluation — so the comparison also battle-tests xlsx import
   on a LibreOffice-produced file.

### Comparison policy

| Kind | Rule |
|---|---|
| LibreOffice `#NAME?` | verdict **`lo_unsupported`**: LO does not know the function at all — it never evaluated the arguments, so the row carries **no oracle signal**: it is recorded as neither a disagreement (nothing was compared) nor corroboration (nothing was corroborated), whatever ironcalc answered. Rows where ironcalc *also* errored are additionally flagged `iron_errored_too` — those errors are exactly as unchecked as a value would be |
| numbers | relative tolerance 1e-9; absolute 1e-12 near zero — **but an exact zero on one side only matches an exact zero on the other** (a zero-vs-tiny pair is underflow or rounding residue: it goes to the triage table, not into "agree") |
| text, booleans | exact |
| errors | if **both** sides error → `both_error` (counted separately, not a disagreement; error-*code* equality is reported on the side) |
| empty | LibreOffice empty string == ironcalc empty cell |
| `engine_error` | ironcalc could not accept/evaluate the case at all |

The `lo_unsupported` class is new in this run (before 2026-07-03 those rows
were spread across `disagree` — 76 rows — and `both_error` — 15 rows —
which understated agreement and overstated corroboration at the same time).
Comparisons with the previous report must account for the reclassification;
no pre-existing case changed its underlying values, only its class.

Numeric agreements that are **not bit-identical** are flagged
`within_tolerance` per case and counted in the totals
(`agree_exact` / `agree_within_tolerance`): of the 1,273 agreements, 935 are
exact and **338 hold only under the tolerance policy**. Most of those 338
sit at LibreOffice's ~1e-15 `<v>` print quantization, but 14 show relative
drift between 4e-11 and 2e-10 — far above print noise — concentrated in one
special-function family (`ERF`/`ERF.PRECISE`/`ERFC`/`ERFC.PRECISE`,
`BESSELI`/`BESSELK`, `GAUSS`, `NORM.S.DIST`/`NORMSDIST`,
`LOGNORM.DIST`/`LOGNORMDIST`, `Z.TEST`/`ZTEST`). That is a real algorithmic
difference that the 1e-9 threshold happens to absorb at these arguments;
`BESSELI` illustrates the arbitrariness — `=BESSELI(1.5,1)` (rel 1.05e-10)
counts as agreement while `=BESSELI(A1,0)` (rel 2.2e-9, same drift family)
lands in disagreement bucket C below.

### LibreOffice is a REFERENCE, not ground truth

Where the engines disagree, **Excel is the arbiter**. A disagreement is a
*finding* that goes to the triage table below for manual investigation — it
is **not** automatically an ironcalc bug. Several disagreements below are in
fact LibreOffice deviating from Excel (e.g. `POWER(0,0)`, boolean-cell
semantics in `COUNT`/`ISNUMBER`) while ironcalc matches Excel. Nothing in
this report has been verified against a live Excel instance; per-case Excel
verdicts in the notes are based on documented Excel semantics and remain to
be confirmed.

### Storage-prefix handling (affects what LibreOffice can compute)

OOXML stores post-2007 function names with the `_xlfn.` prefix
(`_xlfn._xlws.` for `FILTER`/`SORT`); the generator adds these prefixes
(FILTERXML, GROUPBY, and PIVOTBY joined the prefix list with this run),
otherwise LibreOffice imports the names as unknown macro calls. Empirically
verified quirk of LibreOffice 24.8's importer: `IMCOSH IMCOT IMCSC IMCSCH
IMSEC IMSECH IMSINH IMTAN NETWORKDAYS.INTL WORKDAY.INTL ISO.CEILING` are
recognized only *unprefixed*, so those are stored unprefixed. ironcalc
always receives the canonical Excel spelling verbatim.

## Headline results

- **1659 cases**: 1273 agree (935 exact, 338 within tolerance), 85
  disagree (47 one-side-error + 38 both-value), 196 both-error, 105
  lo_unsupported (LibreOffice does not know the function — no oracle;
  ironcalc also errored on 15 of them), 0 engine errors.
- **Agreement where both engines produced a non-error value: 97.1%**
  (1273 of 1311 — the 38 both-value disagreements are bucket C below).
- Counting every real disagreement — the 47 cases where one side errored on
  a function the other computes, plus the 38 value differences — agreement
  is 93.7% (1273/1358). (The pre-reclassification report quoted 88.8%; most
  of that gap was LibreOffice's missing functions masquerading as
  disagreements, which now sit in `lo_unsupported`.)
- Including both-error as agreement-that-it-errors: 94.5% of the 1,554
  cases that carry any oracle signal (1469/1554).
- Of the 196 both-error cases, the error *codes* match in 58 and differ in
  138. The dominant pattern — 118 of 138, across functions that **both
  engines implement** — is ironcalc `#NUM!` vs LibreOffice `#VALUE!` on
  argument-domain errors (e.g. `ACOSH(0.5)`, `ATANH(1)`, `COMBIN(4,5)`).
  Verified against the converted XML: LibreOffice computes its internal
  `Err:502` (illegal argument) for these and its OOXML export writes that
  as `#VALUE!`, so the code comparison here measures LO's error-export
  mapping at least as much as engine semantics — treat code-level equality
  as a weak signal.

| Category | Cases | Agree | Disagree (one side errors) | Disagree (values differ) | Both error | No oracle (LO `#NAME?`) | Both-value agreement |
|---|---|---|---|---|---|---|---|
| Database | 28 | 27 | 0 | 0 | 1 | 0 | 100.0% |
| Date & time | 84 | 77 | 0 | 1 | 6 | 0 | 98.7% |
| Engineering | 177 | 147 | 7 | 3 | 20 | 0 | 98.0% |
| Financial | 178 | 128 | 10 | 10 | 30 | 0 | 92.8% |
| Information | 66 | 60 | 2 | 2 | 2 | 0 | 96.8% |
| Lambda & functional | 32 | 3 | 6 | 0 | 0 | 23 | 100.0% |
| Logical | 41 | 36 | 1 | 0 | 4 | 0 | 100.0% |
| Lookup & reference | 116 | 59 | 3 | 2 | 11 | 41 | 96.7% |
| Math & trig | 279 | 243 | 2 | 1 | 30 | 3 | 99.6% |
| Statistical | 481 | 370 | 12 | 13 | 83 | 3 | 96.6% |
| Text | 169 | 119 | 4 | 4 | 7 | 35 | 96.7% |
| Web & add-in | 8 | 4 | 0 | 2 | 2 | 0 | 66.7% |
| **Total** | 1659 | 1273 | 47 | 38 | 196 | 105 | **97.1%** |

"Both-value agreement" is agree / (agree + values-differ): what fraction of
the cases where **both** engines returned a non-error value the values
match. (Category assignments are hand-classified, an editorial reading aid;
only the totals row is machine-checked against `agreement.json`. GROUPBY and
PIVOTBY are filed under Lambda & functional, the new BAHTTEXT/DBCS/JIS/
PHONETIC under Text, and FILTERXML/EUROCONVERT under the new Web & add-in
row.)

### The 100%-catalog milestone functions, specifically

| Function | Cases | LibreOffice oracle? | Result |
|---|---|---|---|
| JIS | 3 | yes | **3/3 agree**, including ASCII width conversion with the space excluded and half-width-katakana voiced-mark composition (ｶ+ﾞ→ガ) |
| EUROCONVERT | 4 | yes | **3 agree + 1 both-error (#VALUE!, codes match)** — including the spec's worked triangulation example `EUROCONVERT(1,"FRF","DEM",TRUE,3)` → `0.29728616` on both engines |
| FILTERXML | 4 | yes | 1 agree (text node), 1 both-error (invalid XML → #VALUE!, codes match), **2 type disagreements**: for numeric-looking node text ironcalc returns the number `5`, LibreOffice the string `"5"`. Excel converts numeric node text to numbers (the common `SUM(FILTERXML(...))` pattern depends on it), so ironcalc is believed Excel-correct here; bucket C rows 521–522 |
| BAHTTEXT | 4 | no (`#NAME?` on OOXML import) | lo_unsupported — no oracle; values are LibreOffice-verified at implementation time in the engine's own test suite instead |
| DBCS | 3 | no (LO only knows this built-in as JIS) | lo_unsupported — but DBCS and JIS dispatch to the same implementation, and JIS agrees 3/3 |
| GROUPBY / PIVOTBY | 3 / 2 | no (LO 24.8 lacks them) | lo_unsupported |
| PHONETIC | 2 | no (LO 24.8 lacks it) | lo_unsupported |
| TRIMRANGE / PERCENTOF | 3 / 3 (pre-existing cases) | no (LO 24.8 lacks them) | lo_unsupported |

### Functions with no oracle signal (36 functions, 115 cases)

For these functions LibreOffice produced **no comparable non-error value on
any case** (`#NAME?`, or `#VALUE!` for the uncallable inline-LAMBDA rows),
so this report validates nothing about ironcalc's values for them —
ironcalc could be wrong on all 115 cases without this harness noticing:

`ARRAYTOTEXT`, `BAHTTEXT`, `BINOM.DIST.RANGE`, `BYCOL`, `BYROW`,
`CHOOSECOLS`, `CHOOSEROWS`, `DBCS`, `DROP`, `EXPAND`, `GROUPBY`, `HSTACK`,
`ISOMITTED`, `LAMBDA`, `MAKEARRAY`, `MAP`, `ODDFPRICE`, `ODDFYIELD`,
`PERCENTOF`, `PHONETIC`, `PIVOTBY`, `REDUCE`, `REGEXEXTRACT`,
`REGEXREPLACE`, `REGEXTEST`, `SCAN`, `TAKE`, `TEXTAFTER`, `TEXTBEFORE`,
`TOCOL`, `TOROW`, `TRIMRANGE`, `VALUETOTEXT`, `VSTACK`, `WRAPCOLS`,
`WRAPROWS`

They still count toward the headline "492 functions" — read that number as
breadth of exercise, not breadth of validation. Closing this gap needs a
second oracle (live Excel, or targeted hand-checked expectations; for the
milestone functions the engine's test suites carry hand-computed
expectations from the spec).

## Known LibreOffice 24.8 deviations / gaps observed in this run

- **Functions absent from LibreOffice 24.8's OOXML import** (LO returns
  `#NAME?` — verdict `lo_unsupported`, per-function counts in the table
  below): `ARRAYTOTEXT`, `BAHTTEXT`, `BINOM.DIST.RANGE`, `BYCOL`, `BYROW`,
  `CHOOSECOLS`, `CHOOSEROWS`, `DBCS`, `DROP`, `EXPAND`, `GROUPBY`,
  `HSTACK`, `MAKEARRAY`, `MAP`, `PERCENTOF`, `PHONETIC`, `PIVOTBY`,
  `REDUCE`, `REGEXEXTRACT`, `REGEXREPLACE`, `REGEXTEST`, `SCAN`, `TAKE`,
  `TEXTAFTER`, `TEXTBEFORE`, `TEXTSPLIT`, `TOCOL`, `TOROW`, `TRIMRANGE`,
  `VALUETOTEXT`, `VSTACK`, `WRAPCOLS`, `WRAPROWS`.
- **Inline LAMBDA invocation** `=LAMBDA(...)(args)` is parsed but not
  callable in LO 24.8 (`#VALUE!`); this also breaks the `ISOMITTED` cases.
- **FILTERXML returns text for numeric node values** (`"5"`), where
  ironcalc — following Excel's observed behavior — returns the number `5`.
- **Booleans are numbers in Calc**: `COUNT(E1:E2)`→2, `ISNUMBER(E1)`→TRUE,
  `TYPE(E1)`→8, `CONCATENATE(E1,"x")`→"1x". Excel semantics (booleans are
  their own type) match ironcalc's answers on these cases.
- **`POWER(0,0)`→1 and `ATAN2(0,0)`→0** in Calc; Excel raises `#NUM!` /
  `#DIV/0!`, as ironcalc does.
- **`PERCENTRANK` family**: Calc *rounds* the significance digits (0.556),
  Excel *truncates* (0.555) — ironcalc truncates.
- **`CONVERT`** constants differ slightly (`lbm`→`kg`: LO 0.4535923097…,
  exact/Excel 0.45359237).
- **`DOLLAR`** negative formatting: LO `-$1,200` vs Excel/ironcalc `($1,200)`.
- **T-bill functions** (`TBILLEQ`/`TBILLPRICE`/`TBILLYIELD`) and several
  bond functions use slightly different day-count handling in Calc.
- **Error-code export**: LO's internal `Err:502` (illegal argument) is
  written to OOXML as `#VALUE!`, which inflates the `#NUM!`-vs-`#VALUE!`
  code mismatches in the both-error bucket (see Headline results).

## Triage tables (all 85 disagreements + the 15 iron-error lo_unsupported rows)

A row in A/B/C means the two engines decided differently on a function both
implement. Excel is the arbiter for triage; neither column is presumed
correct. Table D lists the `lo_unsupported` rows where ironcalc also
errored — no oracle, but ironcalc's own error deserves eyes.

### A. LibreOffice errors, ironcalc returns a value (21 cases)

All informative disagreements on functions LO *does* recognize
(inline-LAMBDA invocation, `IMCOT`/`IMCSC`/`IMCSCH`/`IMLN` at 0,
`NUMBERVALUE("3.5%")`, `ODDFPRICE`/`ODDFYIELD`/`ODDLPRICE`, `OR(range>4)`,
`PRODUCT`/`SUM` string coercion, `SKEW.P`). LO-`#NAME?` rows no longer
appear here — they are `lo_unsupported`, not disagreement.

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 696 | IMCOT | `=IMCOT("0")` | `inf` | `#VALUE!` |
| 699 | IMCSC | `=IMCSC("0")` | `NaNNaNi` | `#VALUE!` |
| 702 | IMCSCH | `=IMCSCH("0")` | `NaNNaNi` | `#VALUE!` |
| 711 | IMLN | `=IMLN("0")` | `-inf` | `#VALUE!` |
| 820 | ISOMITTED | `=LAMBDA([x],ISOMITTED(x))()` | `TRUE` | `#VALUE!` |
| 821 | ISOMITTED | `=LAMBDA([x],ISOMITTED(x))(5)` | `FALSE` | `#VALUE!` |
| 822 | ISOMITTED | `=LAMBDA([x],IF(ISOMITTED(x),-1,x))(A1)` | `2` | `#VALUE!` |
| 844 | LAMBDA | `=LAMBDA(x,y,x+y)(A1,A2)` | `6` | `#VALUE!` |
| 845 | LAMBDA | `=LAMBDA(x,x*x)(A5)` | `100` | `#VALUE!` |
| 846 | LAMBDA | `=LAMBDA(s,UPPER(s))(B1)` | `ALPHA` | `#VALUE!` |
| 1067 | NUMBERVALUE | `=NUMBERVALUE("3.5%")` | `0.035` | `#VALUE!` |
| 1085 | ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.63871848080437` | `#VALUE!` |
| 1086 | ODDFPRICE | `=ODDFPRICE(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.05,0.06,100,1)` | `95.91405301998944` | `#VALUE!` |
| 1087 | ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2026,3,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.2644236004297` | `#VALUE!` |
| 1088 | ODDFYIELD | `=ODDFYIELD(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,95,100,2)` | `0.06151963143529892` | `#VALUE!` |
| 1089 | ODDFYIELD | `=ODDFYIELD(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.04,101.5,100,1)` | `0.03650600623418319` | `#VALUE!` |
| 1093 | ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2026,3,1),0.05,0.06,100,2)` | `99.71603320227172` | `#VALUE!` |
| 1103 | OR | `=OR(C1:C5>4)` | `TRUE` | `#VALUE!` |
| 1181 | PRODUCT | `=PRODUCT(C1:C3,"2")` | `12` | `#VALUE!` |
| 1333 | SKEW.P | `=SKEW.P(A1:A2)` | `0` | `#DIV/0!` |
| 1395 | SUM | `=SUM("3",2)` | `5` | `#VALUE!` |

### B. ironcalc errors, LibreOffice returns a value (26 cases)

Candidate ironcalc strictness/coverage issues — each needs an Excel check
(for several of these, e.g. `POWER(0,0)`, `ATAN2(0,0)`, `CHIDIST(-1,2)`,
documented Excel behaviour is an error, i.e. ironcalc matches Excel and
LibreOffice deviates).

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 32 | AMORDEGRC | `=AMORDEGRC(10000,DATE(2025,1,1),DATE(2025,12,31),1000,2,0.2,0)` | `#NUM!` | `1440` |
| 46 | AREAS | `=AREAS((A1:A5,C1:C5))` | `#ERROR!` | `2` |
| 66 | ATAN2 | `=ATAN2(0,0)` | `#DIV/0!` | `0` |
| 118 | BETADIST | `=BETADIST(-1,2,3)` | `#NUM!` | `0` |
| 121 | BETAINV | `=BETAINV(0,2,3)` | `#NUM!` | `0` |
| 150 | BITLSHIFT | `=BITLSHIFT(1,54)` | `#NUM!` | `1.8014398509482e+16` |
| 153 | BITOR | `=BITOR(2.5,1)` | `#NUM!` | `3` |
| 184 | CHAR | `=CHAR(0)` | `#VALUE!` | ` ` |
| 187 | CHIDIST | `=CHIDIST(-1,2)` | `#NUM!` | `1` |
| 225 | CODE | `=CODE("")` | `#VALUE!` | `0` |
| 324 | CRITBINOM | `=CRITBINOM(10,0.5,0)` | `#NUM!` | `0` |
| 483 | EXPON.DIST | `=EXPON.DIST(-1,1,TRUE)` | `#NUM!` | `0` |
| 620 | GROWTH | `=INDEX(GROWTH({2,4,8,16,32}, {1,2,3,4,5}, 6), 1, 1)` | `#REF!` | `64` |
| 723 | IMPRODUCT | `=IMPRODUCT("2", "3", "4")` | `#ERROR!` | `24` |
| 759 | INDIRECT | `=INDIRECT("R3C1", FALSE)` | `#N/IMPL!` | `6` |
| 895 | LOGNORM.DIST | `=LOGNORM.DIST(0,0,1,TRUE)` | `#NUM!` | `0` |
| 901 | LOGNORMDIST | `=LOGNORMDIST(0,0,1)` | `#NUM!` | `0` |
| 993 | MROUND | `=MROUND(A1,-3)` | `#NUM!` | `3` |
| 1163 | POWER | `=POWER(0,0)` | `#NUM!` | `1` |
| 1174 | PRICEMAT | `=PRICEMAT(DATE(2026,1,1),DATE(2027,1,1),DATE(2026,1,1),0.04,0.05,1)` | `#NUM!` | `99.0476190476191` |
| 1314 | SHEET | `=SHEET(A1)` | `#N/A` | `1` |
| 1316 | SHEETS | `=SHEETS(A1:A10)` | `#N/IMPL!` | `1` |
| 1427 | SYD | `=SYD(1000,100,5,6)` | `#NUM!` | `0` |
| 1543 | UNICHAR | `=UNICHAR(0)` | `#VALUE!` | ` ` |
| 1550 | UNIQUE | `=ROWS(UNIQUE({1;2;2;3}))` | `#VALUE!` | `3` |
| 1633 | XNPV | `=XNPV(0.05,{-1000,500},{46204,46023})` | `#NUM!` | `-487.755180940009` |

### C. Both return values, values differ (38 cases)

The most interesting bucket: genuine computational differences. Four rows
(204, 207, 873, 1515) are zero-vs-tiny pairs that the pre-2026-07-03 policy
absorbed with the absolute tolerance; the policy now surfaces them. Rows
204/207 look like a real ironcalc finding — `CHISQ.TEST`/`CHITEST` underflow
a 2.55e-25 p-value to exactly 0.0, a 100% relative error on a legitimately
tiny result. Rows 873/1515 are the mirror image and likely benign:
ironcalc carries a 7.1e-15 float residue where the exact-fit intercept is 0
and LibreOffice prints 0. Rows 521/522 are the new FILTERXML type finding
(ironcalc number vs LO text; Excel returns numbers).

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 6 | ACCRINT | `=ACCRINT(DATE(2025,1,1),DATE(2025,7,1),DATE(2026,1,15),0.05,1000,4,1)` | `51.92307692307692` | `51.9178082191781` |
| 98 | BESSELI | `=BESSELI(A1,0)` | `2.279585307296026` | `2.27958530233607` |
| 204 | CHISQ.TEST | `=CHISQ.TEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` |
| 207 | CHITEST | `=CHITEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` |
| 248 | CONCATENATE | `=CONCATENATE(E1,"x")` | `TRUEx` | `1x` |
| 258 | CONVERT | `=CONVERT(1,"lbm","kg")` | `0.45359237` | `0.453592309748811` |
| 259 | CONVERT | `=CONVERT(68,"F","C")` | `19.650000000000034` | `20` |
| 280 | COUNT | `=COUNT(E1:E2)` | `0` | `2` |
| 410 | DOLLAR | `=DOLLAR(-1234.567,-2)` | `($1,200)` | `-$1,200` |
| 434 | DURATION | `=DURATION(DATE(2026,1,1),DATE(2030,1,1),0.08,0.09,2,1)` | `3.4910833018229606` | `3.49163094694892` |
| 518 | FILTER | `=COUNT(FILTER(C1:C5,D1:D5>25))` | `0` | `3` |
| 521 | FILTERXML | `=FILTERXML("<a><b>5</b><b>7</b></a>","//b[1]")` | `5` | `5` (text) |
| 522 | FILTERXML | `=FILTERXML("<a><b>5</b><b>7</b></a>","//b[last()]")` | `7` | `7` (text) |
| 811 | ISNUMBER | `=ISNUMBER(E1)` | `FALSE` | `TRUE` |
| 873 | LINEST | `=INDEX(LINEST(D1:D5,C1:C5),1,2)` | `7.105427357601002e-15` | `0` |
| 925 | MAXA | `=MAXA(A6,E1)` | `-3` | `1` |
| 935 | MDURATION | `=MDURATION(DATE(2026,1,1),DATE(2031,1,1),0.05,0.06,1,1)` | `4.277974103565637` | `4.27840468148732` |
| 954 | MINA | `=MINA(C1:C5,E2)` | `1` | `0` |
| 981 | MODE.MULT | `=COUNT(MODE.MULT({1,1,2,2,3}))` | `0` | `2` |
| 1092 | ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2025,10,15),0.05,0.06,100,2,1)` | `99.60842257486254` | `99.6086078396235` |
| 1125 | PERCENTRANK | `=PERCENTRANK(A1:A10,6)` | `0.555` | `0.556` |
| 1127 | PERCENTRANK | `=PERCENTRANK(A1:A10,7,5)` | `0.62962` | `0.62963` |
| 1131 | PERCENTRANK.EXC | `=PERCENTRANK.EXC(A1:A10,-3)` | `0.09` | `0.0909` |
| 1132 | PERCENTRANK.INC | `=PERCENTRANK.INC(A1:A10,10)` | `0.888` | `0.889` |
| 1169 | PRICE | `=PRICE(DATE(2026,3,15),DATE(2031,3,15),0.05,0.05,100,4,3)` | `100.00000000000007` | `99.9897902308227` |
| 1222 | RATE | `=RATE(10,-100,1000)` | `-3.351249888821255e-11` | `6.1185791462466e-11` |
| 1275 | ROW | `=SUM(ROW(C1:C5))` | `1` | `15` |
| 1305 | SECOND | `=SECOND(0.999999)` | `59` | `0` |
| 1374 | STDEVA | `=STDEVA(A1:A2,B9)` | `1.4142135623730951` | `2` |
| 1406 | SUMPRODUCT | `=SUMPRODUCT(--(A1:A10>5))` | `0` | `5` |
| 1463 | TBILLEQ | `=TBILLEQ(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `0.04669019304858653` | `0.0466811612738202` |
| 1464 | TBILLEQ | `=TBILLEQ(DATE(2026,1,1),DATE(2026,12,1),0.05)` | `0.052534569916227875` | `0.0531372834473723` |
| 1466 | TBILLPRICE | `=TBILLPRICE(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `97.7` | `97.7375` |
| 1469 | TBILLYIELD | `=TBILLYIELD(DATE(2026,3,15),DATE(2026,9,15),98.5)` | `0.02979474729640256` | `0.0302885828869505` |
| 1493 | TEXTSPLIT | `=COUNTA(TEXTSPLIT("a,b,c",","))` | `3` | `1` |
| 1515 | TREND | `=TREND(D1:D5,C1:C5,0)` | `7.105427357601002e-15` | `0` |
| 1517 | TRIM | `=TRIM("  a   b  ")` | `a   b` | `a b` |
| 1538 | TYPE | `=TYPE(E1)` | `4` | `8` |

### D. lo_unsupported rows where ironcalc also errored (15 cases, no oracle)

LO's `#NAME?` means it never evaluated the arguments — these rows
corroborate nothing, and ironcalc's own error on each is unchecked. The two
`VALUETOTEXT` rows answering the internal `#ERROR!` to a documented
argument form are a probable ironcalc defect surfaced by this table.

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 138 | BINOM.DIST.RANGE | `=BINOM.DIST.RANGE(10,0.5,7,4)` | `#NUM!` | `#NAME?` |
| 215 | CHOOSECOLS | `=SUM(CHOOSECOLS(C1:D5,3))` | `#VALUE!` | `#NAME?` |
| 218 | CHOOSEROWS | `=SUM(CHOOSEROWS(C1:D5,6))` | `#VALUE!` | `#NAME?` |
| 479 | EXPAND | `=INDEX(EXPAND(C1:C5,6),6,1)` | `#N/A` | `#NAME?` |
| 648 | HSTACK | `=COLUMNS(HSTACK(A1:A10, A1:A10, A1:A10))` | `#VALUE!` | `#NAME?` |
| 649 | HSTACK | `=INDEX(HSTACK(C1:C3, D1:D5), 4, 1)` | `#N/A` | `#NAME?` |
| 1232 | REGEXEXTRACT | `=REGEXEXTRACT(B1,"\d+")` | `#N/A` | `#NAME?` |
| 1482 | TEXTAFTER | `=TEXTAFTER(B1,"z")` | `#N/A` | `#NAME?` |
| 1486 | TEXTBEFORE | `=TEXTBEFORE(B1,"z")` | `#N/A` | `#NAME?` |
| 1524 | TRIMRANGE | `=ROWS(TRIMRANGE(C1:D5))` | `#VALUE!` | `#NAME?` |
| 1525 | TRIMRANGE | `=COLUMNS(TRIMRANGE(C1:D5))` | `#VALUE!` | `#NAME?` |
| 1559 | VALUETOTEXT | `=VALUETOTEXT(B1,0)` | `#ERROR!` | `#NAME?` |
| 1560 | VALUETOTEXT | `=VALUETOTEXT(B1,1)` | `#ERROR!` | `#NAME?` |
| 1589 | VSTACK | `=ROWS(VSTACK(C1:C5,A1:A10))` | `#VALUE!` | `#NAME?` |
| 1616 | WRAPCOLS | `=INDEX(WRAPCOLS(A1:A10,4),3,3)` | `#N/A` | `#NAME?` |

(The `ROWS`/`COLUMNS`-over-array rows — TRIMRANGE 1524/1525, VSTACK 1589,
HSTACK 648 — share one root cause with bucket B's `=ROWS(UNIQUE({1;2;2;3}))`:
ironcalc's `ROWS`/`COLUMNS` reject array-valued arguments; the array
functions themselves spill correctly.)

### lo_unsupported per-function counts (105 cases, 33 functions)

`ARRAYTOTEXT` 3, `BAHTTEXT` 4, `BINOM.DIST.RANGE` 3, `BYCOL` 3, `BYROW` 3,
`CHOOSECOLS` 4, `CHOOSEROWS` 3, `DBCS` 3, `DROP` 4, `EXPAND` 4, `GROUPBY` 3,
`HSTACK` 4, `MAKEARRAY` 3, `MAP` 3, `PERCENTOF` 3, `PHONETIC` 2, `PIVOTBY` 2,
`REDUCE` 3, `REGEXEXTRACT` 3, `REGEXREPLACE` 3, `REGEXTEST` 3, `SCAN` 3,
`TAKE` 3, `TEXTAFTER` 4, `TEXTBEFORE` 4, `TEXTSPLIT` 2, `TOCOL` 3, `TOROW` 3,
`TRIMRANGE` 3, `VALUETOTEXT` 4, `VSTACK` 3, `WRAPCOLS` 4, `WRAPROWS` 3

## Reproduce

```sh
ORACLE_PYTHON=<python-with-openpyxl> benchmarks/run_oracle.sh
```

The script regenerates the workbook, reconverts with LibreOffice (locale
pinned to C.UTF-8 with a throwaway profile, so host locale does not leak
into the results), rebuilds `oracle-compare` (cargo), and rewrites
`benchmarks/agreement.json`.
