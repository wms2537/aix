#!/usr/bin/env python3
"""EXPLORATORY POST-TEST ANALYSIS: refusal-cause breakdown of the fail-closed
cost measured in the LOCKED in-the-wild run (benchmarks/inthewild_{euses,enron}.json).

Re-runs leg 3a (certify xlq's OWN insert-rows@2 transform) on the SAME corpora
with the SAME eligibility function and the SAME harness outcome classification
(json.loads(stdout).get("status", "ERROR")), then attributes every
non-CERTIFIED outcome to a cause, grounded in the artifact:

  deny:* — certify.rs::verify_noncell_refs fail-closed denylist, grounded by
    scanning the zip exactly the way certify detects it:
      sheet-XML needles over every xl/worksheets/sheet*.xml:
        "<dataValidation" -> dataValidation, "<conditionalFormatting" ->
        conditionalFormatting, "sparkline" -> sparkline
      part-name prefixes: xl/charts/ -> chart, xl/pivotTables/ + xl/pivotCache/
        -> pivot, xl/externalLinks/ -> externalLink
    certify reports only the FIRST hit, so cause sets come from the zip scan,
    not the message (the message label is cross-checked against the zip).
  harness:stdout_pollution — vendored ironcalc's styles.rs line 149 does
    println!("Unexpected feature ...") for unknown font features (e.g.
    LibreOffice's <shadow val="true"/>), polluting xlq's JSON stdout during
    model load; the locked harness's json.loads then fails -> own_ERROR even
    though certify completed and printed its verdict. We recover the trailing
    verdict JSON and report it.
  harness:sheet_name_xml_entity — eligibility's zip_sheet_name returns the RAW
    workbook.xml name attribute (e.g. "JAN 5 &amp; 6, 2002"); xlq decodes XML,
    so restructure refuses "no sheet named ...". Grounded by retrying with the
    entity-decoded name and recording the full pipeline verdict.
  residual:* — xlq restructure itself refused (shift algebra residuals);
    these files never reached certify in leg 3a but still count in the
    fail-closed cost (locked run's own_* tallies skipped them).

FLIP RULES (documented per class in the output):
  deny:X flips only if X is the file's SOLE cause AND ironcalc can load the
    transform (`xlq diff f f`, the same load_from_xlsx path — the denylist
    check runs BEFORE the loader, so an unloadable file would flip to ERROR,
    not CERTIFIED). Files whose load PROBE showed stdout pollution would still
    land own_ERROR under the unmodified locked harness; counted separately.
  harness:stdout_pollution flips iff the recovered verdict is CERTIFIED.
  harness:sheet_name_xml_entity flips iff the decoded-name retry CERTIFIED.
  residual:X flips are UPPER BOUNDS (sole cause, no denylist parts in the
    original, original loads) — the transform for these constructs does not
    exist yet, so certification cannot be confirmed.

HONESTY: this REUSES the locked-test corpora post-hoc. It extracts no new
information about guard soundness — the same deterministic pipeline on the
same files, only refusal-cause attribution. Cost-reduction claims require
confirmation in locked test v2.

Usage: cost_breakdown.py    (runs both corpora, writes cost_breakdown.json)
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import zipfile
from collections import Counter

BENCH = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, BENCH)
from inthewild_run import eligible_files          # identical eligibility scan
from shift_correctness_real import XLQ            # same binary path

WORK = ("/tmp/claude-1000/-home-soh-aix/a1b7f99e-cc58-4254-b95a-10d56f89029d/"
        "scratchpad/cost_breakdown")
CORPORA = [
    ("euses", "/home/soh/aix/data/inthewild/euses/converted"),
    ("enron", "/home/soh/aix/data/inthewild/enron/converted"),
]
CAP = 500  # same as the locked run's default

# certify.rs verify_noncell_refs denylist, detection-method-faithful
DENY_SHEET_NEEDLES = [
    ("<dataValidation", "data_validation"),
    ("<conditionalFormatting", "conditional_formatting"),
    ("sparkline", "sparkline"),
]
DENY_PART_PREFIXES = [
    ("xl/charts/", "chart"),
    ("xl/pivotTables/", "pivot_table"),
    ("xl/pivotCache/", "pivot_cache"),
    ("xl/externalLinks/", "external_link"),
]
# report classes (pivot_table + pivot_cache merged: verifying "pivot" means both)
CLASS_OF = {
    "data_validation": "dataValidation",
    "conditional_formatting": "conditionalFormatting",
    "sparkline": "sparkline",
    "chart": "chart",
    "pivot_table": "pivot",
    "pivot_cache": "pivot",
    "external_link": "externalLink",
}
POLLUTION_NEEDLE = "Unexpected feature"
TAG_RE = re.compile(r"tag_name: \{[^}]*\}(\w+)")


class FileTimeout(Exception):
    pass


def _alarm(sig, frame):
    raise FileTimeout()


def decode_entities(s):
    for ent, ch in (("&lt;", "<"), ("&gt;", ">"), ("&quot;", '"'),
                    ("&apos;", "'"), ("&amp;", "&")):
        s = s.replace(ent, ch)
    return s


def xlq_edit_full(src, sheet, op, at, work):
    """shift_correctness_real.xlq_edit, verbatim invocation, but returns output."""
    dst = os.path.join(work, "x.xlsx")
    shutil.copy(src, dst)
    for suf in (".xlq.jsonl", ".rev-1.xlsx", ".xlq.lock"):
        if os.path.exists(dst + suf):
            os.remove(dst + suf)
    r = subprocess.run([XLQ, "restructure", dst, "--sheet", sheet, "--op", op,
                        "--at", str(at), "--count", "1", "--actor", "s"],
                       capture_output=True, text=True)
    return (dst if '"rev"' in r.stdout else None), r.stdout, r.stderr


def certify_raw(orig, edited, sheet, op="insert-rows", at=2):
    """inthewild_run.certify, verbatim invocation, returning raw stdout+stderr."""
    r = subprocess.run([XLQ, "certify", orig, edited, "--sheet", sheet, "--op", op,
                        "--at", str(at), "--count", "1"],
                       capture_output=True, text=True, timeout=240)
    return r.stdout, r.stderr


def harness_status(stdout):
    """EXACT locked-harness classification (inthewild_run.certify)."""
    try:
        return json.loads(stdout).get("status", "ERROR")
    except Exception:
        return "ERROR"


def recover_verdict(stdout):
    """Recover the trailing pretty-printed verdict JSON from polluted stdout."""
    lines = stdout.splitlines()
    for i, ln in enumerate(lines):
        if ln == "{":
            try:
                return json.loads("\n".join(lines[i:]))
            except Exception:
                continue
    return None


def pollution_tags(stdout):
    return sorted({m.group(1) for ln in stdout.splitlines()
                   if POLLUTION_NEEDLE in ln for m in [TAG_RE.search(ln)] if m})


def zip_denylist(path):
    """Artifact-grounded denylist scan: occurrence counts per certify label."""
    counts = {}
    try:
        z = zipfile.ZipFile(path)
        names = z.namelist()
    except Exception as e:
        return {"_zip_error": str(e)[:120]}
    sheets = [n for n in names
              if n.startswith("xl/worksheets/sheet") and n.endswith(".xml")]
    for n in sheets:
        try:
            data = z.read(n).decode("utf-8", "replace")
        except Exception:
            continue
        for needle, label in DENY_SHEET_NEEDLES:
            k = data.count(needle)
            if k:
                counts[label] = counts.get(label, 0) + k
    for prefix, label in DENY_PART_PREFIXES:
        k = sum(1 for n in names if n.startswith(prefix))
        if k:
            counts[label] = counts.get(label, 0) + k
    return counts


def deny_classes(deny_counts):
    return sorted({CLASS_OF[l] for l in deny_counts if l in CLASS_OF})


def load_probe(path):
    """ironcalc loadability probe: xlq diff f f uses load_from_xlsx on both args.
    Returns (loadable, probe_stdout_polluted, err)."""
    try:
        r = subprocess.run([XLQ, "diff", path, path],
                           capture_output=True, text=True, timeout=240)
        return (r.returncode == 0, POLLUTION_NEEDLE in r.stdout,
                (r.stderr.strip()[-400:] or None))
    except Exception as e:
        return False, False, str(e)[:200]


def run_corpus(name, corpus_dir):
    files, counts = eligible_files(corpus_dir, CAP)
    print(f"[{name}] eligible: {counts['eligible']}  "
          f"parse_failed: {counts['parse_failed']}  "
          f"lt2: {counts['ineligible_lt2_formulas']}", flush=True)
    rows = []
    signal.signal(signal.SIGALRM, _alarm)
    for i, (p, sheet, forms) in enumerate(files):
        work = os.path.join(WORK, name, str(i))
        os.makedirs(work, exist_ok=True)
        rec = {"file": os.path.basename(p)}
        signal.alarm(300)  # same watchdog as the locked run
        try:
            classify_file(rec, p, sheet, work)
        except FileTimeout:
            rec["outcome"] = "TIMEOUT"
            rec["causes"] = ["timeout"]
            rec["flip_ok"] = False
        finally:
            signal.alarm(0)
            shutil.rmtree(work, ignore_errors=True)
        rows.append(rec)
        if (i + 1) % 50 == 0:
            print(f"  [{name}] ...{i+1}/{len(files)}", flush=True)
    return dict(counts), rows


def classify_file(rec, p, sheet, work):
    xf, rst_out, rst_err = xlq_edit_full(p, sheet, "insert-rows", 2, work)
    if xf is None:
        classify_restructure_refused(rec, p, sheet, work, rst_out, rst_err)
        return

    cout, cerr = certify_raw(p, xf, sheet)
    st = harness_status(cout)           # EXACT locked-harness outcome
    rec["outcome"] = st
    if st == "CERTIFIED":
        return

    if st == "ERROR":
        # locked harness could not parse stdout (or xlq emitted {"error": ...})
        verdict = recover_verdict(cout)
        tags = pollution_tags(cout)
        if tags:
            rec["pollution_tags"] = tags
        rec["stderr"] = cerr[-400:] if cerr.strip() else ""
        if verdict is not None and "status" in verdict:
            rec["recovered_status"] = verdict["status"]
            rec["causes"] = ["harness:stdout_pollution"]
            rec["flip_ok"] = verdict["status"] == "CERTIFIED"
            if verdict["status"] == "REFUSED":
                # attribute the underlying refusal too
                rec["recovered_certify"] = {k: verdict[k] for k in
                                            ("reason", "detail", "residuals")
                                            if k in verdict}
                deny = zip_denylist(xf)
                rec["deny_parts"] = deny
                if verdict.get("reason") == "unverified_reference_part":
                    rec["causes"] += ["deny:" + c for c in deny_classes(deny)]
                else:
                    rec["causes"] += ["certify:" + verdict.get("reason", "cell_diff")]
                rec["flip_ok"] = False
        else:
            # true error payload (no recoverable verdict)
            err = ""
            try:
                err = json.loads(cout).get("error", "")
            except Exception:
                err = cout[:300]
            rec["error"] = err[:400]
            rec["causes"] = ["loader:" + (tags[0] if tags else "other")]
            rec["flip_ok"] = False
        return

    # REFUSED with parseable verdict
    cj = json.loads(cout)
    rec["certify"] = {k: cj[k] for k in
                      ("reason", "detail", "residuals", "diff_counts",
                       "sample_diffs") if k in cj}
    deny = zip_denylist(xf)
    deny_o = zip_denylist(p)
    rec["deny_parts"] = deny
    if deny != deny_o:
        rec["deny_parts_orig_differs"] = deny_o
    if cj.get("reason") == "unverified_reference_part":
        classes = deny_classes(deny)
        rec["causes"] = ["deny:" + c for c in classes] or ["deny:UNGROUNDED"]
        det = cj.get("detail", "")
        rec["msg_label"] = det.split(" ", 1)[0] if det else ""
        rec["msg_agrees_with_zip"] = CLASS_OF.get(rec["msg_label"],
                                                  rec["msg_label"]) in classes
        # would it even LOAD if the denylist class were verified?
        ok, polluted, lerr = load_probe(xf)
        rec["ironcalc_loadable"] = ok
        rec["load_probe_polluted"] = polluted
        if not ok:
            rec["load_error"] = lerr
        rec["flip_ok"] = ok
    else:
        rec["causes"] = ["certify:" + cj.get("reason", "cell_diff")]
        rec["flip_ok"] = False


def classify_restructure_refused(rec, p, sheet, work, rst_out, rst_err):
    rec["outcome"] = "RESTRUCTURE_REFUSED"
    try:
        j = json.loads(rst_out)
    except Exception:
        j = None
    res = (j or {}).get("residuals") or []
    err = (j or {}).get("error", "unparseable")
    rec["restructure_error"] = err if isinstance(err, str) else str(err)
    rec["deny_parts_orig"] = zip_denylist(p)
    if res:
        rec["residuals"] = [{"part": r.get("part"), "reason": r.get("reason"),
                             "detail": (r.get("detail") or "")[:200]} for r in res]
        rec["causes"] = sorted({"residual:" + r.get("reason", "?") for r in res})
        # residual flip is an UPPER BOUND: sole cause + no denylist parts + loads
        clean = not deny_classes(rec["deny_parts_orig"])
        ok, polluted, _ = load_probe(p)
        rec["orig_loadable"] = ok
        rec["flip_ok"] = clean and ok
        rec["flip_is_upper_bound"] = True
        return
    if "no sheet named" in rec["restructure_error"]:
        # harness XML-entity artifact: zip_sheet_name returns the raw attribute
        rec["causes"] = ["harness:sheet_name_xml_entity"]
        decoded = decode_entities(sheet)
        rec["decoded_sheet"] = decoded
        if decoded != sheet:
            xf, _, _ = xlq_edit_full(p, decoded, "insert-rows", 2, work)
            if xf is None:
                rec["retry_decoded"] = "RESTRUCTURE_REFUSED"
                rec["flip_ok"] = False
            else:
                cout, _ = certify_raw(p, xf, decoded)
                v = recover_verdict(cout) or {}
                rec["retry_decoded"] = harness_status(cout)
                rec["retry_recovered_status"] = v.get("status")
                rec["flip_ok"] = v.get("status") == "CERTIFIED"
                if v.get("status") == "REFUSED":
                    rec["causes"] = ["harness:sheet_name_xml_entity"] + \
                        ["deny:" + c for c in
                         deny_classes(zip_denylist(xf))] if \
                        v.get("reason") == "unverified_reference_part" else \
                        ["harness:sheet_name_xml_entity",
                         "certify:" + v.get("reason", "cell_diff")]
        else:
            rec["flip_ok"] = False
    else:
        rec["causes"] = ["restructure:" + rec["restructure_error"][:80]]
        rec["flip_ok"] = False


def analyze(name, elig_counts, rows):
    n = len(rows)
    outcome_hist = dict(Counter(r["outcome"] for r in rows))
    noncert = [r for r in rows if r["outcome"] != "CERTIFIED"]
    cost_files = len(noncert)
    cause_hist = Counter()
    for r in noncert:
        for c in r.get("causes", ["?"]):
            cause_hist[c] += 1
    # unified marginal analysis over cause classes
    for r in noncert:
        r["_set"] = frozenset(r.get("causes", ["?"]))
    classes = sorted({c for r in noncert for c in r["_set"]})
    per_class = []
    for cls in classes:
        present = [r for r in noncert if cls in r["_set"]]
        sole = [r for r in present if r["_set"] == {cls}]
        flips = [r for r in sole if r.get("flip_ok")]
        d = {
            "class": cls,
            "kind": ("denylist" if cls.startswith("deny:")
                     else "harness_artifact" if cls.startswith("harness:")
                     else "transform_residual" if cls.startswith("residual:")
                     else "other"),
            "files_containing": len(present),
            "sole_cause_files": len(sole),
            "flips": len(flips),
            "flip_pct_of_eligible": round(100 * len(flips) / n, 1),
        }
        if any(r.get("flip_is_upper_bound") for r in flips):
            d["flip_is_upper_bound"] = True
        if cls.startswith("deny:"):
            d["flips_needing_pollution_fix_too"] = sum(
                1 for r in flips if r.get("load_probe_polluted"))
        per_class.append(d)
    per_class.sort(key=lambda d: (-d["flips"], -d["sole_cause_files"],
                                  -d["files_containing"]))
    cumulative = []
    for k in range(1, min(3, len(per_class)) + 1):
        top = {d["class"] for d in per_class[:k]}
        flip = [r for r in noncert if r["_set"] and r["_set"] <= top
                and r.get("flip_ok")]
        naive = [r for r in noncert if r["_set"] and r["_set"] <= top]
        cumulative.append({
            "top_k": k,
            "classes": sorted(top),
            "flips": len(flip),
            "sole_or_subset_files_naive": len(naive),
            "flip_pct_of_eligible": round(100 * len(flip) / n, 1),
            "residual_cost_pct_after_flips": round(
                100 * (cost_files - len(flip)) / n, 1),
        })
    for r in noncert:
        r.pop("_set", None)
    deny_rows = [r for r in noncert
                 if any(c.startswith("deny:") for c in r.get("causes", []))]
    msg_disagree = [r["file"] for r in deny_rows
                    if r.get("msg_agrees_with_zip") is False]
    unloadable = [r["file"] for r in deny_rows
                  if r.get("ironcalc_loadable") is False]
    return {
        "eligibility": elig_counts,
        "files_run": n,
        "outcome_histogram_locked_harness_semantics": outcome_hist,
        "fail_closed_cost": {"files": cost_files,
                             "pct_of_eligible": round(100 * cost_files / n, 1)},
        "refusal_cause_histogram": dict(cause_hist.most_common()),
        "marginal_analysis_ranked": per_class,
        "cumulative_flips_top_k": cumulative,
        "denylist_refused_but_ironcalc_unloadable": unloadable,
        "certify_msg_label_vs_zip_disagreements": msg_disagree,
        "per_file": noncert,
    }


def print_ranking(name, a):
    n = a["files_run"]
    print(f"\n=== {name}: fail-closed cost {a['fail_closed_cost']['files']}/{n} "
          f"({a['fail_closed_cost']['pct_of_eligible']}%) ===")
    print("outcomes:", a["outcome_histogram_locked_harness_semantics"])
    print("cause histogram (per-file; multi-cause files count once per cause):")
    for c, k in a["refusal_cause_histogram"].items():
        print(f"  {c:50s} {k}")
    print("marginal analysis (sole-cause flips), ranked:")
    print(f"  {'class':38s} {'kind':18s} {'contain':>7s} {'sole':>5s} "
          f"{'flips':>5s} {'%elig':>6s}")
    for d in a["marginal_analysis_ranked"]:
        ub = " (UB)" if d.get("flip_is_upper_bound") else ""
        print(f"  {d['class']:38s} {d['kind']:18s} {d['files_containing']:7d} "
              f"{d['sole_cause_files']:5d} {d['flips']:5d} "
              f"{d['flip_pct_of_eligible']:6.1f}{ub}")
    for c in a["cumulative_flips_top_k"]:
        print(f"  top-{c['top_k']} {c['classes']}: flips {c['flips']} "
              f"({c['flip_pct_of_eligible']}% of eligible); residual cost "
              f"{c['residual_cost_pct_after_flips']}%")


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    out = {
        "analysis": "cost_breakdown: refusal-cause attribution for the own-transform "
                    "fail-closed cost of the locked in-the-wild run (leg 3a, "
                    "insert-rows@2, certify orig vs xlq's own output)",
        "HONESTY_NOTE": "exploratory post-test analysis; REUSES the locked-test "
                        "corpora post-hoc. No new information about guard soundness "
                        "is extracted (same deterministic pipeline, same files) — "
                        "only refusal-cause attribution. Cost-reduction claims "
                        "require confirmation in locked test v2.",
        "method": {
            "outcome_semantics": "outcome = json.loads(certify stdout).get('status', "
                                 "'ERROR'), byte-identical to inthewild_run.certify, "
                                 "so histograms tie out to the locked own_* tallies; "
                                 "RESTRUCTURE_REFUSED covers eligible files the locked "
                                 "run never certified because xlq restructure refused "
                                 "(they count in the fail-closed cost).",
            "denylist_detection": "mirrors xlq/src/certify.rs verify_noncell_refs: "
                                  "sheet-XML substring needles (<dataValidation, "
                                  "<conditionalFormatting, sparkline) over all "
                                  "xl/worksheets/sheet*.xml + zip part prefixes "
                                  "(xl/charts/, xl/pivotTables/, xl/pivotCache/, "
                                  "xl/externalLinks/); certify reports only the FIRST "
                                  "hit, so cause sets are grounded by scanning the "
                                  "artifact zip directly.",
            "flip_rules": "deny:X — sole cause AND ironcalc loads the transform "
                          "(xlq diff f f probe; denylist check precedes the loader). "
                          "harness:stdout_pollution — recovered verdict CERTIFIED. "
                          "harness:sheet_name_xml_entity — decoded-name retry "
                          "CERTIFIED. residual:X — UPPER BOUND (sole cause, no "
                          "denylist parts, original loads); the transform does not "
                          "exist for these, so certification cannot be confirmed. "
                          "pivot = pivotTables + pivotCache verified together.",
        },
        "corpora": {},
    }
    for name, cdir in CORPORA:
        elig_counts, rows = run_corpus(name, cdir)
        a = analyze(name, elig_counts, rows)
        out["corpora"][name] = a
        print_ranking(name, a)
    with open(os.path.join(BENCH, "cost_breakdown.json"), "w") as f:
        json.dump(out, f, indent=2)
    print(f"\nwrote {os.path.join(BENCH, 'cost_breakdown.json')}")
