#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""
Must be Keccak-256, not hashlib.sha3_256.

Simple EVM-side golden vector generator placeholder.
Produces tests/golden/hashes.json with a single sample vector.
Install: pip install pysha3
Run: ./generate.py > ../../programs/zpx_router/tests/golden/hashes.json
"""
import json
try:
    # Prefer pycryptodome's Keccak
    from Crypto.Hash import keccak
    def keccak256(data: bytes):
        k = keccak.new(digest_bits=256)
        k.update(data)
        return k.hexdigest()
except Exception:
    try:
        import sha3
        def keccak256(data: bytes):
            k = sha3.keccak_256()
            k.update(data)
            return k.hexdigest()
    except Exception:
        raise RuntimeError("Missing keccak implementation: install pycryptodome (`pip install pycryptodome`) or pysha3 (`pip install pysha3`)")

# Sample vector (matches the Rust packing used by message_hash_be)
sample = {
    "src_chain_id": 1,
    "dst_chain_id": 2,
    "src_adapter": "0000000000000000000000000000000000000000000000000000000000000000",
    "recipient": "0000000000000000000000000000000000000000000000000000000000000000",
    "asset": "0000000000000000000000000000000000000000000000000000000000000000",
    "amount": 1000,
    "payload_hash": "0000000000000000000000000000000000000000000000000000000000000000",
    "nonce": 1
}

# rudimentary packing to emulate message_hash_be
def to_be_u64(x):
    return x.to_bytes(8, 'big')

def to_be_u128_32(x):
    b = x.to_bytes(16, 'big')
    return b.rjust(32, b'\x00')

buf = bytearray()
buf.extend(to_be_u64(sample['src_chain_id']))
buf.extend(bytes.fromhex(sample['src_adapter']))
buf.extend(bytes.fromhex(sample['recipient']))
buf.extend(bytes.fromhex(sample['asset']))
buf.extend(to_be_u128_32(sample['amount']))
buf.extend(bytes.fromhex(sample['payload_hash']))
buf.extend(to_be_u64(sample['nonce']))
buf.extend(to_be_u64(sample['dst_chain_id']))

hash_hex = keccak256(bytes(buf))

out = {"vectors": [{"input": sample, "hash": hash_hex}]}
print(json.dumps(out, indent=2))
