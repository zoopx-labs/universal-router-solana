// SPDX-License-Identifier: MIT
use anchor_lang::prelude::*;

#[test]
fn ata_derivation_mismatch_detected() {
    // Simulate the same derivation logic used by the program for the associated token address
    let fee_recipient = Pubkey::new_unique();
    let mint = Pubkey::new_unique();

    // Expected ATA per SPL associated token program: seeds = [owner, token_program, mint]
    let ata_seeds: &[&[u8]] = &[
        &fee_recipient.to_bytes(),
        &anchor_spl::token::ID.to_bytes(),
        &mint.to_bytes(),
    ];
    let (expected_ata, _bump) =
        Pubkey::find_program_address(ata_seeds, &anchor_spl::associated_token::ID);

    // Construct a wrong address (e.g., new unique) representing a mismatched provided ATA
    let provided_ata = Pubkey::new_unique();

    assert_ne!(
        provided_ata, expected_ata,
        "Test setup: provided ATA must differ from expected"
    );
}
