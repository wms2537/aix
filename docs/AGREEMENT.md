# ironcalc vs LibreOffice: differential agreement report

Generated 2026-07-03 by `benchmarks/run_oracle.sh` from
`benchmarks/oracle-cases.json` (484 functions, 1,634 cases).
Raw per-case results: `benchmarks/agreement.json`.

Coverage caveat up front: "484 functions" is the number of functions
*exercised*, not the number the oracle actually *validated*. For 31 of them
(101 cases) LibreOffice could not produce a comparable value for a single
case — `#NAME?` (function unknown) or `#VALUE!` (inline-LAMBDA invocation it
cannot call) on every row — so this report says nothing about whether
ironcalc's values for those functions are right. The full list is in the
"functions with no oracle signal" note below.

Engines under comparison:

- **ironcalc 0.7.1+e50ccea8 (vendored master)** (`vendor/upstream`, the
  engine xlq links — the master pin plus the local residual-functions patch,
  not the 0.7.1 crates.io release; see `docs/COVERAGE.md`)
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
   file are the LibreOffice side of the oracle (verified: all 1,634 cells
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
| numbers | relative tolerance 1e-9; absolute 1e-12 near zero — **but an exact zero on one side only matches an exact zero on the other** (a zero-vs-tiny pair is underflow or rounding residue: it goes to the triage table, not into "agree") |
| text, booleans | exact |
| errors | if **both** sides error → `both_error` (counted separately, not a disagreement; error-*code* equality is reported on the side, and rows where LibreOffice's code is `#NAME?` are flagged `lo_name_error` — LO does not know the function, so there is no oracle) |
| empty | LibreOffice empty string == ironcalc empty cell |
| `engine_error` | ironcalc could not accept/evaluate the case at all |

Numeric agreements that are **not bit-identical** are flagged
`within_tolerance` per case and counted in the totals
(`agree_exact` / `agree_within_tolerance`): of the 1,266 agreements, 929 are
exact and **337 hold only under the tolerance policy**. Most of those 337
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
(`_xlfn._xlws.` for `FILTER`/`SORT`); the generator adds these prefixes,
otherwise LibreOffice imports the names as unknown macro calls. Empirically
verified quirk of LibreOffice 24.8's importer: `IMCOSH IMCOT IMCSC IMCSCH
IMSEC IMSECH IMSINH IMTAN NETWORKDAYS.INTL WORKDAY.INTL ISO.CEILING` are
recognized only *unprefixed*, so those are stored unprefixed. ironcalc
always receives the canonical Excel spelling verbatim.

## Headline results

- **1634 cases**: 1266 agree (929 exact, 337 within tolerance), 159
  disagree, 209 both-error, 0 engine errors.
- **Agreement where both engines produced a non-error value: 97.2%**
  (1266 of 1302 — the 36 both-value disagreements are bucket C below).
- Counting every disagreement — including the 123 cases where one side
  errored and the other returned a value, which are dominated by
  LibreOffice's missing functions — agreement is 88.8% (1266/1425).
- Including both-error as agreement-that-it-errors: 90.3% of all cases.
  Caution: 15 of the 209 both-error rows have `#NAME?` on the LibreOffice
  side — LO does not know the function, so "both error" there is
  coincidence, not corroboration (they are tabled separately below; two of
  them are ironcalc answering its internal `#ERROR!` to `VALUETOTEXT` with a
  format argument, a probable ironcalc defect this bucket would otherwise
  hide).
- Of the 209 both-error cases, the error *codes* match in 56 and differ
  in 153. The mismatch is **not** mostly a missing-function artifact: only
  15 of the 153 involve `#NAME?`. The dominant pattern — 118 of 153, across
  118 distinct functions that **both engines implement** — is ironcalc
  `#NUM!` vs LibreOffice `#VALUE!` on argument-domain errors (e.g.
  `ACOSH(0.5)`, `ATANH(1)`, `COMBIN(4,5)`). Verified against the converted
  XML: LibreOffice computes its internal `Err:502` (illegal argument) for
  these and its OOXML export writes that as `#VALUE!`, so the code
  comparison here measures LO's error-export mapping at least as much as
  engine semantics — treat code-level equality as a weak signal.

| Category | Cases | Agree | Disagree (one side errors) | Disagree (values differ) | Both error | Both-value agreement |
|---|---|---|---|---|---|---|
| Database | 28 | 27 | 0 | 0 | 1 | 100.0% |
| Date & time | 84 | 77 | 0 | 1 | 6 | 98.7% |
| Engineering | 177 | 147 | 7 | 3 | 20 | 98.0% |
| Financial | 178 | 128 | 10 | 10 | 30 | 92.8% |
| Information | 66 | 60 | 2 | 2 | 2 | 96.8% |
| Lambda & functional | 27 | 3 | 24 | 0 | 0 | 100.0% |
| Logical | 41 | 36 | 1 | 0 | 4 | 100.0% |
| Lookup & reference | 116 | 59 | 35 | 2 | 20 | 96.7% |
| Math & trig | 279 | 243 | 5 | 1 | 30 | 99.6% |
| Statistical | 481 | 370 | 14 | 13 | 84 | 96.6% |
| Text | 157 | 116 | 25 | 4 | 12 | 96.7% |
| **Total** | 1634 | 1266 | 123 | 36 | 209 | **97.2%** |

"Both-value agreement" is agree / (agree + values-differ): what fraction of
the cases where **both** engines returned a non-error value the values
match. The "one side errors" column is dominated by LibreOffice's missing
functions (`#NAME?` rows), not by computational disagreement — that is why
Lambda & functional shows 24 one-side-error disagreements yet 100%
both-value agreement: on the 3 cases LO *could* compute, the values match,
and on the other 24 there is simply no LO answer to compare.

### Functions with no oracle signal (31 functions, 101 cases)

For these functions LibreOffice produced **no comparable non-error value on
any case** (`#NAME?`, or `#VALUE!` for the uncallable inline-LAMBDA rows),
so this report validates nothing about ironcalc's values for them —
ironcalc could be wrong on all 101 cases without this harness noticing:

`ARRAYTOTEXT`, `BINOM.DIST.RANGE`, `BYCOL`, `BYROW`, `CHOOSECOLS`,
`CHOOSEROWS`, `DROP`, `EXPAND`, `HSTACK`, `ISOMITTED`, `LAMBDA`,
`MAKEARRAY`, `MAP`, `ODDFPRICE`, `ODDFYIELD`, `PERCENTOF`, `REDUCE`,
`REGEXEXTRACT`, `REGEXREPLACE`, `REGEXTEST`, `SCAN`, `TAKE`, `TEXTAFTER`,
`TEXTBEFORE`, `TOCOL`, `TOROW`, `TRIMRANGE`, `VALUETOTEXT`, `VSTACK`,
`WRAPCOLS`, `WRAPROWS`

They still count toward the headline "484 functions" — read that number as
breadth of exercise, not breadth of validation. Closing this gap needs a
second oracle (live Excel, or targeted hand-checked expectations).

## Known LibreOffice 24.8 deviations / gaps observed in this run

- **Functions absent from LibreOffice 24.8's OOXML import** (LO returns
  `#NAME?`; on most cases ironcalc computes a value — these are LO coverage
  gaps and say nothing about ironcalc correctness; but note that on 15
  cases of these same functions *ironcalc errors too*, see the no-oracle
  both-error table below): `ARRAYTOTEXT`, `BINOM.DIST.RANGE`, `BYCOL`, `BYROW`, `CHOOSECOLS`, `CHOOSEROWS`, `DROP`, `EXPAND`, `HSTACK`, `MAKEARRAY`, `MAP`, `PERCENTOF`, `REDUCE`, `REGEXEXTRACT`, `REGEXREPLACE`, `REGEXTEST`, `SCAN`, `TAKE`, `TEXTAFTER`, `TEXTBEFORE`, `TEXTSPLIT`, `TOCOL`, `TOROW`, `TRIMRANGE`, `VALUETOTEXT`, `VSTACK`, `WRAPCOLS`, `WRAPROWS`.
- **Inline LAMBDA invocation** `=LAMBDA(...)(args)` is parsed but not
  callable in LO 24.8 (`#VALUE!`); this also breaks the `ISOMITTED` cases.
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

## Triage tables (all 159 disagreements + the 15 no-oracle both-error rows)

A row in A/B/C means the two engines decided differently. Excel is the
arbiter for triage; neither column is presumed correct. Table D lists the
both-error rows that carry no oracle signal.

### A. LibreOffice errors, ironcalc returns a value (97 cases)

LO answers `#NAME?` in 76 of these (missing functions, see notes above) and
`#VALUE!`/`#DIV/0!` in the other 21 — the latter are informative
disagreements on functions LO *does* recognize (inline-LAMBDA invocation,
`IMCOT`/`IMCSC`/`IMCSCH`/`IMLN` at 0, `NUMBERVALUE("3.5%")`,
`ODDFPRICE`/`ODDFYIELD`/`ODDLPRICE`, `OR(range>4)`, `PRODUCT`/`SUM` string
coercion, `SKEW.P`).

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 47 | ARRAYTOTEXT | `=ARRAYTOTEXT(A1:A3)` | `2, 4, 6` | `#NAME?` |
| 48 | ARRAYTOTEXT | `=ARRAYTOTEXT(B1:B2,1)` | `{"alpha";"Beta"}` | `#NAME?` |
| 49 | ARRAYTOTEXT | `=ARRAYTOTEXT(C1:D2,0)` | `1, 10, 2, 20` | `#NAME?` |
| 132 | BINOM.DIST.RANGE | `=BINOM.DIST.RANGE(60,0.75,48)` | `0.08397496742905441` | `#NAME?` |
| 133 | BINOM.DIST.RANGE | `=BINOM.DIST.RANGE(60,0.75,45,50)` | `0.5236297934718851` | `#NAME?` |
| 156 | BYCOL | `=SUM(BYCOL(C1:D5,LAMBDA(c,SUM(c))))` | `165` | `#NAME?` |
| 157 | BYCOL | `=INDEX(BYCOL(C1:D5,LAMBDA(c,MAX(c))),1,2)` | `50` | `#NAME?` |
| 158 | BYCOL | `=SUM(BYCOL(C1:D5,LAMBDA(c,AVERAGE(c))))` | `33` | `#NAME?` |
| 159 | BYROW | `=SUM(BYROW(C1:D5,LAMBDA(r,SUM(r))))` | `165` | `#NAME?` |
| 160 | BYROW | `=INDEX(BYROW(C1:D5,LAMBDA(r,MAX(r))),2,1)` | `20` | `#NAME?` |
| 161 | BYROW | `=SUM(BYROW(C1:D5,LAMBDA(r,MIN(r))))` | `15` | `#NAME?` |
| 208 | CHOOSECOLS | `=SUM(CHOOSECOLS(C1:D5,2))` | `150` | `#NAME?` |
| 209 | CHOOSECOLS | `=INDEX(CHOOSECOLS(C1:D5,2,1),1,1)` | `10` | `#NAME?` |
| 210 | CHOOSECOLS | `=SUM(CHOOSECOLS(C1:D5,-1))` | `150` | `#NAME?` |
| 212 | CHOOSEROWS | `=SUM(CHOOSEROWS(C1:D5,1,5))` | `66` | `#NAME?` |
| 213 | CHOOSEROWS | `=INDEX(CHOOSEROWS(C1:D5,-1),1,2)` | `50` | `#NAME?` |
| 414 | DROP | `=SUM(DROP(C1:C5,2))` | `12` | `#NAME?` |
| 415 | DROP | `=SUM(DROP(A1:A10,-3))` | `27` | `#NAME?` |
| 416 | DROP | `=SUM(DROP(C1:D5,1,1))` | `140` | `#NAME?` |
| 417 | DROP | `=INDEX(DROP(C1:C5,2),1,1)` | `3` | `#NAME?` |
| 465 | EXPAND | `=INDEX(EXPAND(C1:C5,7,2,0),6,2)` | `0` | `#NAME?` |
| 466 | EXPAND | `=INDEX(EXPAND(C1:C5,7,2,0),3,1)` | `3` | `#NAME?` |
| 467 | EXPAND | `=SUM(EXPAND(C1:C5,6,1,100))` | `115` | `#NAME?` |
| 628 | HSTACK | `=INDEX(HSTACK(C1:C5, D1:D5), 2, 2)` | `20` | `#NAME?` |
| 629 | HSTACK | `=SUM(HSTACK(C1:C5, D1:D5))` | `165` | `#NAME?` |
| 678 | IMCOT | `=IMCOT("0")` | `inf` | `#VALUE!` |
| 681 | IMCSC | `=IMCSC("0")` | `NaNNaNi` | `#VALUE!` |
| 684 | IMCSCH | `=IMCSCH("0")` | `NaNNaNi` | `#VALUE!` |
| 693 | IMLN | `=IMLN("0")` | `-inf` | `#VALUE!` |
| 802 | ISOMITTED | `=LAMBDA([x],ISOMITTED(x))()` | `TRUE` | `#VALUE!` |
| 803 | ISOMITTED | `=LAMBDA([x],ISOMITTED(x))(5)` | `FALSE` | `#VALUE!` |
| 804 | ISOMITTED | `=LAMBDA([x],IF(ISOMITTED(x),-1,x))(A1)` | `2` | `#VALUE!` |
| 823 | LAMBDA | `=LAMBDA(x,y,x+y)(A1,A2)` | `6` | `#VALUE!` |
| 824 | LAMBDA | `=LAMBDA(x,x*x)(A5)` | `100` | `#VALUE!` |
| 825 | LAMBDA | `=LAMBDA(s,UPPER(s))(B1)` | `ALPHA` | `#VALUE!` |
| 888 | MAKEARRAY | `=SUM(MAKEARRAY(3,3,LAMBDA(r,c,r*c)))` | `36` | `#NAME?` |
| 889 | MAKEARRAY | `=INDEX(MAKEARRAY(2,3,LAMBDA(r,c,r*10+c)),2,3)` | `23` | `#NAME?` |
| 890 | MAKEARRAY | `=SUM(MAKEARRAY(1,1,LAMBDA(r,c,r+c)))` | `2` | `#NAME?` |
| 891 | MAP | `=SUM(MAP(C1:C5,LAMBDA(x,x^2)))` | `55` | `#NAME?` |
| 892 | MAP | `=SUM(MAP(C1:C5,D1:D5,LAMBDA(x,y,x*y)))` | `550` | `#NAME?` |
| 893 | MAP | `=INDEX(MAP(A1:A3,LAMBDA(v,v*10)),3,1)` | `60` | `#NAME?` |
| 1046 | NUMBERVALUE | `=NUMBERVALUE("3.5%")` | `0.035` | `#VALUE!` |
| 1064 | ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.63871848080437` | `#VALUE!` |
| 1065 | ODDFPRICE | `=ODDFPRICE(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.05,0.06,100,1)` | `95.91405301998944` | `#VALUE!` |
| 1066 | ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2026,3,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.2644236004297` | `#VALUE!` |
| 1067 | ODDFYIELD | `=ODDFYIELD(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,95,100,2)` | `0.06151963143529892` | `#VALUE!` |
| 1068 | ODDFYIELD | `=ODDFYIELD(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.04,101.5,100,1)` | `0.03650600623418319` | `#VALUE!` |
| 1072 | ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2026,3,1),0.05,0.06,100,2)` | `99.71603320227172` | `#VALUE!` |
| 1082 | OR | `=OR(C1:C5>4)` | `TRUE` | `#VALUE!` |
| 1101 | PERCENTOF | `=PERCENTOF(A1:A5,A1:A10)` | `0.22140221402214022` | `#NAME?` |
| 1102 | PERCENTOF | `=PERCENTOF(C1:C2,C1:C5)` | `0.2` | `#NAME?` |
| 1103 | PERCENTOF | `=PERCENTOF(A6:A7,A1:A10)` | `-0.02214022140221402` | `#NAME?` |
| 1156 | PRODUCT | `=PRODUCT(C1:C3,"2")` | `12` | `#VALUE!` |
| 1202 | REDUCE | `=REDUCE(0,C1:C5,LAMBDA(a,b,a+b))` | `15` | `#NAME?` |
| 1203 | REDUCE | `=REDUCE(1,C1:C5,LAMBDA(a,b,a*b))` | `120` | `#NAME?` |
| 1204 | REDUCE | `=REDUCE(0,A1:A5,LAMBDA(acc,v,MAX(acc,v)))` | `10` | `#NAME?` |
| 1205 | REGEXEXTRACT | `=REGEXEXTRACT(B3,"[A-Z]+")` | `DELTA` | `#NAME?` |
| 1206 | REGEXEXTRACT | `=REGEXEXTRACT(B4,"\d{4}")` | `2026` | `#NAME?` |
| 1208 | REGEXREPLACE | `=REGEXREPLACE(B5,"[,;]","-")` | `x-y-z` | `#NAME?` |
| 1209 | REGEXREPLACE | `=REGEXREPLACE(B3,"[a-z]+","X")` | `X DELTA` | `#NAME?` |
| 1210 | REGEXREPLACE | `=REGEXREPLACE(B4,"(\d+)-(\d+)-(\d+)","$3/$2/$1")` | `15/03/2026` | `#NAME?` |
| 1211 | REGEXTEST | `=REGEXTEST(B1,"^al")` | `TRUE` | `#NAME?` |
| 1212 | REGEXTEST | `=REGEXTEST(B2,"^b")` | `FALSE` | `#NAME?` |
| 1213 | REGEXTEST | `=REGEXTEST(B9,"^\d+$")` | `TRUE` | `#NAME?` |
| 1262 | SCAN | `=SUM(SCAN(0,C1:C5,LAMBDA(a,b,a+b)))` | `35` | `#NAME?` |
| 1263 | SCAN | `=INDEX(SCAN(0,C1:C5,LAMBDA(a,b,a+b)),5,1)` | `15` | `#NAME?` |
| 1264 | SCAN | `=SUM(SCAN(1,C1:C3,LAMBDA(a,b,a*b)))` | `9` | `#NAME?` |
| 1308 | SKEW.P | `=SKEW.P(A1:A2)` | `0` | `#DIV/0!` |
| 1370 | SUM | `=SUM("3",2)` | `5` | `#VALUE!` |
| 1427 | TAKE | `=SUM(TAKE(A1:A10,3))` | `12` | `#NAME?` |
| 1428 | TAKE | `=SUM(TAKE(A1:A10,-2))` | `101` | `#NAME?` |
| 1429 | TAKE | `=INDEX(TAKE(C1:D5,2,2),2,2)` | `20` | `#NAME?` |
| 1454 | TEXTAFTER | `=TEXTAFTER(B3,"gamma ")` | `DELTA` | `#NAME?` |
| 1455 | TEXTAFTER | `=TEXTAFTER("a-b-c","-")` | `b-c` | `#NAME?` |
| 1456 | TEXTAFTER | `=TEXTAFTER("a-b-c","-",-1)` | `c` | `#NAME?` |
| 1458 | TEXTBEFORE | `=TEXTBEFORE(B3," ")` | `gamma` | `#NAME?` |
| 1459 | TEXTBEFORE | `=TEXTBEFORE("a-b-c","-")` | `a` | `#NAME?` |
| 1460 | TEXTBEFORE | `=TEXTBEFORE("a-b-c","-",-1)` | `a-b` | `#NAME?` |
| 1466 | TEXTSPLIT | `=INDEX(TEXTSPLIT(B5,","),1,2)` | `y;z` | `#NAME?` |
| 1467 | TEXTSPLIT | `=INDEX(TEXTSPLIT(B5,",",";"),2,1)` | `z` | `#NAME?` |
| 1479 | TOCOL | `=SUM(TOCOL(C1:D5))` | `165` | `#NAME?` |
| 1480 | TOCOL | `=INDEX(TOCOL(C1:D5),3,1)` | `2` | `#NAME?` |
| 1481 | TOCOL | `=INDEX(TOCOL(C1:D5,0,TRUE),6,1)` | `10` | `#NAME?` |
| 1482 | TOROW | `=SUM(TOROW(A1:A10))` | `135.5` | `#NAME?` |
| 1483 | TOROW | `=INDEX(TOROW(C1:D5),1,4)` | `20` | `#NAME?` |
| 1484 | TOROW | `=INDEX(TOROW(C1:D5,0,TRUE),1,6)` | `10` | `#NAME?` |
| 1498 | TRIMRANGE | `=SUM(TRIMRANGE(A1:A10))` | `135.5` | `#NAME?` |
| 1533 | VALUETOTEXT | `=VALUETOTEXT(A1)` | `2` | `#NAME?` |
| 1536 | VALUETOTEXT | `=VALUETOTEXT(E1)` | `TRUE` | `#NAME?` |
| 1563 | VSTACK | `=SUM(VSTACK(C1:C5,D1:D5))` | `165` | `#NAME?` |
| 1565 | VSTACK | `=INDEX(VSTACK(C1:C5,D1:D5),7,1)` | `20` | `#NAME?` |
| 1588 | WRAPCOLS | `=INDEX(WRAPCOLS(A1:A10,4),2,2)` | `-3` | `#NAME?` |
| 1589 | WRAPCOLS | `=SUM(WRAPCOLS(C1:C5,5))` | `15` | `#NAME?` |
| 1590 | WRAPCOLS | `=INDEX(WRAPCOLS(C1:C5,3,0),3,2)` | `0` | `#NAME?` |
| 1592 | WRAPROWS | `=INDEX(WRAPROWS(A1:A10,5),2,3)` | `7.5` | `#NAME?` |
| 1593 | WRAPROWS | `=SUM(WRAPROWS(C1:C5,5))` | `15` | `#NAME?` |
| 1594 | WRAPROWS | `=INDEX(WRAPROWS(C1:C5,2,-1),3,2)` | `-1` | `#NAME?` |

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
| 114 | BETADIST | `=BETADIST(-1,2,3)` | `#NUM!` | `0` |
| 117 | BETAINV | `=BETAINV(0,2,3)` | `#NUM!` | `0` |
| 146 | BITLSHIFT | `=BITLSHIFT(1,54)` | `#NUM!` | `1.8014398509482e+16` |
| 149 | BITOR | `=BITOR(2.5,1)` | `#NUM!` | `3` |
| 180 | CHAR | `=CHAR(0)` | `#VALUE!` | ` ` |
| 183 | CHIDIST | `=CHIDIST(-1,2)` | `#NUM!` | `1` |
| 221 | CODE | `=CODE("")` | `#VALUE!` | `0` |
| 320 | CRITBINOM | `=CRITBINOM(10,0.5,0)` | `#NUM!` | `0` |
| 472 | EXPON.DIST | `=EXPON.DIST(-1,1,TRUE)` | `#NUM!` | `0` |
| 602 | GROWTH | `=INDEX(GROWTH({2,4,8,16,32}, {1,2,3,4,5}, 6), 1, 1)` | `#REF!` | `64` |
| 705 | IMPRODUCT | `=IMPRODUCT("2", "3", "4")` | `#ERROR!` | `24` |
| 741 | INDIRECT | `=INDIRECT("R3C1", FALSE)` | `#N/IMPL!` | `6` |
| 874 | LOGNORM.DIST | `=LOGNORM.DIST(0,0,1,TRUE)` | `#NUM!` | `0` |
| 880 | LOGNORMDIST | `=LOGNORMDIST(0,0,1)` | `#NUM!` | `0` |
| 972 | MROUND | `=MROUND(A1,-3)` | `#NUM!` | `3` |
| 1138 | POWER | `=POWER(0,0)` | `#NUM!` | `1` |
| 1149 | PRICEMAT | `=PRICEMAT(DATE(2026,1,1),DATE(2027,1,1),DATE(2026,1,1),0.04,0.05,1)` | `#NUM!` | `99.0476190476191` |
| 1289 | SHEET | `=SHEET(A1)` | `#N/A` | `1` |
| 1291 | SHEETS | `=SHEETS(A1:A10)` | `#N/IMPL!` | `1` |
| 1402 | SYD | `=SYD(1000,100,5,6)` | `#NUM!` | `0` |
| 1518 | UNICHAR | `=UNICHAR(0)` | `#VALUE!` | ` ` |
| 1525 | UNIQUE | `=ROWS(UNIQUE({1;2;2;3}))` | `#VALUE!` | `3` |
| 1608 | XNPV | `=XNPV(0.05,{-1000,500},{46204,46023})` | `#NUM!` | `-487.755180940009` |

