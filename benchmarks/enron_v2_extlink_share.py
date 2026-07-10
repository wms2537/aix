#!/usr/bin/env python3
"""Zip-grounded attribution of Enron-v2 own-certify refusals to denylist part
classes (generates enron_v2_extlink_share.json). POST-HOC read-only analysis of
the locked v2 corpus: re-derives eligibility + leg 3a deterministically (same
calls as inthewild_run_v2), then inspects each REFUSED file's zip for
externalLinks / charts / pivot parts. Labeled exploratory-confirmatory for the
pre-registered v2-extlink-share ledger row."""
import sys, os, json, zipfile, shutil
sys.path.insert(0, "/home/soh/aix/benchmarks")
from inthewild_run_v2 import eligible_files, xlq_edit, certify

W = "/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/scratchpad/extlink"
os.makedirs(W, exist_ok=True)
files, _ = eligible_files("/home/soh/aix/data/inthewild/enron/converted_v2", 500)
refused = []
for i, (p, sheet, forms) in enumerate(files):
    work = os.path.join(W, str(i)); os.makedirs(work, exist_ok=True)
    try:
        xf, _ = xlq_edit(p, sheet, "insert-rows", 2, 1, work)
        if xf:
            v, detail = certify(p, xf, sheet)
            if v == "REFUSED":
                names = zipfile.ZipFile(p).namelist()
                refused.append((os.path.basename(p),
                                any(n.startswith("xl/externalLinks/") for n in names),
                                any(n.startswith("xl/charts/") for n in names),
                                any(n.startswith(("xl/pivotTables/", "xl/pivotCache/")) for n in names)))
    except Exception:
        pass
    shutil.rmtree(work, ignore_errors=True)
n = len(refused)
out = {"refused": n,
       "with_externalLinks": sum(1 for _, e, _, _ in refused if e),
       "share": round(sum(1 for _, e, _, _ in refused if e) / n, 4),
       "with_charts": sum(1 for _, _, c, _ in refused if c),
       "with_pivots": sum(1 for _, _, _, pv in refused if pv),
       "extlink_sole": sum(1 for _, e, c, pv in refused if e and not c and not pv),
       "per_file": [{"file": f, "extlink": e, "chart": c, "pivot": pv} for f, e, c, pv in refused]}
json.dump(out, open("/home/soh/aix/benchmarks/enron_v2_extlink_share.json", "w"), indent=2)
print(json.dumps({k: v for k, v in out.items() if k != "per_file"}, indent=1))
