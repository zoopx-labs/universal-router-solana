// SPDX-License-Identifier: MIT
#![cfg(feature = "program-test")]

use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::{
    signature::Keypair, signer::Signer, transaction::Transaction, transport::TransportError,
};

use ::zpx_router as zpx_router_program;

#[tokio::test]
#[ignore]
async fn unauthorized_admin_calls_are_rejected() -> Result<(), TransportError> {
    // Boot program-test environment
    let program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(zpx_router_program::entry),
    );
    let mut ctx = program.start_with_context().await;

    // Create config PDA so add_adapter constraint can find it
    let config_seed = b"zpx_config";
    let (config_pda, _bump) = Pubkey::find_program_address(&[config_seed], &zpx_router_program::ID);

    // create config account (owned by program) to satisfy account constraints (small allocation)
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let config_space = 8 + 32 + 32 + 8 + 2 + 1 + (32 * 8) + 1 + 1;
    let config_lamports = rent.minimum_balance(config_space);
    let payer = &ctx.payer;
    let create_cfg_ix = solana_program::system_instruction::create_account(
        &payer.pubkey(),
        &config_pda,
        config_lamports,
        config_space as u64,
        &zpx_router_program::ID,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_cfg_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Non-admin signer attempts to call add_adapter
    let rogue = Keypair::new();
    // fund rogue
    let fund_ix = solana_program::system_instruction::transfer(
        &payer.pubkey(),
        &rogue.pubkey(),
        rent.minimum_balance(0) + 1_000_000,
    );
    let tx = Transaction::new_signed_with_payer(
        &[fund_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Build add_adapter instruction
    let adapter_to_add = Pubkey::new_unique();
    let ix_data =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::AddAdapter {
            adapter: adapter_to_add,
        });
    let accounts = zpx_router_program::accounts::AdminConfig {
        authority: rogue.pubkey(),
        config: config_pda,
    };
    let metas = accounts.to_account_metas(None);
    let instruction = solana_program::instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: metas,
        data: ix_data,
    };

    // Send transaction signed by rogue (not the configured admin) — expect failure
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&rogue.pubkey()),
        &[&rogue],
        ctx.last_blockhash,
    );
    let res = ctx.banks_client.process_transaction(tx).await;

    assert!(res.is_err(), "unauthorized admin call should fail");

    Ok(())
}
