#!/usr/bin/env bash
# xlq benchmark harness — performance + preservation.
#
# Self-contained: builds xlq (release), runs every benchmark from scratch,
# and writes /home/soh/aix/benchmarks/results.json.
#
# Sections:
#   A. LOAD+CALC   xlq calc vs LibreOffice convert vs openpyxl load,
#                  plus an isolated ironcalc load-only measurement
#   B. INSPECT     xlq inspect per fixture (time + output bytes)
#   C. TOKEN EFF.  xlq census JSON bytes vs naive full-sheet openpyxl dump
#   D. PRESERVATION  zip-part diff after re-save via openpyxl / LibreOffice /
#                    ironcalc roundtrip, plus "still loads in ironcalc" check,
#                    an xlq diff cell-level comparison, and a
#                    core.xml-ignoring-dcterms:modified content check
#
# Timing protocol: for each timed command, 1 untimed warmup run, then 3 warm
# runs; the MEDIAN wall time of the 3 is reported (seconds, ms resolution).
#
# Requirements: bash>=5 (EPOCHREALTIME), jq, cargo, python3 with
# openpyxl on PATH (or set PY), soffice on PATH (or set SOFFICE).
# Override any path via the env vars below.

set -euo pipefail
export LC_ALL=C

# Repo root: derived from this script's location so a fresh clone works
# without configuration; override with ROOT=... if needed.
ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
XLQ_DIR="${XLQ_DIR:-$ROOT/xlq}"
FIXDIR="${FIXDIR:-$ROOT/fixtures}"
BENCH_DIR="${BENCH_DIR:-$ROOT/benchmarks}"
PY="${PY:-$(command -v python3 || true)}"
SOFFICE="${SOFFICE:-$(command -v soffice || echo /usr/bin/soffice)}"
RESULTS="${RESULTS:-$BENCH_DIR/results.json}"

if [[ -z "$PY" ]] || ! "$PY" -c "import openpyxl" >/dev/null 2>&1; then
  echo "error: no python with openpyxl found; set PY=/path/to/python" >&2
  exit 1
fi
if [[ ! -x "$SOFFICE" ]]; then
  echo "error: soffice not found; set SOFFICE=/path/to/soffice" >&2
  exit 1
fi

WORK="$BENCH_DIR/work"
rm -rf "$WORK"
mkdir -p "$WORK"/{scripts,inspect_out,preserve/openpyxl,preserve/libreoffice,preserve/ironcalc,census,lo_out}

echo "== build (cargo build --release) ==" >&2
(cd "$XLQ_DIR" && cargo build --release --quiet)
XLQ="$XLQ_DIR/target/release/xlq"
ROUNDTRIP="$XLQ_DIR/target/release/roundtrip"
LOADONLY="$XLQ_DIR/target/release/load-only"

