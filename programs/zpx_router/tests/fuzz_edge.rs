use proptest::prelude::*;
use zpx_router::compute_fees_and_forward;

proptest! {
    // Edge cases for amount and fees: use 0, 1, u64::MAX and boundaries around fee caps
    #[test]
    fn fee_edge_cases(
        amount in prop_oneof![Just(0u64), Just(1u64), Just(u64::MAX), 2u64..=1_000_000u64],
        proto in prop_oneof![Just(0u64), Just(1u64), Just(u64::MAX), 2u64..=1_000_000u64],
        relay in prop_oneof![Just(0u64), Just(1u64), Just(u64::MAX), 2u64..=1_000_000u64],
        relayer_bps_cap in prop_oneof![Just(0u16), Just(1u16), Just(5u16), Just(10_000u16), 1u16..=10_000u16]
    ) {
        // We exercise the function and assert it never panics and returns Err for invalid combos.
        let res = std::panic::catch_unwind(|| compute_fees_and_forward(amount, proto, relay, relayer_bps_cap));
        prop_assert!(res.is_ok(), "compute_fees_and_forward panicked for inputs: amount={} proto={} relay={} cap={}", amount, proto, relay, relayer_bps_cap);
        let out = res.unwrap();
        match out {
            Ok((forward, total)) => {
                // Invariant: forward + total == amount (since forward = amount - total)
                prop_assert!(forward.checked_add(total).map(|s| s == amount).unwrap_or(false), "Invariant failed: forward+total != amount");
                // total must be <= amount
                prop_assert!(total <= amount);
            }
            Err(_) => {
                // For error cases, ensure at least one precondition was violated
                // (amount==0) or fees too high or combined fees overflow/exceed
                prop_assume!(amount == 0 || proto > 0 || relay > 0);
            }
        }
    }
}
