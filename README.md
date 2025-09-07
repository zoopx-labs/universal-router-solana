# zoopx-router

A universal bridge router supporting SPL Token and Token-2022 via Anchor `token_interface`.
# zoopx-router

A universal bridge router supporting SPL Token and Token-2022 via Anchor `token_interface`.

## Program IDs
- Devnet Program ID: `654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf`
- Source `declare_id!`: matches the above.

See `deployment.md` for full deployment metadata (ProgramData, IDL account, authority) and step-by-step commands.
## Wallet and CLI setup (devnet)
```bash
solana config set --url https://api.devnet.solana.com --keypair ~/.config/solana/zoopx-devnet.json
solana address && solana balance
```
## Build
```bash
anchor build
```
## Test
```bash
# unit + integration tests
cargo test -p zoopx-router

# clippy (deny warnings)
cargo clippy -p zoopx-router --tests --lib -- -D warnings
```
## Deploy to devnet
```bash
anchor deploy --provider.cluster devnet --provider.wallet ~/.config/solana/zoopx-devnet.json

# Upload or update on-chain IDL
anchor idl upgrade 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf \
	--provider.cluster devnet \
	--provider.wallet ~/.config/solana/zoopx-devnet.json \
	--filepath target/idl/zoopx_router.json
```
## Verify deployment
```bash
solana program show 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf
anchor idl fetch 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf --provider.cluster devnet
```

## IDL embed and pin
- Deploy once to create the IDL account, then rebuild to embed `metadata.address` into the IDL.
- Archive copies of `target/idl/zoopx_router.json` into `idl/` named `zoopx_router.<PROGRAM_ID>.<YYYYMMDD>.json`.
- Compute and record SHA-256 for each archived IDL; clients should verify this hash on startup.

## Testing
- Rust unit tests cover fee math, adapter list helpers, and inclusion checks.
- Rust integration tests use `solana-program-test`; adapter CPI can be skipped with feature `skip-adapter-invoke`.
- CI runs fmt, clippy -D warnings, and tests; enable integration tests by running with default features.

- Observability
- Events: `BridgeInitiated`, `AdapterAdded`, `AdapterRemoved`, `ConfigUpdated`.
- Logs: breadcrumbs on invalid token program, cap violations, and allowlist misses.
## Toolchain
- Anchor CLI: 0.31.1; Solana CLI (Anza) v2.x.
- Rust toolchain pinned via `rust-toolchain.toml`.

## Runbooks
See `deployment.md` for deploy/upgrade and `RUNBOOKS.md` for operations (keys, rollback, incident response).

## Toolchain
- Rust toolchain pinned via `rust-toolchain.toml`.
- Anchor/Solana recommended versions: Anchor 0.31.x, Solana CLI (Anza) >= 2.0.

## Devnet Dry Run
- Deploy to devnet and interact with one or more real adapters.
- Measure compute units and add a compute budget instruction if needed.

## Runbooks
See `RUNBOOKS.md` for deploy/upgrade/rollback, key rotation, incident response, and IDL verification.