FIXTURES=()
while IFS= read -r f; do FIXTURES+=("$f"); done < <(ls "$FIXDIR"/*.xlsx | sort)
PERF_LARGE="$FIXDIR/perf-large.xlsx"

LO_PROFILE="file://$WORK/loprofile"
soffice_convert() { # $1=input $2=outdir
  "$SOFFICE" --headless "-env:UserInstallation=$LO_PROFILE" \
    --convert-to xlsx --outdir "$2" "$1" >/dev/null 2>&1
}

# ---------------------------------------------------------------- helpers --
time_once() { # time one run of "$@" -> seconds on stdout
  local t0="$EPOCHREALTIME"
  "$@" >/dev/null 2>&1
  local t1="$EPOCHREALTIME"
  awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.3f", b - a}'
}

median3() { # 1 warmup + 3 warm runs of "$@" -> median seconds on stdout
  "$@" >/dev/null 2>&1 # warmup (not counted)
  local runs=()
  for _ in 1 2 3; do runs+=("$(time_once "$@")"); done
  printf '%s\n' "${runs[@]}" | sort -g | sed -n 2p
}

# ------------------------------------------------- embedded python helpers --
# Naive full-sheet dump: every populated cell's formula + cached value as
# JSON. This is what an agent would have to read without a census.
cat > "$WORK/scripts/dump_naive.py" <<'EOF'
import json, sys
from openpyxl import load_workbook
wf = load_workbook(sys.argv[1], data_only=False)   # formulas
wv = load_workbook(sys.argv[1], data_only=True)    # cached values
out = {}
for ws in wf.worksheets:
    vs = wv[ws.title]
    out[ws.title] = {c.coordinate: {"f": c.value if c.data_type == "f" else None,
                                    "v": vs[c.coordinate].value}
                     for row in ws.iter_rows() for c in row if c.value is not None}
json.dump(out, sys.stdout, default=str)
EOF

# openpyxl load+save roundtrip
cat > "$WORK/scripts/opxl_resave.py" <<'EOF'
import sys
from openpyxl import load_workbook
load_workbook(sys.argv[1]).save(sys.argv[2])
EOF

# openpyxl load only (section A; openpyxl cannot recalculate)
cat > "$WORK/scripts/opxl_load.py" <<'EOF'
import sys
from openpyxl import load_workbook
load_workbook(sys.argv[1], data_only=False)
EOF

# Zip part diff: names, uncompressed sizes, and content CRCs -> JSON on stdout
cat > "$WORK/scripts/zipcmp.py" <<'EOF'
import json, sys, zipfile
# Directory placeholder entries ("xl/", "_rels/") are zip bookkeeping, not
# OOXML parts; excluded so drop/add counts reflect real parts only.
def parts(p): return {i.filename: (i.file_size, i.CRC)
                      for i in zipfile.ZipFile(p).infolist()
                      if not i.filename.endswith("/")}
a, b = parts(sys.argv[1]), parts(sys.argv[2])
common = set(a) & set(b)
ws = lambda d: sum(v[0] for k, v in d.items() if k.startswith("xl/worksheets/"))
json.dump({
    "parts_before": len(a), "parts_after": len(b),
    "dropped": sorted(set(a) - set(b)),
    "added": sorted(set(b) - set(a)),
    "changed_size": sorted(k for k in common if a[k][0] != b[k][0]),
    "changed_content_same_size": sorted(k for k in common
                                        if a[k][0] == b[k][0] and a[k][1] != b[k][1]),
    "identical": sorted(k for k in common if a[k] == b[k]),
    # Uncompressed worksheet-XML footprint, for re-serialization size claims.
    "worksheet_xml_bytes_before": ws(a),
    "worksheet_xml_bytes_after": ws(b),
}, sys.stdout)
EOF

# docProps/core.xml comparison ignoring <dcterms:modified>: verifies (or
# refutes) "the timestamp is the entire difference" claims mechanically.
cat > "$WORK/scripts/corecmp.py" <<'EOF'
import json, re, sys, zipfile
def core(p):
    try:
        with zipfile.ZipFile(p) as z:
            return z.read("docProps/core.xml").decode("utf-8", "replace")
    except KeyError:
        return None
def strip_modified(x):
    return re.sub(r"<dcterms:modified[^>]*>[^<]*</dcterms:modified>", "", x)
a, b = core(sys.argv[1]), core(sys.argv[2])
if a is None or b is None:
    json.dump({"core_xml_present_in_both": False}, sys.stdout)
else:
    json.dump({
        "core_xml_present_in_both": True,
        "core_xml_identical": a == b,
        "core_xml_equal_ignoring_modified": strip_modified(a) == strip_modified(b),
    }, sys.stdout)
EOF

# ------------------------------------------------------------- section A --
echo "== A: LOAD+CALC on $(basename "$PERF_LARGE") ==" >&2
A_XLQ=$(median3 "$XLQ" calc "$PERF_LARGE")
echo "  xlq calc:            ${A_XLQ}s" >&2
A_IC_LOAD=$(median3 "$LOADONLY" "$PERF_LARGE")
echo "  ironcalc load only:  ${A_IC_LOAD}s" >&2
A_LO=$(median3 soffice_convert "$PERF_LARGE" "$WORK/lo_out")
echo "  soffice convert:     ${A_LO}s" >&2
A_OPXL=$(median3 "$PY" "$WORK/scripts/opxl_load.py" "$PERF_LARGE")
echo "  openpyxl load:       ${A_OPXL}s (calc: n/a, cannot)" >&2

jq -n --arg f "$(basename "$PERF_LARGE")" \
  --argjson xlq "$A_XLQ" --argjson icload "$A_IC_LOAD" \
  --argjson lo "$A_LO" --argjson op "$A_OPXL" '{
  fixture: $f,
  protocol: "1 warmup + 3 warm runs, median wall seconds",
  results: [
    {tool: "xlq calc (ironcalc)", operation: "load + full recalc + stored-vs-recomputed compare", median_s: $xlq},
    {tool: "ironcalc load only (load-only bin)", operation: "load_from_xlsx and exit; isolates parse/load from census, hashing, recalc", median_s: $icload},
    {tool: "libreoffice --convert-to xlsx", operation: "process spawn + load + save (closest CLI recalc analog; NOT an isolated recalc — see caveats)", median_s: $lo},
    {tool: "openpyxl load_workbook(data_only=False)", operation: "load only", median_s: $op, calc: "n/a (cannot)"}
  ],
  caveats: [
    "LibreOffice has no headless recalc-only mode; --convert-to re-saves the file, so its number includes process startup, load, and a full write — but Calc does NOT recalculate xlsx formulas on load by default (see docs/BASELINE.md section 4), so recalculation is likely absent. Treat it as the cheapest way to push a file through LibreOffice, NOT as a bound on its load+recalc cost: a forced hard recalc (e.g. via UNO) could exceed it.",
    "openpyxl parses the workbook but has no formula engine; it cannot recalculate at all.",
    "xlq calc includes SHA-256 hashing of the file plus JSON report serialization in addition to load+evaluate."
  ]}' > "$WORK/a.json"

# ------------------------------------------------------------- section B --
echo "== B: INSPECT per fixture ==" >&2
: > "$WORK/b.jsonl"
for f in "${FIXTURES[@]}"; do
  name=$(basename "$f")
  out="$WORK/inspect_out/$name.json"
  "$XLQ" inspect "$f" > "$out"
  bytes=$(wc -c < "$out")
  t=$(median3 "$XLQ" inspect "$f")
  echo "  $name: ${t}s, ${bytes} bytes" >&2
  jq -n --arg f "$name" --argjson t "$t" --argjson b "$bytes" \
    --argjson fb "$(stat -c%s "$f")" \
    '{fixture: $f, fixture_bytes: $fb, median_s: $t, output_bytes: $b}' >> "$WORK/b.jsonl"
done

# ------------------------------------------------------------- section C --
echo "== C: TOKEN EFFICIENCY (census vs naive dump) ==" >&2
: > "$WORK/c.jsonl"
for f in "$FIXDIR/branch-consolidation.xlsx" "$PERF_LARGE"; do
  name=$(basename "$f")
  census="$WORK/census/$name.census.json"
  naive="$WORK/census/$name.naive.json"
  "$XLQ" inspect "$f" > "$census"
  "$PY" "$WORK/scripts/dump_naive.py" "$f" > "$naive"
  cb=$(wc -c < "$census"); nb=$(wc -c < "$naive")
  ratio=$(awk -v n="$nb" -v c="$cb" 'BEGIN{printf "%.1f", n / c}')
  echo "  $name: census ${cb} B vs naive ${nb} B -> ${ratio}x" >&2
  jq -n --arg f "$name" --argjson c "$cb" --argjson n "$nb" --argjson r "$ratio" \
    '{fixture: $f, census_bytes: $c, naive_dump_bytes: $n, naive_over_census_ratio: $r}' >> "$WORK/c.jsonl"
done

# ------------------------------------------------------------- section D --
echo "== D: PRESERVATION (zip-part roundtrip diff) ==" >&2
: > "$WORK/d.jsonl"
for f in "${FIXTURES[@]}"; do
  name=$(basename "$f")
  for tool in openpyxl libreoffice ironcalc; do
    out="$WORK/preserve/$tool/$name"
    ok=true
    case "$tool" in
      openpyxl)    "$PY" "$WORK/scripts/opxl_resave.py" "$f" "$out" || ok=false ;;
      libreoffice) soffice_convert "$f" "$WORK/preserve/libreoffice" || ok=false ;;
      ironcalc)    "$ROUNDTRIP" "$f" "$out" || ok=false ;;
    esac
    if [[ "$ok" != true || ! -s "$out" ]]; then
      jq -n --arg f "$name" --arg t "$tool" \
        '{fixture: $f, tool: $t, resave_ok: false}' >> "$WORK/d.jsonl"
      echo "  $name / $tool: RESAVE FAILED" >&2
      continue
    fi
    cmp_json=$("$PY" "$WORK/scripts/zipcmp.py" "$f" "$out")
    core_json=$("$PY" "$WORK/scripts/corecmp.py" "$f" "$out")
    loads=true
    "$XLQ" inspect "$out" >/dev/null 2>&1 || loads=false
    # Cell-level comparison: does the re-save preserve every cell's value and
    # formula as xlq diff sees them? (kind counts expose formula-text rewrites)
    cell_json=$("$XLQ" diff "$f" "$out" | jq '{
      summary: {changed: .summary.changed, added: .summary.added, removed: .summary.removed},
      kinds: (reduce .changes[].kind as $k ({}; .[$k] += 1)),
      sheets_added: (.sheets_added | length),
      sheets_removed: (.sheets_removed | length)}' \
      || echo '{"diff_failed": true}')
    jq -n --arg f "$name" --arg t "$tool" --argjson c "$cmp_json" \
      --argjson core "$core_json" --argjson cell "$cell_json" \
      --argjson loads "$loads" --argjson ob "$(stat -c%s "$out")" \
      '{fixture: $f, tool: $t, resave_ok: true, output_bytes: $ob,
        loads_in_ironcalc: $loads, cell_diff: $cell} + $c + $core' >> "$WORK/d.jsonl"
    echo "  $name / $tool: dropped=$(echo "$cmp_json" | jq '.dropped|length') added=$(echo "$cmp_json" | jq '.added|length') changed_size=$(echo "$cmp_json" | jq '.changed_size|length') changed_content=$(echo "$cmp_json" | jq '.changed_content_same_size|length') loads_in_ironcalc=$loads cell_changed=$(echo "$cell_json" | jq '.summary.changed // "n/a"')" >&2
  done
done

# ------------------------------------------------------------ section D2 --
# The fixtures above were authored by ironcalc's own writer (xlq-fixtures),
# so D is a BEST CASE for ironcalc. D2 feeds ironcalc files authored by
# OTHER tools (the openpyxl and LibreOffice re-saves of
# branch-consolidation.xlsx from D) and diffs what its roundtrip does to
# OOXML it did not write itself.
echo "== D2: FOREIGN-FILE roundtrip through ironcalc ==" >&2
: > "$WORK/d2.jsonl"
mkdir -p "$WORK/preserve/ironcalc_foreign"
for origin in openpyxl libreoffice; do
  src="$WORK/preserve/$origin/branch-consolidation.xlsx"
  out="$WORK/preserve/ironcalc_foreign/from-$origin-branch-consolidation.xlsx"
  ok=true
  "$ROUNDTRIP" "$src" "$out" || ok=false
  if [[ "$ok" != true || ! -s "$out" ]]; then
    jq -n --arg o "$origin" '{input_authored_by: $o, fixture: "branch-consolidation.xlsx", resave_ok: false}' >> "$WORK/d2.jsonl"
    echo "  from $origin: ROUNDTRIP FAILED" >&2
    continue
  fi
  cmp_json=$("$PY" "$WORK/scripts/zipcmp.py" "$src" "$out")
  loads=true
  "$XLQ" inspect "$out" >/dev/null 2>&1 || loads=false
  jq -n --arg o "$origin" --argjson c "$cmp_json" --argjson loads "$loads" \
    '{input_authored_by: $o, fixture: "branch-consolidation.xlsx",
      tool: "ironcalc roundtrip", resave_ok: true,
      loads_in_ironcalc: $loads} + $c' >> "$WORK/d2.jsonl"
  echo "  from $origin: dropped=$(echo "$cmp_json" | jq '.dropped|length') added=$(echo "$cmp_json" | jq '.added|length') changed_size=$(echo "$cmp_json" | jq '.changed_size|length') changed_content=$(echo "$cmp_json" | jq '.changed_content_same_size|length') loads_in_ironcalc=$loads" >&2
done

# ------------------------------------------------------------ assemble ----
MODEL=$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
CORES=$(nproc)
OPXL_VER=$("$PY" -c "import openpyxl; print(openpyxl.__version__)")
LO_VER=$("$SOFFICE" --version 2>/dev/null | head -1 | tr -d '\n')
XLQ_VER=$("$XLQ" --version | tr -d '\n')

jq -n \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg model "$MODEL" --argjson cores "$CORES" \
  --arg xlq "$XLQ_VER" --arg opxl "openpyxl $OPXL_VER" --arg lo "$LO_VER" \
  --slurpfile a "$WORK/a.json" \
  --slurpfile b "$WORK/b.jsonl" \
  --slurpfile c "$WORK/c.jsonl" \
  --slurpfile d "$WORK/d.jsonl" \
  --slurpfile d2 "$WORK/d2.jsonl" '{
  generated_utc: $date,
  machine: {cpu_model: $model, logical_cores: $cores, os: "Linux"},
  tools: {xlq: $xlq, engine: "ironcalc 0.7.1", openpyxl: $opxl, libreoffice: $lo},
  methodology: {
    timing: "median of 3 warm runs (1 untimed warmup first), wall clock",
    preservation_basis: "zip central directory: part names, uncompressed sizes, and content CRC32s; zip directory placeholder entries excluded",
    fixture_provenance: "All fixtures were generated by ironcalc itself (xlq-fixtures bin), so section D is a BEST CASE for the ironcalc roundtrip; section D2 feeds it openpyxl- and LibreOffice-authored files instead.",
    fixture_scope: "Fixtures contain no VBA, charts, pivot tables, comments, or external links (ironcalc cannot author them); preservation of those parts is UNTESTED here. See openpyxl issue #22044 (anthropics/claude-code) for openpyxl behavior on such parts.",
    loads_in_ironcalc: "proxy for not-corrupt: output parses via ironcalc load_from_xlsx (xlq inspect exit status)"
  },
  A_load_calc: $a[0],
  B_inspect: $b,
  C_token_efficiency: $c,
  D_preservation: $d,
  D2_foreign_file_roundtrip: $d2
}' > "$RESULTS"

echo "== wrote $RESULTS ==" >&2
