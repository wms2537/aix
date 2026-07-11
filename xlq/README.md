# xlq

**Agent-safe transactional operations on Excel (`.xlsx`) workbooks.**

`xlq` is a command-line tool for reading and surgically editing spreadsheets
without silently corrupting them. Every command emits machine-readable JSON on
stdout (diagnostics go to stderr), so it is designed to be driven by an agent or
a script. Writes are transactional: they produce an append-only receipt journal,
immutable revision snapshots, and are crash-atomic.

Its guiding property is **never silently wrong** — an operation it cannot perform
faithfully is *refused*, never approximated.

## Install

```sh
cargo install --path .        # from a checkout of this crate
```

This installs a single binary, `xlq`. (The repository's dev/bench tools are gated
behind the non-default `devtools` feature and are not installed.)

> Engine note: `xlq` links a *vendored* build of the [IronCalc](https://github.com/ironcalc/IronCalc)
> calculation engine (provenance `ironcalc 0.7.1+e50ccea8 (vendored master)`) with
> local hardening patches. It is resolved from `../vendor` via `[patch.crates-io]`,
> so building requires that vendored tree to be present. See `NOTICE.md` and
> `../vendor/PATCHES.md`.

## Commands

Read-only:

| Command | What it does |
|---------|--------------|
| `xlq inspect <file> [--redact]` | Privacy-safe census: sheets, formula/function tallies, error counts, unsupported features, file hash. No cell values. |
| `xlq diff <old> <new>` | Cell-level positional diff of two workbooks (values and formulas). |
| `xlq calc <file>` | Headless recalculation, report-only: compares stored vs freshly recomputed values. Never writes. |
| `xlq log <file>` | Print the receipt history from `<file>.xlq.jsonl` (rev, kind, timestamp, hashes, actor, chain linkage). |
| `xlq verify <file>` | Recompute the file hash and check it against the latest receipt + the whole hash chain. Detects out-of-band tampering (exit 1 on failure). |

Transactional writes (produce a receipt + a `<file>.rev-N.xlsx` backup; `--dry-run`
predicts the effect without writing):

| Command | What it does |
|---------|--------------|
| `xlq apply <file> <patch.json>` | Apply a typed patch surgically — rewrites only the sheet parts containing a changed cell; every other OOXML part (charts, pivots, VBA, styles) stays byte-identical. `xlq apply --schema` prints the patch JSON Schema. |
| `xlq restructure <file> --sheet S --op OP --at N [--count K] [--dest D]` | Insert/delete/move rows or columns, shifting every reference (formulas, cross-sheet, defined names, charts, pivots) while keeping non-coordinate bytes identical. |
| `xlq undo <file>` | Transactionally restore the previous committed snapshot (records a new `undo` receipt). Fails closed on a missing/corrupt backup or an out-of-band edit. |
| `xlq certify <original> <edited> --sheet S --op OP --at N …` | Engine-free certification that a foreign edited workbook equals `xlq`'s own proven-faithful structural transform of the original. |

## Exit codes

- `0` — the operation produced its intended effect or answer.
- `1` — an operational refusal or failure (a `certify` REFUSED, a verification
  failure, a fail-closed guard).
- `2` — a malformed invocation (bad `--op`, missing required arguments).
- `70` — an internal error (a caught panic), reported as JSON on stdout.

## The transactional model

A real write to `book.xlsx` produces:

- `book.xlsx.xlq.jsonl` — an append-only journal of receipts
  (`rev`, `base_hash → result_hash`, `kind`, `timestamp`, `actor`, engine
  provenance), each terminated durably.
- `book.xlsx.rev-N.xlsx` — an immutable snapshot of the result of revision *N*.

The write path is lock → hash → fsync → sibling-temp → fsync → atomic-rename →
receipt, so a crash never leaves a torn workbook, and `verify`/`undo`/`log` read
the journal back tolerantly (a single crash-torn trailing line is recovered; any
interior corruption fails loudly).

## Safety limits

Decompression is bounded against zip bombs (512 MiB/part, 2 GiB/workbook by
default; override with `XLQ_MAX_PART_BYTES` / `XLQ_MAX_TOTAL_BYTES`).

## License

Licensed under either of MIT (`LICENSE-MIT`) or Apache-2.0 (`LICENSE-APACHE`) at
your option. See `NOTICE.md` for third-party attribution (IronCalc).
