# Generating EVM golden vectors

This repository includes Solidity sources under `delete-later/evm-router` that define the canonical hashing scheme in `lib/Hashing.sol`.

Two values are required per test case:

- `messageHash(srcChainId, srcAdapter, recipient, asset, amount, payloadHash, nonce, dstChainId)`
- `globalRouteId(srcChainId, dstChainId, initiator, messageHash, nonce)`

Suggested approaches:

1) Foundry script

- Write a small script contract that imports `Hashing.sol`, computes values for a few cases, and prints them via `console2`.
- Pipe output to JSON.

2) Node (ethers.js)

- Compile the Solidity library or reimplement the exact packing using ethers.js utilities.
- Save the resulting JSON to `programs/zpx_router/tests/golden/hashes.json` with the shape expected by the test:

```
{
  "message_hashes": [
    {
      "src_chain_id": 42161,
      "dst_chain_id": 8453,
      "nonce": 42,
      "src_adapter": "1111111111111111111111111111111111111111",
      "recipient": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "asset": "2222222222222222222222222222222222222222",
      "amount_be_hex": "<64-hex-bytes>",
      "payload_hex": "deadbeef",
      "expected_message_hash_hex": "<64-hex-bytes>",
      "initiator": "3333333333333333333333333333333333333333",
      "expected_global_route_id_hex": "<64-hex-bytes>"
    }
  ]
}
```

Run the `golden_vectors_if_present` ignored test to verify parity:

```
cargo test -p zpx_router --test router_schema -- --ignored --nocapture
```
