#!/usr/bin/env bash
set -euo pipefail

# Create a simple Solana multisig using the spl-token multisig or a small program like
# https://github.com/solana-labs/solana-program-library/tree/master/token/js/example/multisig
# This script assumes solana-cli is installed and configured to the devnet.

if [ -z "${MULTISIG_SIGNERS:-}" ]; then
  echo "Please set MULTISIG_SIGNERS to a comma-separated list of keypair paths"
  exit 1
fi

IFS="," read -ra KEYS <<< "$MULTISIG_SIGNERS"
THRESHOLD=${MULTISIG_THRESHOLD:-2}

echo "Creating multisig with threshold $THRESHOLD and signers: ${KEYS[*]}"

# Example: using spl-token's create-multisig (if installed via spl-token-cli). Fallback: create custom account.
spl-token create-multisig $THRESHOLD ${KEYS[@]}
