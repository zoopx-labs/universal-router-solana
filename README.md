# zoopx-router

A universal bridge router supporting SPL Token and Token-2022 via Anchor `token_interface`.

## Program ID and Upgrade Authority
- Program ID: `JDEioXieTWktNDEUB9ca87jRCL2x9FGFo2r3saSBsTiN` (matches `Anchor.toml` [localnet]).
- Governance: set the upgrade authority to a multisig owned by protocol governance.
- Custody: document who holds the deploy and multisig keys; rotate on schedule; store encrypted backups.

## IDL embed and pin
- Deploy once to create the IDL account, then rebuild to embed `metadata.address` into the IDL.
- Archive copies of `target/idl/zoopx_router.json` into `idl/` named `zoopx_router.<PROGRAM_ID>.<YYYYMMDD>.json`.
- Compute and record SHA-256 for each archived IDL; clients should verify this hash on startup.

## Testing
- Rust unit tests cover fee math, adapter list helpers, and inclusion checks.
- Rust integration tests use `solana-program-test`; adapter CPI can be skipped with feature `skip-adapter-invoke`.
- CI runs fmt, clippy -D warnings, and tests; enable integration tests by running with default features.

## Observability
- Events: `BridgeInitiated`, `AdminConfigInitialized`, `AdminConfigUpdated`, `AdapterAdded`, `AdapterRemoved`.
- Logs: breadcrumbs on invalid token program, cap violations, and allowlist misses.

## Toolchain
- Rust toolchain pinned via `rust-toolchain.toml`.
- Anchor/Solana recommended versions: Anchor 0.31.x, Solana CLI (Anza) >= 2.0.

## Devnet Dry Run
- Deploy to devnet and interact with one or more real adapters.
- Measure compute units and add a compute budget instruction if needed.

## Runbooks
See `RUNBOOKS.md` for deploy/upgrade/rollback, key rotation, incident response, and IDL verification.
