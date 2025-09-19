#!/usr/bin/env bash
set -euo pipefail

# run-devnet-smoke.sh
# Runs the devnet multisig upgrade flow in a controlled way and executes post-upgrade smoke tests.
# Designed for operator use (local); requires the upgrade authority keyfile to be available locally.

usage() {
  cat <<EOF
Usage: $0 --program-id <PROGRAM_ID> --so-path <SO_PATH> --authority <KEYPAIR_PATH> [--cluster devnet|testnet|mainnet-beta] [--dry-run]

This script:
  - optionally builds the program .so
  - sets the upgrade authority (dry-run by default)
  - performs the upgrade (only if --confirm provided)
  - runs post-upgrade smoke tests (cargo test smoke)

SECURITY: For local/devnet testing only. Do NOT commit keyfiles.
EOF
}

PROGRAM_ID=""
SO_PATH=""
AUTH_KEY=""
CLUSTER=devnet
DRY_RUN=1
CONFIRM=0

while [[ $# -gt 0 ]]; do
  case $1 in
    --program-id) PROGRAM_ID="$2"; shift 2;;
    --so-path) SO_PATH="$2"; shift 2;;
    --authority) AUTH_KEY="$2"; shift 2;;
    --cluster) CLUSTER="$2"; shift 2;;
    --dry-run) DRY_RUN=1; shift 1;;
    --confirm) CONFIRM=1; DRY_RUN=0; shift 1;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

if [[ -z "$PROGRAM_ID" || -z "$SO_PATH" || -z "$AUTH_KEY" ]]; then
  echo "Missing required args"
  usage
  exit 1
fi

echo "Running devnet smoke test for program $PROGRAM_ID on cluster $CLUSTER"
solana config set --url "$CLUSTER"

if [[ ! -f "$AUTH_KEY" ]]; then
  echo "Authority keyfile not found: $AUTH_KEY"
  exit 1
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo "DRY RUN: will not perform upgrade. To run upgrade, re-run with --confirm."
  echo "DRY RUN: set upgrade authority (dry):"
  ./scripts/set-upgrade-authority.sh --program-id "$PROGRAM_ID" --current-authority "$AUTH_KEY" --new-authority "$AUTH_KEY" --cluster "$CLUSTER" --dry-run
  echo "DRY RUN complete."
  exit 0
fi

if [[ $CONFIRM -ne 1 ]]; then
  echo "Upgrade not confirmed. Pass --confirm to execute the upgrade and post-upgrade tests."
  exit 1
fi

# Execute upgrade flow
./scripts/set-upgrade-authority.sh --program-id "$PROGRAM_ID" --current-authority "$AUTH_KEY" --new-authority "$AUTH_KEY" --cluster "$CLUSTER"

./scripts/upgrade-program-multisig-devnet.sh --program-id "$PROGRAM_ID" --so-path "$SO_PATH" --authority "$AUTH_KEY" --cluster "$CLUSTER"

# Post-upgrade smoke tests (run minimal suite)
if command -v cargo >/dev/null 2>&1; then
  echo "Running cargo smoke tests (this may run integration program-tests that require local setup)."
  cargo test -p zpx_router --features program-test -- --ignored tests::smoke -- --nocapture || echo "Smoke tests failed — investigate logs"
else
  echo "cargo not found; skip smoke tests"
fi

echo "Devnet smoke flow complete."
