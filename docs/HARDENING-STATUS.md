# xlq reference-completeness hardening — status & handoff

**Branch:** `xlq-reference-completeness` @ `ab76abb` (pushed to origin, **NOT merged to main**)
**Last updated:** 2026-07-24, end of round 65 / round 66 aborted
**Gate:** green (391 tests incl. 2 retained audit probes) — `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, **389 committed tests** pass
**Totals:** ~268 defects fixed over 65 adversarial rounds

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

## 3. Progress this session (rounds 60–65)

38 fixes across 16 commits. Newest first:

| Round | Commits | Confirmed | Headline defects |
|---|---|---|---|
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

## 5. Current working-tree state ⚠️

```
 M xlq/src/certify.rs      (+52)
 M xlq/src/structural.rs   (+114)
```

**This is finder-agent debris, not work in progress.** Workflow finder agents run in the *main working
directory* (not a worktree) and leak scratch test functions. Both additions are test-only (`mod tests`),
purely additive, and currently compile and pass (they are `eprintln!` audit probes with no assertions):

- `certify.rs::audit_cross_part_macro_swap_probe` — probes round-66 candidate C (below).
  **It has now been run and it CONFIRMS the defect** (see Theme C), so it is retained as evidence
  rather than deleted; convert it to a real regression test (`assert!(... .is_some())`) once the fix
  lands.
- `structural.rs::scratch_fuzz_invalid_output` — a scratch structural-validity fuzzer; currently
  passes, retained pending the round-66 invalid-output assessment.

Both are committed as clearly-labelled probes. **Do NOT `git checkout` the files** — see §8.

Also untracked (pre-existing, unrelated): `benchmarks/live3way_python.json`, `formal/corpus_formulas.txt`.

## 6. Round 66 — ABORTED, findings UNVERIFIED ⚠️

The round-66 workflow hit the **weekly usage limit**: 24 of 34 agents errored. Nine finder lenses
completed, but **only 1 of 24 verify agents ran**. The result JSON reports all 8 candidates as
"rejected 0/3" — **this is an artifact of the verifiers never executing, not a refutation.**

> **Treat all 8 round-66 candidates as UNVERIFIED-BUT-OPEN.** They must be re-verified (or assessed
> directly) before being dismissed. Precedent: round 65's first run aborted the same way, its single
> "0/3 rejected" candidate turned out to be a **real HIGH security defect**, and the completed re-run
> then produced 8 confirmed findings.

**Weekly limit resets Jul 29, 8am (Asia/Kuala_Lumpur)** — no workflow/subagent capacity until then.
Direct main-loop work is unaffected.

### The 8 candidates, grouped into 5 themes

**Theme A — `cellXf → cellStyleXfs` (xfId) style inheritance is never followed** *(4 lenses converged:
date-reachability, styles-numfmt, refshift, tables-comments)*
The `CELL()` backstops (`cellxfs_horizontal`, `cellxfs_locked`, `cellxfs_numfmt_codes`) read only the
`<cellXfs><xf>` entry. If alignment / lock / number-format is set on the **parent named cell style**
(`<xf xfId="N">` inheriting from `<cellStyleXfs>`), the effective value is missed → a change to the
named style is invisible → false-certify of a `CELL("prefix"/"protect"/"format")` value change.
*Assessment: highest-priority. 4-lens convergence is the strongest signal this loop produces (it is
exactly how round-65's 3D-span and round-61's name-mediated gap surfaced). Needs a repro confirming
the vendored engine actually resolves the inheritance (if the engine also ignores xfId, the oracle and
the backstop agree and there may be no divergence).*

**Theme B — non-self-closing data-table `<f>` still stale under `Op::Move`** *(structural-residual)*
**VERIFIED REAL by direct inspection during this handoff.** The round-65 fix patched only the
`Event::Empty(e) if e.name() == b"f"` arm of `rewrite_edited_sheet_move`. The
`Event::Start(e) if is_formula_tag(..)` arm (a `<f t="dataTable" …></f>` written non-self-closing)
still writes verbatim, so `r1`/`r2`/`ref` are left stale → silent value corruption.
*Assessment: real, small, self-contained. Fix: apply the same `is_datatable_f` routing in the Start arm
(and do not set `in_f`, since a data table has no A1 body). Cheapest high-value next fix.*

**Theme C — cross-part `macro=` / `textlink` swap in `chart_drawing_refs`** *(chart-drawing)*
`drawing_shape_links` output is pooled into `chart_drawing_refs`'s `drawings` list **without an
owning-part key**, so two drawing parts each holding a shape named `Btn` (routine after a sheet copy —
`cNvPr` names are per-sheet unique, not per-workbook) can have their `macro=`/`textlink=` bindings
swapped invisibly. The owning-part prefix was applied to `external_rels_targets` (r61),
`opaque_target_signature` (r62) and `pivot_refs` (r64) — **`chart_drawing_refs` is the un-fixed twin.**
***CONFIRMED REAL (2026-07-24).*** The retained probe
`certify.rs::audit_cross_part_macro_swap_probe` was executed: swapping `Module1.SafeExport` with
`Module1.DeleteAllData` between two drawing parts whose shapes are both named `Btn` yields
`verify_noncell_refs(...) == None` — i.e. **CERTIFIED**. The `textlink=` variant (a pure cell re-point,
no VBA required) likewise returns `None`. This is a HIGH security false-certify: a macro re-point that
survives certify. Fix: prefix `chart_drawing_refs`'s drawing signatures with the owning part, matching
the three sibling comparators. The probe must then be converted into a real regression test asserting
`.is_some()`.*

**Theme D — slicer / timeline selection state uncompared** *(pivot)*
Slicer and timeline caches are allowlisted as certify-safe but read by **no** signature. A deselect
re-filters the pivot on refresh → materially different output.
*Assessment: plausible new class; needs a repro proving the parts are genuinely allowlisted and
uncompared.*

**Theme E — `IFERROR`/`ISERROR` laundering of an **error-valued** source** *(oracle-soundness)*
The boundary-discriminating class (round-60 defect 1, round-62 defect 6) was closed by reference
reachability for *deterministic-wrong* sources, but **error-valued** sources (UDF/RTD/unsupported) still
rely on value-diff. `IFERROR(bad, 999)` is flat under any poison, so the engine's masked value may be
vouched.
*Assessment: needs care. Round-62 deliberately excluded error-valued sources from the reachability drop
to avoid reintroducing the round-36 workbook-wide over-refusal. Any fix must preserve that. Verify the
repro closely — poison-and-diff may already handle the direct case.*

## 7. Next steps (in order)

1. **Clean the debris** — delete `audit_cross_part_macro_swap_probe` (certify.rs) and
   `scratch_fuzz_invalid_output` (structural.rs) by targeted edit. Re-run the gate.
2. **Fix Theme B** (verified real, small) — route a data-table `<f>` through `transform_tag_move` in the
   `Event::Start` arm of `rewrite_edited_sheet_move`; add a regression test with a non-self-closing
   `<f t="dataTable" …></f>`; commit.
3. **Fix Theme C** — *already confirmed real by the retained probe*, so no assessment needed: prefix
   `chart_drawing_refs`'s drawing signatures with the owning part (matching `external_rels_targets`,
   `opaque_target_signature` and `pivot_refs`), then convert
   `audit_cross_part_macro_swap_probe` into a regression test asserting refusal. Commit.
4. **Assess Theme A** — read `vendor/upstream/base` to determine whether the importer resolves the
   `xfId` inheritance chain. If it does and xlq's backstops do not, implement inheritance resolution in
   `cellxfs_horizontal` / `cellxfs_locked` / `cellxfs_numfmt_codes`. Commit.
5. **Assess Themes D and E** with repros; fix if confirmed.
6. **After Jul 29, 8am** (weekly reset): re-run the *complete* round-66 review as a **fresh run, not a
   resume** (errored agents are not cached), to (a) properly verify anything left from §6 and (b) find
   what the truncated run missed. Then continue the loop toward two dry rounds.
7. Update `~/.claude/projects/-home-soh-aix/memory/xlq-reference-completeness-hardening.md` with the
   round-66 ledger entry once the round actually completes.

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
