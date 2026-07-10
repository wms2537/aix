# Local patches to the vendored IronCalc engine

The engine under `vendor/upstream/` is IronCalc pinned at the commit recorded in
the provenance string `ironcalc 0.7.1+e50ccea8 (vendored master)` (see
`xlq/src/calc.rs` ENGINE / `xlq/src/inspect.rs`). The following LOCAL hardening
patches are applied on top of that base — they are why the provenance string says
*vendored*, and must be carried forward on any engine bump.

## 1. Parser recursion-depth guard (security / robustness)

`base/src/expressions/parser/mod.rs`: the recursive-descent formula parser had no
depth limit, so a pathologically nested formula (e.g. `=((((…))))` ~2200+ deep) in
an untrusted `.xlsx` overflowed the process stack and aborted with SIGABRT — a
non-JSON crash reachable from every read-only xlq command. Added a `depth` counter
on `Parser`, bounded by `MAX_PARSE_DEPTH = 256`; past the bound `parse_expr`
returns a `ParseErrorKind` node instead of recursing. Regression tests:
`base/src/expressions/parser/tests/test_depth_guard.rs`.
