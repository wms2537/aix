# Provenance: in-the-wild spreadsheet corpora

Acquired 2026-07-09 for a pre-registered locked test run (user consent recorded).
Download + conversion only; file contents were not inspected or analyzed during acquisition.

---

## 1. EUSES spreadsheet corpus

- **What**: The EUSES Spreadsheet Corpus (Fisher & Rothermel, 2005) — "a large sample of
  spreadsheets (5607 total files, 4499 of which are unique and suitable for automated
  processing in Excel) that researchers can use to evaluate their methodologies and tools"
  (Zenodo record description).
- **Canonical record**: Zenodo, tera-PROMISE / OpenScience archive.
  - DOI: `10.5281/zenodo.581673` — https://zenodo.org/records/581673
  - License: **CC-BY-4.0** (per Zenodo record metadata, `"license": {"id": "cc-by-4.0"}`).
  - Zenodo record README.txt: "This data is part of the OpenScience tera-PROMISE repository,
    a long-term hosting solution for software engineering research data."
- **Retrieved from** (mirror, for bandwidth reasons): GitHub repo `nc0325/euses`, an exact
  byte-for-byte mirror of the Zenodo record files, fetched as
  `https://codeload.github.com/nc0325/euses/zip/refs/heads/main` on 2026-07-09.
  **Every inner file was verified MD5-identical to the canonical Zenodo record 581673
  checksums (11 zips + README.txt: ALL MATCH).** Direct Zenodo download was attempted first
  but throttled to ~15 KB/s per connection.
- **Archive checksum** (sha256 of downloaded archive `euses-main.zip`, 179,057,233 bytes):
  `b102cc63208601091fb89b4853375c0594e2bd080f9445cdcb6b954d2fbe593f`
- Inner category zips (sha256):
  - cs101.zip `56ca30a8dc87750b4f61520f02cef5b4e08670638d7bff07efcc6237d1f0b221`
  - database.zip `aeab3476f3894934fd391591431c3768d2f160dd7e797a773041717a6f5ee4a8`
  - filby.zip `0a5d9919f77e8a27fa72c5c14dc928ee59e2523df6a09a5f28796da1aaeec581`
  - financial.zip `fad3c1cbb6268be6296657a22732d7dda98014282606df1c31a9d275fe0f6001`
  - forms3.zip `e5177f0b02f78987abd2a0417ac9d133269ada26815d5c388dc11663c7355ae2`
  - grades.zip `d3fbf9b18e4a266ad94e148ba6ba07d27090d690b60474e2c5076fde5a684b7f`
  - homework.zip `e0dd8504b231d480c546fdeab283b4aa123d041ea1394bf439dfdba80749a75a`
  - inventory.zip `df57568878a7aae3d9244e0a0c857516e6520c2de9632809ac0da92fa9d44bf2`
  - jackson.zip `391b5bd73def1aaafb8e12ba8319761e22df7f8b69c0dd4aaddecacf638da154`
  - modeling.zip `adbca56521b0ff43fe7c3c907fdeb7a1a928ecf947f263f5748b50a035ad2983`
  - personal.zip `e45a0997ab4d0cc6987115705792efc6f578207c45c93b911a90d64cb3f5849b`
- **Raw files**: **4,652** `.xls` files (5,598 files total incl. the corpus's numbered
  duplicate artifacts; no `.xlsx`) extracted under `euses/raw/<category>/`
  (11 categories: cs101, database, filby, financial, forms3, grades, homework, inventory,
  jackson, modeling, personal). No subset applied — full corpus retained.
- **Conversion**: first 800 files in byte-wise (LC_ALL=C) lexicographic order of full path
  under `euses/raw/`, converted `.xls -> .xlsx` with LibreOffice headless
  (`--convert-to xlsx`, sequential batches of 25), output in `euses/converted/`.
  - Converted OK: **796** of 800
  - Failures: **4** (listed in `euses/failed.lst`; one retry per failed batch, then recorded)

## 2. Enron spreadsheet corpus

- **What**: Spreadsheets from the Enron Corpus (FERC-released email archive; cf. Hermans &
  Murphy-Hill, "Enron's Spreadsheets and Related Emails: A Dataset and Analysis"). Repo
  README: "The spreadsheets in this dataset are in their original format ... They have been
  de-duplicated by MD5 hash."
