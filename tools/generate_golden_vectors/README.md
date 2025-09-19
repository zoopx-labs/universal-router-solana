Generate golden vectors from EVM reference

This directory should contain the script that runs the Solidity-based generator
and produces `programs/zpx_router/tests/golden/hashes.json` so both implementations
share the same source of truth.

Place the generator script here or add a simple wrapper that calls the EVM repo tool.
