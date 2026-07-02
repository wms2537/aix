# xlq

Agent-safe CLI for Excel workbooks. Single Rust binary, wraps
[IronCalc](https://github.com/ironcalc/IronCalc). Every command emits
machine-readable JSON on stdout; logs and diagnostics go to stderr. Read
commands never modify the target file.

xlq is the enforcement boundary for agents operating on Excel files. A skill
or prompt can *tell* an agent to be careful with a workbook; it cannot make
carelessness impossible. xlq makes the safe path the only path: reads are
guaranteed side-effect-free, output states explicitly what the engine could
and could not evaluate, and (roadmap) writes go through hash-checked patches
with dry-runs and receipts instead of in-place file mutation.

## Why

Agents corrupt Excel files today. [Claude Code issue
#22044](https://github.com/anthropics/claude-code/issues/22044) documents the
built-in xlsx skill editing workbooks through openpyxl, which silently strips
or breaks parts of the file structure of realistic financial models. Excel
reported the files corrupted; users restored from backups and lost formatting,
formulas, and macros. The failure is structural: a general-purpose library
driven by generated Python has no revision model, no dry-run, no receipt, and
no obligation to tell you what it does not understand.

xlq's answer is a purpose-built binary with three properties:

1. **No silent incorrectness.** If the engine cannot evaluate a function in
   your workbook, the output says so (`coverage.reliable: false` plus the
   named functions) instead of guessing.
2. **Read-only by construction in v0.1.** `inspect`, `diff`, and `calc`
   cannot write your file. There is no code path that does.
3. **Local-only.** No network calls, no telemetry, no daemon. Plain files in,
   JSON out.

## The three v0.1 commands

### `xlq inspect` — privacy-safe workbook census

```
xlq inspect payroll.xlsx
```

```json
{
  "xlq": { "version": "0.1.0", "command": "inspect" },
  "file": { "name": "payroll.xlsx", "bytes": 48213, "sha256": "9f2c51…" },
  "sheets": [
    { "name": "Attendance", "state": "visible", "rows": 41, "cols": 32,
      "cells": 1280, "formulas": 0, "errors": {} },
    { "name": "Payroll", "state": "visible", "rows": 41, "cols": 9,
      "cells": 360, "formulas": 280, "errors": { "#N/A": 4 } }
  ],
  "defined_names": { "count": 0 },
  "functions": { "IF": 80, "MAX": 40, "MIN": 40, "SUM": 3, "VLOOKUP": 40 },
  "unsupported_functions": [],
  "volatile_functions": [],
  "ooxml_parts": { "has_vba": false, "has_pivot_cache": false,
                   "has_external_links": false, "has_charts": false,
                   "has_comments": false, "part_count": 12 },
  "coverage": { "engine": "ironcalc 0.7.1", "reliable": true }
}
```

The census contains **no cell values, no formula bodies, no file paths** —
only structure: sheet dimensions, formula and error tallies, function names,
and which OOXML parts (VBA, pivot caches, external links, charts, comments)
exist in the container. It is safe to send to someone else, which is the
point: it lets a tool author learn what a workbook needs without the owner
sharing the workbook. `--redact` additionally anonymizes sheet and defined
names for stricter policies. Format spec: [docs/census-spec.md](docs/census-spec.md).

### `xlq diff` — cell-level positional diff

```
xlq diff close-2026-05.xlsx close-2026-06.xlsx
```

```json
{
  "xlq": { "version": "0.1.0", "command": "diff" },
  "old": { "name": "close-2026-05.xlsx", "sha256": "9f2c51…" },
  "new": { "name": "close-2026-06.xlsx", "sha256": "1d40ab…" },
  "sheets_added": [],
  "sheets_removed": [],
  "changes": [
    { "sheet": "Consolidated", "cell": "D14", "row": 14, "col": 4,
      "kind": "formula",
      "old": { "formula": "=SUM(D2:D13)", "value": "412000" },
      "new": { "formula": null, "value": "415000" } }
  ],
  "summary": { "changed": 1, "added": 0, "removed": 0,
               "by_sheet": { "Consolidated": 1 } }
}
```

The example above is the classic close-process defect: someone pasted a
constant over a formula. Sheets are matched by name; cells are compared
positionally on stored formulas and stored values — the files as they are on
disk, no re-evaluation. v1 has no row-alignment or move detection: an
inserted row reports as many changed cells, by design. The changes list is
capped at 10,000 entries with an explicit `"truncated": true`; summary counts
always reflect the full totals. Unlike `inspect`, diff output *does* contain
values and formulas — it is a comparison tool for the file owner, not a
shareable census.

### `xlq calc` — headless recalculation, report-only

```
xlq calc branch-consolidation.xlsx
```

```json
{
  "xlq": { "version": "0.1.0", "command": "calc" },
  "file": { "name": "branch-consolidation.xlsx", "sha256": "77b0e3…" },
  "changed": [
    { "sheet": "Consolidated", "cell": "C9", "row": 9, "col": 3,
      "stored": "182500", "recomputed": "197300",
      "formula": "=SUM(Branch1!C9,Branch2!C9,Branch3!C9,Branch4!C9,Branch5!C9)",
      "volatile": false }
  ],
  "summary": { "cells": 1120, "formulas": 430, "changed": 1 },
  "coverage": { "engine": "ironcalc 0.7.1", "reliable": true,
                "unsupported_functions": [], "volatile_functions": [] }
}
```

calc loads the file, snapshots every stored value (what Excel last saved),
recomputes, and reports cells where the two disagree — stale caches,
engine/Excel disagreement, or volatile functions. Cells whose formulas call a
volatile function (NOW, TODAY, RAND, RANDBETWEEN, OFFSET, INDIRECT, CELL,
INFO) are flagged `"volatile": true` so expected churn is distinguishable
from real staleness. It never writes the file.

## Reading `coverage`

Every report carries a `coverage` object. `reliable` is `false` whenever the
workbook uses a function the engine cannot evaluate; the functions are listed
in `unsupported_functions`. When `reliable` is `false`, treat value-level
results (calc's changed list, future dry-run predictions) as unverified.
This is the "no silent incorrectness" rule made mechanical: the tool reports
its own blind spots instead of papering over them.

## Roadmap

Next, per [docs/receipt-journal-spec.md](docs/receipt-journal-spec.md) (draft,
v0.2):

- `xlq apply book.xlsx patch.json --dry-run` — typed operations (`set_cell`,
  `set_formula`) against a file-hash revision. The patch carries a
  `base_hash`; if the file on disk has changed, apply refuses with
  `revision_mismatch` instead of clobbering. Dry-run predicts affected cells,
  new formula errors, and before/after values for a `watch` list of named
  outputs.
- Receipts in a hash-chained sidecar journal (`book.xlsx.xlq.jsonl`), one per
  workbook: each receipt's `base_hash` must equal the previous receipt's
  `result_hash`, so any out-of-band edit is detected and recorded, never
  silently absorbed.
- Immutable `book.rev-N.xlsx` history files plus atomic replacement of the
  original — the working file stays authoritative; history is beside it, not
  competing with it.
- `xlq calc --write` routing through the same rev-file + swap + receipt path.
  There will never be a bare in-place write.

Further out: ranged/paged reads, BYO-cloud sync (`xlq push/pull` against your
own S3 bucket or git remote — never a third-party tenant).

## Design principles

- **No silent incorrectness.** Unsupported workbook features are reported,
  never guessed. Coverage flags degrade honestly.
- **Coverage flags over false confidence.** `reliable: false` plus a list of
  reasons beats a wrong answer delivered confidently.
- **Local-only.** Single native binary. No network, no daemon, no Python, no
  Excel installation. All artifacts are plain files that already work with
  git, Dropbox, or S3.
- **Privacy-safe census.** `inspect` output is designed to be shareable:
  structure and tallies only, never contents. See the spec for the exact
  guarantees.
- **Preserve, never execute.** Macros and external connections in a workbook
  are treated as cargo to carry intact, never as code to run.
- **Agent-ergonomic output.** Compact JSON, census-before-content ordering,
  explicit truncation markers — an agent can work a 40-tab workbook without
  flooding its context window.

## Build

```
cd xlq
cargo build --release
```

The binary lands at `xlq/target/release/xlq`. Requires a stable Rust
toolchain; the only notable dependencies are ironcalc, clap, serde, sha2,
and zip.

## Fixture corpus

`cargo run --bin xlq-fixtures -- fixtures/` generates five synthetic
workbooks modeled on real recurring workloads reported by pilot users
(consolidation, reconciliation, payroll, claims), each with planted defects
that `inspect`/`diff`/`calc` should surface:

1. **branch-consolidation.xlsx** — five branch P&L sheets rolled into a
   consolidated P&L and cash flow. Planted: a `#DIV/0!`, a SUM range that
   stops one row short, a constant pasted over a formula.
2. **stock-reconciliation.xlsx** — 600 movement rows reconciled against
   sales and purchases with SUMIFS. Planted: mismatches and a `#N/A` from a
   missing SKU.
3. **payroll.xlsx** — attendance hours to pay via IF/MAX/MIN and VLOOKUP.
   Planted: a negative-hours typo and an employee missing from the rates
   table.
4. **claims.xlsx** — 300-claim register with category limits and check
   columns. Planted: an approval dated before submission.
5. **perf-large.xlsx** — ~100k formula cells for the benchmark harness.

Fixtures are generated with ironcalc itself and a fixed-seed PRNG, so they
are deterministic and their hashes are stable across runs.

## Benchmarks

See [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## License

MIT OR Apache-2.0.
