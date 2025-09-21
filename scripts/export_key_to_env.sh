#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/export_key_to_env.sh --key /path/to/keypair.json --env /path/to/.env --name SOL_PRIVATE_B58

KEYPATH="$HOME/.config/solana/zoopx-devnet.json"
ENVFILE="$HOME/.config/solana/.env"
VARNAME="SOL_PRIVATE_B58"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key) KEYPATH="$2"; shift 2;;
    --env) ENVFILE="$2"; shift 2;;
    --name) VARNAME="$2"; shift 2;;
    -h|--help) echo "Usage: $0 [--key keypath] [--env envfile] [--name varname]"; exit 0;;
    *) echo "Unknown arg: $1"; exit 1;;
  esac
done

echo "This script will append an encoded private key to $ENVFILE as $VARNAME. It will NOT print the secret. Run only on your machine." >&2

if [ ! -f "$KEYPATH" ]; then
  echo "Keypair file not found: $KEYPATH" >&2
  exit 1
fi

if ! python3 -c 'import sys, json; sys.exit(0)' 2>/dev/null; then
  echo "Python3 is required" >&2
  exit 1
fi

if ! python3 -c 'import importlib, sys
try:
  importlib.import_module("base58")
except Exception:
  print("Please install python base58: pip install base58", file=sys.stderr); sys.exit(2)'
then
  exit 2
fi

TMP=$(mktemp)
chmod 600 "$TMP"

python3 - <<PY > "$TMP"
import json
from pathlib import Path
import base58
kp=Path("$KEYPATH")
arr=json.loads(kp.read_text())
b=bytes(arr)
print(base58.b58encode(b).decode())
PY

ENC=$(cat "$TMP")
rm -f "$TMP"

mkdir -p "$(dirname "$ENVFILE")"
touch "$ENVFILE"
chmod 600 "$ENVFILE"

# remove existing var if present
grep -v "^${VARNAME}=" "$ENVFILE" > "$ENVFILE.tmp" || true
printf '%s="%s"\n' "$VARNAME" "$ENC" >> "$ENVFILE.tmp"
mv "$ENVFILE.tmp" "$ENVFILE"
chmod 600 "$ENVFILE"

echo "Appended $VARNAME to $ENVFILE (file secured with 600 perms)." >&2

exit 0
#!/usr/bin/env bash
set -euo pipefail

# scripts/export_key_to_env.sh
# User-run script: reads your Solana JSON keypair, encodes it, and appends a single-line secret entry to a .env file.
# IMPORTANT: This script does not print the private key. Run it locally only.

# Usage:
#   ./scripts/export_key_to_env.sh --keypath /path/to/key.json --envfile /path/to/.env [--b58|--json]
# Example:
#   ./scripts/export_key_to_env.sh --keypath ~/.config/solana/zoopx-devnet.json --envfile ~/.config/solana/.env --b58

show_help() {
  sed -n '1,200p' "$0" | sed -n '1,80p'
}

# defaults
KEYPATH="${HOME}/.config/solana/zoopx-devnet.json"
ENVFILE="${HOME}/.config/solana/.env"
MODE="b58"  # b58 or json

# parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --keypath) KEYPATH="$2"; shift 2;;
    --envfile) ENVFILE="$2"; shift 2;;
    --b58) MODE="b58"; shift;;
    --json) MODE="json"; shift;;
    -h|--help) show_help; exit 0;;
    *) echo "Unknown arg: $1" >&2; show_help; exit 1;;
  esac
done

# validations
if [[ ! -f "$KEYPATH" ]]; then
  echo "Keypair file not found: $KEYPATH" >&2
  exit 2
fi

# ensure env dir exists
mkdir -p "$(dirname "$ENVFILE")"

# create env file if missing
if [[ ! -f "$ENVFILE" ]]; then
  touch "$ENVFILE"
  chmod 600 "$ENVFILE"
fi

# ensure we won't leak secret to stdout accidentally
export GIT_ASKPASS=echo

# append secret safely based on mode
if [[ "$MODE" == "json" ]]; then
  # compact JSON array and append as SOL_PRIVATE_JSON
  tmpfile=$(mktemp)
  jq -c . "$KEYPATH" > "$tmpfile"
  # escape double quotes so value is quoted safely
  value=$(sed 's/"/\\"/g' "$tmpfile")
  # remove any previous lines
  sed -i '/^SOL_PRIVATE_JSON=/d' "$ENVFILE" || true
  printf 'SOL_PRIVATE_JSON="%s"\n' "$value" >> "$ENVFILE"
  shred -u "$tmpfile" || rm -f "$tmpfile"
  echo "Appended SOL_PRIVATE_JSON to $ENVFILE"
elif [[ "$MODE" == "b58" ]]; then
  # base58 encode the full keypair bytes and append as SOL_PRIVATE_B58
  # use python3 to avoid external binary deps
  python3 - <<PY > /dev/null
import json, sys, os
from pathlib import Path
p = Path(os.path.expanduser("$KEYPATH"))
arr = json.loads(p.read_text())
try:
    import base58
except Exception:
    print('Missing python dependency: pip install base58', file=sys.stderr)
    sys.exit(10)
b = bytes(arr)
s = base58.b58encode(b).decode()
# write to stdout in a safe way
sys.stdout.write(s)
PY
  # capture output without printing to terminal
  s=$(python3 - <<PY
import json, sys
from pathlib import Path
p = Path("$KEYPATH")
arr = json.loads(p.read_text())
import base58
b = bytes(arr)
print(base58.b58encode(b).decode())
PY
)
  # remove any previous lines
  sed -i '/^SOL_PRIVATE_B58=/d' "$ENVFILE" || true
  printf 'SOL_PRIVATE_B58="%s"\n' "$s" >> "$ENVFILE"
  # clear variable from shell
  unset s
  echo "Appended SOL_PRIVATE_B58 to $ENVFILE"
else
  echo "Unknown mode: $MODE" >&2
  exit 3
fi

# secure permissions
chmod 600 "$ENVFILE"

# final note (no secrets printed)
echo "Done. Do not commit $ENVFILE to git. Consider encrypting it with gpg for added safety."