#!/usr/bin/env python3
"""Reproduction package for paper/paper-v3.md — verify every quantitative claim
against the committed artifacts, without re-running any experiment.

  python3 repro/verify_claims.py          # exit 0 iff every claim PASSes

Each claim is a row: (claim_id, paper location, claimed value, artifact,
extraction). The script loads the committed JSON/TSV/MD artifact, extracts the
number, and checks it against what the paper says. Three verdicts:

  PASS  the committed artifact reproduces the paper's number
  FAIL  it does not (a finding — report it, do not fudge it)
  SKIP  the check needs a tool this machine lacks (lean / z3); the claim is
        NOT verified here and needs the re-run documented in repro/README.md

Only stdlib. The Lean/Z3 sections *re-check the proofs* (fast, deterministic)
when the toolchain is present; everything else verifies committed artifacts.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# --------------------------------------------------------------------------
# artifact loaders (cached)
# --------------------------------------------------------------------------
_json_cache = {}


def J(rel):
    """Load a committed JSON artifact (cached)."""
    if rel not in _json_cache:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as fh:
            _json_cache[rel] = json.load(fh)
    return _json_cache[rel]


def T(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as fh:
        return fh.read()


def approx(actual, expected, tol=1e-9):
    return abs(actual - expected) <= tol


def pct1(x):
    """Round a fraction to one decimal in percent, like the paper does."""
    return round(x * 1000) / 10.0


# --------------------------------------------------------------------------
# claim table
# --------------------------------------------------------------------------
# Each entry: (claim_id, paper_loc, claimed, artifact, fn)
#   fn() -> (ok: bool, actual: str)
CLAIMS = [
]


def claim(cid, loc, claimed, artifact):
    def register(fn):
        CLAIMS.append((cid, loc, claimed, artifact, fn))
        return fn

    return register


def expect(cid, loc, claimed_display, artifact, extract, expected, tol=None):
    """Register a claim whose extracted value must equal `expected`."""

    @claim(cid, loc, claimed_display, artifact)
    def _fn(extract=extract, expected=expected, tol=tol):
        actual = extract()
        if tol is not None:
            ok = approx(actual, expected, tol)
        else:
            ok = actual == expected
        return ok, repr(actual)


FC = "benchmarks/foreign_certify.json"
AB = "benchmarks/agent_ab.json"
ABMD = "benchmarks/EDIT_PATH_AB.md"
AB2 = "benchmarks/agent_ab_v2.json"
SCR = "benchmarks/shift_correctness_real.json"
MCR = "benchmarks/move_correctness_real.json"
CC1 = "benchmarks/cert_confusion.json"
CC2 = "benchmarks/cert_confusion_v2.json"
TF = "benchmarks/tokenizer_fuzz.json"
TC = "benchmarks/tokenizer_conformance.json"
CV2 = "benchmarks/conformance_v2.json"
COMP = "experiments/generality/composition_coverage.json"
DBT = "experiments/dbt/dbt_results.json"
EDIST = "benchmarks/edit_distribution.json"
CAREFUL = "benchmarks/agent_study/results_live_careful.json"
HASTY = "benchmarks/agent_study/results_live_hasty.json"
PERFECT = "benchmarks/agent_study/results_smoke_perfect.json"
SLOPPY = "benchmarks/agent_study/results_smoke_sloppy.json"
CQ = "benchmarks/coincidence_q.json"
CMC = "benchmarks/coincidence_mc.json"
CQE = "benchmarks/coincidence_q_euses.json"
CQN = "benchmarks/coincidence_q_enron.json"
IWE = "benchmarks/inthewild_euses.json"
IWN = "benchmarks/inthewild_enron.json"
IWD = "benchmarks/inthewild_dbt.json"
M38 = "benchmarks/euses_38_mismatch_classification.json"
TSV = "results.tsv"

# ---------------------------------------------------------------- §5.1 ----
expect("5.1-foreign-edits-tested", "§5.1", "172 foreign edits ruled on",
       FC, lambda: J(FC)["foreign_edits_tested"], 172)
expect("5.1-false-certifications", "§5.1", "0 false certifications",
       FC, lambda: J(FC)["FALSE_CERTIFICATIONS_ref_shift_corruption_certified"], 0)
expect("5.1-corrupted-refused-147of147", "§5.1", "147/147 corrupted foreign edits refused",
       FC, lambda: (J(FC)["corrupted_foreign_edits_caught_REFUSED"],
                    J(FC)["corrupted_foreign_edits_total_excl_oracle_noise"],
                    J(FC)["recall_on_corrupted_foreign_edits"]), (147, 147, 1.0))
expect("5.1-faithful-1-in-23", "§5.1", "certifies ~1 in 23 faithful foreign edits",
       FC, lambda: (J(FC)["faithful_foreign_certified"],
                    J(FC)["faithful_foreign_certified"] + J(FC)["faithful_foreign_refused_conservative"]),
       (1, 23))

# ---------------------------------------------------------------- §5.2 ----
expect("5.2-files-evaluated", "§5.2", "172 development-tier workbooks",
       AB, lambda: J(AB)["files_evaluated"], 172)
expect("5.2-openpyxl-149-as-measured", "§5.2", "openpyxl corrupts 149/172 as originally measured",
       AB, lambda: (J(AB)["openpyxl_path"]["SILENT_CORRUPTION"],
                    J(AB)["openpyxl_path"]["silent_corruption_rate"]), (149, 0.866))
expect("5.2-xlq-150-confirmed", "§5.2", "tool edit engine-confirmed faithful on 150/172",
       AB, lambda: J(AB)["xlq_certify_or_refuse"]["certified_faithful_engine_confirmed"], 150)
expect("5.2-xlq-22-refused", "§5.2", "explicitly refuses the remaining 22",
       AB, lambda: J(AB)["xlq_certify_or_refuse"]["refused_principled"], 22)
expect("5.2-xlq-0-silent", "§5.2", "0 silent corruptions on the guarded path",
       AB, lambda: (J(AB)["xlq_certify_or_refuse"]["certified_but_WRONG_false_certification"],
                    J(AB)["xlq_certify_or_refuse"]["false_certification_rate"]), (0, 0.0))


@claim("5.2-confirmed-genuine-85.5", "§5.2/§6",
       "147/172 = 85.5% confirmed-genuine after 2 mislabels corrected", ABMD)
def _c855():
    txt = T(ABMD)
    ok = re.search(r"147/172\s*=\s*85\.5%\s*\nconfirmed-genuine", txt) is not None
    ok = ok and re.search(r"2 confirmed label", txt) is not None
    return ok, "corrected-headline text present" if ok else "text NOT found"


expect("5.2-v2-insert-rows-100", "§5.2", "naive path corrupts insert-rows 100%",
       AB2, lambda: [o for o in J(AB2)["per_op"] if o["op"] == "insert-rows"][0]["openpyxl"]["libreoffice"]["rate"], 1.0)
expect("5.2-v2-delete-rows-98.8", "§5.2", "delete-rows 98.8%",
       AB2, lambda: [o for o in J(AB2)["per_op"] if o["op"] == "delete-rows"][0]["openpyxl"]["libreoffice"]["rate"], 0.988)
expect("5.2-v2-insert-cols-94.3", "§5.2", "insert-cols 94.3%",
       AB2, lambda: [o for o in J(AB2)["per_op"] if o["op"] == "insert-cols"][0]["openpyxl"]["libreoffice"]["rate"], 0.943)
expect("5.2-v2-delete-cols-80.2", "§5.2", "delete-cols 80.2%",
       AB2, lambda: [o for o in J(AB2)["per_op"] if o["op"] == "delete-cols"][0]["openpyxl"]["libreoffice"]["rate"], 0.802)
expect("5.2-v2-overall-93.8", "§5.2", "93.8% overall naive silent corruption",
       AB2, lambda: (J(AB2)["openpyxl_silent_corruption"]["libreoffice"][2],
                     J(AB2)["openpyxl_silent_corruption"]["formulas"][2]), (0.938, 0.938))
expect("5.2-v2-xlq-0-every-op", "§5.2", "the tool 0% on every op (both engines)",
       AB2, lambda: sorted({o["xlq"][e]["rate"] for o in J(AB2)["per_op"] for e in ("libreoffice", "formulas")}),
       [0.0])


@claim("5.2-v2-engines-agree", "§5.2", "the formulas engine agrees with LibreOffice exactly", AB2)
def _agree():
    d = J(AB2)
    ok = d["openpyxl_silent_corruption"]["libreoffice"] == d["openpyxl_silent_corruption"]["formulas"]
    ok = ok and d["xlq_silent_corruption"]["libreoffice"] == d["xlq_silent_corruption"]["formulas"]
    for o in d["per_op"]:
        ok = ok and o["xlq"]["libreoffice"] == o["xlq"]["formulas"]
        ok = ok and o["openpyxl"]["libreoffice"] == o["openpyxl"]["formulas"]
    return ok, "identical per-op verdict arrays" if ok else "engines DISAGREE somewhere"


expect("5.2-real-insert-rows-1651", "§5.2", "insert-rows 1651 real cells, 0 mismatches",
       SCR, lambda: (J(SCR)["per_op"]["insert-rows"]["cells_checked"],
                     J(SCR)["per_op"]["insert-rows"]["xlq_mismatch"]), (1651, 0))
expect("5.2-real-delete-rows-1608", "§5.2", "delete-rows 1608, 0 mismatches",
       SCR, lambda: (J(SCR)["per_op"]["delete-rows"]["cells_checked"],
                     J(SCR)["per_op"]["delete-rows"]["xlq_mismatch"]), (1608, 0))
expect("5.2-real-insert-cols-1648", "§5.2", "insert-cols 1648, 0 mismatches",
       SCR, lambda: (J(SCR)["per_op"]["insert-cols"]["cells_checked"],
                     J(SCR)["per_op"]["insert-cols"]["xlq_mismatch"]), (1648, 0))
expect("5.2-real-delete-cols-1076", "§5.2", "delete-cols 1076, 0 mismatches",
       SCR, lambda: (J(SCR)["per_op"]["delete-cols"]["cells_checked"],
                     J(SCR)["per_op"]["delete-cols"]["xlq_mismatch"]), (1076, 0))


@claim("5.2-real-approx-6000-cells", "§5.2", "~6,000 real formula cells, xlq 100% on every op", SCR)
def _real6000():
    d = J(SCR)["per_op"]
    total = sum(v["cells_checked"] for v in d.values())
    rates = {v["xlq_correct_rate"] for v in d.values()}
    ok = 5900 <= total <= 6100 and rates == {1.0}
    return ok, f"total={total}, xlq_correct_rates={sorted(rates)}"


@claim("5.2-real-naive-17-72pct", "§5.2", "naive path leaves 17-72% of shift-requiring cells wrong", SCR)
def _naiverange():
    d = J(SCR)["per_op"]
    lo = min(v["openpyxl_wrong_rate"] for v in d.values())
    hi = max(v["openpyxl_wrong_rate"] for v in d.values())
    ok = pct1(lo) == 17.0 and pct1(hi) == 72.1  # 17% and 72% as rounded in the paper
    ok = ok and 0.17 <= lo < 0.18 and 0.72 <= hi < 0.73
    return ok, f"min={lo}, max={hi}"


# --------------------------------------------------------------- §5.2b ----
expect("5.2b-move-1538-cells-100pct", "§5.2b", "move-rows 100% on 1,538 real formula cells",
       MCR, lambda: (J(MCR)["cells_checked"], J(MCR)["xlq_mismatch"], J(MCR)["xlq_correct_rate"]),
       (1538, 0, 1.0))
expect("5.2b-move-25of60-failclosed", "§5.2b", "25/60 workbooks fail-closed as residual/straddle",
       MCR, lambda: (J(MCR)["workbooks_refused_or_residual"], J(MCR)["workbooks_checked"]), (25, 60))

# ---------------------------------------------------------------- §5.3 ----
expect("5.3-45-corruptions-0-false-cert", "§5.3", "all 45 injected corruptions refused, 0 false certs",
       CC2, lambda: (J(CC2)["known_corrupt_edits"], J(CC2)["FALSE_CERTIFICATIONS"]), (45, 0))
expect("5.3-each-type-15of15", "§5.3", "each corruptor type 15/15 refused",
       CC2, lambda: {k: v.get("refused") for k, v in J(CC2)["by_corruptor_certify_verdict"].items()},
       {"openpyxl": 15, "unshift_one": 15, "wrong_delta": 15})
expect("5.3-15-faithful-certified", "§5.3", "all 15 faithful tool-produced edits certified",
       CC2, lambda: (J(CC2)["known_faithful_edits"], J(CC2)["faithful_falsely_refused"]), (15, 0))
expect("5.3-6-value-preserving-caught", "§5.3", "6 value-preserving corruptions refused (stricter than value check)",
       CC2, lambda: J(CC2)["value_preserving_corruptions_certify_caught_but_value_oracle_missed"], 6)


@claim("5.3-initial-matrix-14-fn0", "§5.3", "initial single-corruptor matrix: FN=0 on 14 edits (rule-of-three ~21%)", CC1)
def _cc1():
    d = J(CC1)
    cm = d["confusion_matrix"]
    ok = cm["TP_refused_corrupt"] == 14 and cm["FN_certified_corrupt_FALSE_CERT"] == 0
    r3 = 3.0 / 14  # rule of three
    ok = ok and abs(r3 - 0.21) < 0.005
    return ok, f"TP=14, FN=0, rule-of-three={r3:.3f}"


# ---------------------------------------------------------------- §5.4 ----
expect("5.4-live-agent-20-workbooks", "§5.4", "live fast-model agent on 20 real workbooks",
       "benchmarks/agent_outputs_all.json", lambda: len(J("benchmarks/agent_outputs_all.json")), 20)

# ---------------------------------------------------------------- §5.5 ----
expect("5.5-collapse-100-84-68-52", "§5.5/§7", "100/84/68/52% certified untouched as fills grow",
       COMP, lambda: [t["collapse_pct"] for t in J(COMP)["per_task"]], [100, 84, 68, 52])
expect("5.5-mean-collapse-76", "§5.5/§7", "mean 76% of the artifact certified untouched",
       COMP, lambda: J(COMP)["avg_audit_surface_collapse_pct"], 76.0)
expect("5.5-all-scaffolds-certified", "§5.5/§7", "every scaffold is certified",
       COMP, lambda: J(COMP)["all_scaffolds_certified"], True)


@claim("5.5-27-to-87pct", "§5.5/§7", "certifiable component rises from 27% fully certified to 87% w/ scaffold", COMP)
def _2787():
    c = J(COMP)["edit_distribution_coverage"]
    ok = round(c["fully_certified_pure_structural_pct"]) == 27 and round(c["certifiable_scaffold_any_structural_pct"]) == 87
    return ok, f"{c['fully_certified_pure_structural_pct']}% -> {c['certifiable_scaffold_any_structural_pct']}%"


# ---------------------------------------------------------------- §5.6 ----
expect("5.6-ten-node-project", "§5.6", "ten-node staging->marts project",
       DBT, lambda: J(DBT)["project"]["total_nodes"], 10)
expect("5.6-faithful-rename-certified", "§5.6", "faithful rename CERTIFIED, 100% untouched, no engine run",
       DBT, lambda: (J(DBT)["a_faithful_rename"]["status"], J(DBT)["a_faithful_rename"]["collapse_ratio"],
                     J(DBT)["project"]["engine_free_certify_path"]), ("CERTIFIED", 1.0, True))
expect("5.6-dangling-ref-refused", "§5.6", "rename leaving a dangling downstream reference REFUSED",
       DBT, lambda: J(DBT)["b_rename_dangling_ref"]["status"], "REFUSED")
expect("5.6-silent-logic-change-refused", "§5.6", "SUM->AVG silent change REFUSED; re-materialization confirms values differ",
       DBT, lambda: (J(DBT)["c_rename_silent_logic_change"]["status"],
                     J(DBT)["c_rename_silent_logic_change"]["ground_truth_values_differ"]), ("REFUSED", True))
expect("5.6-declared-fill-certified-0.8", "§5.6", "declared logic change certified as scaffold, collapse 0.8, outside-cone preserved",
       DBT, lambda: (J(DBT)["d_rename_plus_declared_fill"]["status"],
                     J(DBT)["d_rename_plus_declared_fill"]["collapse_ratio"],
                     J(DBT)["d_rename_plus_declared_fill"]["ground_truth_outside_cone_preserved"]),
       ("CERTIFIED", 0.8, True))

# ---------------------------------------------------------------- §5.7 ----
expect("5.7-careful-2of21", "§5.7", "careful agent erred on 2/21 tasks",
       CAREFUL, lambda: (J(CAREFUL)["tasks_scored"], J(CAREFUL)["agent"]["tasks_incorrect"]), (21, 2))
expect("5.7-hasty-4of21", "§5.7", "hasty agent erred on 4/21 tasks",
       HASTY, lambda: (J(HASTY)["tasks_scored"], J(HASTY)["agent"]["tasks_incorrect"]), (21, 4))
expect("5.7-6-saves-all-blocked", "§5.7", "six saves total across the two arms, all blocked",
       CAREFUL, lambda: (J(CAREFUL)["SAVES_corrupt_edits_blocked"] + J(HASTY)["SAVES_corrupt_edits_blocked"],
                         J(CAREFUL)["GUARDED"]["corruption_incidence"],
                         J(HASTY)["GUARDED"]["corruption_incidence"]), (6, 0.0, 0.0))


@claim("5.7-saves-4-distinct-workbooks", "§5.7", "the six saves span four distinct workbooks", CAREFUL)
def _saves4():
    files = set()
    n = 0
    for rel in (CAREFUL, HASTY):
        for t in J(rel)["per_task"]:
            if not t["agent_correct"] and t["guard_verdict"] == "REFUSED":
                files.add(t["file"])
                n += 1
    ok = n == 6 and len(files) == 4
    return ok, f"{n} saves over {len(files)} distinct workbooks"


expect("5.7-false-certs-0", "§5.7", "zero false certifications in both live arms",
       CAREFUL, lambda: (J(CAREFUL)["FALSE_CERT_must_be_0"], J(HASTY)["FALSE_CERT_must_be_0"]), (0, 0))
expect("5.7-137of196-truth-visible", "§5.7", "137/196 truth-visible cells (196 formula cells in tasks)",
       CAREFUL, lambda: (J(CAREFUL)["agent"]["cells_evaluated"],
                         J(CAREFUL)["agent"]["cells_evaluated"] + J(CAREFUL)["agent"]["cells_excluded_from_truth_out_of_grammar"]),
       (137, 196))
expect("5.7-careful-zero-unambiguous-cost", "§5.7", "zero unambiguous completion cost (careful arm refuses 0 correct)",
       CAREFUL, lambda: (J(CAREFUL)["GUARDED"]["refused_correct_COST"],
                         J(CAREFUL)["COST_split"]["refused_correct_truth_total_unambiguous_cost"]), (0, 0))
expect("5.7-hasty-2-refusals-truth-partial", "§5.7", "two refusals of 'correct' hasty work, both on truth-partial tasks",
       HASTY, lambda: (J(HASTY)["GUARDED"]["refused_correct_COST"],
                       J(HASTY)["COST_split"]["refused_correct_truth_partial_possible_hidden_save"],
                       J(HASTY)["COST_split"]["refused_correct_truth_total_unambiguous_cost"]), (2, 2, 0))


@claim("5.7-11of19-truth-blind", "§5.7", "11 of 19 certified tasks carry cells outside the truth grammar", CAREFUL)
def _blind():
    cert = [t for t in J(CAREFUL)["per_task"] if t["guard_verdict"] == "CERTIFIED"]
    blind = [t for t in cert if t["truth_skipped_out_of_grammar"] > 0]
    ok = len(cert) == 19 and len(blind) == 11
    return ok, f"{len(blind)} of {len(cert)} certified tasks truth-blind"


expect("5.7-perfect-zero-both-arms", "§5.7", "synthetic perfect agent: zero corruption in both arms",
       PERFECT, lambda: (J(PERFECT)["UNGUARDED"]["shipped_CORRUPT"],
                         J(PERFECT)["GUARDED"]["shipped_CORRUPT_false_cert"]), (0, 0))
expect("5.7-sloppy-7of7-blocked", "§5.7", "synthetic sloppy agent: all seven injected corruptions blocked",
       SLOPPY, lambda: (J(SLOPPY)["UNGUARDED"]["shipped_CORRUPT"],
                        J(SLOPPY)["SAVES_corrupt_edits_blocked"],
                        J(SLOPPY)["GUARDED"]["shipped_CORRUPT_false_cert"]), (7, 7, 0))

# ---------------------------------------------------------------- §5.8 ----
_M2V = lambda: J(CQ)["models"]["M2v_offbyone_row"]["excel_semantics"]

expect("5.8-230-first-sheets", "§5.8", "measured on 230 real first sheets",
       CQ, lambda: J(CQ)["files_measured"], 230)
expect("5.8-pooled-q-0.178", "§5.8", "off-by-one collision rate q-hat = 0.178 pooled",
       CQ, lambda: round(_M2V()["pooled_q"], 3), 0.178)
expect("5.8-file-median-0.125", "§5.8", "file median 0.125",
       CQ, lambda: _M2V()["file_rate_median"], 0.125)
expect("5.8-p90-0.40", "§5.8", "p90 0.40",
       CQ, lambda: round(_M2V()["file_rate_p90"], 2), 0.40)
expect("5.8-k5-for-99pct", "§5.8", "k = 5 checked cells for 99% detection (mixture bound)",
       CQ, lambda: _M2V()["detection"]["mixture_dependent"]["k_for_99pct"], 5)


@claim("5.8-k9-10-for-99.9pct", "§5.8", "k = 9-10 for 99.9% (mixture; excel k=9, strict k=10)", CQ)
def _k910():
    excel = _M2V()["detection"]["mixture_dependent"]["k_for_99.9pct"]
    strict = J(CQ)["models"]["M2v_offbyone_row"]["strict_both_nonempty"]["detection"]["mixture_dependent"]["k_for_99.9pct"]
    ok = excel == 9 and strict == 10
    return ok, f"excel k={excel}, strict k={strict}"


@claim("5.8-mixture-30x-at-k5", "§5.8", "mixture bound ~30x above naive q^k at k=5", CQ)
def _amp5():
    det = _M2V()["detection"]
    r = det["mixture_dependent"]["miss_k"]["5"] / det["naive_independent"]["miss_k"]["5"]
    ok = 25 <= r < 35  # rounds to 30 at one significant figure
    return ok, f"ratio={r:.1f}x"


@claim("5.8-mixture-2e4x-at-k10", "§5.8", "mixture bound ~2x10^4 x above naive at k=10", CQ)
def _amp10():
    det = _M2V()["detection"]
    r = det["mixture_dependent"]["miss_k"]["10"] / det["naive_independent"]["miss_k"]["10"]
    ok = 1.5e4 <= r < 2.5e4
    return ok, f"ratio={r:.3g}x"


@claim("5.8-mc-tracks-within-25pct", "§5.8", "mixture bound tracks Monte-Carlo within ~25% at every k", CMC)
def _mc25():
    mc = J(CMC)["results"]["m2_openpyxl_insert2"]["mc_miss_k_mean_over_files"]
    mix = _M2V()["detection"]["mixture_dependent"]["miss_k"]
    devs = {k: abs(mc[k] - mix[k]) / mix[k] for k in mc}
    worst = max(devs.values())
    ok = worst <= 0.25
    return ok, f"max relative deviation={worst:.1%} over k={sorted(mc)}"


@claim("5.8-mc-above-mixture-k10", "§5.8", "the MC sits just above the mixture bound at k=10", CMC)
def _mck10():
    mc = J(CMC)["results"]["m2_openpyxl_insert2"]["mc_miss_k_mean_over_files"]["10"]
    mix = _M2V()["detection"]["mixture_dependent"]["miss_k"]["10"]
    ok = mix < mc < 3 * mix
    return ok, f"MC={mc:.2e} vs mixture={mix:.2e}"


_ANCHOR = lambda: J(CMC)["results"]["agent_ab_ground_truth_anchor"]

expect("5.8-161-error-present", "§5.8", "161 workbooks where the naive error is genuinely present",
       CMC, lambda: _ANCHOR()["error_present_files(affected>=1)"], 161)


@claim("5.8-19-pass-full-check-11.8pct", "§5.8", "19 (11.8%) pass a full k=N value check", CMC)
def _latent():
    ev = _ANCHOR()["engine_verified_latent_corruption"]
    ok = ev["count"] == 19 and ev["full_value_check_miss_rate_by_engine_truth"] == 0.118
    ok = ok and pct1(19 / 161) == 11.8
    return ok, f"count={ev['count']}, rate={ev['full_value_check_miss_rate_by_engine_truth']}"


@claim("5.8-conservative-12-7.5pct", "§5.8", "conservatively 12 = 7.5% (ARABIC_ROMAN reclassified text-catchable)", CMC)
def _latent12():
    ev = _ANCHOR()["engine_verified_latent_corruption"]
    ok = ev["by_class"]["output_noninjective"] == 13 and ev["by_class"]["oracle_blind"] == 6
    ok = ok and any("ARABIC_ROMAN" in f["file"] for f in ev["files"])
    conservative = ev["by_class"]["output_noninjective"] - 1  # minus ARABIC_ROMAN (text outputs differ)
    ok = ok and conservative == 12 and pct1(12 / 161) == 7.5
    return ok, f"13 output_noninjective - ARABIC_ROMAN = {conservative} ({pct1(12 / 161)}%)"


def _cqwild(rel):
    return J(rel)["models"]["M2v_offbyone_row"]["excel_semantics"]


expect("5.8-euses-761-sheets", "§5.8", "761 EUSES first sheets in the off-by-one distribution",
       CQE, lambda: _cqwild(CQE)["files_in_distribution"], 761)
expect("5.8-enron-777-sheets", "§5.8", "777 Enron first sheets in the off-by-one distribution",
       CQN, lambda: _cqwild(CQN)["files_in_distribution"], 777)
expect("5.8-euses-q-0.478", "§5.8/§5.9", "EUSES pooled q-hat = 0.478, file-median 0.104",
       CQE, lambda: (round(_cqwild(CQE)["pooled_q"], 3), round(_cqwild(CQE)["file_rate_median"], 3)),
       (0.478, 0.104))
expect("5.8-enron-q-0.566", "§5.8/§5.9", "Enron pooled q-hat = 0.566, file-median 0.245",
       CQN, lambda: (round(_cqwild(CQN)["pooled_q"], 3), round(_cqwild(CQN)["file_rate_median"], 3)),
       (0.566, 0.245))
expect("5.8-euses-k999-12", "§5.8", "99.9%-detection pushed to k=12 on EUSES",
       CQE, lambda: _cqwild(CQE)["detection"]["mixture_dependent"]["k_for_99.9pct"], 12)
expect("5.8-enron-k999-20", "§5.8", "99.9%-detection pushed to k=20 on Enron",
       CQN, lambda: _cqwild(CQN)["detection"]["mixture_dependent"]["k_for_99.9pct"], 20)

# ---------------------------------------------------------------- §5.9 ----
expect("5.9-euses-796-workbooks", "§5.9", "EUSES: 796 converted workbooks",
       IWE, lambda: J(IWE)["eligibility"]["ineligible_lt2_formulas"] + J(IWE)["eligibility"]["eligible"], 796)
expect("5.9-enron-786-workbooks", "§5.9", "Enron: 786 converted workbooks",
       IWN, lambda: (J(IWN)["eligibility"]["ineligible_lt2_formulas"] + J(IWN)["eligibility"]["eligible"]
                     + J(IWN)["eligibility"]["parse_failed"]), 786)


@claim("5.9-518-completed-0-false-certs", "§5.9",
       "0 false certs across 518 completed certify calls = 503 refused (158+345) + 15 fail-closed errors", IWE)
def _guard518():
    e, n = J(IWE)["leg3_guard"], J(IWN)["leg3_guard"]
    refused = e["opx_REFUSED"] + n["opx_REFUSED"]
    errors = e["opx_ERROR"] + n["opx_ERROR"]
    certified = sum(v for k, v in list(e.items()) + list(n.items()) if k.lower().startswith("opx_cert"))
    no_false_samples = J(IWE)["false_cert_samples"] == [] and J(IWN)["false_cert_samples"] == []
    ok = (refused == 503 and e["opx_REFUSED"] == 158 and n["opx_REFUSED"] == 345
          and errors == 15 and refused + errors == 518 and certified == 0 and no_false_samples)
    return ok, f"refused={refused} ({e['opx_REFUSED']}+{n['opx_REFUSED']}), errors={errors}, certified={certified}"


expect("5.9-7-edits-failed-precertify", "§5.9", "7 further edits failed before certification",
       IWE, lambda: J(IWE)["leg3_guard"]["opx_edit_failed"] + J(IWN)["leg3_guard"]["opx_edit_failed"], 7)


def _cells(rel):
    return sum(v["cells_checked"] for v in J(rel)["leg1_shift_correctness"].values())


def _mismatches(rel):
    return sum(v["xlq_mismatch"] for v in J(rel)["leg1_shift_correctness"].values())


expect("5.9-283960-cells-total", "§5.9", "283,960 real formula cells across the four ops",
       IWE, lambda: _cells(IWE) + _cells(IWN), 283960)
expect("5.9-enron-170796-cells-0-err", "§5.9", "Enron: 170,796 cells, 100% (0 mismatches)",
       IWN, lambda: (_cells(IWN), _mismatches(IWN),
                     {v["xlq_correct_rate"] for v in J(IWN)["leg1_shift_correctness"].values()}),
       (170796, 0, {1.0}))
expect("5.9-euses-113164-cells", "§5.9", "113,164 EUSES-leg cells",
       IWE, lambda: _cells(IWE), 113164)


@claim("5.9-38-mismatches-0.034pct", "§5.9", "the only defect: 38 cells = 0.034% of EUSES-leg cells", IWE)
def _m38():
    m = _mismatches(IWE)
    rate = round(m / _cells(IWE) * 100, 3)
    ok = m == 38 and rate == 0.034
    return ok, f"mismatches={m}, rate={rate}%"


@claim("5.9-38-all-encoding-one-file", "§5.9",
       "all 38 in one Japanese workbook; UTF-8 double-encoding, NOT a shift error", M38)
def _m38class():
    d = J(M38)
    per_op = d["per_op_counts"]
    ok = sum(per_op.values()) == 38 and per_op == {"insert-rows": 10, "delete-rows": 9,
                                                   "insert-cols": 10, "delete-cols": 9}
    ok = ok and d["file"] == "10341003.xlsx" and "double-encoding" in d["defect"]
    # cross-check against the locked-run samples: every sampled mismatch is in that one file
    samples = J(IWE)["mismatch_samples"]
    ok = ok and samples and all(s["file"] == "10341003.xlsx" for s in samples)
    return ok, f"per_op={per_op}, file={d['file']}"


@claim("5.9-failclosed-euses-19.6pct", "§5.9", "guarded pipeline refuses 19.6% of eligible EUSES files", IWE)
def _fce():
    d = J(IWE)
    eligible = d["eligibility"]["eligible"]
    not_cert = eligible - d["leg3_guard"]["own_CERTIFIED"]
    ok = pct1(not_cert / eligible) == 19.6
    return ok, f"{not_cert}/{eligible} = {pct1(not_cert / eligible)}%"


@claim("5.9-failclosed-enron-32.0pct", "§5.9", "guarded pipeline refuses 32.0% of eligible Enron files", IWN)
def _fcn():
    d = J(IWN)
    eligible = d["eligibility"]["eligible"]
    not_cert = eligible - d["leg3_guard"]["own_CERTIFIED"]
    ok = pct1(not_cert / eligible) == 32.0
    return ok, f"{not_cert}/{eligible} = {pct1(not_cert / eligible)}%"


expect("5.9-prevalence-euses-69.3", "§5.9", "would-corrupt prevalence 69.3% (EUSES)",
       IWE, lambda: J(IWE)["leg2_prevalence"]["prevalence"], 0.6933)
expect("5.9-prevalence-enron-89.2", "§5.9", "would-corrupt prevalence 89.2% (Enron)",
       IWN, lambda: J(IWN)["leg2_prevalence"]["prevalence"], 0.8923)
expect("5.9-dbt-254-models", "§5.9", "production dbt project: 254 models",
       IWD, lambda: J(IWD)["coverage"]["total"], 254)
expect("5.9-dbt-coverage-40.2", "§5.9", "mini-adapter parse coverage 40.2%",
       IWD, lambda: J(IWD)["coverage"]["parse_coverage"], 0.4016)
expect("5.9-dbt-152-dynamic", "§5.9", "152 models with unmodeled Jinja fail closed",
       IWD, lambda: J(IWD)["coverage"]["dynamic"], 152)
expect("5.9-dbt-79-closed-subgraph", "§5.9", "79-model closed subgraph",
       IWD, lambda: J(IWD)["coverage"]["closed_subgraph"], 79)
expect("5.9-dbt-certify-legs", "§5.9", "faithful rename (4 real dependents) CERTIFIED; dangling botch REFUSED",
       IWD, lambda: (J(IWD)["certify_legs"]["faithful_rename"],
                     J(IWD)["certify_legs"]["botched_rename_dangling_ref"],
                     J(IWD)["certify_legs"]["dependents_in_subgraph"]), ("CERTIFIED", "REFUSED", 4))


@claim("5.9-ledger-4-3-2", "§5.9", "prediction ledger: 4 confirm, 3 disconfirm, 2 partial (9 itw-* rows)", TSV)
def _ledger():
    rows = [ln.split("\t") for ln in T(TSV).splitlines()[1:] if ln.startswith("itw-")]
    sig = {}
    for r in rows:
        sig[r[6]] = sig.get(r[6], 0) + 1
    ok = len(rows) == 9 and sig == {"confirm": 4, "disconfirm": 3, "partial": 2}
    return ok, f"{len(rows)} rows, signals={sig}"


# ------------------------------------------------------------ §4 (TCB) ----
expect("4-tokenizer-fuzz-175-0", "§4", "175 (formula, op) pairs, 0 disagreements vs independent shifter",
       TF, lambda: (J(TF)["pairs_tested"], J(TF)["disagreements"]), (175, 0))
expect("4-conformance-264-0", "§4", "single-op/one-engine run: 264 formulas, 0 divergences",
       TC, lambda: (J(TC)["engine_checked"], J(TC)["divergences"]), (264, 0))


@claim("4-conformance-v2-465x2-0", "§4", "465 formulas per engine, 0 divergences, two engines agree", CV2)
def _cv2():
    d = J(CV2)
    ok = d["libreoffice"] == {"checked": 465, "divergences": 0}
    ok = ok and d["formulas_engine"] == {"checked": 465, "divergences": 0}
    for o in d["per_op"]:
        ok = ok and o["libreoffice"] == o["formulas"]
    return ok, f"LO={d['libreoffice']}, formulas={d['formulas_engine']}"


# ------------------------------------------------------------------ §7 ----
@claim("7-exact-tier-37.5pct-ops", "§7", "exact tier certifies 37.5% of operations", EDIST)
def _ops375():
    d = J(EDIST)
    frac = d["structural_ops"] / d["total_operations"]
    ok = d["structural_ops"] == 42 and d["total_operations"] == 112 and pct1(frac) == 37.5
    return ok, f"{d['structural_ops']}/{d['total_operations']} = {pct1(frac)}%"


@claim("7-exact-tier-27pct-tasks", "§7", "27% of whole tasks fully certified (4/15 pure-structural)", COMP)
def _tasks27():
    c = J(COMP)["edit_distribution_coverage"]
    ok = c["tasks"] == 15 and round(c["fully_certified_pure_structural_pct"]) == 27
    return ok, f"{c['fully_certified_pure_structural_pct']}% of {c['tasks']} tasks"


# --------------------------------------------------------------------------
# formal claims: Lean 4 (live re-check when `lean` is available), Z3, and
# the Lean<->deployed-checker differential battery
# --------------------------------------------------------------------------
LEAN_THEOREMS = {
    "formal/SelfOracle.lean": ["SelfOracle.eval_iso_invariant", "SelfOracle.self_oracle_transfer",
                               "SelfOracle.eval_local", "SelfOracle.audit_surface_bound"],
    "formal/RefShift.lean": ["RefShift.refs_shiftF", "RefShift.shiftF_comp", "RefShift.shiftF_id",
                             "RefShift.shiftF_roundtrip", "RefShift.delete_insert_id",
                             "RefShift.insert_delete_form_id"],
    "formal/Checker.lean": ["Checker.check_spec", "Checker.check_sound",
                            "Checker.check_transports_oracle"],
    "formal/Impossibility.lean": ["Impossibility.eval_override_fresh",
                                  "Impossibility.fresh_skeleton_uncertifiable",
                                  "Impossibility.no_engine_free_predictor",
                                  "Impossibility.two_worlds_disagree"],
}
ALLOWED_AXIOMS = {"propext", "Quot.sound"}


def _lean_env():
    env = dict(os.environ)
    env["PATH"] = os.path.expanduser("~/.elan/bin") + os.pathsep + env.get("PATH", "")
    return env


def _lean_bin():
    return shutil.which("lean", path=_lean_env()["PATH"])


def _strip_lean_comments(src):
    src = re.sub(r"/-.*?-/", "", src, flags=re.S)  # block comments (good enough: none nest here)
    src = re.sub(r"--[^\n]*", "", src)
    return src


def _run_lean_file(rel, theorems):
    """Compile the committed .lean file with `#print axioms` appended for each
    named theorem; return (ok, detail, stdout)."""
    src = T(rel)
    if re.search(r"\bsorry\b", _strip_lean_comments(src)):
        return False, "`sorry` present in source (outside comments)", ""
    tmp = tempfile.NamedTemporaryFile("w", suffix=".lean", delete=False, encoding="utf-8")
    try:
        tmp.write(src + "\n" + "\n".join(f"#print axioms {t}" for t in theorems) + "\n")
        tmp.close()
        r = subprocess.run(["lean", tmp.name], capture_output=True, text=True,
                           env=_lean_env(), timeout=300)
    finally:
        os.unlink(tmp.name)
    if r.returncode != 0:
        return False, f"lean exited {r.returncode}: {(r.stderr or r.stdout)[:200]}", r.stdout
    out = r.stdout
    if "sorryAx" in out:
        return False, "sorryAx in axiom report", out
    bad, seen = [], 0
    for thm in theorems:
        m = re.search(rf"'{re.escape(thm)}' depends on axioms: \[([^\]]*)\]", out)
        if m:
            seen += 1
            axs = {a.strip() for a in m.group(1).split(",") if a.strip()}
            if not axs <= ALLOWED_AXIOMS:
                bad.append(f"{thm}: {sorted(axs)}")
        elif re.search(rf"'{re.escape(thm)}' does not depend on any axioms", out):
            seen += 1
        else:
            bad.append(f"{thm}: no axiom report found")
    ok = not bad and seen == len(theorems)
    detail = ("axioms subset of [propext, Quot.sound] for all "
              f"{seen} theorems, no sorry") if ok else "; ".join(bad)
    return ok, detail, out


def formal_results():
    out = []
    lean = _lean_bin()
    for rel, thms in LEAN_THEOREMS.items():
        cid = f"F-{os.path.basename(rel).replace('.lean', '').lower()}"
        claimed = "sorry-free; axioms only [propext, Quot.sound]"
        if not lean:
            out.append(("SKIP", cid, "§3", claimed, "lean not on PATH (install elan; see README)", rel))
            continue
        try:
            ok, detail, stdout = _run_lean_file(rel, thms)
        except Exception as e:  # noqa: BLE001
            out.append(("FAIL", cid, "§3", claimed, f"exception: {e}", rel))
            continue
        out.append(("PASS" if ok else "FAIL", cid, "§3", claimed, detail, rel))
        if rel.endswith("Checker.lean") and ok:
            evals = [ln.strip() for ln in stdout.splitlines() if ln.strip() in ("true", "false")]
            eok = evals == ["true", "false", "false"]
            out.append(("PASS" if eok else "FAIL", "F-checker-eval-demos", "§3 (Thm 4)",
                        "check executes: faithful->true, two botches->false",
                        f"#eval output: {evals}", rel))

    # Z3 shift-law battery
    z3_ok = False
    try:
        import z3  # noqa: F401
        z3_ok = True
    except ImportError:
        pass
    base_laws = ["insert(k,n) then delete(k,n) = identity",
                 "inserted position survives the matching delete",
                 "insert preserves order (monotone)",
                 "delete preserves order on survivors",
                 "6-case clamp_lo matches shifted first-survivor",
                 "6-case clamp_hi matches shifted last-survivor"]
    if z3_ok:
        r = subprocess.run([sys.executable, os.path.join(ROOT, "formal/shift_laws.py")],
                           capture_output=True, text=True, timeout=600)
        lines = r.stdout.splitlines()
        proved = [ln for ln in lines if ln.startswith("PROVED")]
        failed = [ln for ln in lines if not ln.startswith("PROVED") and ln.strip()]
        ok = r.returncode == 0 and not failed and all(any(b in p for p in proved) for b in base_laws)
        out.append(("PASS" if ok else "FAIL", "F-shift-laws-z3", "§3",
                    "shift algebra laws PROVED for all inputs (Z3)",
                    f"{len(proved)} PROVED lines, {len(failed)} failures (live run)", "formal/shift_laws.py"))
        move_ok = ok and sum("move-" in p for p in proved) >= 6
        out.append(("PASS" if move_ok else "FAIL", "F-move-sigma-bijection-z3", "§5.2b",
                    "move-rows sigma proved a bijection (injective + surjective, both directions)",
                    f"{sum('move-' in p for p in proved)} move-sigma laws PROVED (live run)",
                    "formal/shift_laws.py"))
    else:
        txt = T("formal/shift_laws.out.txt")
        got = [b for b in base_laws if f"PROVED: {b}" in txt]
        ok = len(got) == len(base_laws)
        out.append(("PASS" if ok else "FAIL", "F-shift-laws-z3", "§3",
                    "shift algebra laws PROVED for all inputs (Z3)",
                    f"{len(got)}/{len(base_laws)} PROVED lines in committed shift_laws.out.txt "
                    "(z3 not installed; committed output only)", "formal/shift_laws.out.txt"))
        out.append(("SKIP", "F-move-sigma-bijection-z3", "§5.2b",
                    "move-rows sigma proved a bijection (Z3)",
                    "needs z3-solver (`pip install z3-solver; python3 formal/shift_laws.py`); "
                    "the committed shift_laws.out.txt predates the move laws", "formal/shift_laws.py"))

    # Lean <-> deployed checker differential battery (seeded, deterministic)
    if lean:
        try:
            r = subprocess.run([sys.executable, os.path.join(ROOT, "formal/differential_check.py")],
                               capture_output=True, text=True, timeout=600, env=_lean_env())
            m = re.search(r"agreement: (\d+)/(\d+)", r.stdout)
            ok = r.returncode == 0 and m and m.group(1) == m.group(2) == "30"
            detail = m.group(0) if m else f"exit {r.returncode}: {(r.stderr or r.stdout)[:200]}"
        except Exception as e:  # noqa: BLE001
            ok, detail = False, f"exception: {e}"
        out.append(("PASS" if ok else "FAIL", "F-differential-30of30", "§4.i",
                    "deployed checker agrees with the Lean decision procedure 30/30",
                    detail + " (live seeded run)", "formal/differential_check.py"))
    else:
        out.append(("SKIP", "F-differential-30of30", "§4.i",
                    "deployed checker agrees with the Lean decision procedure 30/30",
                    "lean not on PATH; run `python3 formal/differential_check.py`",
                    "formal/differential_check.py"))
    return out


# --------------------------------------------------------------------------
# runner
# --------------------------------------------------------------------------

# ---- §5.10 locked test v2 + Theorem 7 (added after the clean panel round) ----
EV2 = "benchmarks/inthewild_euses_v2.json"
NV2 = "benchmarks/inthewild_enron_v2.json"
QN2 = "benchmarks/coincidence_q_enron_v2.json"
XLS = "benchmarks/enron_v2_extlink_share.json"
TD = "formal/tokenizer_differential.json"
DBS = "benchmarks/inthewild_dbt_spellbook.json"
DBC = "benchmarks/inthewild_dbt_calitp.json"
SPX = "benchmarks/agent_study/results_smoke_perfect_postfix.json"

expect("v2-euses-cellchecks", "§5.10", "316,746 cell-checks", EV2,
       lambda: sum(v["cells_checked"] for v in J(EV2)["leg1_shift_correctness"].values()), 316746)
expect("v2-euses-mismatch", "§5.10", "0 mismatches", EV2,
       lambda: sum(v["xlq_mismatch"] for v in J(EV2)["leg1_shift_correctness"].values()), 0)
expect("v2-enron-cellchecks", "§5.10", "690,251 cell-checks", NV2,
       lambda: sum(v["cells_checked"] for v in J(NV2)["leg1_shift_correctness"].values()), 690251)
expect("v2-enron-mismatch", "§5.10", "0 mismatches", NV2,
       lambda: sum(v["xlq_mismatch"] for v in J(NV2)["leg1_shift_correctness"].values()), 0)
expect("v2-euses-cost-numerator", "§5.10 21.2%=106/500", "106 refused", EV2,
       lambda: J(EV2)["leg3_guard"]["own_REFUSED"] + J(EV2)["leg3_guard"]["own_not_attempted_restructure_refused"], 106)
expect("v2-enron-cost-numerator", "§5.10 34.3%=124/362", "124 refused", NV2,
       lambda: J(NV2)["leg3_guard"]["own_REFUSED"] + J(NV2)["leg3_guard"]["own_not_attempted_restructure_refused"] + J(NV2)["leg3_guard"].get("file_timeout", 0), 124)
expect("v2-falsecert-euses", "§5.10", "0 false certs", EV2,
       lambda: J(EV2)["leg3_guard"].get("FALSE_CERT_on_would_corrupt", 0), 0)
expect("v2-falsecert-enron", "§5.10", "0 false certs", NV2,
       lambda: J(NV2)["leg3_guard"].get("FALSE_CERT_on_would_corrupt", 0), 0)
expect("v2-k999-enron", "§5.10", "k999 = 237", QN2,
       lambda: J(QN2)["models"]["M2v_offbyone_row"]["excel_semantics"]["detection"]["mixture_dependent"]["k_for_99.9pct"], 237)
expect("v2-extlink-sole", "§5.10 64% sole-cause", "66/103", XLS,
       lambda: (J(XLS)["extlink_sole"], J(XLS)["refused"]), (66, 103))
expect("thm7-diff-agree", "§3 Thm 7", "901,946 in-surface agree", TD,
       lambda: J(TD)["stats"]["in_surface_agree"], 901946)
expect("thm7-diff-disagree", "§3 Thm 7", "0 disagreements", TD,
       lambda: J(TD)["stats"]["in_surface_DISAGREE"] + J(TD)["stats"]["guard_DISAGREE"], 0)
expect("thm7-requests", "§3 Thm 7 (452,384+315)x4", "1,810,796", TD,
       lambda: J(TD)["requests"], 1810796)
expect("v2-dbt-spellbook-coverage", "§5.10", "0/2484 covered", DBS,
       lambda: (J(DBS)["coverage"]["covered_parse"], J(DBS)["coverage"]["total"]), (0, 2484))
expect("v2-dbt-calitp-coverage", "§5.10", "13.7%", DBC,
       lambda: J(DBC)["coverage"]["parse_coverage"], 0.1373)
expect("v2-smoke-postfix", "§5.10 smoke 5->4", "refused_correct = 4", SPX,
       lambda: J(SPX)["GUARDED"]["refused_correct_COST"], 4)


def main():
    rows = []
    for cid, loc, claimed, artifact, fn in CLAIMS:
        try:
            ok, actual = fn()
            status = "PASS" if ok else "FAIL"
        except Exception as e:  # noqa: BLE001
            status, actual = "FAIL", f"exception: {e}"
        rows.append((status, cid, loc, claimed, actual, artifact))
    rows.extend(formal_results())

    wid = max(len(r[1]) for r in rows)
    wloc = max(len(r[2]) for r in rows)
    print(f"{'STATUS':6}  {'CLAIM':{wid}}  {'PAPER':{wloc}}  CLAIMED  |  ACTUAL (artifact)")
    print("-" * 110)
    for status, cid, loc, claimed, actual, artifact in rows:
        print(f"{status:6}  {cid:{wid}}  {loc:{wloc}}  {claimed}")
        print(f"{'':6}  {'':{wid}}  {'':{wloc}}    -> {actual}   [{artifact}]")
    n = len(rows)
    npass = sum(r[0] == "PASS" for r in rows)
    nfail = sum(r[0] == "FAIL" for r in rows)
    nskip = sum(r[0] == "SKIP" for r in rows)
    print("-" * 110)
    print(f"TOTAL {n} claims: {npass} PASS, {nfail} FAIL, {nskip} SKIP")
    if nfail:
        print("\nFAILED CLAIMS (paper number does not reproduce from its committed artifact):")
        for status, cid, loc, claimed, actual, artifact in rows:
            if status == "FAIL":
                print(f"  {cid} ({loc}): claimed {claimed!r}, got {actual} [{artifact}]")
    if nskip:
        print("\nSKIPPED (toolchain missing — see repro/README.md for the re-run command):")
        for status, cid, *_ in rows:
            if status == "SKIP":
                print(f"  {cid}")
    return 1 if nfail else 0


if __name__ == "__main__":
    sys.exit(main())
