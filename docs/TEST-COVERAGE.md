# xlq Test Coverage

Coverage of the `xlq` crate's own code (`xlq/src`), measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (source-based
LLVM instrumentation via `llvm-tools-preview`).

## Reproduce

```sh
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview

cargo llvm-cov --manifest-path xlq/Cargo.toml --summary-only
# line-by-line HTML report:
cargo llvm-cov --manifest-path xlq/Cargo.toml --html
# remaining uncovered lines:
cargo llvm-cov report --manifest-path xlq/Cargo.toml --show-missing-lines
```

Measured 2026-07-02 with the vendored ironcalc master
(`ironcalc 0.7.1+e50ccea8`). All 67 tests pass (`cargo test`).

## Before / after (line coverage)

21 tests were added (3 value, 3 hash, 6 inspect, 4 diff, 3 calc, 1 census,
1 integration).

| File            | Lines before | Lines after | Regions before | Regions after |
|-----------------|-------------:|------------:|---------------:|--------------:|
| src/calc.rs     | 95.41%       | 97.07%      | 94.49%         | 96.54%        |
| src/census.rs   | 96.85%       | 95.28%*     | 98.65%         | 98.90%        |
| src/diff.rs     | 95.73%       | 98.19%      | 96.21%         | 97.89%        |
| src/hash.rs     | 89.47%       | **100%**    | 86.84%         | 100%          |
| src/inspect.rs  | 81.40%       | 95.85%      | 80.62%         | 95.37%        |
| src/main.rs     | 68.75%       | **100%**    | 75.76%         | 100%          |
| src/value.rs    | 71.43%       | **100%**    | 68.00%         | 100%          |
| TOTAL (incl. dev bins) | 81.30% | 85.99%     | 80.97%         | 85.96%        |

Every `src/*.rs` module is ≥ 95% line coverage (target was ≥ 85%).

\* census.rs "dropped" only because explanatory comments were added inside
its one deliberately-uncovered region, widening that region's line span
(llvm-cov counts a region's whole span); its region coverage went up and no
covered code became uncovered.

## What the new tests cover

- **main.rs dispatch error path** (integration, `CARGO_BIN_EXE_xlq`):
  failing command exits 1, prints `xlq error:` on stderr, emits a
  machine-readable `{"error": ...}` JSON payload on stdout with the file
  basename only — never the directory.
- **value.rs**: every `CellValue` variant → JSON mapping; non-finite numbers
  (`inf`, `-inf`, `NaN`) falling back to string rendering; invalid sheet
  index error.
- **hash.rs**: FIPS 180-2 `"abc"` test vector; open-error context carries
  basename only; path with no file-name component (`/`) uses the `<file>`
  placeholder and the read-error path (EISDIR).
- **inspect.rs**: `strip_cse_array_attrs` (double/single-quoted attr,
  plain `<f>`, `<font>` false-positive, text outside tags, malformed
  unclosed tag); `load_with_cse_normalized` direct happy path and non-zip
  rejection; corrupt-zip `run()` failure with basename-only error;
  `ooxml_parts` on corrupt zip and missing file; `run()` on a path with no
  file-name component; all four OOXML part flags (vba / pivot cache /
  external links / charts) flipping to true.
- **diff.rs**: `removed` cell kind in a common sheet (with summary
  buckets); `basename` fallback; load errors for old and new files carrying
  basenames only; `run()` end-to-end (shas, summary).
- **calc.rs**: `cell_reference` R1C1 fallback for out-of-A1-range columns;
  `run()` error paths (missing file at the sha256 step, non-xlsx at the
  load step, basenames only); the 10 000-entry truncation cap through
  `run()` (10 005 stale formula cells → `truncated: true`, `changed` list
  capped, summary keeps full totals).
- **census.rs**: lexer `Illegal` token (unterminated string literal) stops
  extraction without spinning.

## Deliberately not covered (and why)

Remaining uncovered lines in `xlq/src` are all one of:

1. **Defensive arms unreachable through the public API** (each carries a
   `NOT COVERABLE` / defensive comment in the source):
   - `inspect.rs` `run()`: `sheet >= sheet_count` skip — `get_all_cells`
     and `get_worksheets_properties` come from the same model.
   - `inspect.rs` `load_model()`: the `"array formulas"` retry arm — the
     vendored ironcalc master loads CSE array formulas natively, so no
     file triggers the fallback anymore; kept as a guard for future engine
     rejections. This also makes `coverage.unsupported_features`
     (`inspect.rs`, one line) unreachable, since only that arm populates
     it. The fallback machinery itself (`load_with_cse_normalized`,
     `strip_cse_array_attrs`) IS unit-tested directly.
   - `census.rs` `probe_support()`: the `set_user_input` parser-reject
     failure default — verified empirically that the vendored engine
     accepts any `=NAME(1)` probe an Ident can produce; kept so a future
     engine bump fails toward "unsupported" (honest coverage claim).
   - `diff.rs`: `unreachable!("coord came from union of keys")`.
   - `inspect.rs` `load_with_cse_normalized()`: the `entry.is_dir()` skip —
     Excel/ironcalc-produced zips contain no explicit directory entries;
     reachable only with third-party archivers.
2. **Test-only panic/else arms** (`other => panic!` match guards in tests,
   the CSE test's engine-rejects-natively `else` branch): fake inputs to
   force them would test nothing real.
3. **Dev-only binaries at 0%** (`src/bin/coverage_probe.rs`,
   `load_only.rs`, `oracle_compare.rs`, `roundtrip.rs`): engine-probing /
   benchmarking harnesses run by hand, not product code; excluded from the
   ≥ 85% goal on purpose. `src/bin/xlq_fixtures.rs` (97% lines) is
   exercised by the integration suite because it generates the fixtures.

No fake tests were written for these; the goal is real gap closure, not a
number.
