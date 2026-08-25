# Expanded live-agent study v4

Date frozen before live scoring: 2026-08-25.

## Scope

This protocol freezes the existing deterministic `tasks_v3.json` selection as
the live input. It does not regenerate or reselect tasks. The task file SHA-256
is `c7fba373c99bd07bb4e66272c30a0026b9c67568d3506a7d6dfe01f2bb524cc9`.

The experiment runs two live conditions on the same model and task set:

- `careful`: deliberate instruction requiring per-reference verification.
- `hasty`: immediate-answer instruction explicitly forbidding double-checking.

The same frontier model, `openrouter/stealth/ox-alpha`, serves both arms so the
prompt condition remains the manipulated variable. This follows the prior live
study design; the arms are paired conditions, not independent models.

## Request contract

- Endpoint: local codex-router OpenAI-compatible `/responses`.
- Model: `openrouter/stealth/ox-alpha`.
- Temperature: `0`.
- Maximum output tokens per request: `4096`.
- Primary requests: 100 tasks x 2 arms = 200.
- Retries: at most 4 additional attempts per failed primary request.
- Hard request ceiling: 400 API calls including retries.
- Backoff: 45 seconds, then doubling between retries.
- Authorization: operator-approved on 2026-08-25 for this benchmark lane only.
- Reported provider cost at smoke time: `$0`; the runner records returned usage.

The prompt contains only task metadata, public-corpus formula text, cached error
or value strings, and the fixed instruction. No repository paths, workbook bytes,
private files, or credentials are sent.

## Output contract

The model returns one JSON map from original cell address to post-operation
formula body. The scorer ignores extra addresses and leaves unanswered hosts
unshifted, matching the established artifact protocol. The frozen prompt requires
every supplied original address, including deleted-band hosts, so scorer-owned
deletion semantics are explicit.

## Execution and scoring

1. Commit this protocol and `live_v4.py` before any scored live run.
2. Run a one-task live validation with task index 27.
3. Run all 100 `careful` tasks, then all 100 `hasty` tasks.
4. Score both output files with unchanged `score_v3.py` against `tasks_v3.json`.
5. Commit raw outputs, call metadata, results, dated analysis, research-log line,
   and any paper update in one evidence commit.

The one-task validation may consume up to two additional calls under the retry
policy. It is excluded from the scored result but included in authorization
accounting.
