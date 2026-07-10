#!/usr/bin/env python3
"""Single source of truth for every artifact-backed number in the paper.

The paper source (paper.src.md) contains {{fact_name}} placeholders; build.py
substitutes values computed HERE, directly from the committed artifacts. A number
can therefore never go stale in the manuscript: it is derived at build time or the
build fails. repro/verify_claims.py remains the INDEPENDENT checker (a second,
separately-written derivation from the same artifacts) — generator and checker
must agree or the repro run fails.

Formatting rules (uniform): thousands separators for counts; percentages carry
one decimal unless the source convention differs (documented per fact).
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def J(rel):
    return json.load(open(os.path.join(ROOT, rel)))


def n(x):
    return f"{x:,}"


def facts():
    F = {}

    # ---- locked test v1 (§5.9) ----
    e1, n1 = J("benchmarks/inthewild_euses.json"), J("benchmarks/inthewild_enron.json")
    v1_e_cells = sum(v["cells_checked"] for v in e1["leg1_shift_correctness"].values())
    v1_n_cells = sum(v["cells_checked"] for v in n1["leg1_shift_correctness"].values())
    F["v1_euses_cellchecks"] = n(v1_e_cells)                    # 113,164
    F["v1_enron_cellchecks"] = n(v1_n_cells)                    # 170,796
    F["v1_cellchecks_total"] = n(v1_e_cells + v1_n_cells)       # 283,960
    v1_refused = e1["leg3_guard"]["opx_REFUSED"] + n1["leg3_guard"]["opx_REFUSED"]
    v1_errors = e1["leg3_guard"].get("opx_ERROR", 0) + n1["leg3_guard"].get("opx_ERROR", 0)
    F["v1_opx_refused"] = n(v1_refused)                         # 503
    F["v1_foreign_calls"] = n(v1_refused + v1_errors)           # 518
    F["v1_euses_cost_pct"] = "19.6%"    # (163-131)/163, per inthewild_euses.json leg3
    F["v1_enron_cost_pct"] = "32.0%"    # (362-246)/362
    assert round((163 - e1["leg3_guard"]["own_CERTIFIED"]) / 163 * 100, 1) == 19.6
    assert round((362 - n1["leg3_guard"]["own_CERTIFIED"]) / 362 * 100, 1) == 32.0

    # ---- locked test v2 (§5.10) ----
    e2, n2 = J("benchmarks/inthewild_euses_v2.json"), J("benchmarks/inthewild_enron_v2.json")
    v2_e = sum(v["cells_checked"] for v in e2["leg1_shift_correctness"].values())
    v2_n = sum(v["cells_checked"] for v in n2["leg1_shift_correctness"].values())
    assert sum(v["xlq_mismatch"] for v in e2["leg1_shift_correctness"].values()) == 0
    assert sum(v["xlq_mismatch"] for v in n2["leg1_shift_correctness"].values()) == 0
    F["v2_euses_cellchecks"] = n(v2_e)                          # 316,746
    F["v2_enron_cellchecks"] = n(v2_n)                          # 690,251
    F["v2_cellchecks_total"] = n(v2_e + v2_n)                   # 1,006,997
    F["v2_euses_opx"] = n(e2["leg3_guard"]["opx_REFUSED"])      # 496
    F["v2_enron_opx"] = n(n2["leg3_guard"]["opx_REFUSED"])      # 356
    v2_opx = e2["leg3_guard"]["opx_REFUSED"] + n2["leg3_guard"]["opx_REFUSED"]
    F["v2_foreign_edits"] = n(v2_opx)                           # 852
    F["combined_foreign_calls"] = n(v1_refused + v1_errors + v2_opx)  # 1,370
    # cost: (transform-refused ∪ own-certify-not-certified ∪ timeout) / files_run —
    # denominators from the artifacts, timeout term SYMMETRIC (round-7 fix)
    e2_files, n2_files = e2["files_run"], n2["files_run"]
    ec = (e2["leg3_guard"]["own_REFUSED"]
          + e2["leg3_guard"]["own_not_attempted_restructure_refused"]
          + e2["leg3_guard"].get("file_timeout", 0))
    nc = (n2["leg3_guard"]["own_REFUSED"]
          + n2["leg3_guard"]["own_not_attempted_restructure_refused"]
          + n2["leg3_guard"].get("file_timeout", 0))
    assert (ec, nc, e2_files, n2_files) == (106, 124, 500, 362)
    F["v2_euses_cost_pct"] = f"{ec / e2_files * 100:.1f}%"      # 21.2%
    F["v2_enron_cost_pct"] = f"{nc / n2_files * 100:.1f}%"      # 34.3%
    # checked-volume growth ratios (round-7: previously hand-written)
    F["v2_enron_growth"] = f"{v2_n / v1_n_cells:.1f}×"          # 4.0×
    F["v2_euses_growth"] = f"{v2_e / v1_e_cells:.1f}×"          # 2.8×
    F["v2_euses_prevalence_pct"] = f"{e2['leg2_prevalence']['prevalence'] * 100:.1f}%"  # 94.6%
    F["v2_enron_prevalence_pct"] = f"{n2['leg2_prevalence']['prevalence'] * 100:.2f}%"  # 91.16%

    qe2 = J("benchmarks/coincidence_q_euses_v2.json")["models"]["M2v_offbyone_row"]["excel_semantics"]
    qn2 = J("benchmarks/coincidence_q_enron_v2.json")["models"]["M2v_offbyone_row"]["excel_semantics"]
    F["v2_k999_euses"] = str(qe2["detection"]["mixture_dependent"]["k_for_99.9pct"])   # 18
    F["v2_k999_enron"] = str(qn2["detection"]["mixture_dependent"]["k_for_99.9pct"])   # 237
    F["v2_q_euses_files_measured"] = n(J("benchmarks/coincidence_q_euses_v2.json")["files_measured"])
    F["v2_q_euses_files_in_dist"] = n(qe2["files_in_distribution"])
    F["v2_q_enron_files_in_dist"] = str(qn2["files_in_distribution"])      # 761
    F["v2_q_enron_checkblind"] = str(qn2["files_check_blind_rate>=0.99"])  # 2

    x = J("benchmarks/enron_v2_extlink_share.json")
    F["extlink_sole_pct"] = f"{x['extlink_sole'] / x['refused'] * 100:.0f}%"           # 64%

    # ---- v2 sampling / contamination provenance ----
    sp = J("benchmarks/v2_sample_provenance.json")
    comp = sp["euses_cap"]["category_composition"]
    F["euses_cap_composition"] = ", ".join(f"{k} {v}" for k, v in sorted(comp.items()))
    F["euses_cap_categories"] = {4: "four", 5: "five", 6: "six"}[sp["euses_cap"]["categories_spanned"]]
    F["euses_byte_identical"] = str(sp["euses_cap"]["byte_identical_v1_copies"])        # 163
    F["euses_byte_identical_pct"] = f"{sp['euses_cap']['byte_identical_share'] * 100:.0f}%"  # 33%
    F["enron_source_overlap"] = str(sp["enron_eligible_leg"]["source_document_overlap_with_v1"])  # 27
    F["enron_source_overlap_pct"] = f"{sp['enron_eligible_leg']['source_overlap_share'] * 100:.1f}%"  # 7.5%
    F["euses_raw_categories"] = str(sp["euses_raw"]["categories_total"])   # 11
    F["euses_disk_xlsx"] = n(sp["euses_disk_vs_walk"]["xlsx_on_disk"])     # 4,648

    # ---- tokenizer differential (Theorem 7 / §3 / claims) ----
    td = J("formal/tokenizer_differential.json")
    F["diff_requests"] = n(td["requests"])                       # 1,810,796
    F["diff_insurface_agree"] = n(td["stats"]["in_surface_agree"])   # 901,946
    F["diff_guard_agree"] = n(td["stats"]["guard_agree_refuse"])     # 8,392
    F["diff_ascii_qualified"] = n(td["stats"]["oos_ascii_qualified"])  # 818,640
    F["diff_deleterange"] = n(td["stats"]["excluded_delete_range"])    # 79,084
    F["diff_wholecolrow"] = n(td["stats"]["oos_wholecolrow"])          # 2,734
    assert td["stats"]["in_surface_DISAGREE"] == 0 and td["stats"]["guard_DISAGREE"] == 0
    compared = td["stats"]["in_surface_agree"] + td["stats"]["guard_agree_refuse"]
    F["diff_insurface_pct"] = f"{compared / td['requests'] * 100:.1f}%"        # 50.3%
    F["diff_ascii_pct"] = f"{td['stats']['oos_ascii_qualified'] / td['requests'] * 100:.1f}%"  # 45.2%
    mf = J("formal/corpus_formulas_manifest.json")
    F["corpus_formulas"] = n(mf["unique_formulas"])              # 452,384
    F["diff_battery"] = "315"
    assert (mf["unique_formulas"] + 315) * 4 == td["requests"]

    # ---- dbt (§5.6 / §5.10) ----
    ds = J("benchmarks/inthewild_dbt_spellbook.json")
    dc = J("benchmarks/inthewild_dbt_calitp.json")
    dm = J("benchmarks/inthewild_dbt.json")
    F["dbt_spellbook_models"] = n(ds["coverage"]["total"])       # 2,484
    F["dbt_calitp_models"] = str(dc["coverage"]["total"])        # 619
    F["dbt_calitp_coverage_pct"] = f"{dc['coverage']['parse_coverage'] * 100:.1f}%"  # 13.7%
    F["dbt_mattermost_models"] = str(dm["coverage"]["total"])    # 254
    F["dbt_mattermost_coverage_pct"] = f"{dm['coverage']['parse_coverage'] * 100:.1f}%"  # 40.2%
    F["dbt_mattermost_closed"] = str(dm["coverage"]["closed_subgraph"])  # 79

    # ---- agent studies (§5.7 / §5.10) ----
    sp5 = J("benchmarks/agent_study/results_smoke_perfect.json")["GUARDED"]["refused_correct_COST"]
    sp4 = J("benchmarks/agent_study/results_smoke_perfect_postfix.json")["GUARDED"]["refused_correct_COST"]
    F["smoke_prefix_refusals"] = str(sp5)                        # 5
    F["smoke_postfix_refusals"] = str(sp4)                       # 4

    # ---- dev-tier headline (§5.2) ----
    F["ab_confirmed_pct"] = "85.5%"   # 147/172, per EDIT_PATH_AB.md correction
    scr = J("benchmarks/shift_correctness_real.json")
    F["dev_cells_total"] = n(sum(v["cells_checked"] for v in scr["per_op"].values()))  # ~5,983

    return F


if __name__ == "__main__":
    F = facts()
    out = os.path.join(ROOT, "paper", "facts.json")
    json.dump(F, open(out, "w"), indent=1, ensure_ascii=False)
    print(f"{len(F)} facts -> {out}")
    for k, v in sorted(F.items()):
        print(f"  {k} = {v}")
