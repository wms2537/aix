# PR guide — upstreaming our IronCalc function implementations

This is a checklist for opening a PR against `ironcalc/IronCalc` with the
functions we implemented on top of master. It says exactly which files to
include, which to leave out, and the mechanical steps to produce a clean branch.

## (a) The patch

- Location: `docs/upstream/ironcalc-changes.patch`
- Size: **6,779 lines**, a unified diff against pristine upstream master
  `e50ccea8` (the commit our `vendor/upstream` clone is pinned to).
- The patch is the full delta of our vendored tree. It is **not** all
  upstream-worthy as-is — it also contains xlq-specific policy code (see (c)).
  Use it as the source of truth for the implementation diffs; cherry-pick per
  the file list below rather than applying it wholesale.

Every function below is specified in `docs/specs/full-catalog-semantics.md`
(the "full catalog semantics" spec), which records the Excel-documented edge
behavior and confidence level ([P] = Microsoft primary doc, [S] = named
secondary, [U] = implementation-defined) for each. The PR should cite it.

## (b) Upstream-worthy additions (genuinely implemented, with tests)

These are real implementations with Excel-consistent semantics and passing
tests — the parts a maintainer actually wants.

New functions:

| Function(s) | Source file |
|---|---|
| FILTERXML | `base/src/functions/filterxml.rs` |
| EUROCONVERT, DBCS, JIS, BAHTTEXT, PHONETIC | `base/src/functions/euro_dbcs.rs` |
| GROUPBY, PIVOTBY | `base/src/functions/groupby_pivotby.rs` |
| AGGREGATE (all 19 function_nums, full options bitmask) | `base/src/functions/aggregate.rs` |
| ENCODEURL | `base/src/functions/text/encodeurl.rs` |
| HYPERLINK (value semantics) | `base/src/functions/lookup_and_reference/hyperlink.rs` |

