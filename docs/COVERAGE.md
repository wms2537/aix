# IronCalc Excel Function Coverage

**345 of 522 Excel functions supported — 66.1%** (ironcalc 0.7.1)

- Probe date: 2026-07-02
- Function universe: Microsoft's canonical "[Excel functions (alphabetical)](https://support.microsoft.com/en-us/office/excel-functions-alphabetical-b3944572-255d-4efb-bb96-c6d90033e188)" list, fetched 2026-07-02 (522 names; see `benchmarks/excel-functions.txt`).
- Probe method: `cargo run --bin coverage-probe -- benchmarks/excel-functions.txt`. For each name, set `=NAME(1)` in a scratch model, evaluate, and treat `#NAME?` as unsupported — the same `census::probe_support` code path `xlq inspect` uses. A name whose probe formula the engine's parser rejects outright (`set_user_input` error) is also counted as unsupported: the failure default must not inflate the coverage number (no such name exists in the current 522, so this rule does not change the totals today). Excel semantics: unknown names error before argument validation, so a non-`#NAME?` result (even `#VALUE!`) means the engine recognizes the function.
- Caveat: "recognized" is not "bit-perfect". This measures name resolution, not numerical fidelity or full argument-signature coverage. xlq's `calc` command (stored-vs-recomputed comparison) is the fidelity check; this matrix is the breadth check.
- Raw data: `benchmarks/coverage.json` (`{"FUNCTION": supported_bool}`).

## Breakdown by category

**Provenance caveat:** the category assignments below are hand-classified
from Microsoft's function list; no per-function category mapping exists as a
machine-readable artifact in this repo, so the per-category rows cannot be
regenerated or verified from `benchmarks/coverage.json` — only the aggregate
total (345/522) and the name lists below can. Treat the category rows as an
editorial reading aid, not as measured data.

| Category | Supported | Total | % |
|---|---:|---:|---:|
| date & time | 25 | 25 | 100% |
| engineering | 54 | 54 | 100% |
| logical | 11 | 11 | 100% |
| information | 20 | 20 | 100% |
| database (D-functions) | 12 | 12 | 100% |
| math & trig | 71 | 79 | 89.9% |
| lookup & reference | 14 | 22 | 63.6% |
| statistical | 88 | 149 | 59.1% |
| financial | 28 | 56 | 50.0% |
| text | 22 | 50 | 44.0% |
| dynamic-array & lambda | 0 | 30 | 0% |
| cube | 0 | 7 | 0% |
| web / legacy / other | 0 | 7 | 0% |
| **Total** | **345** | **522** | **66.1%** |

Shape of the gap: the classic single-cell compute core (date, logical, information, engineering, database, most math) is essentially complete. What's missing clusters into (a) the entire dynamic-array/LAMBDA generation, (b) legacy pre-2010 statistical aliases, (c) bond-market financial functions, (d) text-manipulation stragglers, and (e) everything that needs an external data source (cube, web, RTD).

## Full unsupported list (177)

**math & trig (8):** AGGREGATE, MDETERM, MINVERSE, MMULT, MULTINOMIAL, MUNIT, SERIESSUM, SUMPRODUCT

**lookup & reference (8):** ADDRESS, AREAS, GETPIVOTDATA, HYPERLINK, IMAGE, RTD, TRANSPOSE, XMATCH

**text (28):** ARRAYTOTEXT, ASC, BAHTTEXT, CHAR, CLEAN, CODE, DBCS, DETECTLANGUAGE, DOLLAR, FINDB, FIXED, JIS, LEFTB, LENB, MIDB, NUMBERVALUE, PHONETIC, PROPER, REGEXEXTRACT, REGEXREPLACE, REGEXTEST, REPLACE, REPLACEB, RIGHTB, SEARCHB, TEXTSPLIT, TRANSLATE, UNICHAR

**statistical (61):** BETADIST, BETAINV, BINOMDIST, CHIDIST, CHIINV, CHITEST, CONFIDENCE, COVAR, CRITBINOM, EXPONDIST, FDIST, FINV, FORECAST, FORECAST.ETS, FORECAST.ETS.CONFINT, FORECAST.ETS.SEASONALITY, FORECAST.ETS.STAT, FORECAST.LINEAR, FREQUENCY, FTEST, GAMMADIST, GAMMAINV, GROWTH, HYPGEOMDIST, LINEST, LOGEST, LOGINV, LOGNORMDIST, MODE, MODE.MULT, MODE.SNGL, NEGBINOMDIST, NORMDIST, NORMINV, NORMSDIST, NORMSINV, PERCENTILE, PERCENTILE.EXC, PERCENTILE.INC, PERCENTRANK, PERCENTRANK.EXC, PERCENTRANK.INC, PERMUT, PERMUTATIONA, POISSON, PROB, QUARTILE, QUARTILE.EXC, QUARTILE.INC, RANK, STDEV, STDEVP, TDIST, TINV, TREND, TRIMMEAN, TTEST, VAR, VARP, WEIBULL, ZTEST

