# xlq — a safe-write boundary for LLM agents editing spreadsheets

Single Rust binary, wraps [IronCalc](https://github.com/ironcalc/IronCalc).
Machine-readable JSON on stdout. An agent editing a spreadsheet today uses
"generate openpyxl code that loads → mutates → saves the whole file," which
rewrites the entire container and silently destroys everything the library
can't model — charts, pivots, VBA. xlq gives the agent a **surgical write**
instead: it edits only the cells you asked for and leaves every other part of
the file byte-identical.

## What it guarantees (v0.3, built and tested — 192 tests)

- **`xlq apply` is surgical.** It rewrites only the OOXML parts that contain a
  changed cell and copies every other part byte-for-byte. Measured
  (`docs/FIDELITY.md`): on a real charts+pivot workbook xlq preserves **48/50
  parts byte-identical**; on a macro workbook it keeps `vbaProject.bin`
  **byte-identical** — where openpyxl preserves 1/N, drops the VBA, and its
  output often won't even re-open.
- **`xlq restructure` does STRUCTURAL edits** (insert/delete rows/columns) via a
  reference-shift algebra σ applied across every reference-bearing part
  (formulas, cross-sheet refs, defined names, charts, pivots, CF/DV) while
  keeping non-coordinate bytes identical — the *minimal-patch* invariant. It
  either shifts every reference correctly **or refuses with a truthful reason**
  (never silently wrong). Shared formulas are materialized and shifted.
  Measured on **231 real Excel/LibreOffice workbooks**: **78.8% safely edited,
  0 silently corrupted**, and an engine-free round-trip oracle confirms
  **182/182 (100%) preserve every Excel cached value** (`benchmarks/corpus-envelope.md`).
  Against the status quo on a charts+formula fixture: xlq shifts **6/6**
  references correctly; openpyxl `insert_rows` shifts **0/6** — `=B5*2` silently
  reads the blank inserted row.
- **The fidelity property is enforced, not hoped.** Every real `apply`
  re-loads its own output, proves the edited cells landed, and aborts if *any*
  non-edited part changed (`fidelity_violation`).
- **Precondition + preview + receipt.** `apply` checks a `base_hash`, offers
  `--dry-run` (predicts affected cells / new errors / watch values), and
  records a hash-chained receipt (`book.xlsx.xlq.jsonl`) with rev-files and an
  advisory lock.
- **The differential oracle gates the write.** xlq refuses to persist an
  engine-computed cached value for any cell whose formula uses a function its
  own cross-engine oracle found the engine computes wrong (CONVERT, TRIM,
  ROW, SUMPRODUCT…). See `benchmarks/agreement.json`, `docs/AGREEMENT.md`.
- **Read commands (`inspect`, `diff`, `calc`) never write.** `inspect` is a
  privacy-safe census (structure, not content).
- **Non-bypassability, tested adversarially.** In a confined harness where
  xlq is the only reachable write tool, an agent that tried to bypass it could
  not forge or (with the read-only broker deployment) destroy the file —
  `docs/NON-BYPASS.md`, `harness/`.

**Interventional result (`docs/AGENT-AB.md`):** same task, same file, only the
tool varies — the status-quo openpyxl agent produced a corrupt, non-reloadable
workbook it *reported as success*; the xlq-confined agent made the identical
edit with charts/pivots/VBA intact and a receipt.

**Also shipped:** a cross-engine differential oracle that found real bugs in
**both** IronCalc and LibreOffice (`docs/upstream/`), 522/522 Excel-function
catalog coverage with an honest 3-number taxonomy (`docs/COVERAGE.md`), and
the AXLE-bench evaluation suite (`benchmarks/README.md`). Paper:
`paper/paper-v2.md`. Scope: `apply` covers in-place cell/formula edits;
`restructure` covers row/column insert-delete (shared formulas materialized;
array formulas and tables are refused, never silently corrupted — the named
open problem).

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
2. **Writes are surgical and checked.** The read commands (`inspect`, `diff`,
   `calc`) have no write path at all; the one write command (`apply`) touches
   only the cells you name, enforces the fidelity property, and records a
   receipt. There is no bare in-place whole-file write anywhere in the code.
3. **Local-only.** No network calls, no telemetry, no daemon. Plain files in,
   JSON out.

## The read commands: inspect / diff / calc

### `xlq inspect` — privacy-safe workbook census

```
xlq inspect payroll.xlsx
```

```json
{
  "xlq": { "version": "0.1.0", "command": "inspect" },
  "file": { "name": "payroll.xlsx", "bytes": 15311, "sha256": "14e424…" },
  "sheets": [
    { "name": "Attendance", "state": "visible", "rows": 41, "cols": 32,
      "cells": 1312, "formulas": 0, "errors": {} },
    { "name": "Rates", "state": "visible", "rows": 40, "cols": 2,
      "cells": 80, "formulas": 0, "errors": {} },
    { "name": "Payroll", "state": "visible", "rows": 41, "cols": 8,
      "cells": 328, "formulas": 280, "errors": { "#N/A": 3 } }
  ],
  "defined_names": { "count": 0 },
  "functions": { "IF": 40, "MAX": 40, "MIN": 40, "SUM": 40, "VLOOKUP": 40 },
  "unsupported_functions": [],
  "policy_limited_functions": {},
  "volatile_functions": [],
  "ooxml_parts": { "has_vba": false, "has_pivot_cache": false,
                   "has_external_links": false, "has_charts": false,
                   "has_comments": false, "part_count": 19 },
  "coverage": { "engine": "ironcalc 0.7.1+e50ccea8 (vendored master)", "reliable": true }
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
      "old": { "formula": "=SUM(D2:D13)", "value": "412000", "raw": 412000.0 },
      "new": { "formula": null, "value": "415000", "raw": 415000.0 } }
  ],
  "summary": { "changed": 1, "added": 0, "removed": 0, "cached_value": 0,
               "by_sheet": { "Consolidated": { "changed": 1, "added": 0,
                                               "removed": 0, "cached_value": 0 } } }
}
```

Change kinds: `formula` (formula text differs), `value` (non-formula raw value
differs), `cached_value` (same formula, different stored result — a tool
stripped or rewrote formula caches; openpyxl does this on every save, and the
file shows those numbers in Excel until a recalc), `format` (same raw value,
different rendering), `added`, `removed`.

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
  "file": { "name": "branch-consolidation.xlsx", "sha256": "16fbc8…" },
  "changed": [],
  "summary": { "cells": 1274, "formulas": 443, "changed": 0 },
  "truncated": false,
  "coverage": { "engine": "ironcalc 0.7.1+e50ccea8 (vendored master)", "reliable": true,
                "unsupported_functions": [], "policy_limited_functions": {},
                "user_defined_functions": [], "volatile_functions": [] }
}
```

calc loads the file, snapshots every stored value (what Excel last saved),
recomputes, and reports cells where the two disagree — stale caches,
engine/Excel disagreement, or volatile functions. The shipped fixtures are
authored by the engine's own writer, so their caches are consistent and
`changed` is empty, as above (rerun the command to verify, including the
hash). On a workbook with a stale cache each entry carries the location,
the stored and recomputed values, and the formula, e.g.
`{ "sheet": "Consolidated", "cell": "C9", "stored": "182500",
"recomputed": "197300", "formula": "=SUM(…)", "volatile": false }`.
Cells whose formulas call a
volatile function (NOW, TODAY, RAND, RANDBETWEEN, OFFSET, INDIRECT, CELL,
INFO) are flagged `"volatile": true` so expected churn is distinguishable
from real staleness. It never writes the file.

## Reading `coverage`

Every report carries a `coverage` object. `reliable` is `false` whenever the
workbook uses a function the engine cannot evaluate (listed in
`unsupported_functions` — empty for every catalog name today) or a
**policy-limited** function (listed in `policy_limited_functions`, mapping
the name to the documented Excel error literal it returns): WEBSERVICE, RTD,
STOCKHISTORY, DETECTLANGUAGE, TRANSLATE, COPILOT, IMAGE, CALL, REGISTER.ID,
the CUBE family, and GETPIVOTDATA depend on external services or connections
that xlq never contacts, so their stored values cannot be *verified* locally
even though the engine recognizes them and reproduces Excel's offline
behavior exactly. When `reliable` is `false`, treat value-level results
(calc's changed list, future dry-run predictions) as unverified. This is the
"no silent incorrectness" rule made mechanical: the tool reports its own
blind spots instead of papering over them.

## `xlq apply` — the surgical write (v0.2, built)

```
# patch.json: base_hash + typed ops
{ "base_hash": "<xlq inspect file | .file.sha256>",
  "actor": "agent",
  "ops": [ { "type": "set_cell", "sheet": "Sheet1", "cell": "A2", "value": 900 } ] }

