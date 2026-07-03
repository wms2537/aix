# Threat Model and the Enforcement Boundary

Addresses the reviewer objection: "even a built v0.2 is opt-in — nothing forces
an agent through xlq, so it enforces nothing." This states precisely what xlq
guarantees, against whom, and where the boundary is (and is not).

## The deployment: a confined agent
xlq is an enforcement boundary **for an agent confined to it**. The concrete
deployment is the standard tool-sandbox model of the 2025–26 agent-enforcement
literature (IsolateGPT, Progent, MiniScope): the agent harness grants the agent
the `xlq` tool and **no capability to write `.xlsx` bytes by any other means** —
no `openpyxl`, no raw file write, no shell that can. The shipped
`skills/xlq/SKILL.md` plus a restricted tool surface is that harness.

Within that harness, `xlq apply` is the agent's *only* path to modify a
workbook, and every such modification is:
- **surgical** — only sheet parts with an edited cell are rewritten; charts,
  pivots, VBA, styles, external links are byte-identical (proven, docs/FIDELITY.md);
- **preview-gated** — `--dry-run` predicts affected cells and new errors first;
- **precondition-checked** — `base_hash` must match the file on disk
  (`revision_mismatch` otherwise), under an advisory lock held across the whole
  check→write sequence (no TOCTOU);
- **coverage-gated** — a write whose predicted cached values are unreliable
  (nondeterministic volatiles in the affected graph) is refused;
- **receipted** — a hash-chained journal entry records base/result hash, ops,
  actor; out-of-band edits are detected, not silently absorbed.

So an agent confined to xlq **cannot reach the issue-#22044 corruption**: it has
no operation that rewrites the whole container or drops a chart/pivot/VBA part.
That is the enforcement claim, and it is now backed by a built, measured system.

## What the boundary is NOT
- It is **not** a guarantee against an agent with unrestricted shell or raw
  filesystem access. Such an agent can call `openpyxl` directly and corrupt the
  file — exactly the status quo. xlq does not, and cannot, prevent that; no
  library-level tool can. Non-bypassability is a property of the **harness**
  (which tools it exposes), which xlq *enables* but does not itself enforce.
  We claim non-bypassability *within the harness*, and say so — we do not claim
  a guarantee against an adversary who is handed raw write access.
- This mirrors how the enforcement-wave papers frame their boundaries: Progent's
  policy engine, IsolateGPT's process isolation, and MiniScope's scoped tokens
  all bound an agent that is *routed through them*; none prevents an operator
  from also handing the agent an unmediated capability. The boundary is the
  mediation surface, and its value is that a harness *can* confine the agent to
  it — for spreadsheets, previously it could not, because no safe write
  primitive existed.

## Threat actors and coverage
| Actor | Can it corrupt via xlq? | Notes |
|---|---|---|
| Careless agent confined to xlq | No | No whole-file-rewrite op exists; surgical by construction. |
| Buggy agent (wrong cell/formula) | Bounded | dry-run previews the effect; base_hash + receipts make every change attributable and the rev-file makes it reversible. |
| Agent with raw shell/openpyxl | Yes (bypasses xlq) | Out of scope by construction; the harness, not xlq, must withhold raw write. Stated as a limitation. |
| Concurrent writers | Serialized | advisory lock; second writer gets `lock_held`. |
| Out-of-band edit between ops | Detected | hash-chain `external_edit_detected` + adoption marker. |

## The honest scope sentence for the paper
"xlq is an enforcement boundary for an agent whose harness confines its
workbook writes to the xlq tool; within that confinement it makes the
issue-#22044 corruption unreachable and every change previewed, attributable,
and reversible. It does not defend a workbook against an agent granted raw
filesystem access — that is the harness's responsibility, and we state it as a
scoping assumption rather than a guarantee."
