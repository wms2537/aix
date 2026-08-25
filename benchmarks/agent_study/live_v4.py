#!/usr/bin/env python3
"""Run frozen v4 live-agent arms through the local OpenAI-compatible router."""
import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).parent
MODEL = "openrouter/stealth/ox-alpha"
TEMPERATURE = 0.0
MAX_OUTPUT_TOKENS = 4096
RETRIES = 5
MAX_CALLS_PER_ARM = 200

COMMON = """Return only a minified JSON object. No prose and no code fence.
Keys must be the ORIGINAL cell addresses provided by the task.
Values must be formula bodies (without a leading equals sign) for each formula's
post-edit destination.
Adjust cell references, ranges, whole-row/whole-column references, sheet
qualifiers, absolute markers, and structured references exactly as spreadsheet
recalculation semantics require.
Do not change function names, argument order, operators, literals, or logic.
Always return every supplied original address, including formulas whose host cell is deleted; the scorer applies deletion semantics.
Do not return addresses that were not supplied."""

CAREFUL = """Carefully reason about every reference before answering.
Check every range endpoint, absolute marker, row/column insertion or deletion,
and whether a referenced source was deleted. Preserve formulas exactly except
for required reference changes.""" + "\n\n" + COMMON

HASTY = """Answer immediately with minimal checking. Do not double-check your
work.""" + "\n\n" + COMMON

ARMS = {"careful": CAREFUL, "hasty": HASTY}


def base_url():
    value = os.environ.get("CODEX_ROUTER_OPENAI_BASE_URL") or os.environ.get(
        "OPENAI_BASE_URL"
    )
    if not value:
        raise SystemExit("set CODEX_ROUTER_OPENAI_BASE_URL to the router /v1 URL")
    return value.rstrip("/")


def prompt_for(task):
    cells = "\n".join(f"{c['cell']}: {c['formula']}" for c in task["cells"])
    return (
        f"Workbook first-sheet name: {task['sheet']}\n"
        f"Operation: {task['operation']} at position {task['at']} "
        f"(count {task['count']}).\n"
        "Original formulas:\n"
        f"{cells}"
    )


def response_text(payload):
    chunks = []
    for item in payload.get("output", []):
        if item.get("type") == "message":
            for content in item.get("content", []):
                if content.get("type") == "output_text":
                    chunks.append(content["text"])
    text = "".join(chunks).strip()
    match = re.search(r"\{.*\}", text, re.S)
    if not match:
        raise ValueError(f"no JSON object in response: {text[:120]}")
    result = json.loads(match.group(0))
    if not isinstance(result, dict) or not all(
        isinstance(k, str) and isinstance(v, str) for k, v in result.items()
    ):
        raise ValueError("response is not a JSON string map")
    return result


def call_model(instructions, user_input, record_attempt):
    body = {
        "model": MODEL,
        "instructions": instructions,
        "input": user_input,
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "temperature": TEMPERATURE,
    }
    request = urllib.request.Request(
        base_url() + "/responses",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    delay = 5
    last_error = None
    attempts = []
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                payload = json.load(response)
            result = response_text(payload)
            meta = {
                "status": payload.get("status"),
                "usage": payload.get("usage"),
            }
            event = {"attempt": attempt + 1, "status": "completed", **meta}
            attempts.append(event);record_attempt(event)
            return result, meta, attempts
        except (urllib.error.HTTPError, urllib.error.URLError, ValueError,
                TimeoutError, json.JSONDecodeError) as exc:
            last_error = f"{type(exc).__name__}: {exc}"
            event = {"attempt": attempt + 1, "status": "failed", "error": last_error}
            attempts.append(event);record_attempt(event)
            if attempt < RETRIES - 1:
                time.sleep(delay)
                delay *= 2
    raise RuntimeError(last_error)


def run_arm(arm, tasks_path, out_path, limit=None):
    tasks = json.loads(Path(tasks_path).read_text())["tasks"]
    if limit is not None:
        tasks = tasks[:limit]
    outputs = json.loads(out_path.read_text()) if out_path.exists() else {}
    calls_path = Path(str(out_path) + ".calls.json")
    calls = json.loads(calls_path.read_text()) if calls_path.exists() else []
    attempts_path = Path(str(out_path) + ".attempts.json")
    if attempts_path.exists():
        attempts = json.loads(attempts_path.read_text())
    else:
        attempts = [{"task": c["task"], "attempt": 1, **c} for c in calls]
        failed_key = "data/inthewild/enron/converted_v2/edrm/native_000%2F3.548757.NIHYSE40U2KXPCI31RZUJR1O1ZASHYDYB.1.xlsx#delete-rows@5"
        attempts.extend(
            {"task": failed_key, "attempt": number, "status": "failed", "error": "JSONDecodeError"}
            for number in range(1, 6)
        )
        attempts_path.write_text(json.dumps(attempts, indent=2) + "\n")
    out_path.parent.mkdir(parents=True, exist_ok=True)
    for index, task in enumerate(tasks):
        key = f"{task['file']}#{task['operation']}@{task['at']}"
        if key in outputs:
            print(f"[{index + 1}/{len(tasks)}] {arm} {key} resume", flush=True)
            continue
        if len(attempts) >= MAX_CALLS_PER_ARM:
            raise RuntimeError(f"{arm} exhausted its {MAX_CALLS_PER_ARM}-call budget")
        def record_attempt(event):
            attempts.append({"task": key, **event})
            attempts_path.write_text(json.dumps(attempts, indent=2) + "\n")
        try:
            answer, meta, _ = call_model(ARMS[arm], prompt_for(task), record_attempt)
        except RuntimeError as exc:
            print(f"FAILED {key}: {exc}", flush=True)
            continue
        outputs[key] = answer
        calls.append({"task": key, **meta})
        print(f"[{index + 1}/{len(tasks)}] {arm} {key}", flush=True)
        Path(str(out_path) + ".calls.json").write_text(json.dumps(calls, indent=2) + "\n")
        Path(out_path).write_text(json.dumps(outputs, indent=2) + "\n")
    print(f"wrote {out_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--arm", choices=tuple(ARMS), required=True)
    parser.add_argument("--tasks", type=Path, default=HERE / "tasks_v3.json")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--limit", type=int)
    args = parser.parse_args()
    run_arm(args.arm, args.tasks, args.out, args.limit)