xlq apply book.xlsx patch.json --dry-run      # predict, write nothing
xlq apply book.xlsx patch.json --actor agent  # surgical write + receipt
```

`apply` checks the `base_hash` (else `revision_mismatch`), predicts the effect
(dry-run: affected cells, new errors, `watch` before/after), then — for a real
write — surgically edits only the affected sheet parts, re-loads its own output
to prove the edit landed and no other part changed (`fidelity_violation`
aborts, original untouched), writes an immutable `book.rev-N.xlsx`, atomically
swaps it onto the working file, and appends a hash-chained receipt to
`book.xlsx.xlq.jsonl`. It refuses (`coverage_unreliable`) when the affected
formulas use functions the differential oracle flagged as engine-divergent, or
nondeterministic volatiles. See [docs/specs/v02-architecture.md](docs/specs/v02-architecture.md).

## Roadmap

- **Structural edits** (`insert_row`/`insert_column`/`delete`) — the named open
  problem: preserving fidelity while shifting references, calcChain, shared
  strings, and pivot caches. This is the next research + engineering push.
- Ranged/paged reads; BYO-cloud sync (`xlq push/pull` against your own S3
  bucket or git remote — never a third-party tenant).

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

## Engine

xlq links a **vendored clone of IronCalc master at `e50ccea8`**
(`vendor/upstream`), reported in every JSON output as
`ironcalc 0.7.1+e50ccea8 (vendored master)`. The vendored tree additionally
carries a small local patch implementing ENCODEURL, HYPERLINK, and AGGREGATE
(offered upstream; see
[docs/upstream/residual-functions-patch.md](docs/upstream/residual-functions-patch.md)).

Function coverage on this pin, in the three-number form (never quote one
number alone): **522/522 catalog names recognized (100%), of which 505 are
locally evaluable and 17 are policy-limited** — recognized and
argument-checked, but their values depend on an external service, OLAP
connection, PivotTable model, or native code that xlq refuses to execute by
design, so they return the documented desktop-Excel refusal literal
(`#VALUE!`/`#N/A`/`#CONNECT!`/`#BLOCKED!`/`#NAME?`/`#REF!`) and are reported
per-workbook in the census. The per-function table and probe method:
[docs/COVERAGE.md](docs/COVERAGE.md).
Name recognition is not numerical fidelity: for value-level confidence, see
the 1,659-case differential comparison against LibreOffice in
[docs/AGREEMENT.md](docs/AGREEMENT.md) (97.1% agreement where both engines
produce a value — 93.7% counting the one-side-error cases on functions both
engines implement; rows where LibreOffice does not know the function are
classed `lo_unsupported`, no oracle — with every disagreement triaged).

## Build

```
cd xlq
cargo build --release
```

The binary lands at `xlq/target/release/xlq`. Requires a stable Rust
toolchain; the only notable dependencies are the vendored ironcalc, clap,
serde, sha2, and zip.

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

The consolidated suite is **AXLE-bench** (Agent-safe eXcel Layer Evaluation):
five axes — correctness, fidelity, efficiency, agent-ergonomics, catalog —
with a comparison matrix against openpyxl, LibreOffice, excel-mcp-server, and
OfficeCLI. Front page: [benchmarks/README.md](benchmarks/README.md); run it
with `bash benchmarks/run_all.sh`. Narrative reports:
[docs/BENCHMARKS.md](docs/BENCHMARKS.md) (perf/preservation/tokens),
[docs/AGREEMENT.md](docs/AGREEMENT.md) (correctness oracle),
[docs/COVERAGE.md](docs/COVERAGE.md) (catalog).

## License

MIT OR Apache-2.0.
