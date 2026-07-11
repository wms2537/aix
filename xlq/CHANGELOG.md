# Changelog

All notable changes to `xlq` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0]

### Added
- New read/recovery verbs over the transactional journal:
  - `xlq log <file>` — receipt history with per-entry chain-linkage.
  - `xlq verify <file>` — recompute the file hash vs the latest receipt and walk
    the whole hash chain; detects out-of-band tampering (exit 1 on failure).
  - `xlq undo <file>` — transactionally restore the previous committed snapshot,
    failing closed on a missing/corrupt backup, no prior snapshot, or an
    out-of-band edit.
- `xlq apply --schema` prints the patch format's JSON Schema (no file needed).
- Decompression caps against zip bombs (512 MiB/part, 2 GiB/workbook by default),
  overridable via `XLQ_MAX_PART_BYTES` / `XLQ_MAX_TOTAL_BYTES`, enforced on every
  xlq-controlled read plus a preflight before the engine loads an untrusted file.

### Changed
- **Security:** upgraded `quick-xml` 0.36 → 0.41, clearing two HIGH RUSTSEC
  advisories (RUSTSEC-2026-0194, -0195). Formula bodies are now reassembled across
  the `Text`/`GeneralRef` events quick-xml ≥0.38 emits, so entity-bearing formulas
  (`A5&gt;0`) shift correctly instead of being silently corrupted.
- Uniform exit-code contract: `0` effect/answer, `1` refusal/failure, `2` bad
  invocation, `70` internal error. `certify` REFUSED now exits `1` (was `0`).
- Engine-provenance string is single-sourced from the vendored engine
  (`ironcalc_base::ENGINE_PROVENANCE`) so it can never drift from the linked build.
- The receipt journal recovers a single crash-torn trailing line and fails loudly
  on interior corruption (previously a torn tail wedged every future write).

### Robustness
- A panic in any command becomes a machine-readable JSON error (exit 70) with a
  path-safe, basename-only source location, instead of a raw multi-line dump.
- `SIGPIPE` is restored to its default disposition, so `xlq … | head` dies cleanly
  instead of panicking on a closed pipe.
- A recursion-depth guard in the vendored formula parser turns a pathologically
  nested formula into a parse error instead of a stack-overflow abort.

### Packaging
- The crate is now self-contained: the Excel function catalog moved to
  `data/excel-functions.txt` and the test fixtures into `tests/fixtures/`, so it
  builds and tests without reaching outside its own directory.
- The five dev/bench binaries are gated behind a non-default `devtools` feature, so
  `cargo install` ships only the `xlq` binary. The full test suite runs under
  `cargo test --features devtools`; a bare `cargo test` runs unit + direct-fixture
  tests.
- Added crate metadata (README, keywords, categories, license/notice copies).

### Publishing
- The engine dependency is now wired for crates.io: the vendored IronCalc fork is
  consumed as renamed publishable packages (`xlq-ironcalc`, `xlq-ironcalc-base`,
  lib names unchanged) via the multiple-locations `path` + `version` pattern, so
  no `[patch.crates-io]` is needed and a published `xlq` links the correct engine.
  `cargo publish --dry-run` is green for the leaf `xlq-ironcalc-base`. Publishing
  the three crates (a permanent, account-scoped action of republishing a
  third-party engine fork) is left to the maintainer — see `PUBLISHING.md` for the
  exact bottom-up sequence. Local dev/install is unchanged (resolves from
  `../vendor`).

## [0.1.0]

- Initial release: read-only `inspect`, `diff`, `calc`, and the first
  transactional `apply` / `restructure` / `certify` write path with receipts.
