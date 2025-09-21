Phase 2 — Adapter integration and testing

This document describes the Phase‑2 testing and verification steps for the hub-and-spoke router and adapter integration.

Key goals
- Verify positive CPI flow: router -> adapter when adapter is registered in `Config.adapters` or assigned to a spoke.
- Ensure adapter replay guard works (replay PDA) under CPI invocation.
- Restrict adapter behavior to intended token(s) (planned: USDC-only) — implemented after positive CPI validation.

Testing
- Unit tests: per-crate unit tests are in `programs/zpx_adapter` and `programs/zpx_router/tests`.
- Run router-specific tests: from repo root
  - `cargo test -p zpx_router --tests`

Design notes
- We intentionally avoid Keccak parity tests. If deterministic external vectors are needed, prefer SHA-256 and Borsh/JSON serialization to avoid keccak parity.

Next steps
- Finalize USDC-only restriction and CCTP spoke shapes.
- Add CI to run both router and adapter test suites on PRs.

---

Devnet smoke deploy (safe, manual)

This project intentionally keeps private key material out of the repository. Below is a low-risk manual checklist for deploying to Devnet and running a small smoke test.

Prerequisites
- Solana CLI and Anchor CLI installed and configured.
- A JSON keypair file for the deployer (not used as any `declare_id!` in the repository). Example path: `secure-keys/devnet-deployer.json` (this path is in `.gitignore`).

Quick steps
1) Build:
  - `anchor build`
2) Set Solana config for devnet:
  - `solana config set --url https://api.devnet.solana.com`
3) Deploy:
  - `solana program deploy target/deploy/zpx_router.so --keypair /path/to/secure-keys/devnet-deployer.json`
    - Example using the provided secure key: `solana program deploy target/deploy/zpx_router.so --keypair secure-keys/ZPXhajNNajSB4AYQMBqZajugyhDVqJPgD9CaRczBQMZ.json`
  - Note the program id printed by the deploy command and set it in `Anchor.toml` under `[programs.devnet]`.
4) (Optional) Set upgrade authority (dry-run first):
  - `scripts/set-upgrade-authority.sh --program-id <PROGRAM_ID> --new-authority /path/to/secure-keys/devnet-deployer.json --network devnet --dry-run`
  - Re-run without `--dry-run` to actually change the authority.
5) Run a minimal smoke: create a USDC-like mint and a token account, then call a register + forward flow against the deployed program.

Notes
- Do NOT use any keypair that appears in `declare_id!` macros in the source as the deploy private key.
- If you want me to update `Anchor.toml` devnet entry with a specific program id or to produce an automated smoke script that runs against devnet, provide the deploy keypair path (kept locally) or the program id and I'll update docs only.
 - The recommended deploy keypair (checked against source) is `secure-keys/ZPXhajNNajSB4AYQMBqZajugyhDVqJPgD9CaRczBQMZ.json` whose pubkey is `ZPXhajNNajSB4AYQMBqZajugyhDVqJPgD9CaRczBQMZ`.
