# E2 — Agent A/B: does the tool boundary prevent the #22044 corruption?

**Status:** interventional result. This closes the reviewer-fatal objection that
"no agent is ever actually run" — every trial below is a real LLM subagent
completing a genuine edit task on a real workbook, scored on an external checker.

- **Measured trials:** [`benchmarks/agent_ab.json`](../benchmarks/agent_ab.json)
- **Task definitions:** `scratchpad/agentab/tasks.json`
- **Arm transcripts:** captured inline in this doc (§Transcripts)
- **Engine used for the reload check:** `ironcalc 0.7.1+e50ccea8 (vendored master)`

---

## 1. Design

We ran a **within-task A/B** with the tool as the only manipulated variable.

- **Agent.** A real LLM subagent (Claude, bounded tool surface: shell + a
  Python interpreter, or the `xlq` binary). It was given a plain-English edit
  task and told to complete and save it. It was **not** told which outcome we
  were measuring, nor that fidelity was scored.
- **Two arms, same agent, same task, same input file:**
  - **`openpyxl`** — the status-quo agent path. The agent edits the workbook
    with the openpyxl library and saves. This is the tool an unconstrained
    coding agent reaches for today, and the path implicated in
    [claude-code#22044](https://github.com/anthropics/claude-code/issues/22044).
  - **`xlq`** — the format-aware boundary. The agent may only mutate the
    workbook through `xlq apply` (inspect → patch.json → apply), which rewrites
    the minimum set of OOXML parts and emits a receipt.
- **Two real tasks**, each a single `set_cell` edit on a workbook carrying a
  feature that whole-file rewriters are known to destroy:
  - **t1-vba** — `macro.xlsm`, carries a VBA macro project. Change `Data!B3`
    120 → 250.
  - **t2-chart** — `pivot-chart.xlsx`, carries 2 charts + a pivot. Change
    `Sheet1!A2` 222 → 900.
- **We score two things per trial:**
  1. **Task success** — did the target cell reach the requested value *and does
     the saved file still open?* (external check; the agent's own claim does not
     count).
  2. **Fidelity of untouched content** — of the OOXML parts the edit did not
     touch, how many survive **byte-identical**; which feature parts survive;
     which parts are **dropped** entirely.

Because the agent, the prompt, the input file, and the environment are held
fixed and only the edit tool differs, any difference in the two outcomes is
attributable to the **tool boundary**.

---

## 2. Per-task results

| Task | Feature | Arm | Target cell | Task success | Reloads in engine | Feature parts survive | Feature byte-identical | Parts byte-identical | Parts dropped | Receipt |
|---|---|---|---|---|---|---|---|---|---|---|
| t1-vba | VBA macro | **openpyxl** | 250 ✓ | ✅ yes | ✅ yes | VBA ✓ | ✅ yes¹ | **2 / 11** | 1 — `sharedStrings.xml` | — none |
| t1-vba | VBA macro | **xlq** | 250 ✓ | ✅ yes | ✅ yes | VBA ✓ | ✅ yes | **10 / 11** | 0 | ✅ `macro.xlsm.xlq.jsonl` + rev-1 |
| t2-chart | 2 charts + pivot | **openpyxl** | *load-error²* | ❌ **no** | ❌ **no** | charts ✓ / pivot ✓ (present) | ❌ **no** / ❌ **no** | **1 / 50** | **13** (see below) | — none |
| t2-chart | 2 charts + pivot | **xlq** | 900 ✓ | ✅ yes | ✅ yes | charts ✓ / pivot ✓ | ✅ yes / ✅ yes | **48 / 50** | 1 — `calcChain.xml`³ | ✅ `pivot-chart.xlsx.xlq.jsonl` + rev-1 |

¹ **In this run the openpyxl agent chose `keep_vba=True`**, so the `vbaProject.bin`
survived byte-identical. That is the *charitable* openpyxl — see threat T4. Even
so, it rewrote 9 of 11 parts and silently dropped `sharedStrings.xml`.

² `target_cell_value` came back as
`<load-error:'Chartsheet' object has no attribute 'defined_names'>`. The saved
file **cannot be reopened** — the value is unverifiable because the workbook is
corrupt. The agent nonetheless *reported success* (it read the raw XML and saw
`<c r="A2" t="n"><v>900</v></c>`). The cell write landed; the workbook did not
survive it.

³ `calcChain.xml` is a recomputable formula-ordering **cache**, not a feature.
Excel/engines regenerate it on open. Its removal is expected and lossless — xlq
drops it deliberately rather than ship a stale chain. This is **not** a fidelity
failure.

**t2-chart / openpyxl — the 13 dropped parts** (everything that made the
workbook more than a grid): `calcChain.xml`, both chart relationship files
(`charts/_rels/chart1.xml.rels`, `chart2.xml.rels`), both chart color parts
(`colors1.xml`, `colors2.xml`), both chart style parts (`style1.xml`,
`style2.xml`), both comment parts (`comments1.xml`, `comments2.xml`), both VML
drawings (`vmlDrawing1.vml`, `vmlDrawing2.vml`), `printerSettings1.bin`, and
`sharedStrings.xml`. The chart XML parts that *remain* are not byte-identical —
they were rewritten and lost their `_rels`, which is why the file will not
reopen.

---

## 3. Finding

> **Both arms land the requested cell edit, but only the tool boundary preserves
> the workbook.** On t2-chart the status-quo openpyxl agent produced a file that
> the agent *believed* it had edited successfully yet which **will not reopen**
> (2 charts + pivot stripped of their relationships, 13 parts dropped, 1 of 50
> parts byte-identical) — a live reproduction of the #22044 harm, committed by a
> real agent. The xlq-confined agent completed the **same task on the same file**
> with charts and pivot **byte-identical**, 48 of 50 parts untouched, and left a
> signed receipt (`rev 1`, base→result hash chain). **An agent confined to `xlq`
> could not commit the #22044 corruption even when it tried the identical edit.**

**Honesty on arm failures:**

- **openpyxl FAILED t2-chart** — corrupt, non-reloadable output, reported as a
  success by the agent. This is the headline failure.
- **openpyxl PASSED t1-vba** — but only because the agent volunteered
  `keep_vba=True`; it still rewrote 9/11 parts and dropped `sharedStrings.xml`.
  It did not *destroy* the VBA here; it degraded fidelity.
- **xlq PASSED both tasks.** It failed nothing. The only part it "dropped"
  (`calcChain.xml`, t2) is a recomputable cache, not content. All
  feature-bearing parts survived byte-identical in both tasks.

---

## 4. Threats to validity

State plainly; do not let the reader over-generalize.

- **T1 — n = 2 tasks.** Two workbooks, two features. This demonstrates the harm
  and the prevention *exist and are reproducible under an agent*; it is not a
  rate estimate. It does not claim "openpyxl corrupts X% of workbooks."
- **T2 — single operation type.** Both tasks are one `set_cell` each. We have
  not exercised structural edits (insert/delete rows, add sheets, formula
  rewrites) under the A/B. The causal claim is scoped to value edits.
- **T3 — the "agent" is an LLM subagent with a bounded tool surface.** It is a
  real model making real tool calls, but it is not a full autonomous IDE agent
  with retrieval, retries, or user follow-up. A more capable agent might notice
  and repair the openpyxl corruption; a less careful one would ship it. We
  measured a single trajectory per cell, not a distribution over agent skill.
- **T4 — the openpyxl arm here is the *charitable* openpyxl.** On t1 the agent
  used `keep_vba=True`. The **realistic default path is `keep_vba=False`**, which
  drops the VBA project entirely and silently — a strictly worse, and more
  common, outcome than what we recorded. Our openpyxl numbers therefore
  *understate* the status-quo harm, not overstate it.
- **T5 — "reloads in engine" is checked against ironcalc**, the same engine xlq
  vendors. The t2 openpyxl failure also reproduces under openpyxl's own reader
  (the `Chartsheet.defined_names` load-error), so the failure is not an
  engine-choice artifact — but a reader that tolerates missing chart `_rels`
  might open the file in a degraded state rather than erroring.

---

## 5. The causal claim and its precise scope

**Claim.** Holding the agent, the task, the input workbook, and the environment
fixed, **routing the edit through the `xlq` boundary instead of openpyxl changes
the outcome from "feature-destroying, non-reloadable, unattested" to
"byte-identical on all feature parts, reloadable, and receipted."** On the
chart/pivot task this is the difference between reproducing #22044 and being
structurally unable to.

**Scope / what this does *not* claim.**

- It is an **existence-and-prevention** result over 2 tasks, not a corruption
  *rate*.
- It covers **single value edits** (`set_cell`), not structural mutations.
- The agent is a **bounded LLM subagent**, one trajectory per cell — not a
  guarantee over all agents or all skill levels.
- xlq's guarantee is **mechanical, not magical**: it holds because `xlq apply`
  rewrites only the parts an op touches and refuses whole-container rewrites. The
  A/B is evidence that this mechanism *survives contact with a real agent*, which
  the earlier fidelity benchmarks (static, no agent) could not show.

---

## Transcripts

### t1-vba / openpyxl
The agent loaded with `keep_vba=True`, set `Data!B3 = 250`, saved, and reloaded
to confirm 120 → 250. VBA survived; `sharedStrings.xml` was dropped and 9/11
parts were rewritten.

### t1-vba / xlq
`xlq inspect` (base hash `99bd0748…`) → wrote `patch.json` (set_cell Data!B3=250,
actor `agent`) → `xlq apply --dry-run` (2 affected: B3=250, B5 SUM→490, no new
errors) → `xlq apply`. Result: `rev 1`, result hash `9e20e766…`, fidelity
10/11 parts byte-identical, 1 rewritten. Receipt: `macro.xlsm.xlq.jsonl`.

### t2-chart / openpyxl
The agent set `Sheet1!A2 = 900` with openpyxl and saved. Its **post-save reload
raised** `'Chartsheet' object has no attribute 'defined_names'`; the agent
worked around the check by reading raw XML (`<c r="A2" t="n"><v>900</v></c>`) and
**reported success**. External check: file does not reopen → task failed. 13
parts dropped, charts/pivot rewritten and de-related, 1/50 byte-identical.

### t2-chart / xlq
`xlq inspect` (base hash `35010189…`) → `patch.json` (set_cell Sheet1!A2=900,
actor `agent`) → `xlq apply --dry-run` (A2 222.0→900.0, 1 affected, no new
errors, `write_reliable: true`) → `xlq apply`. Result: `rev 1`, fidelity 48/50
byte-identical, 2 rewritten (the edited sheet + dropped `calcChain.xml` cache).
Receipt: `pivot-chart.xlsx.xlq.jsonl`.
