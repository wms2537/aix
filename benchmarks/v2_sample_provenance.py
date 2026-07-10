#!/usr/bin/env python3
"""Generates benchmarks/v2_sample_provenance.json — the committed artifact behind
every §5.10 sampling/contamination number that previously lived only in prose:

  * the EUSES v2 cap-500 per-category composition (recomputed via the harness's
    own eligibility walk, pinned to the locked counters 500/1223/2924);
  * EUSES cap-vs-v1 byte-identical overlap (sha256);
  * Enron v2 eligible-leg source-document overlap with v1's converted corpus
    (basename match) and its byte-identity count;
  * raw corpus walk counts (incl. the one dot-prefixed file glob cannot see —
    disclosed rather than papered over).

Deterministic; read-only over the locked corpora. Run:
  <venv-python> benchmarks/v2_sample_provenance.py
"""
import glob
import hashlib
import json
import os
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from inthewild_run_v2 import eligible_files

ROOT = "/home/soh/aix"
OUT = os.path.join(ROOT, "benchmarks", "v2_sample_provenance.json")


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def main():
    # --- EUSES cap composition, pinned to the locked eligibility counters ---
    files, c = eligible_files(f"{ROOT}/data/inthewild/euses/converted_v2", 500)
    counters = (c["eligible"], c["ineligible_lt2_formulas"], c["beyond_cap"])
    assert counters == (500, 1223, 2924), f"eligibility drifted: {counters}"
    comp = Counter(p.split("/converted_v2/")[1].split("/")[0] for p, _, _ in files)

    # --- EUSES cap vs v1 byte-identical overlap ---
    v1 = {os.path.basename(p): sha(p)
          for p in glob.glob(f"{ROOT}/data/inthewild/euses/converted/*.xlsx")}
    euses_byte_identical = sum(
        1 for p, _, _ in files
        if os.path.basename(p) in v1 and sha(p) == v1[os.path.basename(p)])

    # --- Enron v2 eligible leg vs v1 source-document overlap ---
    efiles, ec = eligible_files(f"{ROOT}/data/inthewild/enron/converted_v2", 500)
    ecounters = (ec["eligible"], ec["ineligible_lt2_formulas"], ec["parse_failed"])
    assert ecounters == (362, 432, 5), f"enron eligibility drifted: {ecounters}"
    v1e = {os.path.basename(p): sha(p)
           for p in glob.glob(f"{ROOT}/data/inthewild/enron/converted/*.xlsx")}
    shared = [(p, os.path.basename(p)) for p, _, _ in efiles
              if os.path.basename(p) in v1e]
    enron_byte_identical = sum(1 for p, b in shared if sha(p) == v1e[b])

    # --- disk-vs-walk disclosure (glob skips one dot-prefixed EUSES file) ---
    disk_count = sum(1 for root, _, names in os.walk(f"{ROOT}/data/inthewild/euses/converted_v2")
                     for n in names if n.endswith(".xlsx"))
    walk_count = sum(counters)  # 500 + 1223 + 2924

    out = {
        "artifact": "v2 sampling + contamination provenance (locked-corpora, deterministic)",
        "euses_cap": {
            "eligibility_counters": {"eligible": 500, "lt2": 1223, "beyond_cap": 2924},
            "category_composition": dict(sorted(comp.items())),
            "categories_spanned": len(comp),
            "byte_identical_v1_copies": euses_byte_identical,
            "byte_identical_share": round(euses_byte_identical / 500, 4),
        },
        "enron_eligible_leg": {
            "eligibility_counters": {"eligible": 362, "lt2": 432, "parse_failed": 5},
            "source_document_overlap_with_v1": len(shared),
            "source_overlap_share": round(len(shared) / 362, 4),
            "byte_identical_of_shared": enron_byte_identical,
        },
        "euses_disk_vs_walk": {
            "xlsx_on_disk": disk_count,
            "visible_to_harness_walk": walk_count,
            "note": "glob-based eligibility walk does not see dot-prefixed names; "
                    "the one hidden file was never eligible for any leg",
        },
    }
    json.dump(out, open(OUT, "w"), indent=2)
    print(json.dumps(out, indent=1)[:900])


if __name__ == "__main__":
    main()