**financial (28):** ACCRINT, ACCRINTM, AMORDEGRC, AMORLINC, COUPDAYBS, COUPDAYS, COUPDAYSNC, COUPNCD, COUPNUM, COUPPCD, DISC, DURATION, FVSCHEDULE, INTRATE, MDURATION, ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD, PRICE, PRICEDISC, PRICEMAT, RECEIVED, STOCKHISTORY, VDB, YIELD, YIELDDISC, YIELDMAT

**dynamic-array & lambda (30):** BYCOL, BYROW, CHOOSECOLS, CHOOSEROWS, DROP, EXPAND, FILTER, GROUPBY, HSTACK, ISOMITTED, LAMBDA, LET, MAKEARRAY, MAP, PERCENTOF, PIVOTBY, RANDARRAY, REDUCE, SCAN, SEQUENCE, SORT, SORTBY, TAKE, TOCOL, TOROW, TRIMRANGE, UNIQUE, VSTACK, WRAPCOLS, WRAPROWS

**cube (7):** CUBEKPIMEMBER, CUBEMEMBER, CUBEMEMBERPROPERTY, CUBERANKEDMEMBER, CUBESET, CUBESETCOUNT, CUBEVALUE

**web / legacy / other (7):** CALL, COPILOT, ENCODEURL, EUROCONVERT, FILTERXML, REGISTER.ID, WEBSERVICE

Notable asymmetries worth knowing about:

- Modern dotted statistical names are supported while their legacy aliases are not: `STDEV.S` works, `STDEV` does not; likewise `VAR`/`VAR.S`, `PERCENTILE`/`PERCENTILE.INC`, `QUARTILE`/`QUARTILE.INC`, `MODE`/`MODE.SNGL`, `RANK`/`RANK.EQ`, `NORMDIST`/`NORM.DIST`, etc. Older workbooks (pre-2010 authoring) hit these aliases constantly.
- `XLOOKUP` is supported but `XMATCH` is not.
- `SUMPRODUCT` — arguably the most-used "power" function in finance workbooks — is unsupported, while `SUMIF`/`SUMIFS`/`SUMSQ` all work.
- `CHAR`, `CLEAN`, `PROPER`, `REPLACE` are missing from an otherwise solid text set (`LEFT`/`MID`/`FIND`/`SEARCH`/`SUBSTITUTE`/`TRIM`/`TEXTJOIN` all work).

## What this means for the four target workloads

Function inventory measured from the fixture corpus with `xlq inspect` (tallies are formula call sites), cross-checked against the fixture generator source (`xlq/src/bin/xlq_fixtures.rs`) and `fixtures/planted-defects.json`:

| Workload | Functions used (call sites) | All supported? |
|---|---|---|
| branch consolidation | SUM (203) | Yes |
| stock reconciliation | SUMIFS (124), VLOOKUP (31) | Yes |
| payroll | IF (40), MAX (40), MIN (40), SUM (40), VLOOKUP (40) | Yes |
| claims | IF (900), DATE (438), VLOOKUP (300), COUNTIF (4) | Yes |

**All four target workloads are safe today.** Every function they call — SUM, SUMIFS, VLOOKUP, IF, MIN, MAX, DATE, COUNTIF (and AVERAGE in the perf fixture) — resolves in ironcalc 0.7.1, and `xlq inspect` reports `unsupported_functions: []` for every fixture. The planted defects (#DIV/0!, #N/A from missing lookup keys, range-short SUMs, date-order violations) all reproduce under recalculation, which is exactly what xlq's calc/diff receipts need.

The realistic risk is not these fixtures but their wild cousins:

- **Stock recon** authored before SUMIFS (Excel 2007) idiomatically uses `SUMPRODUCT((range=key)*qty)` — unsupported, and arguably the single most consequential gap for these four workloads (a judgment call, not a measured ranking).
- **Claims / payroll** books doing severity stats will reach for `PERCENTILE`, `QUARTILE`, `FREQUENCY`, `MODE`, legacy `STDEV`/`VAR` — all unsupported (dotted modern forms are fine).
- **Any workbook** touched by a Microsoft 365 user may pick up `FILTER`/`UNIQUE`/`SORT`/`LET`/`XMATCH` spills — the entire dynamic-array family is unsupported.
- **Consolidation** books built on pivot tables use `GETPIVOTDATA` — unsupported.

Because `xlq inspect` runs this same probe per-workbook, an unsupported function in a real customer file is detected up front (`unsupported_functions` + `coverage.reliable: false`) rather than silently miscalculated.

## Reproducing

```
cd xlq
cargo run --bin coverage-probe -- ../benchmarks/excel-functions.txt > ../benchmarks/coverage.json
```
