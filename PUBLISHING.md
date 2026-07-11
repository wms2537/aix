# Publishing xlq to crates.io

`xlq` links a **vendored, locally-hardened fork of IronCalc** (provenance
`ironcalc 0.7.1+e50ccea8 (vendored master)` plus the patches in
`vendor/PATCHES.md`). Because `cargo publish` strips `[patch.crates-io]`, a
published `xlq` cannot depend on the upstream `ironcalc 0.7.1` (that would link a
*different* engine than the one it was built and certified against). The engine
fork must therefore be published under its own names first.

The crates are already wired for this (multiple-locations `path` + `version`
deps; no `[patch]`):

| Package (crates.io name) | `[lib] name` (unchanged) | Directory |
|---|---|---|
| `xlq-ironcalc-base` | `ironcalc_base` | `vendor/upstream/base` |
| `xlq-ironcalc` | `ironcalc` | `vendor/upstream/xlsx` |
| `xlq` | `xlq` | `xlq` |

The `[lib]` names stay `ironcalc_base` / `ironcalc`, so **no source changes** —
`use ironcalc::…`, `use ironcalc_base::…`, and the `ENGINE_PROVENANCE` const are
all unaffected.

## Prerequisites

- A crates.io account and an API token: `cargo login <token>`.
- The three crate names must be available (or already owned by you):
  `xlq-ironcalc-base`, `xlq-ironcalc`, `xlq`.
- You are (re)publishing a fork of a third-party MIT/Apache project. Attribution
  is preserved (original author retained in `authors`, `NOTICE.md`, and
  `vendor/PATCHES.md`). This is permitted by IronCalc's license; it is **your**
  decision to make these forks public and permanent.

## Publish in dependency order

Each `cargo publish` is irreversible (versions can be yanked but not deleted).
Publish bottom-up; wait for each to appear on crates.io before the next (the next
crate's verify-build resolves the previous one from the registry).

```sh
# 1. the engine base
cd vendor/upstream/base
cargo publish                      # xlq-ironcalc-base v0.7.1

# 2. the xlsx engine layer (depends on xlq-ironcalc-base)
cd ../xlsx
cargo publish                      # xlq-ironcalc v0.7.1

# 3. the CLI (depends on xlq-ironcalc)
cd ../../../xlq
cargo publish                      # xlq v0.2.0
```

Verify each step offline first with `cargo publish --dry-run` (already confirmed
green for `xlq-ironcalc-base`; the later two can only dry-run once their
dependency is live).

## Notes

- **Version scheme.** The forks keep version `0.7.1` under their new names (a
  fresh namespace, so the number is free). The `+e50ccea8 (vendored master)`
  build metadata in the provenance string records the exact upstream commit.
- **`xlq-ironcalc` ships a `test` bin** (`src/bin/test.rs`, inherited from
  upstream). It is harmless but you may wish to remove or gate it before
  publishing the fork.
- **Local development is unaffected.** `cargo build` / `cargo test --features
  devtools` / `cargo install --path xlq` all resolve the engine from
  `vendor/upstream/*` via the local `path`; the crates.io `version` is used only
  when publishing.