- **Source**: GitHub repo `SheetJS/enron_xls` (https://github.com/SheetJS/enron_xls),
  fetched as `https://codeload.github.com/SheetJS/enron_xls/zip/refs/heads/master`
  on 2026-07-09.
  - License: **CC0-1.0** (repo LICENSE file: "CC0 1.0 Universal ... Statement of Purpose ...").
    Note the license covers the compilation/tooling; the underlying spreadsheets are public
    records released by FERC during the Enron investigation.
- **Archive checksum** (sha256 of `enron_xls-master.zip`, 1,413,678,326 bytes):
  `437316ed32927e0465d91690fb5fd36823fbf19d89a3701716df6c4d12003503`
- **Archive contents**: 20,905 files (6.51 GB uncompressed), of which 20,872 are `.xls`.
- **Subset rule applied** (disk budget): extracted only the **first 1,500 `.xls` files in
  ascending byte-wise (LC_ALL=C) lexicographic order of full archive path** within
  `enron_xls-master.zip`, into `enron/raw/` (all fall under `edrm/`). The archive itself is
  retained at `enron/enron_xls-master.zip`, so the subset is reproducible and extendable.
- **Raw files**: 1,500 `.xls` files under `enron/raw/edrm/` (443 MB).
- **Conversion**: first 800 files in byte-wise (LC_ALL=C) lexicographic order of full path
  under `enron/raw/`, converted `.xls -> .xlsx` with LibreOffice headless
  (`--convert-to xlsx`, sequential batches of 25), output in `enron/converted/`.
  - Converted OK: **786** of 800
  - Failures: **14** (listed in `enron/failed.lst`; one retry per failed batch, then recorded)

## Totals

- Total downloaded: ~1.59 GB (enron_xls-master.zip 1,413,678,326 B + euses-main.zip
  179,057,233 B) — within the ~2.5 GB budget.
- Total on-disk footprint of `data/inthewild/`: ~2.8 GB (euses 903 MB, enron 1.9 GB,
  both retained archives included).
- Conversion logs: `euses/convert.log`, `enron/convert.log`; selected-file lists:
  `{euses,enron}/first800.lst`.

## Data-handling note

The Enron corpus contains real business data and personal names (FERC-released public
records). Handling is confined to this machine per recorded user consent. Spreadsheet
contents were not opened or analyzed during acquisition; the only processing was format
conversion (LibreOffice) and file counting. All spreadsheet files are git-ignored
(`data/` is excluded from the repo; only this PROVENANCE.md is tracked).

---

# v2 acquisition (2026-07-10, pre-registered locked test v2 — research-log/018)

Acquired for LOCKED TEST V2. Download/convert/extract only; no spreadsheet contents were
opened or analyzed. Conversion tool: LibreOffice 24.8.7.2 headless (same as v1); sampling
Python 3.14.3. Same batch method as v1 throughout: sequential batches of 25, dedicated
LibreOffice profile dir (`-env:UserInstallation`), 300 s timeout per batch, one retry per
failed batch, failures recorded.

## 1. EUSES v2 — full-corpus conversion (`euses/converted_v2/`)

- **Scope**: ALL 4,652 raw `.xls` under `euses/raw/` (same archive as v1; no new download).
- **Layout note (disclosed)**: `converted_v2/` mirrors the raw relative directory structure
  (`<category>/<subdir>/<name>.xlsx`) instead of v1's flat layout: the full corpus has 149
  exact output-basename collisions that a flat outdir would silently overwrite. The batch
  method itself is unchanged; a batch spanning >1 raw dir is run as one soffice invocation
  per raw dir within the batch.
- **v1 prefix reused**: the 796 v1-converted files were **copied** (not moved, not
  reconverted) from `euses/converted/` into their mirrored `converted_v2/` locations. The
  bytewise-sorted full-corpus list's first 800 entries were verified identical to v1's
  `first800.lst` before splitting.
- **Remaining 3,852** (sorted positions 801–4,652) converted 2026-07-10: **3,852 OK, 0
  failures** (log: `euses/convert_v2.log`).
- **v1's 4 failed files** (all `database/bad/`, "source file could not be loaded") were
  re-attempted once via the same method: all 4 failed again → recorded in
  `euses/failed_v2.lst`.
- **Totals**: `converted_v2/` = **4,648** `.xlsx` of 4,652 raw; failures **4**.
- Per-category converted composition: cs101 9, database 798, filby 45, financial 809,
  forms3 26, grades 676, homework 691, inventory 795, jackson 13, modeling 781, personal 5.

## 2. Enron v2 — seeded-random sample (`enron/raw_v2/`, `enron/converted_v2/`)

- **Sample rule (pre-registered)**: list all **20,872** `.xls` member paths of the retained
  archive `enron/enron_xls-master.zip`; sort bytewise (all paths ASCII; key =
  UTF-8-encoded path, equivalent to LC_ALL=C); draw
  `random.Random(20260710).sample(sorted_paths, 1500)` (Python 3.14.3). Seed **20260710**.
  Sample recorded as drawn in `enron/sample1500_v2.lst`.
- **Extraction**: exactly those 1,500 members extracted to `enron/raw_v2/` (top-level
  `enron_xls-master/` prefix stripped, as in v1; all fall under `edrm/`). **1,500 files,
  448 MB.**
- **Conversion**: first **800** of the sample in bytewise-sorted full-path order
  (`enron/first800_v2.lst`), same batch method, output `enron/converted_v2/` (mirrored
  relative layout, i.e. `edrm/…xlsx`; no basename collisions among the 800).
  - Converted OK: **799** of 800
  - Failures: **1** (`enron/failed_v2.lst`; one retry, then recorded). Log:
    `enron/convert_v2.log`.

## 3. dbt v2 — two additional public dbt projects (`dbt/v2/`)

**Acquisition rule (pre-registered, research-log/018 L6)**: the two most-starred GitHub
search hits at acquisition time for REAL (non-demo, non-tutorial) public dbt projects with
a `models/` directory of ≥50 `.sql` files; `mattermost/mattermost-data-warehouse` excluded
(already used in v1). Downloaded as codeload branch tarballs; tarballs retained.

1. **duneanalytics/spellbook** — Dune Analytics' production "SQL views for Dune" repo.
   - Stars at acquisition: **1,506**. License: **Business Source License 1.1** (BUSL-1.1;
     repo `LICENSE`).
   - Source: `https://codeload.github.com/duneanalytics/spellbook/tar.gz/refs/heads/main`,
     fetched 2026-07-10; branch HEAD at acquisition:
     `a1c0ed8561c0c61c70f2886a894ad909289a72af` (2026-07-09T12:57:18Z).
   - sha256(`dbt/v2/spellbook.tar.gz`, 92,477,958 B):
     `a6de826f51152c4138a9e1d762d2194db9c2ce01285e23031739c14195900165`
   - Extracted to `dbt/v2/spellbook/` (tarball top dir `spellbook-main/` stripped).
   - Model count: **7,419** `.sql` under `models/` dirs (excluding `macros/`), organized
     as 5 dbt subprojects under `dbt_subprojects/`: daily_spellbook 2,484;
     hourly_spellbook 2,190; dex 1,819; tokens 664; solana 262.
2. **cal-itp/data-infra** — California Integrated Travel Project's production data
   infrastructure (dbt warehouse under `warehouse/`).
   - Stars at acquisition: **69**. License: **AGPL-3.0** (repo `LICENSE`).
   - Source: `https://codeload.github.com/cal-itp/data-infra/tar.gz/refs/heads/main`,
     fetched 2026-07-10; branch HEAD at acquisition:
     `04734927ce903502381597f4e3f0d3225facc8a4` (2026-07-09T23:24:11Z).
   - sha256(`dbt/v2/data-infra.tar.gz`, 338,649,099 B):
     `3e3e27e66007210ca21a061e61944ba70d8a1d101e6fb67dbeed845df0b252c6`
   - Extracted to `dbt/v2/data-infra/` (top dir `data-infra-main/` stripped).
   - Model count: **619** `.sql` under `warehouse/models/`.

**Higher-starred candidates considered and excluded (disclosure)**:
`matsonj/nba-monte-carlo` (604★ — README: "end to end example of running the 'Modern Data
Stack' on a single node" → demo/showcase); `dagster-io/dagster-open-platform` (463★ — only
1 `.sql` under `models/` at HEAD, fails the ≥50-model rule); `Levers-Labs/SOMA-B2B-SaaS`
(385★ — reference spec/template, not an operating project's warehouse; no license file);
`rittmananalytics/ra_data_warehouse` (270★ — a dbt *package* of pre-built models, not a
project); `gitlab-data/analytics` (hosted on gitlab.com — no GitHub codeload tarball);
FlipsideCrypto per-chain model repos (archived or removed).

## v2 data-handling note

Same as v1: Enron data handling confined to this machine per recorded user consent; no
spreadsheet contents opened; processing limited to zip listing/extraction, format
conversion, and file counting. All corpus files remain git-ignored; only this
PROVENANCE.md is tracked.
