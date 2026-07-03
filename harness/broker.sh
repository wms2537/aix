#!/bin/sh
# The broker runs OUTSIDE the confinement (it is the harness owner, not the
# agent). It promotes a workbook from the agent's writable work/ dir to the
# read-only authoritative store ONLY if the change is a valid, receipted xlq
# operation: the work file must hash to the last receipt's result_hash, and
# that receipt must chain from the authoritative file's current hash. This is
# the last link that makes "the authoritative file only ever changes through a
# receipted xlq operation" a real guarantee, not a request.
set -e
XLQ="$1"; WORK="$2"; AUTH="$3"; NAME="$4"
work_file="$WORK/$NAME"
journal="$work_file.xlq.jsonl"
auth_file="$AUTH/$NAME"

[ -f "$journal" ] || { echo "REJECT: no receipt journal — the work file was not produced by xlq"; exit 1; }
# last receipt
last=$(tail -n 1 "$journal")
want=$(printf '%s' "$last" | sed -n 's/.*"result_hash":"\([0-9a-f]*\)".*/\1/p')
base=$(printf '%s' "$last" | sed -n 's/.*"base_hash":"\([0-9a-f]*\)".*/\1/p')
# actual hash of the work file (via xlq inspect — dogfood)
got=$("$XLQ" inspect "$work_file" | sed -n 's/.*"sha256": *"\([0-9a-f]*\)".*/\1/p')
authhash=$("$XLQ" inspect "$auth_file" | sed -n 's/.*"sha256": *"\([0-9a-f]*\)".*/\1/p')

[ "$got" = "$want" ] || { echo "REJECT: work file hash ($got) != last receipt result_hash ($want) — content not receipt-backed"; exit 1; }
[ "$base" = "$authhash" ] || { echo "REJECT: receipt base_hash ($base) != authoritative hash ($authhash) — not a clean successor"; exit 1; }

# Promote: broker has write access to authoritative; atomic replace.
tmp="$auth_file.promote.$$"
chmod u+w "$AUTH"
cp "$work_file" "$tmp"
chmod 444 "$tmp"
mv -f "$tmp" "$auth_file"
chmod 555 "$AUTH"
echo "PROMOTED: authoritative $NAME -> $want (receipt-verified, chained from $base)"
