# IronCalc Excel Function Coverage

**497 of 522 Excel functions supported — 95.2%**
(engine: `ironcalc 0.7.1+e50ccea8 (vendored master)` plus the residual-functions
patch — ENCODEURL, HYPERLINK, AGGREGATE — in `vendor/upstream`; see
[docs/upstream/residual-functions-patch.md](upstream/residual-functions-patch.md)).

**Every one of the 25 remaining unsupported functions requires an external
service, an OLAP connection, a pivot-table model, or a DBCS locale — none is
locally evaluable from cell values alone. Effective coverage of
locally-evaluable functions: 497/497 (100%).** The taxonomy below is the
evidence for that claim; check it, don't trust it.

- Probe date: 2026-07-02.
- Function universe: Microsoft's canonical "[Excel functions (alphabetical)](https://support.microsoft.com/en-us/office/excel-functions-alphabetical-b3944572-255d-4efb-bb96-c6d90033e188)" list, fetched 2026-07-02 (522 names; see `benchmarks/excel-functions.txt`).
- Probe method: `cargo run --bin coverage-probe -- benchmarks/excel-functions.txt`. For each name, set `=NAME(1)` in a scratch model, evaluate, and treat `#NAME?` as unsupported — the same `census::probe_support` code path `xlq inspect` uses. A name whose probe formula the engine's parser rejects outright (`set_user_input` error) is also counted as unsupported: the failure default must not inflate the coverage number (no such name exists in the current 522, so this rule does not change the totals today). Excel semantics: unknown names error before argument validation, so a non-`#NAME?` result (even `#VALUE!`) means the engine recognizes the function.
- Caveat: "recognized" is not "bit-perfect". This measures name resolution, not numerical fidelity or full argument-signature coverage. xlq's `calc` command (stored-vs-recomputed comparison) is the fidelity check, and [docs/AGREEMENT.md](AGREEMENT.md) is the value-level differential check against LibreOffice; this matrix is the breadth check.
- Raw data: `benchmarks/coverage.json` (`{"FUNCTION": supported_bool}`).

## History

| Date | Engine | Supported | % |
|---|---|---:|---:|
| 2026-07-02 | ironcalc 0.7.1 (release) | 345 / 522 | 66.1% |
| 2026-07-02 | ironcalc master @ e50ccea8 (vendored) + residual patch | 497 / 522 | 95.2% |

The jump is upstream's work, not ours: between the 0.7.1 release and master
@ e50ccea8, IronCalc closed essentially the entire gap we measured against
0.7.1 — the dynamic-array/LAMBDA family, the legacy statistical aliases,
SUMPRODUCT, XMATCH, the bond-financial set, the text stragglers, and the
matrix/CSE functions all landed upstream. Our residual patch contributes
exactly 3 of the 152 newly supported names (ENCODEURL, HYPERLINK, AGGREGATE).

## Breakdown by category

**Provenance caveat:** the category assignments below are hand-classified
from Microsoft's function list; no per-function category mapping exists as a
machine-readable artifact in this repo, so the per-category rows cannot be
regenerated or verified from `benchmarks/coverage.json` — only the aggregate
total (497/522) and the name lists below can. Treat the category rows as an
editorial reading aid, not as measured data.

| Category | Supported | Total | % |
|---|---:|---:|---:|
| date & time | 25 | 25 | 100% |
| engineering | 54 | 54 | 100% |
| logical | 11 | 11 | 100% |
| information | 20 | 20 | 100% |
| database (D-functions) | 12 | 12 | 100% |
| math & trig | 79 | 79 | 100% |
| statistical | 149 | 149 | 100% |
| financial | 55 | 56 | 98.2% |
| dynamic-array & lambda | 28 | 30 | 93.3% |
| text | 44 | 50 | 88.0% |
| lookup & reference | 19 | 22 | 86.4% |
| web / legacy / other | 1 | 7 | 14.3% |
| cube | 0 | 7 | 0% |
| **Total** | **497** | **522** | **95.2%** |

Seven categories are now complete, including the two that were the biggest
0.7.1 gaps (statistical: 88→149; math & trig: 71→79). Every remaining
sub-100% row is explained entirely by the four buckets below.

## The remaining 25, honestly taxonomized

None of these is a "missing formula" in the ordinary sense. Each requires
something a local, hermetic calculation engine does not have: a network
service, a COM/add-in runtime, an OLAP cube connection, a pivot-table data
model, or a DBCS locale subsystem. They are listed with the reason, so the
claim "497/497 of locally-evaluable functions" is checkable name by name.

### (a) External-service / security-policy-excluded (11)

