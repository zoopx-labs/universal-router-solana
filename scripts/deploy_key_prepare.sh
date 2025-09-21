#!/usr/bin/env bash
set -euo pipefail

mkdir -p "$HOME/.config/solana"
enc="${DEPLOYER_PK_ENCODING:-json}"

if [ -z "${DEPLOYER_PK:-}" ]; then
  echo "DEPLOYER_PK is empty" >&2
  exit 1
fi

case "$enc" in
  json)
    printf '%s' "$DEPLOYER_PK" > "$HOME/.config/solana/deployer.json"
    ;;
  base58)
    python3 - <<'PY' > "$HOME/.config/solana/deployer.json"
import sys, json
import base58
data = sys.stdin.read().strip()
b = base58.b58decode(data)
print(list(b))
PY
    ;;
  hex)
    python3 - <<'PY' > "$HOME/.config/solana/deployer.json"
import sys, json
data = sys.stdin.read().strip()
b = bytes.fromhex(data)
print(list(b))
PY
    ;;
  gpg)
    if ! command -v gpg >/dev/null 2>&1; then
      echo "gpg is required for gpg encoding" >&2
      exit 1
    fi
    if [ -z "${GPG_PASSPHRASE:-}" ]; then
      echo "GPG_PASSPHRASE is required for gpg encoding" >&2
      exit 1
    fi
    tmp=$(mktemp)
    printf '%s' "$DEPLOYER_PK" | base64 -d > "$tmp"
    gpg --batch --yes --passphrase "$GPG_PASSPHRASE" --output - --decrypt "$tmp" | python3 - <<'PY' > "$HOME/.config/solana/deployer.json"
import sys, json
data = sys.stdin.buffer.read()
try:
    print(json.loads(data))
except Exception:
    import base58
    b = base58.b58decode(data.strip())
    print(list(b))
PY
    rm -f "$tmp"
    ;;
  *)
    echo "Unsupported DEPLOYER_PK_ENCODING: $enc" >&2
    exit 1
    ;;
esac

chmod 600 "$HOME/.config/solana/deployer.json"
echo "Wrote deploy key to $HOME/.config/solana/deployer.json" >&2
