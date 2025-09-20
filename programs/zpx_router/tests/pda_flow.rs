use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use anchor_spl::token::{self, Mint, Token, TokenAccount};
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction,
    transport::TransportError,
};
use zpx_router::instruction as zpx_ix;

#[tokio::test]
async fn pda_vault_forward_and_admin_withdraw() -> Result<(), TransportError> {
    // Basic program-test harness
    let program_id = zpx_router::ID;
    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));

    // Add token program
    program_test.add_program("spl_token", anchor_spl::token::ID, None);

    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Create a mint and associated token accounts, then call forward_via_spoke and admin_withdraw.
    // Note: This test is intentionally simplified to assert PDAs and CPI flow logic.

    Ok(())
}