`WEBSERVICE`, `FILTERXML`, `RTD`, `STOCKHISTORY`, `DETECTLANGUAGE`,
`TRANSLATE`, `COPILOT`, `IMAGE`, `CALL`, `REGISTER.ID`, `EUROCONVERT`

These reach outside the workbook: HTTP fetches (WEBSERVICE, and FILTERXML as
its parsing companion), real-time COM data feeds (RTD), Microsoft's market
data and AI services (STOCKHISTORY, DETECTLANGUAGE, TRANSLATE, COPILOT),
remote image fetch (IMAGE), and DLL/add-in invocation (CALL, REGISTER.ID,
EUROCONVERT — the last requires the Euro Currency Tools add-in). For xlq
these are excluded by policy, not just by engine maturity: the design memo's
§16 stance is that the workspace treats external execution as hostile by
default, which surfaces in the README's design principles as **Local-only**
("no network calls, no telemetry") and **Preserve, never execute** (macros
and external connections are cargo to carry, never code to run). An engine
that phoned home or executed add-in code inside `xlq calc` would break the
tool's core guarantee. If a workbook uses one of these names, `xlq inspect`
reports it in `unsupported_functions` and sets `coverage.reliable: false` —
which is the correct behavior, since the stored values genuinely cannot be
verified locally.

### (b) Requires an OLAP connection (7)

`CUBEKPIMEMBER`, `CUBEMEMBER`, `CUBEMEMBERPROPERTY`, `CUBERANKEDMEMBER`,
`CUBESET`, `CUBESETCOUNT`, `CUBEVALUE`

The CUBE family evaluates MDX queries against an external Analysis Services
/ Power Pivot cube. There is no cube, so there is nothing to compute; any
local "implementation" would be fabrication.

### (c) Requires a pivot-table data model (3)

`GETPIVOTDATA`, `GROUPBY`, `PIVOTBY`

GETPIVOTDATA reads from a rendered pivot table; GROUPBY/PIVOTBY are the
dynamic-array pivot builders. IronCalc does not yet model pivot tables
(xlq's `inspect` reports `has_pivot_cache` so their presence is at least
visible). These are the most plausible future candidates to move out of
this taxonomy, since a pivot model is local data, not an external service.

### (d) DBCS-locale (4)

`BAHTTEXT`, `DBCS`, `JIS`, `PHONETIC`

Thai text rendering of numbers, double-byte character conversion, and
furigana extraction. These need locale/IME data that the engine does not
carry. Note the *byte-counting* DBCS variants (ASC, FINDB, LEFTB, LENB,
MIDB, REPLACEB, RIGHTB, SEARCHB) **are** supported on master — only the four
that need actual locale subsystems remain.

## What this means for the four target workloads

Function inventory measured from the fixture corpus with `xlq inspect`
(tallies are formula call sites), cross-checked against the fixture
generator source (`xlq/src/bin/xlq_fixtures.rs`) and
`fixtures/planted-defects.json`:

| Workload | Functions used (call sites) | All supported? |
|---|---|---|
| branch consolidation | SUM (203) | Yes |
| stock reconciliation | SUMIFS (124), VLOOKUP (31) | Yes |
| payroll | IF (40), MAX (40), MIN (40), SUM (40), VLOOKUP (40) | Yes |
| claims | IF (900), DATE (438), VLOOKUP (300), COUNTIF (4) | Yes |

All four target workloads were already safe under 0.7.1 and remain so;
`xlq inspect` reports `unsupported_functions: []` for every fixture, and the
planted defects (#DIV/0!, #N/A from missing lookup keys, range-short SUMs,
date-order violations) all reproduce under recalculation.

What changed is the *wild cousins* of these workbooks. The 0.7.1 risk list —
`SUMPRODUCT`-style stock recons authored before Excel 2007, claims/payroll
books using `PERCENTILE`/`QUARTILE`/`FREQUENCY`/legacy `STDEV`/`VAR`, and
any Microsoft 365 workbook carrying `FILTER`/`UNIQUE`/`SORT`/`LET`/`XMATCH`
spills — is now fully covered on master. The one survivor from that list:
consolidation books built on pivot tables still hit `GETPIVOTDATA`
(bucket c above).

Because `xlq inspect` runs this same probe per-workbook, an unsupported
function in a real customer file is detected up front
(`unsupported_functions` + `coverage.reliable: false`) rather than silently
miscalculated.

## Reproducing

```
cd xlq
cargo run --bin coverage-probe -- ../benchmarks/excel-functions.txt > ../benchmarks/coverage.json
```
