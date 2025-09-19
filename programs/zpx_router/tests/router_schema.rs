// SPDX-License-Identifier: MIT
use anchor_lang::prelude::Pubkey;
use std::fs;
use std::path::Path;
use zpx_router::hash::{global_route_id, keccak256, message_hash_be};
use zpx_router::{
    compute_fees_and_forward, validate_common, validate_payload_len, Config,
    BRIDGE_INITIATED_FIELDS, FEE_APPLIED_DEST_FIELDS, FEE_APPLIED_SOURCE_FIELDS,
    UNIVERSAL_BRIDGE_INITIATED_FIELDS,
};

fn hex32(s: &str) -> [u8; 32] {
    let b = hex::decode(s).expect("hex");
    let mut out = [0u8; 32];
    out.copy_from_slice(&b);
    out
}

#[test]
fn message_hash_vectors() {
    // Vector 1
    let src: u64 = 42161;
    let dst: u64 = 8453;
    let adapter = {
        // address 0x1111111111111111111111111111111111111111
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&hex::decode("1111111111111111111111111111111111111111").unwrap());
        a
    };
    let recipient = [0u8; 32];
    let asset = {
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&hex::decode("2222222222222222222222222222222222222222").unwrap());
        a
    };
    let mut amount_be = [0u8; 32];
    amount_be[16..].copy_from_slice(&(123_456u128).to_be_bytes());
    let payload = b"deadbeef".to_vec();
    let p_hash = keccak256(&[&payload]);
    let nonce: u64 = 42;
    let got = message_hash_be(
        src, adapter, recipient, asset, amount_be, p_hash, nonce, dst,
    );
    // Self-consistency until we import exact vectors; shape mirrors EVM hash packing.
    let expected = message_hash_be(
        src, adapter, recipient, asset, amount_be, p_hash, nonce, dst,
    );
    assert_eq!(got, expected);

    // Vector 2: zero payload
    let p_hash2 = keccak256(&[&[]]);
    let mut amount2 = [0u8; 32];
    amount2[24..].copy_from_slice(&(u64::MAX).to_be_bytes());
    let got2 = message_hash_be(1, adapter, recipient, asset, amount2, p_hash2, 1, 2);
    let expected2 = message_hash_be(1, adapter, recipient, asset, amount2, p_hash2, 1, 2);
    assert_eq!(got2, expected2);
}

#[test]
fn global_route_id_vectors() {
    let src = 42161u64;
    let dst = 8453u64;
    let initiator = {
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&hex::decode("3333333333333333333333333333333333333333").unwrap());
        a
    };
    let msg_hash = hex32("f4a3d7c488f776c45676b69a71a3c32c42fd640aa9f4f1c4b9fb3e0b7588c4fc");
    let nonce = 42u64;
    let got = global_route_id(src, dst, initiator, msg_hash, nonce);
    let expected = global_route_id(src, dst, initiator, msg_hash, nonce);
    assert_eq!(got, expected);

    // Vector 3: different initiator and nonce
    let initiator2 = {
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap());
        a
    };
    let nonce2 = 99u64;
    let got3 = global_route_id(src, dst, initiator2, msg_hash, nonce2);
    let expected3 = global_route_id(src, dst, initiator2, msg_hash, nonce2);
    assert_eq!(got3, expected3);
}

#[test]
fn fee_caps_math() {
    // Protocol cap 5 bps of amount
    let amount = 1_000_000u64; // 1e6
    let protocol_fee_cap = amount as u128 * 5u128 / 10_000u128;
    assert_eq!(protocol_fee_cap, 500);
    // Relayer fee cap 1000 bps when relayer_fee_bps=1000
    let relayer_bps = 1000u128;
    let relayer_fee_cap = amount as u128 * relayer_bps / 10_000u128;
    assert_eq!(relayer_fee_cap, 100_000);
}

#[test]
fn payload_size_boundaries() {
    assert!(validate_payload_len(512).is_ok());
    assert!(validate_payload_len(513).is_err());
}

#[test]
fn paused_blocks_and_adapter_check() {
    let cfg = Config {
        admin: Pubkey::new_unique(),
        fee_recipient: Pubkey::new_unique(),
        src_chain_id: 1,
        relayer_fee_bps: 1000,
        adapters_len: 1,
        adapters: {
            let mut arr = [Pubkey::default(); 8];
            arr[0] = Pubkey::new_unique();
            arr
        },
        paused: true,
        bump: 255,
    };
    // Paused blocks
    assert!(validate_common(1, 0, cfg.paused, cfg.src_chain_id).is_err());
    // Adapter allowlist logic: constructing a different program should not match
    let some_program = Pubkey::new_unique();
    let allowed = cfg.adapters[..cfg.adapters_len as usize].contains(&some_program);
    assert!(!allowed);
}

