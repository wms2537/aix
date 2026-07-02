# AXLE-bench

**A**gent-safe e**X**cel **L**ayer **E**valuation — xlq's consolidated,
reproducible benchmark suite. One name, five axes, every number traceable to
a JSON artifact in this directory.

Run everything that can run on this machine:

```sh
bash benchmarks/run_all.sh
```

The meta-runner chains the three suites below and **skips (with a notice)**
any suite whose prerequisites (python3+openpyxl, soffice, cargo) are missing,
instead of failing.

## Machine and tools

All RUN-HERE numbers were produced on (recorded in
[`results.json`](results.json) → `machine`/`tools`):

- **CPU:** Intel Core i7-8700K @ 3.70 GHz, 12 logical cores
- **OS:** Linux
- **xlq:** 0.1.0, release build (engine `ironcalc 0.7.1+e50ccea8 (vendored master)` + local patches, see [docs/COVERAGE.md](../docs/COVERAGE.md))
- **openpyxl:** 3.1.5 (CPython 3)
- **LibreOffice:** 24.8.7.2 headless
- Timing protocol: 1 untimed warmup + 3 warm runs, median wall time; single
  machine, synthetic fixtures, n=3 — treat sub-20% deltas as noise.

## The five axes

| # | Axis | What it measures | One command | Artifact | Narrative doc |
|---|---|---|---|---|---|
| 1 | **Correctness** | Cross-engine value agreement (ironcalc vs LibreOffice, 1,659 cases / 492 functions) + Excel-arbiter triage of every disagreement | `bash benchmarks/run_oracle.sh` | [`agreement.json`](agreement.json) | [docs/AGREEMENT.md](../docs/AGREEMENT.md) |
| 2 | **Fidelity** | OOXML part preservation on re-save (drop/add/rewrite per part) + cached-value integrity (`xlq diff` cell-level, incl. the foreign-file D2 correction) | `bash benchmarks/run_bench.sh` (sections D/D2) | [`results.json`](results.json) | [docs/BENCHMARKS.md](../docs/BENCHMARKS.md) §D/D2 |
| 3 | **Efficiency** | Wall time to load+recalculate a 100k-cell / 99,800-formula workbook, plus isolated engine load time | `bash benchmarks/run_bench.sh` (section A) | [`results.json`](results.json) | [docs/BENCHMARKS.md](../docs/BENCHMARKS.md) §A |
| 4 | **Agent-ergonomics** | Token cost of the structural census vs a naive full dump (bytes as token proxy); JSON operability of every command | `bash benchmarks/run_bench.sh` (sections B/C) | [`results.json`](results.json) | [docs/BENCHMARKS.md](../docs/BENCHMARKS.md) §B/C |
| 5 | **Catalog** | The 3-number coverage accounting: recognized / locally evaluable / policy-limited, over Microsoft's 522-name catalog | `cd xlq && cargo run --release --bin coverage-probe -- ../benchmarks/excel-functions.txt > ../benchmarks/coverage.json` | [`coverage.json`](coverage.json) | [docs/COVERAGE.md](../docs/COVERAGE.md) |

## Comparison matrix

Cell provenance rules — every cell is exactly one of:

- a number traceable to `benchmarks/agreement.json`, `benchmarks/results.json`,
  or `benchmarks/coverage.json` (marked `[A]`, `[R]`, `[C]`);
- **n/a (cannot)** — the tool cannot perform the operation by design, with the
  documentation cited;
- **not-run (cite)** — a documented-capability row we did not benchmark here
  because no runnable, comparable setup exists in this environment. **No
  fabricated numbers**; the citation is the tool's own docs/source via
  [docs/BASELINE.md](../docs/BASELINE.md).

