# IronCalc Excel Function Coverage

Part of **AXLE-bench** (suite front page:
[benchmarks/README.md](../benchmarks/README.md)) — this document is the
narrative for axis 5 (Catalog).

Three numbers, per the coverage accounting rule in
[docs/specs/full-catalog-semantics.md](specs/full-catalog-semantics.md) —
never quote one of them alone:

| # | Measure | Count | % |
|---|---|---:|---:|
| 1 | **Catalog recognized** (parses and dispatches; never an unknown-name error) | **522 / 522** | **100%** |
| 2 | **Locally evaluable** (full semantics computed from workbook data alone) | **505 / 522** | **96.7%** |
| 3 | **Policy-limited** (recognized + argument-checked, then the documented desktop-Excel refusal literal; the true value depends on an external service the tool never contacts) | **17 / 522** | **3.3%** |

505 + 17 = 522. Engine: `ironcalc 0.7.1+e50ccea8 (vendored master)` plus the
local patches in `vendor/upstream`: the residual-functions patch (ENCODEURL,
HYPERLINK, AGGREGATE; see
[docs/upstream/residual-functions-patch.md](upstream/residual-functions-patch.md))
and the 100%-catalog milestone (Tier I implementations — FILTERXML,
EUROCONVERT, DBCS/JIS, BAHTTEXT, PHONETIC, GROUPBY, PIVOTBY — plus the
17 Tier II policy-limited functions with Excel-exact refusal literals).

- Probe date: 2026-07-03.
- Function universe: Microsoft's canonical "[Excel functions (alphabetical)](https://support.microsoft.com/en-us/office/excel-functions-alphabetical-b3944572-255d-4efb-bb96-c6d90033e188)" list, fetched 2026-07-02 (522 names; see `benchmarks/excel-functions.txt`).
- Probe method: `cargo run --bin coverage-probe -- benchmarks/excel-functions.txt`.
  For each name, set `=NAME(1)` in a scratch model, evaluate, and treat
  `#NAME?` as unrecognized — the same `census::probe_support` code path
  `xlq inspect` uses. A name whose probe formula the engine's parser rejects
  outright (`set_user_input` error) is also counted as unrecognized: the
  failure default must not inflate the coverage number. Excel semantics:
  unknown names error before argument validation, so a non-`#NAME?` result
  (even `#VALUE!`) means the engine recognizes the function.
- **CUBE carve-out** (spec, "Coverage accounting rule"): the seven CUBE
  functions answer `#NAME?` *by design* when evaluated without an OLAP
  connection — Microsoft documents "if the connection name is not a valid
  workbook connection… #NAME?", and with no OLAP connectivity every
  connection string is invalid, so `#NAME?` is the Excel-exact *result*, not
  a recognition failure. The probe therefore probes exactly these seven
  names with **zero arguments** instead: a recognized cube function fails
  argument-count validation first (`#ERROR!`, never `#NAME?`), while a name
  the engine truly does not know still answers `#NAME?`. Recognition stays
  measured, not asserted (`CUBE_NAME_CARVE_OUT` in `xlq/src/census.rs`).
- Caveat: "recognized" is not "bit-perfect". Number 1 measures name
  resolution; number 2 is the honest evaluability claim; neither measures
  numerical fidelity or full argument-signature coverage. xlq's `calc`
  command (stored-vs-recomputed comparison) is the fidelity check, and
  [docs/AGREEMENT.md](AGREEMENT.md) is the value-level differential check
  against LibreOffice (1,659 cases); this matrix is the breadth check.
- Raw data: `benchmarks/coverage.json` — the three counts, a per-name
  classification (`locally_evaluable` / `policy_limited` / `unrecognized`),
  and the per-function literal + reason for the policy set.

## History

| Date | Engine | Recognized | Locally evaluable | Policy-limited |
|---|---|---:|---:|---:|
| 2026-07-02 | ironcalc 0.7.1 (release) | 345 / 522 (66.1%) | — (not yet measured separately) | — |
| 2026-07-02 | ironcalc master @ e50ccea8 (vendored) + residual patch | 497 / 522 (95.2%) | 497 (the 25 unrecognized were taxonomized as not-locally-evaluable) | 0 (concept not yet implemented) |
| 2026-07-03 | + full-catalog milestone (Tier I + Tier II) | **522 / 522 (100%)** | **505** | **17** |

