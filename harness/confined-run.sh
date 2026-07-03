#!/bin/sh
# Run a command in a confined environment where the ONLY executable available
# is xlq. No python, no openpyxl, no shell utilities beyond the POSIX builtins
# of /bin/sh. This makes "the agent's only xlsx-write capability is xlq" an
# enforced property of the ENVIRONMENT, not a request the agent may ignore.
CONFINED_BIN="$(cd "$(dirname "$0")/confined-bin" && pwd)"
exec env -i PATH="$CONFINED_BIN" HOME=/nonexistent /bin/sh -c "$1"
