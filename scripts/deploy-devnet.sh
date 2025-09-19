#!/usr/bin/env bash
set -euo pipefail

# Builds the program and deploys to devnet. Requires solana CLI and anchor.

NETWORK=${NETWORK:-devnet}
KEYPAIR=${KEYPAIR:-~/.config/solana/id.json}

echo "Building program..."
anchor build

echo "Deploying to $NETWORK"
solana config set --url https://api.$NETWORK.solana.com
solana program deploy target/deploy/zpx_router.so --keypair $KEYPAIR
