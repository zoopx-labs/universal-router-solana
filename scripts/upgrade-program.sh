#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/upgrade-program.sh <PROGRAM_KEYPAIR_OR_PUBKEY> <PROGRAM_SO_PATH> [--cluster devnet|mainnet-beta|localnet]

PROGRAM_ID=${1:-}
PROGRAM_SO=${2:-}
CLUSTER=${3:-devnet}

if [[ -z "$PROGRAM_ID" || -z "$PROGRAM_SO" ]]; then
  echo "Usage: $0 <PROGRAM_KEYPAIR_OR_PUBKEY> <PROGRAM_SO_PATH> [cluster]"
  exit 1
fi

SOLANA_ARGS=(--url "https://${CLUSTER}.solana.com")

echo "Uploading program buffer for $PROGRAM_ID from $PROGRAM_SO"
BUFFER=$(solana program write-buffer $PROGRAM_SO "${SOLANA_ARGS[@]}" --output json | jq -r '.programData')
echo "Buffer uploaded: $BUFFER"

echo "Upgrading program $PROGRAM_ID using the current upgrade authority (you will be prompted to sign)."
solana program upgrade --program-id $PROGRAM_ID $PROGRAM_SO "${SOLANA_ARGS[@]}"

echo "Upgrade complete. Program info:"
solana program show $PROGRAM_ID "${SOLANA_ARGS[@]}"
