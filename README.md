# Universal Router (Solana)

Thin, adapter-agnostic router that mirrors the core ideas of the ZoopX EVM router.

## Quick start

- Build everything with Cargo (we avoid Anchor CLI due to environment GLIBC constraints):

```
cargo build --workspace
```

- Run tests for router only:

```
cargo test -p zpx_router
```

## Design

- Stateless per-transfer: source-side fee skim and forward.
- Adapter allowlist on the source side; no hardcoded adapter.
- Canonical hashing utilities (Keccak256, big-endian packing) for message hash and global route id.
- Events mirror the EVM schema. Field order is frozen and guarded by tests.

## Hashing

- `message_hash_be(src_chain_id, src_adapter_32, recipient_32, asset_32, amount_be, payload_hash, nonce, dst_chain_id)`
- `global_route_id(src_chain_id, dst_chain_id, initiator_32, message_hash, nonce)`

Payload hashing uses `keccak256(payload)`.

**NOTE:** Golden vectors use Keccak-256; do not use Python's `hashlib.sha3_256` (which implements SHA3-256 and not Keccak-256). Use `pycryptodome` or `pysha3`/`sha3` to generate Keccak-256 vectors.

## Events (schema frozen)

- BridgeInitiated(route_id, user, token, target, forwarded_amount, protocol_fee, relayer_fee, payload_hash, src_chain_id, dst_chain_id, nonce)
- UniversalBridgeInitiated(route_id, payload_hash, message_hash, global_route_id, user, token, target, forwarded_amount, protocol_fee, relayer_fee, src_chain_id, dst_chain_id, nonce)
- FeeAppliedSource(message_hash, asset, payer, target, protocol_fee, relayer_fee, fee_recipient, applied_at)

Schema field names are exported as constants for snapshot tests.

## Notes

- Anchor 0.30.1 and SPL 0.30.1. We build and test with Cargo.
- Program IDs in Anchor.toml are placeholders and should be updated when deploying.

