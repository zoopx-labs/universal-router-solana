# Multisig Upgrade Authority Runbook

This runbook documents recommended approaches to managing program upgrade authority for the `zpx_router` Solana program and provides sample scripts for common operations (set upgrade authority, perform an upgrade). The goal is to ensure upgrades require multiple trusted people or a governance process.

High-level options
- Option A — Off-chain multisig wallet (recommended, simple):
  - Create a single upgrade-authority keypair whose private key is held in multi-party custody (HSMs or split with a secure signing workflow).
  - Use an offline signing ceremony where multiple custodians co-sign upgrade transactions.
  - Pros: simple to implement; aligns with existing tooling.
  - Cons: requires careful operational discipline for key custody and signing.

- Option B — On-chain governance / multisig program (advanced):
  - Use an on-chain governance or multisig program that supports approval flows and can act as upgrade authority.
  - Pros: richer policy (quorum, timelock, proposal history).
  - Cons: requires integration and additional trust in the chosen governance program.

Recommended pattern
- For most teams, start with Option A (dedicated upgrade-authority keypair, custody policy) and later migrate to an on-chain governance program once the process and stakeholders are stable.

Files added in this repo
- `scripts/set-upgrade-authority.sh` — convenience script to set the program upgrade authority.
- `scripts/upgrade-program.sh` — convenience upgrade script that uploads a program buffer and invokes the upgrade.

Devnet test plan (example)
1. Create or identify the current authority keypair (keyfile A) that currently controls the program.
2. Create a new upgrade-authority keypair (keyfile B) whose private key will be placed in multisig custody. Distribute it to custody parties according to your policy (HSMs, GPG-encrypted shards, secure storage).
3. Use `scripts/set-upgrade-authority.sh` to set the on-chain upgrade authority to B (on `devnet`).
4. Test an upgrade using `scripts/upgrade-program.sh` with B signing — verify the program works and that program data and account ownerships are preserved.
5. If needed, exercise a rollback by upgrading to a previously-saved program binary.

Operational checklist for upgrades
- Pre-upgrade:
  - Create and verify backups of the current program binary and program account info.
  - Notify stakeholders and open a change ticket.
  - Ensure the multisig custody holders have sign-off and ability to sign the transaction.

- During upgrade:
  - Use offline signing where each custodian signs the prepared transaction.
  - Upload the buffer and run `solana program upgrade` or the equivalent sequence.

- Post-upgrade:
  - Run sanity checks (basic E2E flows: initialize config, finalize message, replay check).
  - Monitor logs for unexpected errors.

Security notes
- Never store the upgrade-authority keyfile in the repository.
- Use hardware wallets or secure enclaves where possible for the custodians.
- Consider adding a timelock (delay) to upgrades via an on-chain governance program if upgrades require public coordination.

If you want, I can draft an example governance-based flow integrating an on-chain multisig or the Solana governance program — tell me which governance program you'd prefer and I will draft the integration steps.
