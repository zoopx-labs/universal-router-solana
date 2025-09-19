# SPDX-License-Identifier: MIT
.PHONY: ci

ci:
	python3 -m pip install --upgrade pip
	python3 -m pip install pycryptodome pysha3
	python3 tools/generate_golden_vectors/generate.py > programs/zpx_router/tests/golden/hashes.json
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test -p zpx_router
	cargo test -p zpx_router --features program-test -- --ignored
