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
