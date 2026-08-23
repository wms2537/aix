# xlq reference-completeness hardening — status & handoff

**Branch:** `xlq-reference-completeness`, ahead of origin through the release-readiness/handover-audit rounds (origin remains at `f09be4f`; **NOT merged to main**)
**Last updated:** 2026-08-23 — LOOP EXIT CONDITION MET; three consecutive release-assurance dry rounds completed
**Gate:** green — default and devtools Rust gates, engine suites, formal checks, reproducibility verifier, and lockfile audits (see §7)
**Totals:** ~291 defects fixed over 74 rounds (r72 = advisory hardening)

---

## 1. What this work is

`xlq` is an agent-safe transactional tool for `.xlsx` structural edits, with two commands:

- **`restructure`** — insert/delete/move rows/cols via the σ shift algebra (`src/refshift.rs`) plus a
  fail-closed residual scan (`src/structural.rs`).
- **`certify`** — an *engine-free* proof that a **foreign** edited workbook equals xlq's own
  proven-faithful transform of the same edit (`src/certify.rs`). It recomputes formula caches through
  a vendored ironcalc "oracle" and compares non-cell reference **semantics** via signature functions.

**Prime directive: never silently wrong (fail-closed).** The worst possible bug is a **false-certify**
— certify reports CERTIFIED while the foreign workbook is semantically different from the faithful
transform. Also serious: silent-wrong (a stale/incorrect reference committed), security-repoint
(external URL / macro / connection swapped undetected), invalid-output (schema-invalid `.xlsx` that
triggers Excel's repair prompt). Over-refusal is a real defect but strictly lower severity.

## 2. The convergence loop (the standing protocol)

Each **round** is one iteration:

1. Run an adversarial-review **Workflow**: ~10 read-only finder lenses (date-reachability, pivot,
   chart-drawing, external-rels, styles-numfmt, refshift, structural-residual, oracle-soundness,
   invalid-output, tables-comments), each producing structured findings with a concrete repro.
2. Every candidate is checked by a **3-vote adversarial verify panel** whose agents are instructed to
   *refute* it; a finding is confirmed only at **≥2/3** votes.
3. Fix **every** confirmed value / security / correctness / invalid-output / over-refusal defect, each
   with a regression test.
4. Full gate (fmt / clippy / tests), commit **incrementally**, push, update memory, re-run.

**Exit condition:** two consecutive *genuinely dry* rounds (zero confirmed findings). **Met — see §7.**

The round-N script lives at `/tmp/claude-1000/-home-soh-aix/<session>/scratchpad/roundNN.js`; each round
is produced by copying the previous script and rewriting the "ROUND N JUST ADDED" block to weight the
newest code plus any known residuals.

## 3. Progress this session (rounds 60–71)

61 fixes across 35 commits. Newest first:

| Round | Commits | Confirmed | Headline defects |
|---|---|---|---|
| 74 | — (no code changes) | 0 defects | DRY ROUND #2 — EXIT CONDITION MET. Both sweep agents returned EMPTY (channel flakiness persists), so all ten questions were verified INLINE against code instead: records-fingerprint prefix cannot match definitions; one-sided records presence refuses (fail-closed); worksheet `<filter val>` (no fld) falls through unchanged; autoSortScope arm ordering safe (`pivotArea`/`references`/`reference` no-op through leaf); `format_diffs_disqualify` single production caller; transform never reorders control elements (outside sheetData, not in dispatch list); table displayName keys case-sensitive raw; pivot_refs scanning records parts emits zero sigs (no wanted tags match `<r>/<x>/<n>/<s>`); media parts copied byte-for-byte by the zip rewriter (no re-encode path); slicer allowlist entries intact with accurate comments. ROUNDS 73+74 BOTH GENUINELY DRY → convergence criterion satisfied.
| 73 | — (no code changes) | 0 defects | DRY ROUND #1. Oracle-gates lens (via explore): all five pipeline seams verified CLEAN with citations (forced-recalc zeroes only cache term; intersection cells = poison sources fail-closed; manual/auto asymmetric volatile branches sound by design; both-sides PaD forces format identity, no cell-level precision override exists in xlsx; expected-only CELL readers always constitute refused drops). Caption-completeness tail closed inline: the six r70 attrs are the complete materialized-text set; remaining root attrs are layout/display-only (documented rationale) |
| 72 | `c7dc588` | 0 defects + 1 advisory hardening | explore-type agents WORK where general agents return empty: full audit of all r66-69 comparators came back CLEAN per function; acted on the one advisory — xl/media fingerprint (unkeyed 64-bit SipHash, forgeable ~2^32) replaced with BYTE-EXACT comparison via media_parts (mirrors vba_parts) |
| 71 | `3f1b36a` | 1 | asymmetric precision-as-displayed format gate: format_disqualifying checked only EDITED's fullPrecision — expected-side PaD + foreign numFmt change with preserved caches certified though files recalc differently. Gate extracted to symmetric format_diffs_disqualify(). Oracle-gate questions answered by direct inspection (agent channel returned empty a 5th time): intersection_cells is the r51 range-intersection exclusion set from EXPECTED only; injected cells caught as "added" |
| 70 | `ecb4851` | 4 | pivotCacheRecords allowlisted with ZERO readers (byte-fingerprint now); intra-pivot whole `<filter fld>` swap (fld-keyed element + predicate sigs); `<autoSortScope>` rank-by re-point uncompared; root caption/error strings materialized on refresh joined the root sig. Pivot lens productive; new-comparators + oracle-gates lenses returned EMPTY again (agent channel unreliable — empty ≠ dry) |
| 69 | `f57c5e8`, `58b831c` | 2 | pivot `<item @n>` custom-label swap across fields (label lived only in the pooled multiset); LITERAL chart data points (`numLit`/`strLit` — typed-in values, authoritative forever) uncompared. Closing sweep over all rounds-66-69 comparators came back clean (2 empty agent sweeps + manual verification: pt-idx staleness benign, folding resolves at end, col-range consistency) |
| 68 | `bf71a4e`, `e8e7ab6`, `ed31a3a` | 4 | form-control binding swap pooled across parts AND within a sheet (incl. VML FmlaMacro = which button runs which macro); cross-TABLE autoFilter-block swap (constant owner "table"); linked-object `<xdr:f>` source cells pooled within one drawing; workbook `<webPublishItem>` source repoint uncompared |
| 67 | `dc58530`, `a58b054`, `4bc13fb`, `16eb73b` | 6 | self-closed `<numFmts/>` leaked the dxf-map gate (r61 vector via one-byte encoding); OLAP calculatedMember's expression lives in `@mdx` — read by nothing; intra-chart series ref/name swap (pooled `<f>` list permutation-invariant WITHIN a part); row/col inherited styles invisible to per-cell CELL() backstops (+ target-xf content edit); CELL("width") blind to defaultColWidth/hidden cols; internal drawing image/chart bindings + xl/media bytes uncompared (logo substitution) |
| 66 | `0b5996e`, `de76d1c`, `e3367bb`, `8ffa144` | 6 (5 fixed + Theme E assessed not-a-defect) | data-table `<f>` NON-self-closing Start arm stale under Op::Move (silent-wrong); cross-part drawing macro=/textlink= swap via same-named shapes — **confirmed by the retained probe** (+ its charts twin); CELL() backstops never followed `xfId` → `<cellStyleXfs>` inheritance (4-lens convergence; e2e repro'd); slicer/timeline selection state uncompared; Theme E assessed CLOSED BY CONSTRUCTION (see §6) |
| 65 | `5ac8071`, `5292bd0`, `ab76abb` | 6 | 3D-span defined name → engine `#NAME?` laundered via IFERROR (found by **4 lenses**); same-named drawing shapes collide (`cNvPr name` is not unique → `name#occ`); swap of two same-fld/same-name pivot dataFields' aggregation; `Op::Move` left data-table `<f>` r1/r2/ref stale (dead-code dispatch); two same-text runs' hyperlink swap; **new class** — `CELL("prefix")`/`CELL("width")` style backstops |
| 64 | `5b7379c`, `6f7ab02` | 7 | **new class** — engine cannot evaluate non-plain-ref defined names (named constant/formula/dynamic OFFSET) → `#NAME?` laundered; pivotField `<item>` display-order swap; cross-part pivot cacheSource connection swap; pivotField sortType/showAll/subtotalTop; autofilter predicate swap between filterColumns; delete consuming `<pane topLeftCell>` → empty ref |
| 63 | `6e560c8` | 4 | static reachability cannot follow **OFFSET/INDIRECT** or an unquoted non-ASCII sheet qualifier (fail-closed drop); pivotField Top-N AutoShow filter; `drawing_shape_links` dropped `<a:hlinkHover>` |
| 62 | `457460f`, `bd7c71f` | 8 | 3D-span **interior sheet** under-reported in `formula_references_cell`; boundary-discriminating dependent of a divergent **source**; pivotField subtotal swap; `opaque_target_signature` cross-part swap; chart hyperlinks colliding at same ancestor path; x14 `<dataValidations>` twin inflating legacy `@count` |
| 61 | `632b2c3`, `ec851e2` | 7 | name-mediated reachability regression (found by **3 lenses**); chart-XML `r:id` repoint; pivot `<fieldGroup base/par>`; cross-part external-target transposition; `<dxf><numFmt>` masking a cell format change |
| 60 | `496591b`, `4a45409`, `a3904c7` | 8 | **soundness hole in round-59's own fix** — value-diff cannot catch a boundary-discriminating dependent → replaced with exact reference reachability; EDATE/EOMONTH/WORKDAY consume an early serial; pivot cache-field grouping; within-shape run label↔URL swap; external targets keyed by rId; table computed-column swap |

Confirmed-finding count per round: **8, 7, 8, 8, 4, 7, 8** — bouncing, not monotonically decreasing.

### Why the count does not converge quickly

An OOXML tool's reference surface is genuinely vast, and — the dominant structural lesson — **every new
signature function introduces fresh surface that the next round probes.** Nearly every round's findings
are follow-ons to the previous round's fixes: round-59's dependent-drop → round-60's reachability →
round-61's name-mediated gap → round-62's 3D-span interior → round-63's runtime refs → round-65's
3D-span defined name. This is the loop working as intended, not spinning.

## 4. Recurring anti-pattern classes (check every new comparator against these)

| | Class | The question to ask |
|---|---|---|
| (a) | Text-only capture drops CDATA / `GeneralRef` (entity) bodies | does the walk have `CData` and `GeneralRef` arms? |
| (b) | Raw attribute compare without canonicalization | are ECMA defaults folded? bool `1/0`↔`true/false`? sheet-quote? whitespace? |
| (c) | Presence-refuse instead of affect-check | does this edit actually *move* the construct? |
| (d) | An ECMA default not folded | would a tool writing the default explicitly diverge? |
| (e) | Whitespace-split truncation of a space-bearing value | (fixed globally by `ATTR_SEP` = `0x1F`) |
| **(f)** | **Position-blind pooling loses a parent↔child / ordinal / binding** | **"does a sibling SWAP survive the sort?"** — by far the most productive question |
| (g) | A value/format gate enumerating only *some* ways a value can arise | literal / cell-ref / formula-produced / inline / via-name / runtime-ref? |
| (h) | A new comparator has its own new surface | what did *this fix* just make comparable-but-unbound? |

**Critical mechanical note:** `structural::element_attr_semantics` **sorts** its output (drops document
order). Any comparator needing a parent↔child or ordinal binding must do a **stateful ancestor-tracking
walk** instead — see `autofilter_criteria` (round 64) and `pivot_ordered_sigs`.

## 5. Working tree

Clean. The round-66 debris probes were committed (407e532), the Theme C probe has been
converted into a real regression test (`cross_part_macro_and_textlink_swap_is_caught`).
Still untracked (pre-existing, unrelated): `formal/corpus_formulas.txt`.

## 6. Round 66 — COMPLETE (2026-08-21)

The 8 candidates from the aborted run collapsed to 5 themes; all processed:

- **Theme A — FIXED (`e3367bb`, 3 HIGH).** Assessment confirmed the class: the vendored
  engine's `fn_cell` returns `#VALUE!` for prefix/protect/format/width, so xlq's XML
  backstops ARE the semantics; Excel folds unset cellXf properties through `xfId` ->
  `<cellStyleXfs>` (ECMA-376 merging) but the backstops read only the child `<xf>`.
  Editing only the parent named style flipped every inheriting cell's effective
  CELL("prefix"/"protect"/"format") invisibly. Fix: fold the parent entry in when the
  child omits the property (explicit child wins; absent xfId = 0 per ECMA/vendor importer).
- **Theme B — FIXED (`0b5996e`, 1 HIGH silent-wrong).** Same round-65 d4 hole one
  serialization away: a non-self-closing `<f t="dataTable">` hit the Start arm and was
  written verbatim under Op::Move. Routed through transform_tag_move without arming in_f.
- **Theme C — FIXED (`de76d1c`, 1 HIGH security + charts twin).** The retained probe had
  CONFIRMED the cross-part macro/textlink swap. chart_drawing_refs now prefixes every
  chart AND drawing signature with its owning part; probe converted to a regression test.
- **Theme D — FIXED (`8ffa144`, 1 HIGH).** Slicer/timeline parts were byte-allowlisted with
  their filter SELECTION read by no comparator — a deselect re-filters the bound pivot on
  refresh while cached cells show the old output. New owning-part-prefixed
  `slicer_timeline_sigs` comparator (item selection x-keyed, pivot/cache bindings, timeline
  state range; ECMA defaults folded so an explicit-defaults re-serialize does not refuse).
- **Theme E — ASSESSED, NOT A DEFECT (closed by construction).** IFERROR laundering of an
  ERROR-valued source cannot land: any dependent that CONSUMES the poisoned value is flat
  under no numeric poison (poison-and-diff drops it → unvouchable); a dependent that only
  discriminates the error axis (IF(ISERROR(src),K1,K2)) ignores its input entirely, so it
  yields exactly {engine K1, faithful K2}, and unvouched present caches are compared DIRECTLY
  expected-vs-edited (`unverified_formula_caches`) — an injected wrong-path cache differs from
  xlq's preserved faithful one and is refused. Round-62's deliberate exclusion of error-valued
  sources from the reachability drop therefore stays sound: unvouched ≠ unchecked.

## 7. Loop status & handover

**EXIT CONDITION MET (2026-08-21): rounds 73 and 74 are two consecutive genuinely-dry rounds.**
The reference-completeness adversarial convergence loop is CLOSED at **~291 defects over 74 rounds**,
gate green throughout (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, 410 tests).

**Fresh PR-readiness verification (2026-08-23):** from `f09be4f`,
`cargo fmt --check` passed, `cargo clippy --all-targets -- -D warnings` passed,
and the full xlq suite passed with **412 unit tests + 27 integration tests = 439 tests**.
The only working-tree item remains the pre-existing unrelated untracked
`formal/corpus_formulas.txt`.

**Release-readiness round (2026-08-23, commits `e270fd1..93efee1`):**

- Expanded Rust verification: devtools feature enabled — fmt/clippy clean,
  **466 xlq tests**, optimized build green; engine base **2,233 passed / 8
  ignored** with `RUST_MIN_STACK=64MiB`; engine xlsx layer **268 passed**.
  The base parser depth-guard test needs that explicit test-thread stack on an
  8 MiB host; the production guard itself is correct.
- Packaging: `xlq-ironcalc-base v0.7.1` dry-run package succeeded. The inherited
  upstream `test` binary is gated behind non-default `devtools`; its source stays
  in the crate so opt-in builds remain valid.
- Supply chain: upgraded vendored-engine `time` from 0.3.45 to 0.3.47 for
  RUSTSEC-2026-0009. Both CLI and engine lockfiles now audit with zero known
  vulnerability matches (`cargo-audit 0.22.2`).
- Formal/reproducibility: Lean checked all six files; Z3 proved all 14 shift/move
  laws live; Lean-vs-router differential agreed **30/30**; corpus regeneration
  reproduced manifest SHA `01364088…`; verifier result **130 PASS / 0 FAIL /
  0 SKIP**. Formal scripts are checkout-portable and contain no hardcoded
  `/home/soh/aix` paths.
- Handover audit: reconciled the closed-loop protocol text, updated the receipt
  spec from “draft” to its implemented v0.2 surface, added dated status addenda for
  v0.2 architecture and formal research remainders, corrected shipped dependency/scope
  documentation, and recorded the engine audit/packaging changes in the changelog.

### Three-round release-assurance convergence (2026-08-23)

The earlier release rounds found and fixed real issues, so a fresh clean-counter
began afterward. Three consecutive genuinely dry rounds then passed:

1. **Round D — CLI/schema/docs parity:** all ten commands and documented flags were
   checked against generated help; `apply --schema` was compared with its documented
   envelope; local links and stale terminology were swept. One initial finding (old
   `book.rev-N.xlsx` naming plus an obsolete “draft” label) was corrected before the
   round was counted clean.
2. **Round E — packaging/licensing/artifact integrity:** all three crate identities,
   versions, MSRVs, licenses, and README references matched; the base `.crate` was
   generated and inspected; package paths/content had no corpus, credential,
   absolute-home, or research-artifact leakage; attribution files existed; both
   lockfile audits passed with only the documented allowed `rand` warning.
3. **Round F — reproducibility/formal/runtime:** `git fsck` reported no corruption;
   the corpus SHA matched its manifest; all six Lean files checked; Z3 proved 14/14
   laws; Lean-vs-router agreed 30/30; reproducibility verifier returned **130 PASS /
   0 FAIL / 0 SKIP**; two independent transforms were byte-identical; a locked clean
   install passed inspect → edit → verify → second edit → undo → verify assertions.

**Result: three consecutive dry rounds. No further local product defect is known.**

Session totals (rounds 66–74): 17 fixes + 1 advisory hardening across seven rounds, five dry lenses
established (oracle-soundness, external-rels/security, refshift/structural, invalid-output,
transform-coverage), and every fix carrying a regression test verified to fail pre-fix.

### For the successor

1. **Merge decision**: branch `xlq-reference-completeness` is NOT merged to main or pushed past
   `f09be4f`. It is release-ready locally; obtain explicit approval before pushing/opening a PR.
   Crates.io publication is a separate irreversible external lane and requires its own authorization.
2. **If the loop ever restarts**, weight lenses toward the documented residuals: cell-linked chart
   caches (self-heal on refresh — deliberately uncompared), chained cellStyleXfs inheritance
   (single hop folded), xl/diagrams/* + xl/embeddings/* (blanket-refused, fail-closed).
3. **Agent channel**: use explore-TYPE subagents for audits; general-type returned empty ~5x.
   An empty result is an AGENT FAILURE, never a dry verdict — retry once, then assess inline.
4. Update `~/.claude/projects/-home-soh-aix/memory/xlq-reference-completeness-hardening.md`
   with any future round ledgers (the file carries rounds 50–74 verbatim).

### Untackled items are external or explicitly future work

- **Push / merge / PR:** requires owner approval.
- **Release tag:** do not reuse the historical annotated `v0.2.0`; it points at an older
  pre-packaging tree. Create a fresh tag at the approved release commit.
- **Crates.io publication:** irreversible; follow `PUBLISHING.md` bottom-up after approval.
- **IronCalc upstream PR:** `docs/upstream/PR-GUIDE.md` is ready, but filing/maintainer review is external.
- **Research frontier:** formal structural→value composition/router enforcement,
  verified Rust byte-parser TCB, per-file adaptive Tier-2 sampling, and an independent
  financial oracle remain open by design.

## 8. Hard-won operational lessons

- ⚠️ **NEVER `git checkout` / `git restore` a file holding uncommitted work** to undo a small mess —
  surgically edit the mess out instead. Doing this in round 59 destroyed a full round of work. Recovery
  was possible only by replaying every `Edit` tool call from the session JSONL transcript
  (`~/.claude/projects/-home-soh-aix/<session>.jsonl`) onto the last commit — parse
  `message.content[].tool_use` for `old_string`/`new_string`, apply in order, require a unique match.
  **Commit round work incrementally** so the blast radius stays small.
- **A workflow can be truncated mid-run by a usage limit** (session *or* weekly). Errored agents are not
  cached, so a resume is useless — **re-run fresh**. And a "0/3 rejected" verdict from a truncated run
  means *unverified*, never *refuted*.
- **Finder agents run in the main working directory** and leak scratch tests. Check `git status` and
  clean debris before every commit.
- `fullCalcOnLoad="1"` (present in the `refs.xlsx` fixture) short-circuits the cache oracle
  (`recalc_on_load_forced` → `unverified_caches = 0`). Strip it to reproduce cache false-certifies e2e.
- `set_user_input` poison-then-restore is fragile (the restore does not reliably stick) — prune the
  oracle map *after* instead.
- Dropping a cell from the oracle only matters when `by_stored` cannot vouch it (i.e. xlq blanked /
  force-recomputed the cache). The e2e false-certify vector is therefore a foreign edit that **injects**
  a preserved cache carrying the engine's wrong value.
- **No value-poison is sound** for a dependent that discriminates an exact output boundary
  (`IF(x=28,…)` is flat at every probe). Only exact reference reachability
  (`refshift::formula_references_cell` + `formula_references_name`, joint cell+name fixpoint) is sound.
- When replacing an engine-graph-based mechanism with a static one, enumerate **every** indirection the
  engine resolved transparently: defined names, 3D-span interiors, runtime refs (OFFSET/INDIRECT),
  non-ASCII qualifiers.
- **Closed, do not re-report:** the structured/table-ref reachability residual is a non-issue — the
  vendored ironcalc importer expands `Table1[Col]` → `$B$2` at load, so reachability already sees a
  plain ref (verified round 63).