### C. Both return values, values differ (36 cases)

The most interesting bucket: genuine computational differences. Four rows
(200, 203, 852, 1490) are zero-vs-tiny pairs that the pre-2026-07-03 policy
absorbed with the absolute tolerance; the policy now surfaces them. Rows
200/203 look like a real ironcalc finding — `CHISQ.TEST`/`CHITEST` underflow
a 2.55e-25 p-value to exactly 0.0, a 100% relative error on a legitimately
tiny result. Rows 852/1490 are the mirror image and likely benign:
ironcalc carries a 7.1e-15 float residue where the exact-fit intercept is 0
and LibreOffice prints 0.

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 6 | ACCRINT | `=ACCRINT(DATE(2025,1,1),DATE(2025,7,1),DATE(2026,1,15),0.05,1000,4,1)` | `51.92307692307692` | `51.9178082191781` |
| 94 | BESSELI | `=BESSELI(A1,0)` | `2.279585307296026` | `2.27958530233607` |
| 200 | CHISQ.TEST | `=CHISQ.TEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` |
| 203 | CHITEST | `=CHITEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` |
| 244 | CONCATENATE | `=CONCATENATE(E1,"x")` | `TRUEx` | `1x` |
| 254 | CONVERT | `=CONVERT(1,"lbm","kg")` | `0.45359237` | `0.453592309748811` |
| 255 | CONVERT | `=CONVERT(68,"F","C")` | `19.650000000000034` | `20` |
| 276 | COUNT | `=COUNT(E1:E2)` | `0` | `2` |
| 403 | DOLLAR | `=DOLLAR(-1234.567,-2)` | `($1,200)` | `-$1,200` |
| 427 | DURATION | `=DURATION(DATE(2026,1,1),DATE(2030,1,1),0.08,0.09,2,1)` | `3.4910833018229606` | `3.49163094694892` |
| 507 | FILTER | `=COUNT(FILTER(C1:C5,D1:D5>25))` | `0` | `3` |
| 793 | ISNUMBER | `=ISNUMBER(E1)` | `FALSE` | `TRUE` |
| 852 | LINEST | `=INDEX(LINEST(D1:D5,C1:C5),1,2)` | `7.105427357601002e-15` | `0` |
| 904 | MAXA | `=MAXA(A6,E1)` | `-3` | `1` |
| 914 | MDURATION | `=MDURATION(DATE(2026,1,1),DATE(2031,1,1),0.05,0.06,1,1)` | `4.277974103565637` | `4.27840468148732` |
| 933 | MINA | `=MINA(C1:C5,E2)` | `1` | `0` |
| 960 | MODE.MULT | `=COUNT(MODE.MULT({1,1,2,2,3}))` | `0` | `2` |
| 1071 | ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2025,10,15),0.05,0.06,100,2,1)` | `99.60842257486254` | `99.6086078396235` |
| 1104 | PERCENTRANK | `=PERCENTRANK(A1:A10,6)` | `0.555` | `0.556` |
| 1106 | PERCENTRANK | `=PERCENTRANK(A1:A10,7,5)` | `0.62962` | `0.62963` |
| 1110 | PERCENTRANK.EXC | `=PERCENTRANK.EXC(A1:A10,-3)` | `0.09` | `0.0909` |
| 1111 | PERCENTRANK.INC | `=PERCENTRANK.INC(A1:A10,10)` | `0.888` | `0.889` |
| 1144 | PRICE | `=PRICE(DATE(2026,3,15),DATE(2031,3,15),0.05,0.05,100,4,3)` | `100.00000000000007` | `99.9897902308227` |
| 1197 | RATE | `=RATE(10,-100,1000)` | `-3.351249888821255e-11` | `6.1185791462466e-11` |
| 1250 | ROW | `=SUM(ROW(C1:C5))` | `1` | `15` |
| 1280 | SECOND | `=SECOND(0.999999)` | `59` | `0` |
| 1349 | STDEVA | `=STDEVA(A1:A2,B9)` | `1.4142135623730951` | `2` |
| 1381 | SUMPRODUCT | `=SUMPRODUCT(--(A1:A10>5))` | `0` | `5` |
| 1438 | TBILLEQ | `=TBILLEQ(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `0.04669019304858653` | `0.0466811612738202` |
| 1439 | TBILLEQ | `=TBILLEQ(DATE(2026,1,1),DATE(2026,12,1),0.05)` | `0.052534569916227875` | `0.0531372834473723` |
| 1441 | TBILLPRICE | `=TBILLPRICE(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `97.7` | `97.7375` |
| 1444 | TBILLYIELD | `=TBILLYIELD(DATE(2026,3,15),DATE(2026,9,15),98.5)` | `0.02979474729640256` | `0.0302885828869505` |
| 1468 | TEXTSPLIT | `=COUNTA(TEXTSPLIT("a,b,c",","))` | `3` | `1` |
| 1490 | TREND | `=TREND(D1:D5,C1:C5,0)` | `7.105427357601002e-15` | `0` |
| 1492 | TRIM | `=TRIM("  a   b  ")` | `a   b` | `a b` |
| 1513 | TYPE | `=TYPE(E1)` | `4` | `8` |

### D. Both error, but LibreOffice's error is `#NAME?` (15 cases, no oracle)

These count as `both_error` in the totals, but LO's `#NAME?` means it never
evaluated the arguments — the coincidence of "both errored" corroborates
nothing. ironcalc's own error on each row is unchecked; the two
`VALUETOTEXT` rows answering the internal `#ERROR!` to a documented
argument form are a probable ironcalc defect surfaced by this table.

| # | Function | Formula | ironcalc | LibreOffice |
|---|---|---|---|---|
| 134 | BINOM.DIST.RANGE | `=BINOM.DIST.RANGE(10,0.5,7,4)` | `#NUM!` | `#NAME?` |
| 211 | CHOOSECOLS | `=SUM(CHOOSECOLS(C1:D5,3))` | `#VALUE!` | `#NAME?` |
| 214 | CHOOSEROWS | `=SUM(CHOOSEROWS(C1:D5,6))` | `#VALUE!` | `#NAME?` |
| 468 | EXPAND | `=INDEX(EXPAND(C1:C5,6),6,1)` | `#N/A` | `#NAME?` |
| 630 | HSTACK | `=COLUMNS(HSTACK(A1:A10, A1:A10, A1:A10))` | `#VALUE!` | `#NAME?` |
| 631 | HSTACK | `=INDEX(HSTACK(C1:C3, D1:D5), 4, 1)` | `#N/A` | `#NAME?` |
| 1207 | REGEXEXTRACT | `=REGEXEXTRACT(B1,"\d+")` | `#N/A` | `#NAME?` |
| 1457 | TEXTAFTER | `=TEXTAFTER(B1,"z")` | `#N/A` | `#NAME?` |
| 1461 | TEXTBEFORE | `=TEXTBEFORE(B1,"z")` | `#N/A` | `#NAME?` |
| 1499 | TRIMRANGE | `=ROWS(TRIMRANGE(C1:D5))` | `#VALUE!` | `#NAME?` |
| 1500 | TRIMRANGE | `=COLUMNS(TRIMRANGE(C1:D5))` | `#VALUE!` | `#NAME?` |
| 1534 | VALUETOTEXT | `=VALUETOTEXT(B1,0)` | `#ERROR!` | `#NAME?` |
| 1535 | VALUETOTEXT | `=VALUETOTEXT(B1,1)` | `#ERROR!` | `#NAME?` |
| 1564 | VSTACK | `=ROWS(VSTACK(C1:C5,A1:A10))` | `#VALUE!` | `#NAME?` |
| 1591 | WRAPCOLS | `=INDEX(WRAPCOLS(A1:A10,4),3,3)` | `#N/A` | `#NAME?` |

## Reproduce

```sh
ORACLE_PYTHON=<python-with-openpyxl> benchmarks/run_oracle.sh
```

The script regenerates the workbook, reconverts with LibreOffice (locale
pinned to C.UTF-8 with a throwaway profile, so host locale does not leak
into the results), rebuilds `oracle-compare` (cargo), and rewrites
`benchmarks/agreement.json`.