The 0.7.1 → master jump was upstream's work (the dynamic-array/LAMBDA
family, legacy statistical aliases, SUMPRODUCT, XMATCH, the bond-financial
set, the text stragglers, and the matrix/CSE functions), plus 3 residual
names from the local patch. The 497 → 522 close-out is this repo's
full-catalog milestone: 8 catalog names genuinely implemented (FILTERXML,
EUROCONVERT, DBCS, JIS, BAHTTEXT, PHONETIC, GROUPBY, PIVOTBY; PERCENTOF and
TRIMRANGE — the other two Tier I names — had already landed with the master
pin, inside the 497) and 17 recognized as policy-limited with Excel-exact
refusal literals: 497 + 8 + 17 = 522.

## The 17 policy-limited functions, one by one

These are **not missing formulas**: the engine recognizes each name,
validates its arguments per the documented rules (wrong argument counts,
over-long CUBE expressions, bad enums, and malformed URLs all error exactly
as documented, *before* the refusal), and then returns precisely the error
literal desktop Excel produces when the external work cannot happen. What
the engine never does is the external work itself — xlq's design memo §16
treats external execution as hostile by default, which surfaces in the
README's design principles as **Local-only** ("no network calls, no
telemetry") and **Preserve, never execute**. The table is generated from
the same source of truth the tool uses at runtime
(`POLICY_LIMITED_FUNCTIONS` in `xlq/src/census.rs`, mirrored in
`benchmarks/coverage.json` under `policy_limited_detail`).

| Function | Returns | Why it cannot be computed locally |
|---|---|---|
| WEBSERVICE | `#VALUE!` | external HTTP fetch; `#VALUE!` is Excel's literal for every failure to fetch, including offline (after >2048-char / non-http(s) URL validation) |
| RTD | `#N/A` | real-time COM data feed; documented result when no RTD server is installed |
| STOCKHISTORY | `#CONNECT!` | Microsoft market-data service; offline/service literal after argument validation (`#VALUE!` for bad enums first) |
| DETECTLANGUAGE | `#CONNECT!` | Microsoft language-detection service; offline literal after text coercion |
| TRANSLATE | `#CONNECT!` | Microsoft translation service; offline literal after language-code validation (`#VALUE!` for invalid codes first) |
| COPILOT | `#CONNECT!` | Copilot AI service; timeout/no-service literal after prompt-argument validation |
| IMAGE | `#CONNECT!` | remote image fetch; cannot-retrieve literal after the documented `#VALUE!` sizing/dimension validation |
| CALL | `#BLOCKED!` | XLM/DLL procedure invocation, disabled in worksheets since MS98-018; never executed |
| REGISTER.ID | `#BLOCKED!` | XLM/DLL procedure registration; same blocked-XLM policy basis as CALL |
| CUBEVALUE | `#NAME?` | OLAP cube query; with no OLAP connectivity every connection name is invalid → documented `#NAME?` (NOT name-unknown) |
| CUBEMEMBER | `#NAME?` | OLAP cube member lookup; documented `#NAME?` for an invalid workbook connection (`#VALUE!` for >255-char expressions first) |
| CUBESET | `#NAME?` | OLAP cube set definition; documented `#NAME?` for an invalid workbook connection |
| CUBESETCOUNT | `#NAME?` | counts a CUBESET set; without OLAP the set argument is itself `#NAME?` and propagates (a non-set value → `#VALUE!`) |
| CUBERANKEDMEMBER | `#NAME?` | OLAP cube ranked member; documented `#NAME?` for an invalid workbook connection |
| CUBEKPIMEMBER | `#NAME?` | OLAP cube KPI member; documented `#NAME?` for an invalid workbook connection |
| CUBEMEMBERPROPERTY | `#NAME?` | OLAP cube member property; documented `#NAME?` for an invalid workbook connection |
| GETPIVOTDATA | `#REF!` | reads a rendered PivotTable; the engine has no pivot model, so the reference never contains one → documented `#REF!` |

