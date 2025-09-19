#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --program-id <PROGRAM_ID> --current-authority <KEYPAIR> --new-authority <KEYPAIR> [--cluster devnet|mainnet-beta|testnet] [--dry-run]

Sets the upgrade authority for a deployed Solana program.

Notes:
- This script is intended for devnet testing and operator convenience.
- Do NOT store real private keys in the repository.
EOF
}

PROGRAM_ID=""
CURRENT_AUTH=""
NEW_AUTH=""
CLUSTER=devnet
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --program-id) PROGRAM_ID="$2"; shift 2;;
    --current-authority) CURRENT_AUTH="$2"; shift 2;;
    --new-authority) NEW_AUTH="$2"; shift 2;;
    --cluster) CLUSTER="$2"; shift 2;;
    --dry-run) DRY_RUN=1; shift 1;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

if [[ -z "$PROGRAM_ID" || -z "$CURRENT_AUTH" || -z "$NEW_AUTH" ]]; then
  echo "Missing required args"
  usage
  exit 1
fi

echo "Setting solana config to cluster: $CLUSTER"
solana config set --url "$CLUSTER"

echo "Current program id: $PROGRAM_ID"

if [[ $DRY_RUN -eq 1 ]]; then
  echo "DRY RUN: solana program set-upgrade-authority $PROGRAM_ID --new-upgrade-authority $NEW_AUTH --keypair $CURRENT_AUTH"
  exit 0
fi

# Use solana program set-upgrade-authority (CLI >=1.14). This will fail if the current authority doesn't match CURRENT_AUTH
solana program set-upgrade-authority "$PROGRAM_ID" --new-upgrade-authority "$NEW_AUTH" --keypair "$CURRENT_AUTH"

echo "Set new upgrade authority to $NEW_AUTH for program $PROGRAM_ID on $CLUSTER"
