#!/usr/bin/env bash
# Differential oracle: ironcalc vs LibreOffice.
# Chains: workbook generation -> LibreOffice conversion -> oracle-compare
# -> benchmarks/agreement.json. Rerunnable; work happens in $ORACLE_WORKDIR.
#
# Env overrides:
#   ORACLE_PYTHON   python with openpyxl   (default: python3)
#   ORACLE_SOFFICE  LibreOffice binary     (default: /usr/bin/soffice)
#   ORACLE_WORKDIR  scratch directory      (default: mktemp -d)
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$BENCH_DIR")"
PYTHON="${ORACLE_PYTHON:-python3}"
SOFFICE="${ORACLE_SOFFICE:-/usr/bin/soffice}"
WORKDIR="${ORACLE_WORKDIR:-$(mktemp -d)}"
CASES="$BENCH_DIR/oracle-cases.json"
OUT="$BENCH_DIR/agreement.json"

mkdir -p "$WORKDIR"
echo "[1/4] generating oracle workbook in $WORKDIR" >&2
"$PYTHON" "$BENCH_DIR/gen_oracle_workbook.py" "$CASES" "$WORKDIR" >&2

echo "[2/4] LibreOffice convert (computes all formulas)" >&2
rm -f "$WORKDIR/lo/oracle.xlsx"
# The case table contains locale-sensitive formulas (TEXT format codes,
# DATEVALUE, DOLLAR, FIXED, VALUE, NUMBERVALUE) whose LibreOffice results
# depend on the UI/format locale. Pin the locale (C.UTF-8 resolves to en-US
# inside LibreOffice) and use a throwaway profile so the run is reproducible
# regardless of the host machine's locale or user profile.
timeout 300 env LC_ALL=C.UTF-8 LANG=C.UTF-8 LANGUAGE= \
    "$SOFFICE" -env:UserInstallation="file://$WORKDIR/lo-profile" \
    --headless --convert-to xlsx --outdir "$WORKDIR/lo" \
    "$WORKDIR/oracle.xlsx" >&2
test -f "$WORKDIR/lo/oracle.xlsx" || { echo "conversion failed" >&2; exit 1; }

echo "[3/4] building oracle-compare" >&2
cargo build --quiet --release --manifest-path "$REPO_DIR/xlq/Cargo.toml" \
    --bin oracle-compare >&2

echo "[4/4] comparing -> $OUT" >&2
"$REPO_DIR/xlq/target/release/oracle-compare" "$CASES" "$WORKDIR/lo/oracle.xlsx" > "$OUT"

"$PYTHON" - "$OUT" <<'EOF' >&2
import json, sys
print("totals:", json.load(open(sys.argv[1]))["totals"])
EOF