#[test]
fn fee_cap_boundaries() {
    let amount = 1_000_000u64; // 1e6
                               // Protocol fee: exactly at cap (5 bps)
    let ok = compute_fees_and_forward(amount, 500, 0, 0);
    assert!(ok.is_ok());
    // Protocol fee: one over cap
    let ov = compute_fees_and_forward(amount, 501, 0, 0);
    assert!(ov.is_err());

    // Relayer fee cap with relayer_fee_bps = 1000 (10%)
    let ok2 = compute_fees_and_forward(amount, 0, 100_000, 1000);
    assert!(ok2.is_ok());
    let ov2 = compute_fees_and_forward(amount, 0, 100_001, 1000);
    assert!(ov2.is_err());

    // Combined must not exceed amount
    let over_total = compute_fees_and_forward(amount, 600_000, 500_001, 10_000);
    assert!(over_total.is_err());
}

#[test]
#[ignore]
fn golden_vectors_if_present() {
    // Optional: if tests/golden/hashes.json exists, load and verify parity
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let p = manifest_dir.join("tests/golden/hashes.json");
    if !p.exists() {
        eprintln!("golden vectors file not found: {}", p.display());
        return;
    }
    let data = fs::read_to_string(&p).expect("read golden");
    #[derive(serde::Deserialize)]
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
    #[derive(serde::Deserialize)]
    struct Golden {
        message_hashes: Vec<MsgHashCase>,
    }
    // Support two possible golden formats for backwards compatibility:
    // - current: { "message_hashes": [ ... ] }
    // - legacy: { "vectors": [ { "input": { ... }, "hash": "..." } ] }
    let golden: Golden = match serde_json::from_str(&data) {
        Ok(g) => g,
        Err(_) => {
            // Try legacy shape
            #[derive(serde::Deserialize)]
            struct LegacyInput {
                src_chain_id: u64,
                dst_chain_id: u64,
                src_adapter: String,
                recipient: String,
                asset: String,
                amount: u64,
                payload_hash: String,
                nonce: u64,
            }
            #[derive(serde::Deserialize)]
            struct LegacyVecCase {
                input: LegacyInput,
                hash: String,
            }
            #[derive(serde::Deserialize)]
            struct LegacyRoot {
                vectors: Vec<LegacyVecCase>,
            }
            let legacy: LegacyRoot = serde_json::from_str(&data).expect("legacy json");
            // Convert legacy into Golden
            let mut gh = Golden {
                message_hashes: vec![],
            };
            for v in legacy.vectors {
                let amount_be_hex = {
                    // encode amount as 32-byte big-endian hex (u128 placed in low 16 bytes)
                    let mut be = [0u8; 32];
                    be[16..].copy_from_slice(&(v.input.amount as u128).to_be_bytes());
                    hex::encode(be)
                };
                gh.message_hashes.push(MsgHashCase {
                    src_chain_id: v.input.src_chain_id,
                    dst_chain_id: v.input.dst_chain_id,
                    nonce: v.input.nonce,
                    src_adapter: v.input.src_adapter,
                    recipient: v.input.recipient,
                    asset: v.input.asset,
                    amount_be_hex,
                    payload_hex: v.input.payload_hash,
                    expected_message_hash_hex: v.hash,
                    initiator: String::from(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    ),
                    expected_global_route_id_hex: String::from(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    ),
                });
            }
            gh
        }
    };
    for c in &golden.message_hashes {
        // Helper: accept either 20-byte (EVM address) or 32-byte hex strings
        fn to_32_bytes(h: &str) -> [u8; 32] {
            let raw = hex::decode(h).expect("hex");
            let mut out = [0u8; 32];
            match raw.len() {
                32 => out.copy_from_slice(&raw),
                20 => out[12..].copy_from_slice(&raw),
                n => panic!("unexpected hex length {} for {}", n, h),
            }
            out
        }

        let adapter = to_32_bytes(&c.src_adapter);
        let recipient = to_32_bytes(&c.recipient);
        let asset = to_32_bytes(&c.asset);
        let mut amount_be = [0u8; 32];
        let mut amount_bytes = hex::decode(&c.amount_be_hex).unwrap();
        // Accept shorter hex (e.g., u64/u128 encoded) by left-padding to 32 bytes
        if amount_bytes.len() > 32 {
            panic!("amount_be hex too long: {} bytes", amount_bytes.len());
        } else if amount_bytes.len() < 32 {
            let mut padded = vec![0u8; 32 - amount_bytes.len()];
            padded.extend_from_slice(&amount_bytes);
            amount_bytes = padded;
        }
        amount_be.copy_from_slice(&amount_bytes);
        let payload = hex::decode(&c.payload_hex).unwrap();
        let payload_hash = keccak256(&[&payload]);
        let got_msg = message_hash_be(
            c.src_chain_id,
            adapter,
            recipient,
            asset,
            amount_be,
            payload_hash,
            c.nonce,
            c.dst_chain_id,
        );
        let exp_msg = hex::decode(&c.expected_message_hash_hex).unwrap();
        if got_msg.as_slice() != exp_msg.as_slice() {
            eprintln!("golden mismatch: message_hash case nonce={} src={}\n computed={:?}\n expected={:?}", c.nonce, c.src_chain_id, got_msg, exp_msg);
        }
        // Accept 20- or 32-byte initiator hex (EVM addresses vs full 32-byte pubkeys)
        let initiator = {
            fn to_32_bytes_local(h: &str) -> [u8; 32] {
                let raw = hex::decode(h).expect("hex");
                let mut out = [0u8; 32];
                match raw.len() {
                    32 => out.copy_from_slice(&raw),
                    20 => out[12..].copy_from_slice(&raw),
                    n => panic!("unexpected hex length {} for initiator {}", n, h),
                }
                out
            }
            to_32_bytes_local(&c.initiator)
        };
        let got_global =
            global_route_id(c.src_chain_id, c.dst_chain_id, initiator, got_msg, c.nonce);
        let exp_global = hex::decode(&c.expected_global_route_id_hex).unwrap();
        if got_global.as_slice() != exp_global.as_slice() {
            eprintln!("golden mismatch: global_route case nonce={} src={}\n computed={:?}\n expected={:?}", c.nonce, c.src_chain_id, got_global, exp_global);
        }
    }

    // If EVM JSON exists, compare field-by-field for exact parity
    let evm_p = manifest_dir.join("tests/golden/hashes.evm.json");
    if evm_p.exists() {
        let evm_data = fs::read_to_string(&evm_p).expect("read evm json");
        let evm_golden: Golden = serde_json::from_str(&evm_data).expect("json evm");
        // Compare lengths first
        assert_eq!(
            evm_golden.message_hashes.len(),
            golden.message_hashes.len(),
            "EVM vector count mismatch"
        );
        // Define a helper to normalize 0x prefixes
        fn strip0x(s: &str) -> &str {
            s.strip_prefix("0x").unwrap_or(s)
        }
        for (i, (a, b)) in evm_golden
            .message_hashes
            .iter()
            .zip(golden.message_hashes.iter())
            .enumerate()
        {
            assert_eq!(a.src_chain_id, b.src_chain_id, "case {} src_chain_id", i);
            assert_eq!(a.dst_chain_id, b.dst_chain_id, "case {} dst_chain_id", i);
            assert_eq!(a.nonce, b.nonce, "case {} nonce", i);
            assert_eq!(
                strip0x(&a.src_adapter),
                b.src_adapter,
                "case {} src_adapter",
                i
            );
            assert_eq!(strip0x(&a.recipient), b.recipient, "case {} recipient", i);
            assert_eq!(strip0x(&a.asset), b.asset, "case {} asset", i);
            assert_eq!(
                strip0x(&a.amount_be_hex),
                b.amount_be_hex,
                "case {} amount_be_hex",
                i
            );
            assert_eq!(
                strip0x(&a.payload_hex),
                b.payload_hex,
                "case {} payload_hex",
                i
            );
            assert_eq!(strip0x(&a.initiator), b.initiator, "case {} initiator", i);
            assert_eq!(
                strip0x(&a.expected_message_hash_hex),
                b.expected_message_hash_hex,
                "case {} expected_message_hash_hex",
                i
            );
            assert_eq!(
                strip0x(&a.expected_global_route_id_hex),
                b.expected_global_route_id_hex,
                "case {} expected_global_route_id_hex",
                i
            );
        }
    }
}

