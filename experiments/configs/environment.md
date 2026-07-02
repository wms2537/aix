# Environment

- OS: Linux 6.11.0-29-generic (Ubuntu-family), x86_64
- CPU: Intel Core i7-8700K @ 3.70GHz, 12 logical cores
- RAM: (recorded at benchmark time; single-machine, no GPU required)
- Rust: 1.96.0 (cargo 1.96.0)
- Python: 3.14.3 (venv with openpyxl 3.1.5 for baselines/harness)
- LibreOffice: 24.8.7.2 (headless, differential oracle reference)
- Engine under test: IronCalc, vendored upstream master @ e50ccea8 + xlq patches
- All experiments local; no cloud, no API-billed compute in the measurement path.
- Reproduction: `benchmarks/run_all.sh` (AXLE-bench meta-runner), individual axes
  via run_bench.sh / run_oracle.sh / coverage-probe. Repo state: commit 150fb66.
