// SPDX-License-Identifier: MIT
#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::{
    instruction,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
    system_program, // needed for system_program::id()
};

// Ignored heavy integration test that asserts finalize_message_v1 rejects unknown adapters
#[tokio::test]
#[ignore]
async fn finalize_rejects_unknown_adapter() -> Result<()> {
    fn entry_wrapper(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        input: &[u8],
    ) -> solana_sdk::entrypoint::ProgramResult {
        let accounts_coerced: &[AccountInfo<'_>] = unsafe { std::mem::transmute(accounts) };
        zpx_router_program::entry(program_id, accounts_coerced, input)
    }

    let program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(entry_wrapper),
    );
    let mut ctx = program.start_with_context().await;
    let payer = &ctx.payer;

    // init config with no adapters
    let (config_pda, _bump) =
        Pubkey::find_program_address(&[b"zpx_config"], &zpx_router_program::ID);
    let init_accounts = zpx_router_program::accounts::InitializeConfig {
        payer: payer.pubkey(),
        config: config_pda,
        system_program: system_program::id(),
    };
    let init_ix_data =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::InitializeConfig {
            admin: payer.pubkey(),
            fee_recipient: payer.pubkey(),
            src_chain_id: 1u64,
            relayer_fee_bps: 0u16,
        });
    let init_ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: init_accounts.to_account_metas(None),
        data: init_ix_data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Build finalize call with an adapter that is NOT in the config adapters (should be rejected)
    let src_chain_id = 1u64;
    let dst_chain_id = 2u64;
    let forwarded_amount = 1_000u64;
    let nonce = 7u64;
    let payload_hash = [0u8; 32];
    let src_adapter = Pubkey::new_unique(); // not added to config
    let asset_mint = Pubkey::new_unique();
    let initiator = Pubkey::new_unique();

    // Compute expected message hash for deterministic replay PDA (hash parity not critical for rejection path)
    let message_hash = [0u8; 32];
    let (replay_pda, _rbump) =
        Pubkey::find_program_address(&[b"replay", &message_hash], &zpx_router_program::ID);

    let relayer = Keypair::new();
    // fund relayer
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let fund_ix = solana_sdk::system_instruction::transfer(
        &payer.pubkey(),
        &relayer.pubkey(),
        rent.minimum_balance(0) + 1_000_000,
    );
    let tx2 = Transaction::new_signed_with_payer(
        &[fund_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx2).await.unwrap();

    let accounts = zpx_router_program::accounts::FinalizeMessageV1 {
        relayer: relayer.pubkey(),
        config: config_pda,
        replay: replay_pda,
        system_program: system_program::id(),
    };
    let ix_data =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::FinalizeMessageV1 {
            message_hash,
            src_chain_id,
            dst_chain_id,
            forwarded_amount,
            nonce,
            payload_hash,
            src_adapter,
            asset_mint,
            _initiator: initiator,
        });
    let ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: accounts.to_account_metas(None),
        data: ix_data,
    };
    let tx3 = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, &relayer],
        ctx.last_blockhash,
    );
    let res = ctx.banks_client.process_transaction(tx3).await;
    assert!(res.is_err(), "finalize should reject unknown adapter");
    Ok(())
}
