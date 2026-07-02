# Upstream patch: ENCODEURL, HYPERLINK, AGGREGATE

Implements the three residual Excel functions in the vendored IronCalc master
(`vendor/upstream`, clone of upstream at `e50ccea8`). The diff below is
self-contained inside `vendor/upstream` and is ready to become an upstream PR.

## What was implemented

### ENCODEURL(text) — `base/src/functions/text/encodeurl.rs`

RFC 3986 percent-encoding: every byte of the UTF-8 representation of `text` is
percent-encoded with uppercase hex except the unreserved characters
`A-Z a-z 0-9 - . _ ~`. The argument is coerced to string with the same rules as
the other text functions (`get_string`): numbers/booleans stringify, errors
propagate. Written to xlsx as the future function `_xlfn.ENCODEURL`.

### HYPERLINK(link_location, [friendly_name]) — `base/src/functions/lookup_and_reference/hyperlink.rs`

Value semantics only — IronCalc does not attach link objects to cells, so the
function returns what the cell displays:

- 1 arg: `link_location` cast to text (errors propagate).
- 2 args: the evaluated `friendly_name`, keeping its type — a numeric friendly
  name stays a number, a boolean stays a boolean, errors propagate. An empty
  friendly-name reference displays as `0`, like a plain reference.

There was no existing partial implementation to extend (grep for
`Hyperlink|HYPERLINK` found nothing). No `_xlfn.` prefix (pre-2007 function).

### AGGREGATE(function_num, options, ref1, ...) — `base/src/functions/aggregate.rs`

All 19 scalar forms:

| function_num | function | function_num | function |
|---|---|---|---|
| 1 | AVERAGE | 11 | VAR.P |
| 2 | COUNT | 12 | MEDIAN |
| 3 | COUNTA | 13 | MODE.SNGL |
| 4 | MAX | 14 | LARGE |
| 5 | MIN | 15 | SMALL |
| 6 | PRODUCT | 16 | PERCENTILE.INC |
| 7 | STDEV.S | 17 | QUARTILE.INC |
| 8 | STDEV.P | 18 | PERCENTILE.EXC |
| 9 | SUM | 19 | QUARTILE.EXC |

The full options bitmask 0–7 is honored, **including hidden rows**: the engine
already tracks row visibility at calc time (`worksheet.rows[..].hidden`,
exposed through `cell_hidden_status`, the same machinery SUBTOTAL uses), so the
`#VALUE!` fallback contemplated for engines without visibility was not needed.
Decomposition: `ignore_hidden = options & 1`, `ignore_errors = options & 2`,
`ignore_nested = options < 4`. Written to xlsx as `_xlfn.AGGREGATE`.

## Semantics decisions

- **Filtered vs hidden rows (AGGREGATE)**: unlike SUBTOTAL — which always skips
  filtered rows and only optionally skips manually hidden ones — AGGREGATE
  treats both as "hidden rows", skipped only when the option bit asks for it.
  This matches LibreOffice (which only has `RowHidden`) and the Microsoft docs,
  which list a single "ignore hidden rows" behavior.
- **Nested SUBTOTAL/AGGREGATE (options 0–3)**: skipped as formula cells inside
  referenced ranges (checked via the parsed formula node, mirroring SUBTOTAL's
  `cell_is_subtotal`). A *direct* SUBTOTAL/AGGREGATE call passed as a ref
  argument is rejected with `#VALUE!` — it is not a reference, and Excel
  errors rather than silently skipping it (changed in the 2026-07-03
  follow-up below; the original patch skipped it).
- **Errors**: with options 2/3/6/7 error values are silently dropped while
  walking refs; otherwise the first error encountered is returned, for every
  function_num (LibreOffice behaves the same, including for COUNT/COUNTA).
- **Arity**: function_num 1–13 accept one or more refs; 14–19 require exactly
  `(array, k)`, otherwise `#ERROR!` (args-number error, upstream convention).
- **k handling for 14–19**: LARGE/SMALL truncate k and require `1 <= k <= n`;
  PERCENTILE.INC requires `0 <= k <= 1`; QUARTILE.INC accepts quarts 0–4,
  QUARTILE.EXC accepts 1–3; out-of-range gives `#NUM!` — all consistent with
  the existing standalone implementations.
- **Strings/booleans in ranges** are ignored by the numeric forms and counted
  by COUNTA; empty cells are always skipped.
