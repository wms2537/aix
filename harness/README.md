# The confined harness — non-bypassability, enforced

xlq is an enforcement boundary for an agent *confined to it*. This directory
is the concrete confinement mechanism, and it is enforced by the environment,
not by the agent's cooperation.

## Mechanism
`confined-run.sh CMD` runs CMD in a cleared environment
(`env -i PATH=./confined-bin`) whose only executable is `xlq` (a symlink in
`confined-bin/`). No `python`, no `openpyxl`, no `cp`/`dd`/`mv` — nothing that
can parse or rewrite an `.xlsx`. An agent driven through this runner has
exactly one way to modify a workbook: `xlq apply`, which is surgical,
preview-gated, coverage/oracle-gated, proof-carrying, and receipted.

## What it guarantees, and what it does not
- WITHIN the harness, the #22044 corruption is unreachable: there is no tool
  to whole-file-rewrite or drop a chart/pivot/VBA part. Verified: `python3`,
  `cp`, and even `head` return "not found"; only `xlq` runs.
- It does NOT defend a workbook against an agent handed an *unconfined* shell.
  Non-bypassability is a property of the harness (which tools it exposes),
  which xlq enables; the harness must withhold raw write. This matches how
  the enforcement-wave sandboxes (IsolateGPT, Progent, MiniScope) scope their
  boundaries.

## Reproduce
```
./confined-run.sh 'python3 -c "import openpyxl"'   # -> python3: not found
./confined-run.sh 'xlq --version'                  # -> works
```
The adversarial experiment (docs/NON-BYPASS.md) runs an agent that actively
tries to bypass the safe path inside this harness and measures whether it can.
