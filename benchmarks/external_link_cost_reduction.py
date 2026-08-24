#!/usr/bin/env python3
"""Measure the shipped external-link passthrough on the retained Enron-v2 evidence.

The baseline is the committed post-hoc attribution of locked-v2 own-certify refusals.
This reruns only the 66 files whose sole recorded denial class was an external link,
using the same deterministic eligibility rule, edit, and certify calls as the locked
harness. It does not alter the frozen v2 result; it is a focused v3 cost probe.
"""
import json
import os
import shutil
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BENCH = ROOT / "benchmarks"
sys.path.insert(0, str(BENCH))
from inthewild_run_v2 import certify, eligible_files, xlq_edit  # noqa: E402

BASELINE = BENCH / "enron_v2_extlink_share.json"
OUTPUT = BENCH / "external_link_cost_reduction.json"
CORPUS = ROOT / "data/inthewild/enron/converted_v2"


def main():
    attribution = json.loads(BASELINE.read_text())
    wanted = {r["file"] for r in attribution["per_file"]
              if r["extlink"] and not r["chart"] and not r["pivot"]}
    files, eligibility = eligible_files(str(CORPUS), 500)
    selected = [(p, sheet) for p, sheet, _ in files if os.path.basename(p) in wanted]
    if len(selected) != len(wanted):
        raise SystemExit(f"baseline/corpus mismatch: found {len(selected)} of {len(wanted)}")

    work_root = Path("/tmp/aix-external-link-cost-probe")
    shutil.rmtree(work_root, ignore_errors=True)
    rows = []
    for i, (src, sheet) in enumerate(selected):
        work = work_root / str(i)
        work.mkdir(parents=True)
        try:
            edited, restructure_reason = xlq_edit(
                src, sheet, "insert-rows", 2, 1, str(work)
            )
            if edited is None:
                row = {"status": "REFUSED", "detail": f"restructure:{restructure_reason}"}
            else:
                status, detail = certify(src, edited, sheet)
                row = {"status": status, "detail": detail}
        except Exception as e:
            row = {"status": "ERROR", "detail": type(e).__name__}
        finally:
            shutil.rmtree(work, ignore_errors=True)
        rows.append({"file": os.path.basename(src), **row})

    statuses = Counter(r["status"] for r in rows)
    reasons = Counter(r["detail"] or r["status"] for r in rows if r["status"] != "CERTIFIED")
    newly_certified = statuses["CERTIFIED"]
    baseline_cost_numerator = 124  # 103 own refusals + 18 restructure + 3 timeouts
    projected_cost_numerator = baseline_cost_numerator - newly_certified
    out = {
        "benchmark": "external-link passthrough focused cost probe",
        "source_state": "ebb78da plus the uncommitted external-link exact-comparison guard",
        "baseline_artifact": "benchmarks/enron_v2_extlink_share.json",
        "operation": "insert-rows@2 count=1",
        "eligibility": dict(eligibility),
        "selected_sole_external_link_refusals": len(rows),
        "statuses": dict(statuses),
        "remaining_refusal_reasons": dict(reasons),
        "newly_certified": newly_certified,
        "own_cost_reduction_on_subset": round(newly_certified / len(rows), 4),
        "projected_enron_own_cost_numerator": {
            "before": baseline_cost_numerator,
            "after": projected_cost_numerator,
            "files_run": 362,
            "before_pct": round(baseline_cost_numerator / 362 * 100, 1),
            "after_pct": round(projected_cost_numerator / 362 * 100, 1),
        },
        "per_file": rows,
    }
    OUTPUT.write_text(json.dumps(out, indent=2) + "\n")
    print(json.dumps({k: v for k, v in out.items() if k != "per_file"}, indent=2))


if __name__ == "__main__":
    main()
