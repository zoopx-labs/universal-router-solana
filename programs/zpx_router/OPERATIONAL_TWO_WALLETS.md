Two-wallet backend model (Adapter + Relayer)

Purpose
-------
This document records the operational decision to run the router with two distinct wallets controlled by the backend:

- Adapter identity (source-side canonical identity)
- Relayer identity (transaction submitter / finalizer)

Keep this note next to the program so future maintainers know the intended deployment pattern.

Why two wallets?
-----------------
- Canonical semantics: the adapter value embedded in the canonical message hash should represent the source-side identity (on EVM this is typically a 20-byte address). Keeping adapter as a persistent identity preserves cross-chain parity and traceability.
- Separation of duty: the relayer submits finalize transactions and pays fees. Having a separate relayer wallet reduces the blast radius if one identity is compromised.
- Operational flexibility: the backend controls both keys and can rotate or replace the relayer without changing the canonical adapter identity.

Recommended mapping when EVM uses relayer-as-adapter
-----------------------------------------------------
If your EVM pipeline uses the relayer address as the adapter value (common in practice), adopt this mapping on Solana:
- Adapter (on Solana): 32-byte left-padded representation of the 20-byte EVM address used as the adapter.
- Relayer (on Solana): the Solana signer keypair that will call `finalize_message_v1` and fund the replay PDA.

Admin actions required
----------------------
1) Add the adapter identity to the router config (admin-only):
   - Use the program `add_adapter` instruction to register the adapter (the 32-byte left-padded EVM address).
   - This makes `finalize_message_v1` accept messages whose src_adapter matches that registered adapter (unless adapters list is permissive as configured).

2) Configure relayer governance:
   - Use a hardened signing policy for the relayer wallet (multisig, HSM, or machine with strict key rotation).
   - Optionally add a relayer whitelist in `Config` (if configured) so only approved relayer pubkeys can call `finalize_message_v1`.

Operational controls and safeguards
----------------------------------
- Emergency pause: keep an admin process ready to set `config.paused = true` if suspicious activity is detected.
- Monitoring: run off-chain monitors on `UniversalBridgeInitiated`, `FeeAppliedDest`, and `BridgeInitiated` logs to detect unusual adapters or large volumes.
- Access hardening: use multisig for the relayer private key in production. Rotate keys and keep narrow permissions for admin actions.
- Auditability: log every adapter add/remove and keep a changelog for on-chain config updates.

Quick checklist for deployment
------------------------------
- [ ] Generate Adapter identity (EVM relayer address). Left-pad to 32 bytes when registering on Solana.
- [ ] Generate Relayer keypair and secure it (multisig/HSM recommended).
- [ ] Call program `initialize_config` (admin) and then `add_adapter` for the adapter pubkey.
- [ ] Ensure relayer has sufficient lamports to submit finalize transactions and create PDAs.
- [ ] Start the backend process with both keys available (adapter identity used in message creation on EVM and relayer used for finalize submission on Solana).

Notes
-----
- This document is intentionally concise. If you prefer a different operational model (adapter-agnostic, permissive mode, or relayer-only finalizers), update this file and coordinate a code change (e.g., permissive `adapters_len == 0` policy or adding a relayer whitelist field).

Contact
-------
For questions about implementing the two-wallet model, reach out to the dev who made this note or open an issue in the repo referencing this file.
