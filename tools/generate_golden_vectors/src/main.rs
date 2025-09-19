use serde::Serialize;
use zpx_router::hash::{global_route_id, keccak256, message_hash_be};

#[derive(Serialize)]
struct MsgHashCase {
    src_chain_id: u64,
    dst_chain_id: u64,
    nonce: u64,
    src_adapter: String,
    recipient: String,
    asset: String,
    amount_be_hex: String,
    payload_hex: String,
    expected_message_hash_hex: String,
    initiator: String,
    expected_global_route_id_hex: String,
}

#[derive(Serialize)]
struct Golden {
    message_hashes: Vec<MsgHashCase>,
}

fn addr32(addr_hex_no0x: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let raw = hex::decode(addr_hex_no0x).unwrap();
    assert_eq!(raw.len(), 20);
    out[12..].copy_from_slice(&raw);
    out
}

fn hex32_from_u128_be(v: u128) -> (String, [u8; 32]) {
    let mut be = [0u8; 32];
    be[16..].copy_from_slice(&v.to_be_bytes());
    (hex::encode(be), be)
}

fn main() {
    // Define a few canonical cases
    let cases = vec![
        (
            42161u64,
            8453u64,
            42u64,
            "1111111111111111111111111111111111111111",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "2222222222222222222222222222222222222222",
            123_456u128,
            "deadbeef",
            "3333333333333333333333333333333333333333",
        ),
        (
            1u64,
            2u64,
            1u64,
            "0000000000000000000000000000000000000001",
            "0000000000000000000000000000000000000002",
            "0000000000000000000000000000000000000003",
            u128::from(u64::MAX),
            "",
            "0000000000000000000000000000000000000004",
        ),
        (
            10u64,
            56u64,
            9999u64,
            "1234567890abcdef1234567890abcdef12345678",
            "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
            "9999999999999999999999999999999999999999",
            1_000_000_000_000_000_000u128,
            "0102030405",
            "7777777777777777777777777777777777777777",
        ),
    ];

    let mut out: Vec<MsgHashCase> = Vec::new();
    for (src, dst, nonce, src_adapter, recipient, asset, amount_u128, payload_hex, initiator) in
        cases
    {
        let src_adapter32 = addr32(src_adapter);
        let recipient32 = addr32(recipient);
        let asset32 = addr32(asset);
        let (amount_be_hex, amount_be) = hex32_from_u128_be(amount_u128);
        let payload = hex::decode(payload_hex).unwrap_or_default();
        let payload_hash = keccak256(&[&payload]);
        let msg_hash = message_hash_be(
            src,
            src_adapter32,
            recipient32,
            asset32,
            amount_be,
            payload_hash,
            nonce,
            dst,
        );
        let initiator32 = addr32(initiator);
        let global = global_route_id(src, dst, initiator32, msg_hash, nonce);
        out.push(MsgHashCase {
            src_chain_id: src,
            dst_chain_id: dst,
            nonce,
            src_adapter: src_adapter.to_string(),
            recipient: recipient.to_string(),
            asset: asset.to_string(),
            amount_be_hex,
            payload_hex: payload_hex.to_string(),
            expected_message_hash_hex: hex::encode(msg_hash),
            initiator: initiator.to_string(),
            expected_global_route_id_hex: hex::encode(global),
        });
    }

    let golden = Golden {
        message_hashes: out,
    };
    let json = serde_json::to_string_pretty(&golden).unwrap();
    println!("{}", json);
}