| Tool | 1 Correctness (1,659-case oracle) | 2a Fidelity: OOXML parts on re-save (5 fixtures) | 2b Fidelity: cached values on re-save (101,961 formula cells) | 3 Efficiency: 100k-formula workbook | 4 Agent-ergonomics: structural read of the 100k-cell book | 5 Catalog (522-name probe) | Dry-run / receipts / revisions | Reports its own blind spots |
|---|---|---|---|---|---|---|---|---|
| **xlq 0.1.0 / ironcalc-master** (RUN HERE) | 97.1% agreement where both engines produce a value (1,273/1,311); 93.7% counting one-side-error rows (1,273/1,358). Its side of the triage: 26 cases erroring where LO computes + 38 value differences `[A]` | Own-authored files: 0 dropped, 0 added, 1 changed part (`docProps/core.xml` timestamp only) — **provenance-biased best case**; on foreign-authored files (D2) it rewrites every part, drops `docProps/custom.xml`, injects `xl/metadata.xml`/`xl/sharedStrings.xml` `[R]` | **0** cached-value drifts on all 5 fixtures `[R]` | `xlq calc` (load + full recalc + stored-vs-recomputed audit + report): **1.264 s**; isolated engine load: 0.599 s `[R]` | Census: **999 B** vs 7,239,303 B naive dump = **7,246×** smaller (28.4× on the small fixture); inspect latency 7–20 ms small / 0.908 s on 100k cells; JSON on stdout for every command `[R]` | **522/522 recognized (100%), 505 locally evaluable (96.7%), 17 policy-limited (3.3%)** `[C]` | v0.1 is read-only by construction (no write path exists); dry-run + hash-chained receipts + rev-file history are spec'd for v0.2 ([receipt-journal-spec](../docs/receipt-journal-spec.md)) — not shipped yet | **Yes, today**: per-workbook `coverage.reliable`, `unsupported_functions`, `policy_limited_functions` in every report ([census-spec](../docs/census-spec.md); policy table in `[C]` `policy_limited_detail`) |
| **openpyxl 3.1.5** (RUN HERE) | **n/a (cannot)** — "openpyxl **never** evaluates formula" ([docs](https://openpyxl.readthedocs.io/en/stable/simple_formulae.html); [BASELINE §1](../docs/BASELINE.md)) | Drops 2 parts on every fixture (`xl/sharedStrings.xml`, `xl/metadata.xml`); rewrites every remaining part except the theme (1 byte-identical) `[R]` | Strips **101,961 / 101,961** cached values — every formula cell reads 0 in Excel until recalc `[R]` | Parse-only: **0.875 s** — faster than xlq's 1.264 s full calc (see honesty notes); recalc: **n/a (cannot)** `[R]` | Naive dump is the only full read: 7,239,303 B on the 100k book — beyond any context window; no census equivalent `[R]` | **n/a (cannot)** — evaluates 0 functions | No — `wb.save()` "will overwrite existing files without warning" ([docs](https://openpyxl.readthedocs.io/en/stable/tutorial.html); [BASELINE §1](../docs/BASELINE.md)) | No |
| **LibreOffice 24.8 headless** (RUN HERE) | The oracle's reference side. Its side of the triage: 21 cases erroring where ironcalc computes + 38 value differences; does not recognize 33 of the 492 functions exercised (105 cases `#NAME?` on OOXML import). Documented-Excel-semantics analysis attributes several bucket-B/C rows to **LO** deviating (`POWER(0,0)`→1, `ATAN2(0,0)`→0, booleans-as-numbers, PERCENTRANK rounding) `[A]` | Drops `xl/metadata.xml` on every fixture (+`xl/sharedStrings.xml` on perf-large), adds `docProps/custom.xml`, rewrites every part — 0 byte-identical `[R]` | Rewrites the text of **620** formulas (semantics preserved, bytes not) + **90,448** cached-value drifts (15-digit re-serialization of 17-digit stored floats — last-ulp, not corruption) `[R]` | `--convert-to xlsx`: **1.494 s** — process spawn + load + full write; recalc on load NOT guaranteed by default ([BASELINE §4](../docs/BASELINE.md)) `[R]` | **n/a (cannot)** — no structural JSON read/census facility | Not probed against the 522 catalog; observed floor from the oracle: 33 exercised functions unrecognized `[A]` | No ([BASELINE §4](../docs/BASELINE.md)) | No |
| **excel-mcp-server** (~3,978★) — **NOT-RUN-HERE** | not-run — openpyxl underneath (`openpyxl>=3.1.5` in [pyproject.toml](https://github.com/haris-musa/excel-mcp-server/blob/main/pyproject.toml)) → cannot evaluate; [BASELINE §5](../docs/BASELINE.md) | not-run — inherits openpyxl's losses; additionally loads without `keep_vba`, so editing .xlsm **silently drops the VBA project** ([workbook.py](https://github.com/haris-musa/excel-mcp-server/blob/main/src/excel_mcp/workbook.py); [BASELINE §5](../docs/BASELINE.md)) | not-run — inherits openpyxl's cached-value stripping ([BASELINE §1, §5](../docs/BASELINE.md)) | not-run | JSON-over-MCP tool results, but reads are range dumps; no census / structure-only mode ([TOOLS.md](https://github.com/haris-musa/excel-mcp-server/blob/main/TOOLS.md)) | not-run — n/a (cannot evaluate; openpyxl inside) | No — no dry-run, backup, undo, audit, or versioning anywhere in TOOLS.md (verified by grep; [BASELINE §5](../docs/BASELINE.md)) | No |
| **OfficeCLI** (~8,299★) — **NOT-RUN-HERE** | not-run — claims "350+ built-in Excel functions evaluated automatically on write" incl. dynamic arrays ([README](https://github.com/iOfficeAI/OfficeCLI#readme); [BASELINE §6](../docs/BASELINE.md)); no agreement data exists here | not-run — creates its own charts/pivots natively, but **edit-preservation and VBA survival are undocumented** ([BASELINE §6](../docs/BASELINE.md)) | not-run | not-run | JSON `batch`/`dump` commands and HTML/PNG rendering documented ([README](https://github.com/iOfficeAI/OfficeCLI#readme)); no census / structure-only read | not-run — "350+" is the vendor's claim, not probed against the 522-name catalog | No — `batch --stop-on-error` and `dump` only; no dry-run, receipts, or versioning documented ([BASELINE §6](../docs/BASELINE.md)) | No |

Star counts as of 2026-07-02 via the GitHub API ([BASELINE.md](../docs/BASELINE.md), header).

## Honesty notes — where xlq does NOT win

Reported straight, same artifacts:

- **openpyxl parses faster than xlq calculates.** 0.875 s parse vs 1.264 s
  `xlq calc` `[R]`. xlq's number buys a full recalculation of 99,800 formulas
  plus a stored-vs-recomputed audit that openpyxl cannot do at any speed — but
  if all you need is a parse, the like-for-like comparison is ironcalc's
  isolated load, 0.599 s, and openpyxl's 0.875 s is entirely respectable.
- **LibreOffice is genuinely fast.** 1.494 s to spawn an entire office
  process, load, and write the 1.6 MB file back out — only ~1.2× xlq's
  in-process calc `[R]`. And because Calc does not recalculate xlsx on load
  by default, 1.494 s is likely *not* even its full load+recalc cost; an
  honest engine-vs-engine number would need a UNO-scripted hard recalc,
  which this harness does not do.
- **97.1% agreement means 38 value disagreements — and 93.7% means 85.**
  Counting everything where the two engines decided a case differently
  `[A]`: **26** cases where ironcalc errors on something LibreOffice computes
  (candidate ironcalc strictness/coverage bugs — though for several,
  documented Excel semantics side with ironcalc: `POWER(0,0)`,
  `ATAN2(0,0)`, `CHIDIST(-1,2)`), **21** where LibreOffice errors on
  something ironcalc computes, and **38** where both compute and the values
  differ (including a real ironcalc finding: `CHISQ.TEST` underflowing a
  2.55e-25 p-value to exactly 0). A further **196** cases error on both
  sides (error codes match in only 58 — weak signal, largely LO's error-export
  mapping), and **36 functions (115 cases) received no oracle signal at
  all** — ironcalc could be wrong on every one of them without this suite
  noticing. Excel is the arbiter for all of it, and **nothing here has been
  verified against a live Excel instance**; per-case Excel verdicts rest on
  documented semantics ([AGREEMENT.md](../docs/AGREEMENT.md)).
- **The clean ironcalc preservation row is provenance-biased.** The fixtures
  are authored by ironcalc's own writer; section D2 `[R]` shows that on
  openpyxl- and LibreOffice-authored inputs, ironcalc rewrites every part,
  drops `docProps/custom.xml`, and injects the parts its writer always
  emits — the same re-serialize-the-world behavior as everyone else. And the
  fixtures contain **no VBA, charts, or pivot tables** (ironcalc cannot
  author them), so the headline real-world failure mode
  ([claude-code#22044](https://github.com/anthropics/claude-code/issues/22044))
  is *untested here*, not disproven.
- **LibreOffice's 90,448 "cached-value drifts" are last-ulp float
  re-formatting**, not corruption — flagged as such in
  [BENCHMARKS.md](../docs/BENCHMARKS.md) §D. They matter to byte-level audits
  and signatures, not to the numbers a user sees.

## The claim this suite does support

Two matrix columns are empty for **every other row** — running or not-run,
open source or 8k-star:

1. **No other tool in the matrix offers dry-run, change receipts, or
   revision semantics.** Not openpyxl, not LibreOffice, not
   excel-mcp-server, not OfficeCLI, not Excel itself via COM — verified
   against each tool's own documentation in
   [docs/BASELINE.md](../docs/BASELINE.md) ("Gaps": *"No tool wraps agent
   writes in preview → apply → receipt → revision"*). xlq v0.1 earns its
   cell honestly — read-only by construction today, with the write path
   spec'd ([receipt-journal-spec](../docs/receipt-journal-spec.md)) rather
   than claimed.
2. **No other tool reports its own blind spots.** xlq ships coverage flags
   today: every `inspect`/`calc` output carries `coverage.reliable` plus the
   named `unsupported_functions` and `policy_limited_functions` for *that
   workbook* ([census-spec](../docs/census-spec.md),
   [COVERAGE.md](../docs/COVERAGE.md), `coverage.json`
   → `policy_limited_detail`). Every other row answers confidently or not at
   all; none tells you when it is guessing.

That — not raw speed — is the gap this suite exists to measure, and to keep
honest.
