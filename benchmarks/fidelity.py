#!/usr/bin/env python3
"""
E1 - Fidelity-preservation experiment.

Applies ONE representative logical edit (set a specific data cell to a new
number) to each fixture THREE ways and records OOXML part-level outcomes vs
the untouched original:

  (a) xlq apply   - surgical typed patch. Expectation: only the edited sheet
                    part changes (+ the stale xl/calcChain.xml cache is
                    dropped); charts / pivots / VBA / sharedStrings / styles
                    stay byte-identical.
  (b) openpyxl    - load_workbook + set cell + save. For macro.xlsm this uses
                    the DEFAULT keep_vba=False (the realistic agent path: the
                    macro is silently lost and the file is saved as .xlsx).
  (c) LibreOffice - `soffice --convert-to` re-save proxy. LibreOffice is not a
                    CLI cell-editor, so this measures what a headless re-save
                    (open + write-back) does to the parts. METHODOLOGY CAVEAT:
                    this is a re-save, NOT the targeted edit the other two
                    tools perform, so its 100% rewrite is an upper bound on
                    re-save churn, not a like-for-like edit.

Self-contained and rerunnable. Paths to the three external tools are taken
from CLI flags (with sensible defaults) so the experiment can be reproduced on
another machine:

  python fidelity.py \
      --xlq   /path/to/xlq \
      --load-only /path/to/load-only \
      --python /path/to/python-with-openpyxl \
      --soffice /usr/bin/soffice \
      --repo-root /home/soh/aix \
      --out benchmarks/fidelity.json

Writes benchmarks/fidelity.json (per fixture per tool metrics). Never touches
the fixtures in place - every tool operates on a fresh copy in a work dir.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile


# ---------------------------------------------------------------------------
# The experiment matrix: ONE representative edit per fixture. Each cell was
# picked from the real fixture via `xlq inspect` / openpyxl as a genuine
# numeric DATA cell (an input, not a formula), so the edit is the kind an
# agent actually makes ("set this number to that number").
# ---------------------------------------------------------------------------
FIXTURES = [
    {
        "name": "pivot-chart.xlsx",
        "rel": "fixtures/t1/pivot-chart.xlsx",
        "kind": "charts + pivot cache",
        "sheet": "Sheet1",
        "cell": "A2",
        "from": 222,
        "to": 999,
    },
    {
        "name": "macro.xlsm",
        "rel": "fixtures/t1/macro.xlsm",
        "kind": "VBA macros",
        "sheet": "Data",
        "cell": "B2",
        "from": 100,
        "to": 200,
    },
    {
        "name": "payroll.xlsx",
        "rel": "fixtures/payroll.xlsx",
        "kind": "base: multi-sheet formulas",
        "sheet": "Rates",
        "cell": "B2",
        "from": 16,
        "to": 25,
    },
    {
        "name": "claims.xlsx",
        "rel": "fixtures/claims.xlsx",
        "kind": "base: 1.3k formulas",
        "sheet": "Claims",
        "cell": "D2",
        "from": 13335,
        "to": 20000,
    },
]


def read_parts(path):
    """name -> bytes for every (non-dir) member of the zip package."""
    z = zipfile.ZipFile(path)
    out = {}
    for n in z.namelist():
        if n.endswith("/"):
            continue
        out[n] = z.read(n)
    z.close()
    return out


def is_chart(name):
    return (
        name.startswith("xl/charts/chart")
        and name.endswith(".xml")
        and "_rels" not in name
    )


def is_pivot(name):
    return (
        name.startswith("xl/pivotTables/pivotTable")
        and name.endswith(".xml")
        and "_rels" not in name
    )


def is_vba(name):
    return name == "xl/vbaProject.bin"


def features_present(parts):
    keys = parts.keys()
    return {
        "charts": any(is_chart(k) for k in keys),
        "pivot": any(is_pivot(k) for k in keys),
        "vba": any(is_vba(k) for k in keys),
    }


def compare(original_parts, output_path):
    """Part-level diff of an output package against the original parts map."""
    out_parts = read_parts(output_path)
    o = set(original_parts)
    n = set(out_parts)
    identical = sorted(k for k in (o & n) if original_parts[k] == out_parts[k])
    rewritten = sorted(k for k in (o & n) if original_parts[k] != out_parts[k])
    dropped = sorted(o - n)
    added = sorted(n - o)
    orig_feat = features_present(original_parts)
    out_feat = features_present(out_parts)
    # feature_survival: for a feature the ORIGINAL had, did it survive to the
    # output? For a feature the original lacked, survival is trivially N/A ->
    # reported as false (there was nothing to preserve).
    survival = {
        f: (out_feat[f] if orig_feat[f] else False) for f in ("charts", "pivot", "vba")
    }
    return {
        "parts_total": len(o),
        "parts_byte_identical": len(identical),
        "parts_rewritten": len(rewritten),
        "parts_dropped": dropped,
        "parts_added": added,
        "parts_rewritten_names": rewritten,
        "feature_survival": survival,
    }


def sha256_file(path):
    import hashlib

    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Tool (a): xlq apply
# ---------------------------------------------------------------------------
def run_xlq(fx, src, workdir, xlq_bin):
    ext = os.path.splitext(fx["rel"])[1]
    work = os.path.join(workdir, "xlq_" + fx["name"])
    shutil.copy(src, work)
    base_hash = sha256_file(work)
    patch = {
        "base_hash": base_hash,
        "actor": "e1-fidelity",
        "ops": [
            {
                "type": "set_cell",
                "sheet": fx["sheet"],
                "cell": fx["cell"],
                "value": fx["to"],
            }
        ],
        "watch": [f"{fx['sheet']}!{fx['cell']}"],
        "clock": 1751500000000,
        "seed": 1,
    }
    patch_path = work + ".patch.json"
    with open(patch_path, "w") as f:
        json.dump(patch, f)
    res = subprocess.run(
        [xlq_bin, "apply", work, patch_path, "--actor", "e1-fidelity"],
        capture_output=True,
        text=True,
    )
    report = None
    try:
        report = json.loads(res.stdout)
    except Exception:
        report = {"stderr": res.stderr[-2000:], "returncode": res.returncode}
    return work, {"self_reported_fidelity": report.get("fidelity") if isinstance(report, dict) else None,
                  "result_hash": report.get("result_hash") if isinstance(report, dict) else None}


# ---------------------------------------------------------------------------
# Tool (b): openpyxl load + set cell + save
# ---------------------------------------------------------------------------
OPENPYXL_DRIVER = r'''
import sys, warnings, openpyxl
warnings.simplefilter("ignore")
src, out, sheet, cell, to = sys.argv[1:6]
to = float(to)
if to.is_integer():
    to = int(to)
# keep_vba defaults to False -> the realistic agent path for .xlsm too.
wb = openpyxl.load_workbook(src)
wb[sheet][cell] = to
wb.save(out)
print("OK")
'''


def run_openpyxl(fx, src, workdir, python_bin):
    # openpyxl with keep_vba=False cannot write .xlsm; it emits .xlsx. That is
    # exactly the realistic failure we want to record for macro.xlsm.
    out = os.path.join(workdir, "openpyxl_" + os.path.splitext(fx["name"])[0] + ".xlsx")
    driver = os.path.join(workdir, "_openpyxl_driver.py")
    with open(driver, "w") as f:
        f.write(OPENPYXL_DRIVER)
    res = subprocess.run(
        [python_bin, driver, src, out, fx["sheet"], fx["cell"], str(fx["to"])],
        capture_output=True,
        text=True,
    )
    if res.returncode != 0 or not os.path.exists(out):
        return None, {"error": res.stderr[-2000:], "returncode": res.returncode}
    return out, {"output_ext": ".xlsx"}


# ---------------------------------------------------------------------------
# Tool (c): LibreOffice re-save proxy
# ---------------------------------------------------------------------------
def run_libreoffice(fx, src, workdir, soffice_bin):
    ext = os.path.splitext(fx["rel"])[1]
    outdir = os.path.join(workdir, "lo_" + os.path.splitext(fx["name"])[0])
    os.makedirs(outdir, exist_ok=True)
    # Re-save to the SAME format so this measures re-save churn, not a format
    # downgrade. .xlsm uses the VBA-preserving Calc filter so the macro has a
    # fair chance to survive; .xlsx round-trips as .xlsx.
    if ext == ".xlsm":
        convert = "xlsm:Calc MS Excel 2007 VBA XML"
        out_name = fx["name"]
    else:
        convert = "xlsx:Calc MS Excel 2007 XML"
        out_name = os.path.splitext(fx["name"])[0] + ".xlsx"
    env = dict(os.environ)
    # Isolate the LO user profile so a stale lock does not block headless runs.
    profile = os.path.join(workdir, "lo_profile")
    res = subprocess.run(
        [
            soffice_bin,
            "--headless",
            "--norestore",
            f"-env:UserInstallation=file://{profile}",
            "--convert-to",
            convert,
            "--outdir",
            outdir,
            src,
        ],
        capture_output=True,
        text=True,
        env=env,
        timeout=180,
    )
    out = os.path.join(outdir, out_name)
    if not os.path.exists(out):
        return None, {"error": (res.stdout + res.stderr)[-2000:], "returncode": res.returncode}
    return out, {"filter": convert, "note": "re-save proxy, not a targeted edit"}


# ---------------------------------------------------------------------------
# Verification: does the produced file re-open?
# ---------------------------------------------------------------------------
def loads_in_ironcalc(path, load_only_bin):
    if not load_only_bin or not os.path.exists(load_only_bin):
        return None
    res = subprocess.run([load_only_bin, path], capture_output=True, text=True)
    return res.returncode == 0


def loads_in_soffice(path, soffice_bin, workdir):
    """Best-effort: convert the produced file to xlsx in a scratch dir; success
    (a non-empty output appears) means LibreOffice could open it."""
    if not soffice_bin:
        return None
    verify_dir = tempfile.mkdtemp(prefix="lo_verify_", dir=workdir)
    profile = os.path.join(workdir, "lo_profile")
    try:
        subprocess.run(
            [
                soffice_bin,
                "--headless",
                "--norestore",
                f"-env:UserInstallation=file://{profile}",
                "--convert-to",
                "xlsx:Calc MS Excel 2007 XML",
                "--outdir",
                verify_dir,
                path,
            ],
            capture_output=True,
            text=True,
            timeout=180,
        )
        stem = os.path.splitext(os.path.basename(path))[0]
        produced = os.path.join(verify_dir, stem + ".xlsx")
        return os.path.exists(produced) and os.path.getsize(produced) > 0
    except Exception:
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo-root", default=os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))
    ap.add_argument("--xlq", default=None, help="path to the xlq release binary")
    ap.add_argument("--load-only", default=None, help="path to the load-only helper binary")
    ap.add_argument("--python", default=sys.executable, help="python with openpyxl installed")
    ap.add_argument("--soffice", default="/usr/bin/soffice")
    ap.add_argument("--out", default=None)
    ap.add_argument("--workdir", default=None)
    args = ap.parse_args()

    root = os.path.abspath(args.repo_root)
    xlq = args.xlq or os.path.join(root, "xlq/target/release/xlq")
    load_only = args.load_only or os.path.join(root, "xlq/target/release/load-only")
    out_path = args.out or os.path.join(root, "benchmarks/fidelity.json")
    soffice = args.soffice if args.soffice and os.path.exists(args.soffice) else None

    workdir = args.workdir or tempfile.mkdtemp(prefix="e1_fidelity_")
    os.makedirs(workdir, exist_ok=True)

    # Engine provenance for the meta block.
    engine = None
    try:
        insp = subprocess.run([xlq, "inspect", os.path.join(root, FIXTURES[0]["rel"])],
                              capture_output=True, text=True)
        engine = json.loads(insp.stdout)["coverage"]["engine"]
    except Exception:
        pass

    result = {
        "experiment": "E1 fidelity-preservation",
        "meta": {
            "engine": engine,
            "xlq": xlq,
            "python": args.python,
            "soffice": soffice,
            "methodology": {
                "part_diff": "byte-level comparison of every OOXML part (zip member) in the output vs the untouched original.",
                "openpyxl_macro": "macro.xlsm uses openpyxl's DEFAULT keep_vba=False (realistic agent path): the workbook is re-saved as .xlsx and the macro is dropped.",
                "libreoffice_caveat": "LibreOffice has no CLI cell-edit; the LibreOffice column is a `soffice --convert-to` RE-SAVE proxy (open + write-back to the same format), NOT the targeted edit the other two tools perform. Its rewrite counts are an upper bound on re-save churn, not a like-for-like edit.",
                "calcchain": "xl/calcChain.xml is a derived formula-dependency CACHE. Dropping it is lossless: Excel/LibreOffice regenerate it on open. xlq deliberately drops it because a surgical edit makes it stale.",
                "feature_survival": "A feature counts as surviving only if it was present in the original AND a core part for it (xl/charts/chartN.xml, xl/pivotTables/pivotTableN.xml, xl/vbaProject.bin) is present in the output. Presence does NOT imply the feature is byte-identical - see parts_rewritten.",
                "ironcalc_load": "output re-opened with the ironcalc engine (load-only helper); nonzero exit = load failure.",
                "soffice_load": "best-effort: output re-converted to xlsx by LibreOffice; a non-empty result = LibreOffice opened it.",
            },
        },
        "fixtures": {},
    }

    for fx in FIXTURES:
        src = os.path.join(root, fx["rel"])
        original_parts = read_parts(src)
        orig_feat = features_present(original_parts)
        print(f"\n=== {fx['name']} ({fx['kind']}): set {fx['sheet']}!{fx['cell']} "
              f"{fx['from']} -> {fx['to']} ===", file=sys.stderr)

        entry = {
            "edit": {
                "sheet": fx["sheet"],
                "cell": fx["cell"],
                "from": fx["from"],
                "to": fx["to"],
                "kind": fx["kind"],
            },
            "original": {
                "parts_total": len(original_parts),
                "features": orig_feat,
            },
            "tools": {},
        }

        runners = [
            ("xlq", run_xlq, xlq),
            ("openpyxl", run_openpyxl, args.python),
            ("libreoffice", run_libreoffice, soffice),
        ]
        for tool, runner, toolarg in runners:
            print(f"  -> {tool}", file=sys.stderr)
            if tool == "libreoffice" and soffice is None:
                entry["tools"][tool] = {"error": "soffice not available"}
                continue
            try:
                out_file, extra = runner(fx, src, workdir, toolarg)
            except Exception as e:
                entry["tools"][tool] = {"error": f"{type(e).__name__}: {e}"}
                continue
            if out_file is None:
                entry["tools"][tool] = {"error": extra}
                continue
            metrics = compare(original_parts, out_file)
            metrics["output_ext"] = os.path.splitext(out_file)[1]
            metrics["output_loads_in_ironcalc"] = loads_in_ironcalc(out_file, load_only)
            metrics["output_loads_in_soffice"] = loads_in_soffice(out_file, soffice, workdir)
            metrics.update({k: v for k, v in extra.items() if k not in metrics})
            entry["tools"][tool] = metrics

        result["fixtures"][fx["name"]] = entry

    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)
    print(f"\nwrote {out_path}", file=sys.stderr)
    # Echo a compact summary to stdout.
    for name, e in result["fixtures"].items():
        print(f"\n{name}: {e['original']['parts_total']} parts, "
              f"features={ {k:v for k,v in e['original']['features'].items() if v} }")
        for tool, m in e["tools"].items():
            if "error" in m:
                print(f"  {tool:12} ERROR {m['error']}")
                continue
            print(f"  {tool:12} identical={m['parts_byte_identical']}/{m['parts_total']} "
                  f"rewritten={m['parts_rewritten']} dropped={len(m['parts_dropped'])} "
                  f"added={len(m['parts_added'])} survive={m['feature_survival']} "
                  f"ironcalc={m['output_loads_in_ironcalc']} soffice={m['output_loads_in_soffice']}")


if __name__ == "__main__":
    main()
