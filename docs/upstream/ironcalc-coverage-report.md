# IronCalc master (e50ccea8): coverage re-measurement, a 3-function patch offer, and 83 differential-testing questions

Hi — we build [xlq](https://github.com/wmhy/xlq), an agent-safe CLI for
inspecting/diffing/recalculating .xlsx files, now on a vendored clone of
IronCalc master at `e50ccea8`. An earlier version of this report measured
the 0.7.1 release at 345/522 Excel function names and listed a prioritized
gap list. That report is obsolete, and this one replaces it with three
things: the re-measurement, a small patch we'd like to offer upstream, and a
set of value-level differences we found while differential-testing against
LibreOffice that may (or may not) be worth your time.

## 1. Master closed the gap we measured against 0.7.1

Same probe as before (Microsoft's "Excel functions (alphabetical)" list, 522
names; `=NAME(1)` in a scratch model via `set_user_input` + `evaluate`;
`#NAME?` ⇒ unrecognized; this measures name resolution, not numerical
fidelity):

| Engine | Recognized |
|---|---|
| 0.7.1 release | 345 / 522 (66.1%) |
| master @ e50ccea8 (+ our 3-function patch, §2) | **497 / 522 (95.2%)** |

Everything our earlier report ranked as Tier 1 and Tier 2 now resolves on
master: SUMPRODUCT, the ~25 legacy statistical aliases (STDEV, VAR,
PERCENTILE, QUARTILE, MODE, RANK, NORMDIST, …), XMATCH, the text stragglers
(CHAR, CODE, CLEAN, PROPER, REPLACE, FIXED, DOLLAR, NUMBERVALUE), the full
dynamic-array family (FILTER, UNIQUE, SORT, SORTBY, SEQUENCE, TAKE, DROP,
HSTACK/VSTACK, TOCOL/TOROW, WRAPROWS/WRAPCOLS, EXPAND, CHOOSECOLS/ROWS,
TRIMRANGE), LET/LAMBDA plus all the helpers (BYROW, BYCOL, MAP, REDUCE,
SCAN, MAKEARRAY, ISOMITTED), the CSE-array set (TRANSPOSE, MMULT, MINVERSE,
MDETERM, MUNIT, FREQUENCY, LINEST, TREND), the bond/coupon financial set,
and GETPIVOTDATA-adjacent lookups like ADDRESS and HYPERLINK-free AREAS.
The 25 names still unrecognized are exactly the ones our earlier report
filed under "probably fine to skip indefinitely": external-service functions
(WEBSERVICE, FILTERXML, RTD, STOCKHISTORY, DETECTLANGUAGE, TRANSLATE,
COPILOT, IMAGE, CALL, REGISTER.ID, EUROCONVERT), the 7 CUBE functions,
pivot-model functions (GETPIVOTDATA, GROUPBY, PIVOTBY), and 4 DBCS-locale
functions (BAHTTEXT, DBCS, JIS, PHONETIC). For our use case that is
effectively complete: every locally-evaluable name resolves.

Raw per-function JSON and the input list: `benchmarks/coverage.json` and
`benchmarks/excel-functions.txt` in our repo; the probe binary is
`xlq/src/bin/coverage_probe.rs` and runs in under a second if it's useful
for your own tracking.

## 2. Patch offer: ENCODEURL, HYPERLINK, AGGREGATE

Three names that are locally evaluable were still missing on master, so we
implemented them in our vendored tree, following the existing code
conventions (dispatch, static analysis signatures, localized names for all
five languages, `_xlfn.` xlsx serialization, tests):

- **ENCODEURL** — RFC 3986 percent-encoding, Excel's unreserved set
  (one documented divergence from LibreOffice, which also encodes `.`/`~`).
- **HYPERLINK** — value semantics (returns the friendly name / location
  text); no link object is attached, matching the engine's current model.
- **AGGREGATE** — all 19 scalar function_nums with the full options bitmask,
  including ignore-hidden-rows (reusing SUBTOTAL's row-visibility machinery)
  and ignore-nested-SUBTOTAL/AGGREGATE.

Details, semantics decisions, the 15 new engine tests (2125 pass total),
clippy/fmt status, and a 17-case LibreOffice cross-validation are written up
in [residual-functions-patch.md](residual-functions-patch.md); the diff is
self-contained inside our `vendor/upstream` clone (~1,050 insertions). If
you want it, we'll rebase it onto current master and open a PR — or feel
free to take the writeup and implement it differently; the semantics notes
(especially AGGREGATE's hidden-vs-filtered-row decision) are probably the
useful part either way.

## 3. Differential-testing questions (IronCalc vs LibreOffice 24.8)

We ran 1,634 formula cases across 484 functions through both engines from
one shared workbook (methodology and full data:
`benchmarks/run_oracle.sh`, `benchmarks/agreement.json`, and the analysis in
`docs/AGREEMENT.md` in our repo). Result: 1,266 agree, 209 both-error, 159
disagree. Of the 159, 76 are LibreOffice lacking the function (`#NAME?` on
LO's side — those say nothing about IronCalc and are omitted here). The
remaining 83 are below, **as questions, not bug reports**: LibreOffice is a
reference, not ground truth, and for several of these we believe IronCalc is
the one matching Excel (e.g. `POWER(0,0)`→`#NUM!`, `ATAN2(0,0)`→`#DIV/0!`,
boolean-cell semantics in `COUNT`/`ISNUMBER`/`TYPE`, PERCENTRANK's
truncate-vs-round). None have been checked against a live Excel; each row is
"is this the behavior you intend?"

### 3a. IronCalc errors where LibreOffice returns a value (26)

| Function | Formula | IronCalc | LibreOffice | Question |
|---|---|---|---|---|
| AMORDEGRC | `=AMORDEGRC(10000,DATE(2025,1,1),DATE(2025,12,31),1000,2,0.2,0)` | `#NUM!` | `1440` | Is rate 0.2 with basis 0 intended to be out of domain? |
| AREAS | `=AREAS((A1:A5,C1:C5))` | `#ERROR!` | `2` | Are union references in AREAS planned? Excel gives 2. |
| ATAN2 | `=ATAN2(0,0)` | `#DIV/0!` | `0` | Matches Excel; LO deviates — intended? |
| BETADIST | `=BETADIST(-1,2,3)` | `#NUM!` | `0` | Excel errors on x<0 for legacy BETADIST; likely correct. |
| BETAINV | `=BETAINV(0,2,3)` | `#NUM!` | `0` | Excel requires 0<p; likely correct. |
| BITLSHIFT | `=BITLSHIFT(1,54)` | `#NUM!` | `1.8014E+16` | Excel errors when result ≥ 2^48 — is 54 within your intended domain? |
| BITOR | `=BITOR(2.5,1)` | `#NUM!` | `3` | Excel requires integers; likely correct. |
| CHAR | `=CHAR(0)` | `#VALUE!` | NUL char | Excel errors on 0; likely correct. |
| CHIDIST | `=CHIDIST(-1,2)` | `#NUM!` | `1` | Excel errors on x<0; likely correct. |
| CODE | `=CODE("")` | `#VALUE!` | `0` | Excel errors on empty string; likely correct. |
| CRITBINOM | `=CRITBINOM(10,0.5,0)` | `#NUM!` | `0` | Is alpha=0 intended to error? Excel docs say 0<alpha<1. |
| EXPON.DIST | `=EXPON.DIST(-1,1,TRUE)` | `#NUM!` | `0` | Excel errors on x<0; likely correct. |
| GROWTH | `=INDEX(GROWTH({2,4,8,16,32},{1,2,3,4,5},6),1,1)` | `#REF!` | `64` | Array-literal known_x/new_x — supported? `#REF!` looks like a reference-shape assumption. |
| IMPRODUCT | `=IMPRODUCT("2","3","4")` | `#ERROR!` | `24` | More than 2 scalar args rejected at parse/signature level? |
| INDIRECT | `=INDIRECT("R3C1",FALSE)` | `#N/IMPL!` | `6` | R1C1 mode: is `#N/IMPL!` the intended long-term answer? |
| LOGNORM.DIST | `=LOGNORM.DIST(0,0,1,TRUE)` | `#NUM!` | `0` | Excel errors on x≤0; likely correct. |
| LOGNORMDIST | `=LOGNORMDIST(0,0,1)` | `#NUM!` | `0` | Same as above. |
| MROUND | `=MROUND(A1,-3)` (A1=2) | `#NUM!` | `3` | Excel errors on mixed signs; likely correct. |
| POWER | `=POWER(0,0)` | `#NUM!` | `1` | Matches Excel; LO deviates — intended? |
| PRICEMAT | `=PRICEMAT(DATE(2026,1,1),DATE(2027,1,1),DATE(2026,1,1),0.04,0.05,1)` | `#NUM!` | `99.0476…` | settlement == issue date: out of domain intentionally? |
| SHEET | `=SHEET(A1)` | `#N/A` | `1` | Is SHEET with a reference argument planned? |
| SHEETS | `=SHEETS(A1:A10)` | `#N/IMPL!` | `1` | Same question for SHEETS with an argument. |
| SYD | `=SYD(1000,100,5,6)` | `#NUM!` | `0` | period > life: Excel returns `#NUM!`; likely correct. |
| UNICHAR | `=UNICHAR(0)` | `#VALUE!` | NUL char | Excel errors on 0; likely correct. |
| UNIQUE | `=ROWS(UNIQUE({1;2;2;3}))` | `#VALUE!` | `3` | UNIQUE over an array literal (not a range) — intended to work? |
| XNPV | `=XNPV(0.05,{-1000,500},{46204,46023})` | `#NUM!` | `-487.755…` | Dates not in ascending order: Excel accepts this; is the ordering check intentional? |

### 3b. Both return values, values differ (36)

| Function | Formula | IronCalc | LibreOffice | Question |
|---|---|---|---|---|
| ACCRINT | `=ACCRINT(DATE(2025,1,1),DATE(2025,7,1),DATE(2026,1,15),0.05,1000,4,1)` | `51.92307692307692` | `51.9178082191781` | Day-count for actual/actual with quarterly frequency — which convention is intended? |
| BESSELI | `=BESSELI(A1,0)` (A1=2) | `2.279585307296026` | `2.27958530233607` | Differs at 1e-9 — series truncation choice? Probably fine. (The same ~1e-10 drift family shows up under our tolerance in ERF/ERFC, BESSELK, GAUSS, NORM.S.DIST and Z.TEST.) |
| CHISQ.TEST | `=CHISQ.TEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` | The p-value underflows to exactly 0.0 in IronCalc — a 100% relative error on a legitimately tiny result. Looks like a real finding. |
| CHITEST | `=CHITEST(C1:C5,D1:D5)` | `0` | `2.55415463086148e-25` | Same as CHISQ.TEST. |
| CONCATENATE | `=CONCATENATE(E1,"x")` (E1=TRUE) | `TRUEx` | `1x` | Matches Excel; LO treats booleans as numbers. |
| CONVERT | `=CONVERT(1,"lbm","kg")` | `0.45359237` | `0.453592309748811` | IronCalc uses the exact legal constant; LO doesn't. Looks right. |
| CONVERT | `=CONVERT(68,"F","C")` | `19.650000000000034` | `20` | 68°F is exactly 20°C — this one looks like a real finding: an offset/scale ordering issue in the F→C path? |
| COUNT | `=COUNT(E1:E2)` (booleans) | `0` | `2` | Matches Excel (booleans in ranges aren't counted). |
| DOLLAR | `=DOLLAR(-1234.567,-2)` | `($1,200)` | `-$1,200` | Matches Excel's parenthesized negatives. |
| DURATION | `=DURATION(DATE(2026,1,1),DATE(2030,1,1),0.08,0.09,2,1)` | `3.4910833018229606` | `3.49163094694892` | Actual/actual coupon-period handling — which convention? |
| FILTER | `=COUNT(FILTER(C1:C5,D1:D5>25))` | `0` | `3` | 3 rows satisfy the condition — does FILTER's result lose numeric typing through COUNT, or is the include-mask evaluation off? Seems worth a look. |
| ISNUMBER | `=ISNUMBER(E1)` (TRUE) | `FALSE` | `TRUE` | Matches Excel. |
| LINEST | `=INDEX(LINEST(D1:D5,C1:C5),1,2)` | `7.105427357601002e-15` | `0` | Exact-fit intercept is 0; IronCalc carries a float residue. Probably benign, listed for completeness. |
| MAXA | `=MAXA(A6,E1)` (A6=-3, E1=TRUE) | `-3` | `1` | Excel's MAXA counts TRUE as 1 even via reference — is `-3` intended? Possible finding. |
| MDURATION | `=MDURATION(DATE(2026,1,1),DATE(2031,1,1),0.05,0.06,1,1)` | `4.277974103565637` | `4.27840468148732` | Same day-count question as DURATION. |
| MINA | `=MINA(C1:C5,E2)` (E2=FALSE) | `1` | `0` | Excel's MINA counts FALSE as 0 — is ignoring the boolean intended? Possible finding (pairs with MAXA). |
| MODE.MULT | `=COUNT(MODE.MULT({1,1,2,2,3}))` | `0` | `2` | Two modes exist — does MODE.MULT's array result lose numeric typing through COUNT? (pairs with FILTER) |
| ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2025,10,15),0.05,0.06,100,2,1)` | `99.60842257486254` | `99.6086078396235` | Odd-period day-count convention. |
| PERCENTRANK | `=PERCENTRANK(A1:A10,6)` | `0.555` | `0.556` | Truncation (Excel) vs rounding (LO) — IronCalc matches Excel. |
| PERCENTRANK | `=PERCENTRANK(A1:A10,7,5)` | `0.62962` | `0.62963` | Same. |
| PERCENTRANK.EXC | `=PERCENTRANK.EXC(A1:A10,-3)` | `0.09` | `0.0909` | Same. |
| PERCENTRANK.INC | `=PERCENTRANK.INC(A1:A10,10)` | `0.888` | `0.889` | Same. |
| PRICE | `=PRICE(DATE(2026,3,15),DATE(2031,3,15),0.05,0.05,100,4,3)` | `100.00000000000007` | `99.9897902308227` | At coupon == yield a bond prices at par — IronCalc's answer looks self-consistent; LO's doesn't. Confirm? |
| RATE | `=RATE(10,-100,1000)` | `-3.35e-11` | `6.1e-11` | Both ≈ 0; different iteration stopping points. Probably fine. |
| ROW | `=SUM(ROW(C1:C5))` | `1` | `15` | Excel (with implicit intersection/legacy) vs array semantics: should ROW over a range spill 1..5 inside SUM (→15)? |
| SECOND | `=SECOND(0.999999)` | `59` | `0` | 0.999999 of a day is 23:59:59.91 — does Excel round to :00 here? Rounding-vs-truncation of sub-second time. |
| SKEW.P | `=SKEW.P(A1:A2)` | `0` | `#DIV/0!` | 2 identical-distance points: is population skewness of a 2-sample defined as 0 in your model, or should it error? |
| STDEVA | `=STDEVA(A1:A2,B9)` (B9="100") | `1.4142135623730951` | `2` | Excel's STDEVA counts text-in-reference as 0 (numeric-looking text is not coerced) — which side matches Excel here depends on that rule; worth a check. |
| SUM | `=SUM("3",2)` | `5` | `#VALUE!` | Excel coerces direct string args in SUM → 5. Matches Excel. |
| SUMPRODUCT | `=SUMPRODUCT(--(A1:A10>5))` | `0` | `5` | 5 of the 10 values exceed 5 — does the double-unary over a comparison array produce a zero array? Looks like a real finding. |
| TBILLEQ | `=TBILLEQ(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `0.04669019304858653` | `0.0466811612738202` | T-bill day-count (360 vs 365/366 legs) — which convention? |
| TBILLPRICE | `=TBILLPRICE(DATE(2026,3,15),DATE(2026,9,15),0.045)` | `97.7` | `97.7375` | 184 vs 183 days (DSM inclusive/exclusive)? |
| TBILLYIELD | `=TBILLYIELD(DATE(2026,3,15),DATE(2026,9,15),98.5)` | `0.02979474729640256` | `0.0302885828869505` | Same DSM question. |
| TEXTSPLIT | `=COUNTA(TEXTSPLIT("a,b,c",","))` | `3` | `1` | IronCalc spills 3 items; LO's COUNTA sees 1 — LO-side limitation most likely. |
| TREND | `=TREND(D1:D5,C1:C5,0)` | `7.105427357601002e-15` | `0` | Same float residue as LINEST. Probably benign. |
| TRIM | `=TRIM("  a   b  ")` | `a   b` | `a b` | Excel's TRIM collapses interior runs to single spaces (`a b`). Possible finding. |
| TYPE | `=TYPE(E1)` | `4` | `8` | Matches Excel (4 = boolean); LO reports 8. |

### 3c. LibreOffice errors where IronCalc returns a value (21)

An earlier revision of this report folded these into the omitted `#NAME?`
rows; that was a miscount — LibreOffice *recognizes* these functions and
rejected the arguments (`#VALUE!`, one `#DIV/0!`), so each row is an
evaluation difference worth a glance. For most of them IronCalc's answer
looks like the Excel one; the two clear LO-side limitations are inline
LAMBDA invocation and `OR` over a comparison range.

| Function | Formula | IronCalc | LibreOffice | Question |
|---|---|---|---|---|
| IMCOT | `=IMCOT("0")` | `inf` | `#VALUE!` | Excel errors (`#NUM!`) at the pole — is a textual `inf` result intended? |
| IMCSC | `=IMCSC("0")` | `NaNNaNi` | `#VALUE!` | Same pole question; `NaNNaNi` looks like an unintended NaN print-through. |
| IMCSCH | `=IMCSCH("0")` | `NaNNaNi` | `#VALUE!` | Same as IMCSC. |
| IMLN | `=IMLN("0")` | `-inf` | `#VALUE!` | Excel gives `#NUM!` for IMLN(0) — intended? |
| ISOMITTED | `=LAMBDA([x],ISOMITTED(x))()` | `TRUE` | `#VALUE!` | IronCalc matches Excel; LO 24.8 cannot invoke inline LAMBDA. |
| ISOMITTED | `=LAMBDA([x],ISOMITTED(x))(5)` | `FALSE` | `#VALUE!` | Same. |
| ISOMITTED | `=LAMBDA([x],IF(ISOMITTED(x),-1,x))(A1)` | `2` | `#VALUE!` | Same. |
| LAMBDA | `=LAMBDA(x,y,x+y)(A1,A2)` | `6` | `#VALUE!` | Same (LO parses but cannot call). |
| LAMBDA | `=LAMBDA(x,x*x)(A5)` | `100` | `#VALUE!` | Same. |
| LAMBDA | `=LAMBDA(s,UPPER(s))(B1)` | `ALPHA` | `#VALUE!` | Same. |
| NUMBERVALUE | `=NUMBERVALUE("3.5%")` | `0.035` | `#VALUE!` | IronCalc matches Excel (trailing `%` divides by 100). |
| ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.63871848080437` | `#VALUE!` | LO rejects these odd-first-period shapes; no second oracle for the value. |
| ODDFPRICE | `=ODDFPRICE(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.05,0.06,100,1)` | `95.91405301998944` | `#VALUE!` | Same. |
| ODDFPRICE | `=ODDFPRICE(DATE(2026,2,1),DATE(2031,3,1),DATE(2026,3,1),DATE(2026,9,1),0.05,0.06,100,2)` | `95.2644236004297` | `#VALUE!` | Same. |
| ODDFYIELD | `=ODDFYIELD(DATE(2026,2,1),DATE(2031,3,1),DATE(2025,12,1),DATE(2026,9,1),0.05,95,100,2)` | `0.06151963143529892` | `#VALUE!` | Same. |
| ODDFYIELD | `=ODDFYIELD(DATE(2026,5,15),DATE(2031,3,1),DATE(2026,3,1),DATE(2027,3,1),0.04,101.5,100,1)` | `0.03650600623418319` | `#VALUE!` | Same. |
| ODDLPRICE | `=ODDLPRICE(DATE(2026,2,1),DATE(2026,6,15),DATE(2026,3,1),0.05,0.06,100,2)` | `99.71603320227172` | `#VALUE!` | Same (odd-last-period). |
| OR | `=OR(C1:C5>4)` | `TRUE` | `#VALUE!` | IronCalc matches Excel's array coercion; LO wants CSE entry. |
| PRODUCT | `=PRODUCT(C1:C3,"2")` | `12` | `#VALUE!` | Excel coerces direct string args — IronCalc matches. |
| SKEW.P | `=SKEW.P(A1:A2)` | `0` | `#DIV/0!` | 2 identical-distance points: is population skewness of a 2-sample defined as 0 in your model, or should it error? |
| SUM | `=SUM("3",2)` | `5` | `#VALUE!` | Excel coerces direct string args in SUM → 5. Matches Excel. |

The rows we'd flag as most likely to be genuine engine findings, if you only
look at a few: `CHISQ.TEST`/`CHITEST` underflowing a tiny p-value to 0.0,
`CONVERT(68,"F","C")`, `SUMPRODUCT(--(range>cmp))`,
`FILTER`+`COUNT` and `MODE.MULT`+`COUNT` (possibly one shared array-typing
cause), `MAXA`/`MINA` boolean handling, `TRIM` interior-space collapsing,
and `XNPV`'s date-ordering requirement. But we may be wrong about any of
them — the shared workbook and both engines' raw values are all in
`benchmarks/agreement.json` if you want to check our work rather than take
our word.

## Reproducing

- Coverage probe: `cargo run --bin coverage-probe -- benchmarks/excel-functions.txt`
- Differential run: `ORACLE_PYTHON=<python-with-openpyxl> benchmarks/run_oracle.sh`
  (regenerates the shared workbook via openpyxl, recalculates with headless
  LibreOffice, evaluates the same cases in-memory in IronCalc, and rewrites
  `benchmarks/agreement.json`).
