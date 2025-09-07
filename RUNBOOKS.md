# Runbooks

## Deploy
1. Ensure `Anchor.toml` program IDs match the intended cluster.
2. `anchor build` → `anchor deploy` to publish program and create IDL account.
3. Re-run `anchor build` to embed `metadata.address`.
4. Archive `target/idl/zoopx_router.json` into `idl/` with PROGRAM_ID and date; record SHA-256.
5. Set/verify upgrade authority to governance multisig.

## Upgrade
1. Prepare release notes and updated IDL.
2. `anchor build` and verify tests/CI green.
3. Governance approves upgrade; multisig submits `setUpgradeAuthority` if needed.
4. `anchor deploy --program-id ...` signed by upgrade authority.
5. Update archived IDL and README hashes; clients verify.

## Rollback
1. Keep previous program binaries and IDLs in release artifacts.
2. Use upgrade authority to deploy previous artifact.
3. Communicate rollback and validate invariants.

## Key Rotation
- Rotate governance multisig keys per policy; update records and access lists.
- Maintain encrypted backups and HSM usage where applicable.

## Incident Response
- Freeze upgrades if compromise suspected.
- Rotate keys, redeploy clean artifacts, and publish postmortem.

## IDL Verification
- Clients fetch on-chain IDL or use embedded file.
- Verify SHA-256 matches archived value in `idl/` and README.