Statistical fixes (touch existing files — the mode / percentile / quartile
corrections that also back AGGREGATE's 13/16/17/18/19 forms):

- `base/src/functions/statistical/mode_functions.rs` (`find_modes` made
  `pub(crate)` for AGGREGATE 13)
- `base/src/functions/statistical/percentile.rs` (`percentile_exc_impl`
  extracted and reused by PERCENTILE.EXC / QUARTILE.EXC / AGGREGATE 18/19)
- `base/src/functions/statistical/quartile.rs`
- `base/src/functions/statistical/mod.rs`

Tests (six new files, one per feature area):

- `base/src/test/test_fn_filterxml.rs`
- `base/src/test/test_fn_euro_dbcs.rs`
- `base/src/test/test_fn_groupby_pivotby.rs`
- `base/src/test/test_fn_aggregate.rs`
- `base/src/test/test_fn_encodeurl.rs`
- `base/src/test/test_fn_hyperlink.rs`

Plus the registration/plumbing each new function needs (all mechanical):
`base/src/functions/mod.rs` (enum variants, lowercase lookup, dispatch arms,
`into_iter`), `base/src/expressions/parser/static_analysis.rs` (arg signatures),
`base/src/functions/text/mod.rs` and
`base/src/functions/lookup_and_reference/mod.rs` (module decls),
`base/src/functions/subtotal.rs` (`cell_hidden_status` shared with AGGREGATE),
`base/src/test/mod.rs`, and the localization set
(`base/src/language/mod.rs`, `generate_language/src/languages.json`,
`generate_language/src/main.rs`, regenerated `base/src/language/language.bin`).

## (c) EXPLICITLY EXCLUDE from any upstream PR

**Do not include `base/src/functions/policy_limited.rs` or its registrations.**

That file holds 17 "policy-limited" functions — CUBE*, WEBSERVICE, RTD,
STOCKHISTORY, and the other external-execution names — that validate their
arguments and then deliberately return the exact error desktop Excel produces
when the external work cannot happen (`#VALUE!` / `#N/A` / `#NAME?` /
`#BLOCKED!` / `#REF!`). That is **xlq's security policy** — we refuse network
fetches and XLM/native execution by design — and it is the *opposite* of what a
spreadsheet engine should do. A maintainer wants real implementations, not
deliberate-error stubs; adopting these would make IronCalc worse, not better.

Leave out, together:

- `base/src/functions/policy_limited.rs`
- its module declaration and every dispatch/enum/signature/localization entry
  that only exists to wire up those 17 names
- `base/src/test/test_fn_policy_limited.rs` (if present)

When cherry-picking by file (below), simply never stage `policy_limited.rs`,
and drop the enum variants / dispatch arms that reference it. Everything in (b)
is independent of it.

## (d) Mechanical steps

1. **Fork** `ironcalc/IronCalc` on GitHub.
2. **Branch** from the base our patch was cut against:
   ```
   git checkout -b feat/catalog-functions e50ccea8
   ```
   (Or branch from current `main` and rebase — the diffs are localized to the
   files in (b) and should apply with minor context drift at most.)
3. **Bring in only the implementation files.** Easiest path: apply the patch,
   then unstage the excluded policy code —
   ```
   git apply --3way docs/upstream/ironcalc-changes.patch
   git checkout -- base/src/functions/policy_limited.rs        # drop the stub file
   # then remove the policy_limited module decl + its enum/dispatch/signature
   # /localization entries by hand (they are self-contained, grep policy_limited)
   ```
   Or cherry-pick file-by-file from `vendor/upstream` using the (b) list. Either
   way the goal is: every file in (b), none of the (c) policy wiring.
4. **Regenerate the language binary** if you touched localizations:
   `make test-language-bin` (or run the `generate_language` tool) so
   `base/src/language/language.bin` matches `languages.json`.
5. **Test:**
   ```
   cargo test -p ironcalc_base
   ```
   Our tree reports **2129 passed, 0 failed** (plus 23 doc-tests) with these
   changes. Also run `cargo fmt -- --check` and the upstream lint
   (`cargo clippy -p ironcalc_base --all-targets --all-features -- -W
   clippy::unwrap_used -W clippy::expect_used -W clippy::panic -D warnings`),
   both clean in our tree.
6. **Open the PR**, citing `docs/specs/full-catalog-semantics.md` for the
   per-function semantics decisions and the differential-test evidence
   (`benchmarks/agreement.json`) for the behaviors we validated against Excel.

## Suggested PR title + description

**Title:**
`Add FILTERXML, EUROCONVERT/DBCS/JIS/BAHTTEXT/PHONETIC, GROUPBY/PIVOTBY, AGGREGATE, ENCODEURL, HYPERLINK (+ statistical fixes)`

**Description:**

> Adds eleven locally-evaluable Excel functions that were unrecognized on
> master, plus the mode/percentile/quartile fixes they depend on. Each function
> follows the existing conventions (dispatch, `static_analysis` signatures,
> `_xlfn.` xlsx serialization where applicable, localized names for all five
> languages) and ships with tests.
>
> **New functions**
> - FILTERXML — local XPath-1.0-subset over a string; multiple matches spill
>   vertically; `#VALUE!` on invalid xml/xpath.
> - EUROCONVERT — the 14 legacy-currency legal rates, legacy→legacy
>   triangulation through EUR, triangulation-precision rounding.
> - DBCS / JIS — half-width → full-width (ASCII and half-width katakana,
>   composing voiced marks).
> - BAHTTEXT — Thai money-text algorithm.
> - PHONETIC — furigana-run concatenation, falling back to the cell text.
> - GROUPBY / PIVOTBY — pure data aggregation (no PivotTable object), lambda
>   or eta-reduced aggregation with headers/totals/sort/filter options.
> - AGGREGATE — all 19 function_nums and the full 0–7 options bitmask,
>   including ignore-hidden-rows (reusing SUBTOTAL's row-visibility machinery)
>   and ignore-nested-SUBTOTAL/AGGREGATE.
> - ENCODEURL — RFC 3986 percent-encoding over Excel's unreserved set.
> - HYPERLINK — value semantics (returns the friendly name / location text),
>   matching the engine's current no-link-object model.
>
> **Fixes** to mode / percentile / quartile so PERCENTILE.EXC, QUARTILE.EXC and
> the AGGREGATE 13/16–19 forms share one correct implementation.
>
> **Semantics** for every function (including the edge cases and the confidence
> level for each decision) are documented in the linked full-catalog-semantics
> spec. The AGGREGATE hidden-vs-filtered-row decision and the ENCODEURL
> unreserved-set divergence from LibreOffice (Excel does not encode `.`/`~`)
> are the two calls worth reviewing.
>
> **Validation:** `cargo test -p ironcalc_base` → 2129 passed; fmt + the
> `make lint` clippy flags clean. Cross-checked against LibreOffice on a shared
> workbook (differential-testing harness); every difference is either an
> intended Excel-match or a documented LO divergence.
>
> Not included: our xlq-specific policy-blocked functions (CUBE*, WEBSERVICE,
> RTD, STOCKHISTORY, …), which deliberately return Excel's external-failure
> errors and are a security-policy choice, not an engine feature.