By kind: 7 external services (WEBSERVICE, RTD, STOCKHISTORY,
DETECTLANGUAGE, TRANSLATE, COPILOT, IMAGE), 2 blocked native-code entry
points (CALL, REGISTER.ID), 7 OLAP-connection functions (the CUBE family),
1 pivot-model function (GETPIVOTDATA).

If a workbook uses one of these names, `xlq inspect` and `xlq calc` report
it in the census's `policy_limited_functions` bucket (name → literal) and
set `coverage.reliable: false` — the stored values genuinely cannot be
*verified* locally — while `unsupported_functions` stays empty: "the engine
does not know this name" would be false. See
[docs/census-spec.md](census-spec.md).

## What became locally evaluable in the milestone

| Function | Semantics now computed locally |
|---|---|
| FILTERXML | XPath-1.0 subset over an XML string (`//`, `/`, `@attr`, `[n]`, `last()`, `text()`, `contains()`, `starts-with()`, `not()`, namespace prefixes); multiple matches spill vertically; numeric-looking matches return as numbers. LibreOffice cross-checks in AGREEMENT.md |
| EUROCONVERT | the 14 irrevocably-fixed EU conversion rates with per-currency rounding and EUR triangulation — offline by definition since 1999/2001; the add-in dependency was packaging, not data. LibreOffice agrees on all value cases, including the spec's worked example |
| DBCS / JIS | half-width → full-width conversion (ASCII `U+0021–U+007E`, half-width katakana with voiced-mark composition); same built-in under two names. LibreOffice (JIS) agrees 3/3 |
| BAHTTEXT | Thai text money algorithm (2dp rounding, 6-digit ล้าน block stacking, เอ็ด/ยี่ rules, สตางค์/ถ้วน) |
| PHONETIC | furigana `rPh` runs from sharedStrings; cells without runs return their own text unchanged |
| GROUPBY / PIVOTBY | pure dynamic-array grouping/pivoting: eta-reduced and LAMBDA aggregations, field headers, total depths, signed sort indices, filtering, `field_relationship` / `relative_to` |
| PERCENTOF / TRIMRANGE | already present on the master pin; TRIMRANGE's all-blank input now returns the documented `#REF!` |

## What this means for the four target workloads

Function inventory measured from the fixture corpus with `xlq inspect`
(tallies are formula call sites), cross-checked against the fixture
generator source (`xlq/src/bin/xlq_fixtures.rs`) and
`fixtures/planted-defects.json`:

| Workload | Functions used (call sites) | All locally evaluable? |
|---|---|---|
| branch consolidation | SUM (203) | Yes |
| stock reconciliation | SUMIFS (124), VLOOKUP (31) | Yes |
| payroll | IF (40), MAX (40), MIN (40), SUM (40), VLOOKUP (40) | Yes |
| claims | IF (900), DATE (438), VLOOKUP (300), COUNTIF (4) | Yes |

All four report `unsupported_functions: []` and
`policy_limited_functions: {}`, and the planted defects (#DIV/0!, #N/A from
missing lookup keys, range-short SUMs, date-order violations) all reproduce
under recalculation.

For workbooks in the wild the remaining honest boundary is the policy set:
a consolidation book built on pivot tables still hits GETPIVOTDATA and a
market-data book still hits STOCKHISTORY — those books get the documented
Excel error value and a census that says exactly why
(`policy_limited_functions` + `coverage.reliable: false`) rather than a
guess. Because `xlq inspect` runs this same probe per-workbook, the
distinction is made up front, never silently.

## Reproducing

```
cd xlq
cargo run --bin coverage-probe -- ../benchmarks/excel-functions.txt > ../benchmarks/coverage.json
```

Output: `catalog_size` / `catalog_recognized` / `locally_evaluable` /
`policy_limited` counts, the `unrecognized` list (empty today), per-name
classifications under `functions`, and the policy table under
`policy_limited_detail`.
