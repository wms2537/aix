# Local patches to the vendored IronCalc engine

The engine under `vendor/upstream/` is IronCalc pinned at the commit recorded in
the provenance string `ironcalc 0.7.1+e50ccea8 (vendored master)`. That string is
now defined in exactly one place — `base/src/constants.rs` `ENGINE_PROVENANCE`
(see §2) — and consumed by every xlq site; do not hand-copy it. The following
LOCAL hardening patches are applied on top of that base — they are why the
provenance string says *vendored*, and must be carried forward on any engine bump.

## 1. Parser recursion-depth guard (security / robustness)

`base/src/expressions/parser/mod.rs`: the recursive-descent formula parser had no
depth limit, so a pathologically nested formula (e.g. `=((((…))))` ~2200+ deep) in
an untrusted `.xlsx` overflowed the process stack and aborted with SIGABRT — a
non-JSON crash reachable from every read-only xlq command. Added a `depth` counter
on `Parser`, bounded by `MAX_PARSE_DEPTH = 256`; past the bound `parse_expr`
returns a `ParseErrorKind` node instead of recursing. Regression tests:
`base/src/expressions/parser/tests/test_depth_guard.rs`.

## 2. Single-source engine provenance const (maintainability / integrity)

`base/src/constants.rs`: added `pub const ENGINE_PROVENANCE`, re-exported from
`base/src/lib.rs`, so the engine-provenance string that xlq stamps into every
receipt and report has exactly one definition. Previously the literal
`"ironcalc 0.7.1+e50ccea8 (vendored master)"` was hand-copied into 7 xlq sites
(`calc.rs`, `apply.rs`, `journal.rs`, `inspect.rs` ×2, `bin/oracle_compare.rs`,
`tests/integration.rs`); any drift would have silently falsified receipts.

The const is composed at compile time as
`concat!("ironcalc ", env!("CARGO_PKG_VERSION"), "+e50ccea8 (vendored master)")`.
The version segment is derived from **this crate's** `Cargo.toml` (`0.7.1`), so it
is structurally impossible for it to disagree with the linked engine — xlq's own
`CARGO_PKG_VERSION` is `0.1.0` and cannot supply it, which is precisely why the
const must live in `ironcalc_base`, not in an xlq module (the dev `[[bin]]`
targets share no xlq library crate and reach it only through the engine dependency).

The commit hash `e50ccea8` and the `(vendored master)` tag are the only maintained
parts: the vendored tree has no ironcalc `.git` and `base/build.rs`'s `git describe`
resolves to the parent `aix` repo, so the hash genuinely cannot be build-derived.
**On any engine bump, update the hash/tag in `base/src/constants.rs` (one place)**;
the version segment updates itself from `base/Cargo.toml`. The base-crate test
`engine_provenance_tracks_crate_version` pins the exact output.
