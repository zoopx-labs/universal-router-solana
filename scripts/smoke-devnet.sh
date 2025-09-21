#!/usr/bin/env bash
set -euo pipefail

# scripts/smoke-devnet.sh
# Minimal, safe Devnet smoke script for universal-router-solana.
# - By default this runs in dry-run mode and will not change chain state.
# - Requires Solana CLI, Anchor CLI, jq, and spl-token CLI installed.
# - Do NOT hardcode private keys: pass the path as --keypair.

PROG="zpx_router"
KEYPAIR=""
PROGRAM_ID=""
DRY_RUN=true
RPC_URL="https://api.devnet.solana.com"

usage(){
  cat <<USAGE
Usage: $0 [--keypair /path/to/key.json] [--program-id <PROGRAM_ID>] [--rpc <rpc-url>] [--no-dry-run]

Options:
  --keypair PATH    Path to local JSON keypair for deployer (kept locally, not committed)
  --program-id ID   If you already deployed and want to skip deploy step
  --rpc URL         RPC URL (default: $RPC_URL)
  --no-dry-run      Actually perform network actions (use only when ready)
  -h|--help         Show this help

Notes:
 - Script is intentionaly conservative: default is dry-run. To run live, pass --no-dry-run and provide a valid --keypair.
 - This script updates local files and prints guidance but will NOT modify repository files.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --keypair)
      KEYPAIR="$2"; shift 2;;
    --program-id)
      PROGRAM_ID="$2"; shift 2;;
    --rpc)
      RPC_URL="$2"; shift 2;;
    --no-dry-run)
      DRY_RUN=false; shift;;
    -h|--help)
      usage; exit 0;;
    *)
      echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

if [[ -z "$KEYPAIR" && -z "$PROGRAM_ID" ]]; then
  echo "Either --keypair or --program-id must be supplied."
  usage
  exit 1
fi

echo "RPC: $RPC_URL"
if [[ -n "$KEYPAIR" ]]; then
  echo "Using keypair: $KEYPAIR"
fi
if $DRY_RUN; then
  echo "DRY RUN mode: no changes will be made. Use --no-dry-run to execute actions."
fi

# 1) Build
echo "\n[1/6] Building with anchor..."
anchor build

# 2) Deploy (dry-run unless program id provided)
if [[ -z "$PROGRAM_ID" ]]; then
  echo "\n[2/6] Deploying program (dry-run check)..."
  if $DRY_RUN; then
    echo "Dry-run: printing the deploy command to run manually if desired:" 
    echo "  solana program deploy target/deploy/${PROG}.so --keypair ${KEYPAIR} --url ${RPC_URL}"
  else
    if [[ -z "$KEYPAIR" ]]; then
      echo "Error: --keypair is required when not in dry-run mode."; exit 1
    fi
    solana program deploy target/deploy/${PROG}.so --keypair ${KEYPAIR} --url ${RPC_URL}
    # capture printed program id (user still must set Anchor.toml manually if needed)
  fi
else
  echo "Program id provided: $PROGRAM_ID; skipping deploy step."
fi

# 3) Create a USDC-like mint on devnet for the test (dry-run prints commands)
TMP_MINT_KEY=./tmp_devnet_mint.json
MINT_PUB=""
if $DRY_RUN; then
  echo "\n[3/6] Dry-run creating mint: commands to run manually:" 
  echo "  spl-token create-token --url ${RPC_URL} --fee-payer ${KEYPAIR}"
  echo "  spl-token create-account <MINT_ADDRESS> --url ${RPC_URL} --owner <OWNER_PUBKEY>"
else
  echo "\n[3/6] Creating test mint..."
  # create token requires keypair; guard
  if [[ -z "$KEYPAIR" ]]; then echo "Error: keypair required"; exit 1; fi
  MINT_PUB=$(spl-token create-token --url ${RPC_URL} --fee-payer ${KEYPAIR} | grep -oE "[A-Za-z0-9]{32,}" | head -n1)
  echo "Created mint: $MINT_PUB"
fi

# 4) Create or find an associated token account for the payer (dry-run prints commands)
if $DRY_RUN; then
  echo "\n[4/6] Dry-run create associated token account commands:" 
  echo "  spl-token create-account <MINT_ADDRESS> --url ${RPC_URL} --owner <OWNER_PUBKEY>"
else
  echo "\n[4/6] Creating token account (owner = deployer)"
  OWNER_PUB=$(solana-keygen pubkey ${KEYPAIR})
  ATA=$(spl-token create-account $MINT_PUB --url ${RPC_URL} --owner ${OWNER_PUB} | grep -oE "[A-Za-z0-9]{32,}" | head -n1)
  echo "Created associated token account: $ATA"
fi

# 5) Run a minimal router register + forward flow using solana CLI or a small rust/js client.
# For now we print instructions since the repo contains integration tests that exercise this flow.

echo "\n[5/6] Minimal smoke actions (manual or use integration tests):"
if $DRY_RUN; then
  echo " - Use the integration tests in programs/zpx_router/tests to exercise register+forward flows locally."
  echo " - To run a live devnet flow, you'll need to craft transactions that call the deployed program's InitializeConfig, InitializeRegistry, CreateSpoke and ForwardViaSpoke instructions using the program id from deploy step."
else
  echo "Running a minimal on-chain test is environment and program-id specific; please follow the README or provide a program-id to proceed."
fi

# 6) Cleanup
echo "\n[6/6] Cleanup guidance"
if $DRY_RUN; then
  echo "No temp artifacts were created in dry-run mode."
else
  echo "Temporary mint created at: $MINT_PUB (you may want to burn/close it after tests)."
fi

echo "\nSmoke script finished (dry-run=$DRY_RUN). Review output and re-run with --no-dry-run + --keypair when ready to actually execute."

exit 0
