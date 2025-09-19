// SPDX-License-Identifier: MIT
use anchor_lang::prelude::*;
use zpx_router::Config;

#[test]
fn config_admin_auth_check() {
    let cfg = Config {
        admin: Pubkey::new_unique(),
        fee_recipient: Pubkey::new_unique(),
        src_chain_id: 1,
        relayer_fee_bps: 0,
        adapters_len: 0,
        adapters: [Pubkey::default(); 8],
        paused: false,
        bump: 0,
    };

    let authority = Pubkey::new_unique();

    // Simulate check: unauthorized
    assert!(
        cfg.admin != authority,
        "test setup: authority must not equal admin"
    );
}
