# ZoopX Router — Devnet Deployment

This document records the current devnet deployment of the Anchor program and the exact steps to reproduce, verify, and upgrade it.

## Summary

- Cluster: devnet (https://api.devnet.solana.com)
- Program name: zoopx_router
- Program ID: 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf
- Upgrade authority (pubkey): 7iWDd7GLkeCtDHJerXpzdrvsTLc8MXtJAvM1UT3hW7fE
- Upgrade authority wallet: ~/.config/solana/zoopx-devnet.json
- ProgramData address: 3qVTtd6UMXrSJxuzc3fazzjVCwxhLSmFsHkuzRrynTxn
- On-chain IDL: present (IDL account: 4QMiYVXRrRgrKWRJLaCrcPpL6DTvVQenijzYvHrrmp7C)
- On-chain IDL: present (IDL account: 4QMiYVXRrRgrKWRJLaCrcPpL6DTvVQenijzYvHrrmp7C)

- Config PDA (seed "zoopx_config"): FhpudAPCSpEyqD91QNcA6gDA2bDRinu2yWuwmdvVReok
- Config initialized: true — fee_recipient set to 2QpAnre7Wjc8qWmKHyZBfb5Sda55CRrJh61d1oUALLrd
- Init transaction: 7EJNiB2t4R9MuYCrgQSZVL6J6y99BpU5ktT8A5ZY5M9ptx3xK6UhateSGQjvTDsSBSzN862qSUWLkj5jugAhxAJ
- Anchor CLI: 0.31.1
- declare_id! in source: 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf

Anchor.toml is configured for devnet:

```toml
[programs.devnet]
zoopx_router = "654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf"

[provider]
cluster = "devnet"
wallet = "~/.config/solana/zoopx-devnet.json"
```

## Build

```bash
anchor build
```

Artifacts:
- target/deploy/zoopx_router.so (program binary)
- target/deploy/zoopx_router-keypair.json (program keypair; its pubkey must equal declare_id!)
- target/idl/zoopx_router.json (IDL)

To confirm the keypair pubkey:

```bash
solana-keygen pubkey target/deploy/zoopx_router-keypair.json
```

## Deploy

```bash
# Ensure CLI uses the devnet wallet
solana config set --url https://api.devnet.solana.com --keypair ~/.config/solana/zoopx-devnet.json

# Deploy with Anchor
anchor deploy --provider.cluster devnet --provider.wallet ~/.config/solana/zoopx-devnet.json
```

If the program is already deployed, `anchor deploy` will perform an upgrade using the same upgrade authority.

## Verify on chain

```bash
solana program show 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf
anchor idl fetch 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf --provider.cluster devnet
```

Expected fields include the ProgramData address and the upgrade authority listed above.

## IDL management

Upload or update the on-chain IDL after a build:

```bash
# Initialize once (if no IDL exists)
anchor idl init 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf \
  --provider.cluster devnet \
  --provider.wallet ~/.config/solana/zoopx-devnet.json \
  --filepath target/idl/zoopx_router.json

# Or upgrade when an IDL already exists
anchor idl upgrade 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf \
  --provider.cluster devnet \
  --provider.wallet ~/.config/solana/zoopx-devnet.json \
  --filepath target/idl/zoopx_router.json
```

Optional (governance hardening):

```bash
# Remove the ability to modify the IDL account
anchor idl erase-authority 654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf --provider.cluster devnet
```

Local IDL copy

The active on-chain IDL has been fetched and saved locally at `target/idl/zoopx_router.json`. Use this file for client generation or to re-upload with `anchor idl upgrade` if you change the interface.

Note about initialization

The program's config PDA (seed "zoopx_config") was uninitialized prior to the action recorded above; the config was initialized and the `fee_recipient` field was set to the address shown. No further on-chain initialization is required for the default config. To change the fee recipient later, run `scripts/set_fee_recipient.ts` or call `update_config` directly.

## Upgrading the program

1) Make code changes and ensure `declare_id!` remains the same.
2) Build: `anchor build`
3) Deploy (upgrade): `anchor deploy --provider.cluster devnet --provider.wallet ~/.config/solana/zoopx-devnet.json`
4) Update IDL if the interface changed (see IDL management).

Changing the program ID is not recommended. If it must change, update both:
- Source: `declare_id!("<new_program_id>")`
- Anchor.toml: `[programs.devnet].zoopx_router = "<new_program_id>"`

## Troubleshooting

- Mismatch: "Program keypair does not match declared program id"
  - Ensure `declare_id!` equals `solana-keygen pubkey target/deploy/zoopx_router-keypair.json`.
  - Rebuild after changes.

- Insufficient funds
  - Fund the devnet wallet: `solana airdrop 2 <WALLET_PUBKEY> --url https://api.devnet.solana.com`

- IDL command errors
  - Check subcommands: `anchor idl --help` (init, upgrade, set-authority, etc.).

- Confirm environment
  - `anchor --version` should be 0.31.1; `solana config get` should point to devnet with the expected keypair.