- **ENCODEURL vs LibreOffice**: LO percent-encodes `.` and `~`; Excel does not
  (Microsoft's own doc example leaves `.` unencoded). We follow Excel/RFC 3986
  unreserved. This is the single expected divergence in the validation below.

### Registration / plumbing (the part that touches many files)

- `Function` enum variants `Encodeurl`, `Hyperlink`, `Aggregate` + lowercase
  lookup entries + display-name arms + `into_iter` (494 → 497) + dispatch arms
  in `base/src/functions/mod.rs`.
- `to_xlsx_string`: `_xlfn.ENCODEURL`, `_xlfn.AGGREGATE` (future functions).
- `static_analysis.rs`: argument signatures (`ENCODEURL` 1 scalar; `HYPERLINK`
  1+1 scalars; new `args_signature_aggregate` — 2 leading scalars then vectors,
  min 3 args) and static-result arms.
- Localized names added to `generate_language/src/languages.json` for all five
  languages (en/it/fr/de/es) using the official Excel localizations
  (e.g. `CODIFICA.URL`, `LIEN_HYPERTEXTE`, `AGGREGAT`, `URLCODIF`, ...);
  `Functions` structs extended in `base/src/language/mod.rs` and
  `generate_language/src/main.rs`; `base/src/language/language.bin`
  regenerated with the `generate_language` tool (matches `make test-language-bin`).
- Small reuse refactors: `cell_hidden_status` made `pub(crate)` (shared with
  SUBTOTAL), `percentile_exc_impl` extracted in `percentile.rs` and reused by
  `fn_percentile_exc`, `fn_quartile_exc` and AGGREGATE 18/19, `find_modes`
  made `pub(crate)` for AGGREGATE 13.

## Test evidence

- `cargo test -p ironcalc_base`: **2125 passed, 0 failed** (plus 23 doc-tests),
  including 15 new tests in `base/src/test/test_fn_encodeurl.rs` (4),
  `test_fn_hyperlink.rs` (4) and `test_fn_aggregate.rs` (7) covering arg-count
  errors, UTF-8 encoding, friendly-name typing, all 19 function_nums, invalid
  function_num/options, ignore-errors, ignore-hidden-rows (via
  `set_row_hidden`) and ignore-nested-SUBTOTAL/AGGREGATE.
  (Run with the workspace `members` temporarily trimmed to `base`/`xlsx`; the
  vendored clone omits `bindings/*`, so the pristine workspace manifest cannot
  be loaded in-tree. `Cargo.toml`/`Cargo.lock` restored afterwards.)
- `cargo fmt -- --check`: clean. `cargo clippy -p ironcalc_base --all-targets
  --all-features -- -W clippy::unwrap_used -W clippy::expect_used -W
  clippy::panic -D warnings` (upstream `make lint` flags): clean.
- `cargo test --manifest-path /home/soh/aix/xlq/Cargo.toml`: **all suites pass**
  (12 + 35 + 5 + 6 integration). One pre-existing failure was fixed along the
  way: `xlq/tests/integration.rs` still asserted the engine string
  `"ironcalc 0.7.1"` although committed `xlq/src/calc.rs` reports
  `"ironcalc 0.7.1+e50ccea8 (vendored master)"` — stale expectation from the
  vendoring commit, unrelated to this patch (xlq-local, not part of the
  upstream PR).

## LibreOffice validation

Workbook authored cachelessly with openpyxl (future functions written with the
`_xlfn.` prefix, as the file format requires), recalculated with
`soffice --headless --convert-to xlsx`, then compared by running `xlq calc` on
the LO output (IronCalc recomputes every formula and reports drift against the
stored LO values). Data: `B1:B6 = 2,4,4,6,8,12`, `B7 = "text"`,
`C1 = =1/0, C2 = 1, C3 = 3`.

| Cell | Formula | LibreOffice | IronCalc | Match |
|---|---|---|---|---|
| A1 | `=ENCODEURL("http://a b/c?d=e")` | `http%3A%2F%2Fa%20b%2Fc%3Fd%3De` | same | yes |
| A2 | `=ENCODEURL("AZaz09-._~")` | `AZaz09-%2E_%7E` | `AZaz09-._~` | no — LO encodes `.`/`~`; Excel does not (we follow Excel) |
| A3 | `=ENCODEURL("€ü")` | `%E2%82%AC%C3%BC` | same | yes |
| A4 | `=HYPERLINK("http://example.com")` | `http://example.com` | same | yes |
| A5 | `=HYPERLINK("http://example.com","Example")` | `Example` | same | yes |
| A6 | `=HYPERLINK("http://example.com",42)` | `42` (number) | same | yes |
| A7 | `=AGGREGATE(1,4,B1:B7)` | `6` | same | yes |
| A8 | `=AGGREGATE(9,4,B1:B7)` | `36` | same | yes |
| A9 | `=AGGREGATE(12,4,B1:B7)` | `5` | same | yes |
| A10 | `=AGGREGATE(13,4,B1:B7)` | `4` | same | yes |
| A11 | `=AGGREGATE(9,6,C1:C3)` | `4` | same | yes |
| A12 | `=AGGREGATE(14,4,B1:B6,2)` | `8` | same | yes |
| A13 | `=AGGREGATE(15,4,B1:B6,2)` | `4` | same | yes |
| A14 | `=AGGREGATE(16,4,B1:B6,0.5)` | `5` | same | yes |
| A15 | `=AGGREGATE(17,4,B1:B6,1)` | `4` | same | yes |
| A16 | `=AGGREGATE(18,4,B1:B6,0.5)` | `5` | same | yes |
| A17 | `=AGGREGATE(19,4,B1:B6,1)` | `3.5` | same | yes |

`xlq calc` summary on the LO-recalculated workbook: 18 formulas, **1 changed**
(A2, the documented Excel-vs-LO ENCODEURL divergence above).

## Diff summary (`git -C vendor/upstream diff --stat`, vendored subtree)

```
 base/src/expressions/parser/static_analysis.rs     |  17 +
 base/src/functions/aggregate.rs                    | 547 +++++++++++++++++++++
 base/src/functions/lookup_and_reference/hyperlink.rs |  42 ++
 base/src/functions/lookup_and_reference/mod.rs     |   1 +
 base/src/functions/mod.rs                          |  20 +-
 base/src/functions/statistical/mod.rs              |   4 +-
 base/src/functions/statistical/mode_functions.rs   |   2 +-
 base/src/functions/statistical/percentile.rs       |  31 +-
 base/src/functions/statistical/quartile.rs         |  19 +-
 base/src/functions/subtotal.rs                     |   2 +-
 base/src/functions/text/encodeurl.rs               |  31 ++
 base/src/functions/text/mod.rs                     |   1 +
 base/src/language/language.bin                     | Bin 23718 -> 23885 bytes
 base/src/language/mod.rs                           |   3 +
 base/src/test/mod.rs                               |   3 +
 base/src/test/test_fn_aggregate.rs                 | 199 ++++++++
 base/src/test/test_fn_encodeurl.rs                 |  71 +++
 base/src/test/test_fn_hyperlink.rs                 |  66 +++
 generate_language/src/languages.json               |  25 +-
 generate_language/src/main.rs                      |   3 +
 20 files changed, 1050 insertions(+), 37 deletions(-)
```

## 2026-07-03 follow-up: four AGGREGATE fixes (also in `vendor/upstream`)

A code review of `base/src/functions/aggregate.rs` after the original patch
surfaced four issues; all are fixed in the vendored tree and belong in any
upstream PR of this patch:

1. **Open ranges are clamped and hidden rows pre-collected** (perf, results
   unchanged). `=AGGREGATE(9,4,B:B)` used to walk all 1,048,576 rows (~280 ms
   per formula), and with a hidden-ignoring option it also called
   `cell_hidden_status` — a linear scan of `worksheet.rows` — per row
   (`=AGGREGATE(9,5,B:B)` with 2,000 hidden rows took ~8.8 s for one cell).
   The range walk now clamps whole-column/row refs to `dimension()` exactly
   like `fn_sum`, and hidden row indices are collected into a `HashSet` once
   per range.
2. **Direct nested call as a ref argument → `#VALUE!`** (semantics).
   `=AGGREGATE(3,0,SUBTOTAL(9,B1:B2))` returned `0` (the argument was
   silently skipped); Excel returns `#VALUE!` because the ref-form argument
   is not a reference. Skipping still applies to SUBTOTAL/AGGREGATE formula
   cells *inside* referenced ranges.
3. **AGGREGATE(17, …) rejects negative quart** (semantics). A quart in
   (-1, 0) truncated to `-0.0`, slipped through the `0.0..=1.0` bounds check
   and returned MIN; Excel and the engine's own QUARTILE.INC return `#NUM!`.
4. **AGGREGATE(6, …) over no numeric values returns 0** (semantics). The
   empty-product identity `1` diverged from Excel's `0` for PRODUCT-style
   aggregation over blank/filtered-to-empty refs.

Test evidence: 4 new tests in `base/src/test/test_fn_aggregate.rs`
(`fn_aggregate_direct_nested_call_is_value_error`,
`fn_aggregate_whole_column_range`, `fn_aggregate_quartile_inc_negative_quart`,
`fn_aggregate_product_no_values`); `cargo test -p ironcalc_base` now
**2129 passed, 0 failed** (plus 23 doc-tests), fmt and the upstream clippy
flags still clean.
