use proptest::prelude::*;
use zpx_router::{compute_fees_and_forward, validate_payload_len};

proptest! {
    // Any protocol_fee that exceeds 5 bps of amount should error
    #[test]
    fn protocol_fee_over_cap_errors(amount in 1u64..=u64::MAX, excess in 1u64..=1_000_000u64) {
        // Cap is floor(amount * 5 / 10_000)
        let cap = (amount as u128 * 5u128) / 10_000u128;
        // Make protocol_fee > cap safely (bounded by u64)
    let proto_fee = ((cap + 1).min(u128::from(u64::MAX))) as u64 + (excess % 5);
        let res = compute_fees_and_forward(amount, proto_fee, 0, 1000);
        prop_assert!(res.is_err());
    }

    // Any relayer_fee that exceeds relayer_bps_cap should error
    #[test]
    fn relayer_fee_over_cap_errors(amount in 1u64..=u64::MAX, cap_bps in 1u16..=10_000u16, extra in 1u64..=1_000_000u64) {
        let cap_amt = (amount as u128 * cap_bps as u128) / 10_000u128;
    let relayer_fee = ((cap_amt + 1).min(u128::from(u64::MAX))) as u64 + (extra % 5);
        let res = compute_fees_and_forward(amount, 0, relayer_fee, cap_bps);
        prop_assert!(res.is_err());
    }

    // Combined fees exceeding amount should error
    #[test]
    fn total_fees_exceed_amount_errors(amount in 1u64..=u64::MAX, proto in 0u64..=u64::MAX, relay in 0u64..=u64::MAX) {
        // Ensure sum > amount, but constrain to u64 range
        let total = (proto as u128).saturating_add(relay as u128);
        prop_assume!(total > amount as u128);
        // No bps cap so that we test the total fees guard
        let res = compute_fees_and_forward(amount, proto, relay, 10_000);
        prop_assert!(res.is_err());
    }

    // Zero amount must error
    #[test]
    fn zero_amount_errors(protocol in 0u64..=u64::MAX, relayer in 0u64..=u64::MAX) {
        let res = compute_fees_and_forward(0, protocol, relayer, 10_000);
        prop_assert!(res.is_err());
    }

    // Payload length > 512 must error, <= 512 ok
    #[test]
    fn payload_len_guard(len in 0usize..=2048usize) {
        let res = validate_payload_len(len);
        if len <= 512 { prop_assert!(res.is_ok()); } else { prop_assert!(res.is_err()); }
    }
}
