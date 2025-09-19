# Devnet Multisig Upgrade Test Plan

This document describes step-by-step how to test program upgrade authority using a multisig custody model on Solana Devnet. These steps are intended for testing only — never store real private keys or production secrets in this repository.

Prerequisites
- `solana` CLI configured to `devnet` (run `solana config get` to verify).
- `spl-token` CLI installed if using the SPL token multisig helper.
- A set of local keypairs for custodians (for testing, generate ephemeral keypairs with `solana-keygen new --no-passphrase --outfile /path/to/key.json`).

Summary of flow
1. Create or identify the current upgrade authority (keyfile A).
2. Generate a new upgrade authority keypair (keyfile B) that will be placed under multisig custody for testing.
3. Create a multisig on devnet (Option 1: off-chain offline signing using the B key held by custodians; Option 2: on-chain multisig using SPL Token multisig as a simple account).
4. Set the program's upgrade authority to the new keypair B.
5. Prepare an upgrade transaction and exercise the offline multisig signing flow.
6. Submit the upgrade to devnet and verify the program.
7. Run post-upgrade sanity checks (smoke E2E flows) and optionally roll back.

Detailed steps

1) Environment setup
- Ensure `solana` is set to devnet and funded keypairs exist:

```bash
solana config set --url devnet
solana airdrop 2 /path/to/authority-keypair.json
```

2) Create keyfiles for custodians (example)

```bash
solana-keygen new --no-passphrase --outfile ./keys/custodian1.json
solana-keygen new --no-passphrase --outfile ./keys/custodian2.json
solana-keygen new --no-passphrase --outfile ./keys/custodian3.json
```

3) Create test multisig (SPL token multisig example)

```bash
export MULTISIG_SIGNERS=./keys/custodian1.json,./keys/custodian2.json,./keys/custodian3.json
export MULTISIG_THRESHOLD=2
scripts/create-devnet-multisig.sh
# Note: The script prints the multisig address when using spl-token create-multisig.
```

4) Create upgrade-authority key (B) and distribute for custody (test only)

```bash
solana-keygen new --no-passphrase --outfile ./keys/upgrade_authority_b.json
# For testing, copy this file to each custodian folder (or simulate custody by letting each custodian hold their own copy).
```

5) Set program upgrade authority to B

```bash
# Replace PROGRAM_ID and path to keyfile
scripts/set-upgrade-authority.sh \
  --program-id <PROGRAM_ID> \
  --current-authority ./keys/authority_a.json \
  --new-authority ./keys/upgrade_authority_b.json \
  --cluster devnet
```

6) Prepare and sign upgrade transaction (offline multisig flow, simulated)

- For devnet testing, simulate multisig by having custodians sign the same offline keyfile (not secure in prod).
- Alternatively, use the SPL multisig program: construct a transaction that requires multisig approval and submit via the multisig program's execution flow.

7) Perform program upgrade using `scripts/upgrade-program.sh`

```bash
scripts/upgrade-program.sh \
  --program-id <PROGRAM_ID> \
  --so-path ./dist/zpx_router.so \
  --authority ./keys/upgrade_authority_b.json \
  --cluster devnet
```

8) Post-upgrade checks
- Run quick program-tests smoke flow locally or run a set of minimal RPC checks:

```bash
cargo test -p zpx_router --features program-test -- --ignored tests::smoke -- --nocapture
# Or run the project's E2E smoke script if available
```

9) Rollback (if needed)
- Keep a saved program binary and repeat `upgrade-program.sh` with the older `.so` file.

Security notes
- Never commit keyfiles to the repo.
- For production, use hardware-backed key storage or an on-chain multisig/governance program with timelocks.

Next steps
- Wire `scripts/upgrade-program.sh` and `scripts/set-upgrade-authority.sh` to support multisig signing helpers and to optionally interact with an on-chain approval program.
- Add an automated devnet test that runs the smoke tests after upgrade.