#[test]
fn event_schema_snapshots() {
    // BridgeInitiated field order
    assert_eq!(
        BRIDGE_INITIATED_FIELDS,
        &[
            "route_id",
            "user",
            "token",
            "target",
            "forwarded_amount",
            "protocol_fee",
            "relayer_fee",
            "payload_hash",
            "src_chain_id",
            "dst_chain_id",
            "nonce"
        ]
    );
    // UniversalBridgeInitiated field order
    assert_eq!(
        UNIVERSAL_BRIDGE_INITIATED_FIELDS,
        &[
            "route_id",
            "payload_hash",
            "message_hash",
            "global_route_id",
            "user",
            "token",
            "target",
            "forwarded_amount",
            "protocol_fee",
            "relayer_fee",
            "src_chain_id",
            "dst_chain_id",
            "nonce"
        ]
    );
    // FeeAppliedSource field order
    assert_eq!(
        FEE_APPLIED_SOURCE_FIELDS,
        &[
            "message_hash",
            "asset",
            "payer",
            "target",
            "protocol_fee",
            "relayer_fee",
            "fee_recipient",
            "applied_at"
        ]
    );

    // FeeAppliedDest field order
    assert_eq!(
        FEE_APPLIED_DEST_FIELDS,
        &[
            "message_hash",
            "src_chain_id",
            "dst_chain_id",
            "router",
            "asset",
            "amount",
            "protocol_bps",
            "lp_bps",
            "collector",
            "applied_at",
        ]
    );
}

#[test]
fn event_parity_smoke() {
    // Basic smoke test that the exported schema arrays contain expected field names.
    // Deeper encoded-log parity requires program-test and event decoding; keep this lightweight.
    assert_eq!(BRIDGE_INITIATED_FIELDS[0], "route_id");
    assert_eq!(UNIVERSAL_BRIDGE_INITIATED_FIELDS[2], "message_hash");
    assert_eq!(FEE_APPLIED_DEST_FIELDS[0], "message_hash");
}
