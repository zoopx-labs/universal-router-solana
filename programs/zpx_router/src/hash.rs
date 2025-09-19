// SPDX-License-Identifier: MIT
use tiny_keccak::{Hasher, Keccak};

/// SCHEMA FROZEN. Do not change packing or order. Add V2 functions if changes are ever required.
pub fn keccak256(parts: &[&[u8]]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut hasher = Keccak::v256();
    for p in parts {
        hasher.update(p);
    }
    hasher.finalize(&mut out);
    out
}

/// Pack canonical tuple and keccak it:
/// (srcChainId u64 BE) | (srcAdapter [32]) | (recipient [32]) | (asset [32]) |
/// (amount uint256 BE [32]) | (payloadHash [32]) | (nonce u64 BE) | (dstChainId u64 BE)
#[allow(clippy::too_many_arguments)] // SCHEMA FROZEN: keep explicit args for parity and readability
pub fn message_hash_be(
    src_chain_id: u64,
    src_adapter_32: [u8; 32],
    recipient_32: [u8; 32],
    asset_32: [u8; 32],
    amount_be: [u8; 32],
    payload_hash: [u8; 32],
    nonce: u64,
    dst_chain_id: u64,
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 6 + 8 + 8);
    buf.extend_from_slice(&src_chain_id.to_be_bytes());
    buf.extend_from_slice(&src_adapter_32);
    buf.extend_from_slice(&recipient_32);
    buf.extend_from_slice(&asset_32);
    buf.extend_from_slice(&amount_be);
    buf.extend_from_slice(&payload_hash);
    buf.extend_from_slice(&nonce.to_be_bytes());
    buf.extend_from_slice(&dst_chain_id.to_be_bytes());
    keccak256(&[&buf])
}

/// globalRouteId = keccak256(abi.encodePacked(srcChainId, dstChainId, initiator, messageHash, nonce))
pub fn global_route_id(
    src_chain_id: u64,
    dst_chain_id: u64,
    initiator_32: [u8; 32],
    message_hash: [u8; 32],
    nonce: u64,
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(8 + 8 + 32 + 32 + 8);
    buf.extend_from_slice(&src_chain_id.to_be_bytes());
    buf.extend_from_slice(&dst_chain_id.to_be_bytes());
    buf.extend_from_slice(&initiator_32);
    buf.extend_from_slice(&message_hash);
    buf.extend_from_slice(&nonce.to_be_bytes());
    keccak256(&[&buf])
}
