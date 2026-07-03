# NOTICE

## This project (xlq, AXLE-bench, the paper and specs)
Copyright the xlq authors. Licensed under **MIT OR Apache-2.0** (see
`LICENSE-MIT`, `LICENSE-APACHE`).

## Vendored dependency: IronCalc
`vendor/upstream/` contains a vendored copy of **IronCalc**
(https://github.com/ironcalc/IronCalc), upstream master commit `e50ccea8`,
**modified by this project** (Tier-I/residual function implementations —
FILTERXML, EUROCONVERT, DBCS/JIS, BAHTTEXT, PHONETIC, GROUPBY, PIVOTBY,
ENCODEURL, HYPERLINK, AGGREGATE — plus the policy-limited function stubs).
IronCalc is licensed under MIT OR Apache-2.0; its original license files are
preserved at `vendor/upstream/LICENSE-MIT` and
`vendor/upstream/LICENSE-Apache-2.0`. Our modifications are offered back
upstream (see `docs/upstream/`). No affiliation or endorsement by the
IronCalc project is implied.

## Test fixtures
`fixtures/*.xlsx` are synthetic, generated deterministically by
`cargo run --bin xlq-fixtures`; they contain no real or personal data.
