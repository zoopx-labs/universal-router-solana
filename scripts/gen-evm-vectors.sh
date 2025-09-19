#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cd delete-later/evm-router
# Requires Foundry (forge)
forge script script/HashVectors.t.sol --silent | tail -n 1 > ../../programs/zpx_router/tests/golden/hashes.evm.json
