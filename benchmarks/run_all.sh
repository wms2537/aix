#!/usr/bin/env bash
# AXLE-bench meta-runner: chains the whole suite.
#
#   axis 5 Catalog       coverage-probe        -> benchmarks/coverage.json
#   axes 2-4 Fidelity/Efficiency/Ergonomics
#                        run_bench.sh          -> benchmarks/results.json
#   axis 1 Correctness   run_oracle.sh         -> benchmarks/agreement.json
#
# Tolerant of missing prerequisites: a suite whose requirements (cargo,
# python3+openpyxl, soffice) are absent is SKIPPED with a notice, never a
# hard failure. Existing artifacts are only replaced by a successful run.
#
# Env overrides: PY (python with openpyxl), SOFFICE (LibreOffice binary).
set -u

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$BENCH_DIR")"
PY="${PY:-$(command -v python3 || true)}"
SOFFICE="${SOFFICE:-$(command -v soffice || true)}"

have_cargo=false
command -v cargo >/dev/null 2>&1 && have_cargo=true
have_py=false
[[ -n "$PY" ]] && "$PY" -c "import openpyxl" >/dev/null 2>&1 && have_py=true
have_soffice=false
[[ -n "$SOFFICE" && -x "$SOFFICE" ]] && have_soffice=true

ran=()
skipped=()
failed=()
note() { echo "== AXLE-bench: $*" >&2; }

# ---- axis 5: Catalog (needs cargo only) -----------------------------------
if $have_cargo; then
  note "[catalog] coverage-probe -> coverage.json"
  tmp="$BENCH_DIR/coverage.json.tmp"
  if (cd "$ROOT/xlq" && cargo run --quiet --release --bin coverage-probe -- \
        "$BENCH_DIR/excel-functions.txt") > "$tmp"; then
    mv "$tmp" "$BENCH_DIR/coverage.json"
    ran+=("catalog -> coverage.json")
  else
    rm -f "$tmp"
    failed+=("catalog (coverage-probe)")
  fi
else
  note "SKIP [catalog]: cargo not found"
  skipped+=("catalog: cargo not found")
fi

# ---- axes 2-4: Fidelity / Efficiency / Agent-ergonomics -------------------
missing=()
$have_cargo   || missing+=("cargo")
$have_py      || missing+=("python3+openpyxl")
$have_soffice || missing+=("soffice")
if [[ ${#missing[@]} -eq 0 ]]; then
  note "[fidelity/efficiency/ergonomics] run_bench.sh -> results.json"
  if PY="$PY" SOFFICE="$SOFFICE" bash "$BENCH_DIR/run_bench.sh"; then
    ran+=("fidelity/efficiency/ergonomics -> results.json")
  else
    failed+=("run_bench.sh")
  fi
else
  note "SKIP [fidelity/efficiency/ergonomics]: missing ${missing[*]}"
  skipped+=("fidelity/efficiency/ergonomics: missing ${missing[*]}")
fi

# ---- axis 1: Correctness --------------------------------------------------
if [[ ${#missing[@]} -eq 0 ]]; then
  note "[correctness] run_oracle.sh -> agreement.json"
  if ORACLE_PYTHON="$PY" ORACLE_SOFFICE="$SOFFICE" bash "$BENCH_DIR/run_oracle.sh"; then
    ran+=("correctness -> agreement.json")
  else
    failed+=("run_oracle.sh")
  fi
else
  note "SKIP [correctness]: missing ${missing[*]}"
  skipped+=("correctness: missing ${missing[*]}")
fi

# ---- summary ---------------------------------------------------------------
echo >&2
note "summary"
for r in "${ran[@]-}";     do [[ -n "$r" ]] && echo "   ran:     $r" >&2; done
for s in "${skipped[@]-}"; do [[ -n "$s" ]] && echo "   skipped: $s" >&2; done
for f in "${failed[@]-}";  do [[ -n "$f" ]] && echo "   FAILED:  $f" >&2; done

[[ ${#failed[@]} -eq 0 ]] || exit 1
exit 0
