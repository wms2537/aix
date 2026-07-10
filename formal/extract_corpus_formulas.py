#!/usr/bin/env python3
"""Extract every formula body from the real-workbook corpora into one
deduplicated, sorted line-per-formula file — the input universe for the
Lean↔Rust tokenizer differential (formal/differential_check.py).

Sources:
  vendor/upstream/xlsx/tests/**/*.xlsx     (ironcalc upstream test corpus)
  data/inthewild/{euses,enron}/converted/**/*.xlsx

Unlike benchmarks/shift_correctness_real.py:formulas_of, this walks ALL
sheet parts of every workbook (not just the first) and applies NO volatile
filter — the differential must see every formula the tokenizer can meet.
The FTAG/FBODY cell-association regexes are the same (self-closing-safe:
an <f> is only taken from INSIDE its own <c>...</c> body, so shared-string
or empty self-closing cells can never leak a neighbouring cell's formula).
"""

import re
import sys
import zipfile
from pathlib import Path

ROOT = Path("/home/soh/aix")
SOURCES = [
    ROOT / "vendor/upstream/xlsx/tests",
    ROOT / "data/inthewild/euses/converted",
    ROOT / "data/inthewild/enron/converted",
]
OUT = ROOT / "formal/corpus_formulas.txt"

# Same regexes as benchmarks/shift_correctness_real.py (self-closing-safe).
FTAG = re.compile(rb'<c r="([A-Z]+)(\d+)"[^>]*(?:/>|>((?:(?!</c>|<c[ >/]).)*)</c>)', re.S)
FBODY = re.compile(rb'<f[^>]*>([^<]*)</f>')

SHEET_PART = re.compile(r"xl/worksheets/sheet\d+\.xml$")


def decode_entities(f):
    # &amp; last, standard order (research-log/017).
    for ent, ch in (("&lt;", "<"), ("&gt;", ">"), ("&quot;", '"'),
                    ("&apos;", "'"), ("&amp;", "&")):
        f = f.replace(ent, ch)
    return f


def formulas_of_all_sheets(path):
    """Yield every <f> body from every sheet part of one workbook."""
    with zipfile.ZipFile(path) as z:
        parts = sorted(n for n in z.namelist() if SHEET_PART.search(n))
        for part in parts:
            data = z.read(part)
            for m in FTAG.finditer(data):
                body = m.group(3)
                if body is None:              # self-closing empty cell
                    continue
                fm = FBODY.search(body)       # the <f> INSIDE this cell only
                if not fm:
                    continue
                yield decode_entities(fm.group(1).decode("utf-8", "replace"))


def main():
    files = []
    for src in SOURCES:
        if not src.is_dir():
            print(f"WARNING: missing source dir {src}", file=sys.stderr)
            continue
        files.extend(sorted(src.rglob("*.xlsx")))

    formulas = set()
    bad = 0
    for p in files:
        try:
            formulas.update(formulas_of_all_sheets(p))
        except (zipfile.BadZipFile, KeyError, OSError) as e:
            bad += 1
            print(f"WARNING: skipping {p}: {e}", file=sys.stderr)

    formulas.discard("")  # empty <f/> bodies carry nothing to tokenize
    ordered = sorted(formulas)
    OUT.write_text("\n".join(ordered) + ("\n" if ordered else ""), encoding="utf-8")

    non_ascii = sum(1 for f in ordered if any(ord(c) > 127 for c in f))
    sheet_qualified = sum(1 for f in ordered if "!" in f)
    with_string_lit = sum(1 for f in ordered if '"' in f)

    print(f"workbooks scanned : {len(files)} ({bad} unreadable, skipped)")
    print(f"unique formulas   : {len(ordered)} -> {OUT}")
    print(f"  non-ASCII       : {non_ascii}")
    print(f"  sheet-qualified : {sheet_qualified} (contain '!')")
    print(f"  string literals : {with_string_lit} (contain '\"')")


if __name__ == "__main__":
    main()
