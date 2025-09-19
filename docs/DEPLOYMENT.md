Deployment checklist for mainnet

1. Pin the Rust toolchain
   - Ensure `rust-toolchain.toml` is present and CI uses it.

2. Replace placeholder program id
   - Update `programs/zpx_router/src/lib.rs` `declare_id!("...")` with the real program id.
   - Update `Anchor.toml` program id and IDL outputs.

3. Release profile
   - Confirm `/Cargo.toml` `[profile.release]` has `opt-level`, `lto`, `codegen-units`, `panic=\"abort\"`, and `strip = \"symbols\"`.

4. Tests and tooling
   - Run unit tests locally: `cargo test -p zpx_router`.
   - Run program-tests in CI with feature `program-test` enabled on a runner with matching toolchain.
   - Generate golden vectors and verify event parity.

5. Security hardening
   - Verify `finalize_message_v1` creates rent-exempt replay PDA and owner is the program.
   - Verify strict ATA derivation and token program checks.
   - Verify chain-id width guards.
   - Verify admin authority checks on all mutating methods.

6. CI
   - Add job to run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`.
   - Add job to run program-tests with feature `program-test` on dedicated runner.

7. Release process
   - Build artifacts with the pinned toolchain and upload them to your deployment pipeline.
   - Re-run a smoke test on devnet before mainnet anchor deploy.

8. Monitoring
   - Ensure alerts are configured for failed finalize attempts or runtime errors.
