#!/usr/bin/env bash
set -euo pipefail

# upgrade-program-multisig-devnet.sh
# Simulate an upgrade where the upgrade authority is a key held under "multisig custody".
# For devnet testing only: this script uses an authority keypair file to perform the upgrade.

usage() {
  cat <<EOF
Usage: $0 --program-id <PROGRAM_ID> --so-path <SO_PATH> --authority <KEYPAIR> [--cluster devnet|testnet|mainnet-beta] [--dry-run]

This script will:
  - build the .so (optional, if not present)
  - create an upgrade buffer
  - write the program buffer and invoke the upgrade using the provided authority keypair

Security: For devnet testing only. Never store real production authority keys in the repo.
EOF
}

PROGRAM_ID=""
SO_PATH=""
AUTH_KEY=""
CLUSTER=devnet
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --program-id) PROGRAM_ID="$2"; shift 2;;
    --so-path) SO_PATH="$2"; shift 2;;
    --authority) AUTH_KEY="$2"; shift 2;;
    --cluster) CLUSTER="$2"; shift 2;;
    --dry-run) DRY_RUN=1; shift 1;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

if [[ -z "$PROGRAM_ID" || -z "$SO_PATH" || -z "$AUTH_KEY" ]]; then
  echo "Missing required args"
  usage
  exit 1
fi

solana config set --url "$CLUSTER"

if [[ ! -f "$SO_PATH" ]]; then
  echo "SO file not found at $SO_PATH. Attempting to build release binary..."
  # Try to build the program (assumes a workspace Makefile or cargo build script exists)
  # We won't assume specific crate names; the repo's build should produce the .so at the given path.
  cargo build-bpf --manifest-path programs/zpx_router/Cargo.toml --release || true
  if [[ ! -f "$SO_PATH" ]]; then
    echo "Failed to build .so; please build the program and retry."
    exit 1
  fi
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo "DRY RUN: solana program write-buffer --program $PROGRAM_ID --so $SO_PATH --keypair $AUTH_KEY"
  echo "DRY RUN: solana program upgrade $PROGRAM_ID --buffer <buffer-address> --keypair $AUTH_KEY"
  exit 0
fi

# Create buffer and write program
BUFFER_ADDRESS=$(solana program write-buffer "$SO_PATH" --keypair "$AUTH_KEY" --output json 2>/dev/null | jq -r '.buffer' || true)

if [[ -z "$BUFFER_ADDRESS" || "$BUFFER_ADDRESS" == "null" ]]; then
  echo "Failed to write buffer via solana program write-buffer. Falling back to solana program deploy --upgrade-authority"
  solana program deploy "$SO_PATH" --program-id "$PROGRAM_ID" --keypair "$AUTH_KEY"
  echo "Deployed program via fallback deploy command. Verify program id and state." 
  exit 0
fi

echo "Wrote program buffer: $BUFFER_ADDRESS"

echo "Upgrading program $PROGRAM_ID using authority $AUTH_KEY"
solana program upgrade "$PROGRAM_ID" "$BUFFER_ADDRESS" --keypair "$AUTH_KEY"

echo "Upgrade complete. Verify program accounts and run post-upgrade tests."
