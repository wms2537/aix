# xlq reference-completeness hardening — status & handoff

**Branch:** `xlq-reference-completeness` @ `22d6d88` (pushed to origin, **NOT merged to main**)
**Last updated:** 2026-08-21, end of round 68
**Gate:** green — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, **406 tests** pass
**Totals:** ~285 defects fixed over 69 rounds

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

**Exit condition:** two consecutive *genuinely dry* rounds (zero confirmed findings). **Not yet reached.**

The round-N script lives at `/tmp/claude-1000/-home-soh-aix/<session>/scratchpad/roundNN.js`; each round
is produced by copying the previous script and rewriting the "ROUND N JUST ADDED" block to weight the
newest code plus any known residuals.

## 3. Progress this session (rounds 60–68)

54 fixes across 29 commits. Newest first:

| Round | Commits | Confirmed | Headline defects |
|---|---|---|---|
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

## 7. Next steps (in order)

1. **Round 69**: fresh workflow. Rounds 67-68 lenses came back DRY on oracle-soundness,
   external-rels/security, refshift/structural coverage, and invalid-output — weight the next
   run toward: **(a)** chart numCache/strCache point values (open residual, self-repairs on
   refresh); **(b)** any NEW surface from rounds 66-69 comparators. (The pivot `<item @n>`
   label-swap lead was fixed first thing in round 69: `n` joined the per-field ordered sigs.)
2. **Accepted residuals** (documented, not defects): chart numCache/strCache point values are
   deliberately uncompared (self-repairs on refresh; re-derivation is common faithful behavior);
   chained cellStyleXfs→cellStyleXfs inheritance folds one hop only (exotic construct);
   xl/diagrams/* + xl/embeddings/* remain blanket-refused (fail-closed availability gap — OLE
   content is executable and must not be allowlisted without a comparator).
3. Continue the loop toward two consecutive genuinely-dry rounds.
4. Update `~/.claude/projects/-home-soh-aix/memory/xlq-reference-completeness-hardening.md`
   after each completed round.

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
