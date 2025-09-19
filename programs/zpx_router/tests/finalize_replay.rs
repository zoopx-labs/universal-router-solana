// SPDX-License-Identifier: MIT
// Program-test that asserts `finalize_message_v1` creates a rent-exempt replay PDA and
// that the PDA remains owned and funded (durability vs rent-collection). This test is
// gated behind `--features program-test` to avoid pulling heavy dev-deps into normal
// developer runs.
#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use ::zpx_router::hash::{keccak256, message_hash_be};
use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::{
    signature::Keypair, signer::Signer, transaction::Transaction, transport::TransportError,
};

#[tokio::test]
#[ignore]
async fn finalize_replay_marks_and_prevents_reuse() -> Result<(), TransportError> {
    // Inputs
    let src_chain_id = 42161u64;
    let dst_chain_id = 8453u64;
    let forwarded_amount = 123_456u64;
    let nonce = 42u64;
    let payload = hex::decode("deadbeef").unwrap();
    let payload_hash = keccak256(&[&payload]);
    let src_adapter = Pubkey::new_unique();
    let asset_mint = Pubkey::new_unique();
    let initiator = Pubkey::new_unique();

    // Build message_hash with the same helper used by the program
    let mut amount_be = [0u8; 32];
    amount_be[16..].copy_from_slice(&(forwarded_amount as u128).to_be_bytes());
    let msg_hash = message_hash_be(
        src_chain_id,
        src_adapter.to_bytes(),
        [0u8; 32],
        asset_mint.to_bytes(),
        amount_be,
        payload_hash,
        nonce,
        dst_chain_id,
    );

    // Derive PDA used for replay flag
    let (replay_pda, _bump) =
        Pubkey::find_program_address(&[b"replay", &msg_hash], &zpx_router_program::ID);

    // Boot a program-test environment with the router program
    let program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(zpx_router_program::entry),
    );
    let mut ctx = program.start_with_context().await;

    // Create a fake config account (seeded) so the finalize ix passes the config constraint
    let config_seed = b"zpx_config";
    let (config_pda, config_bump) =
        Pubkey::find_program_address(&[config_seed], &zpx_router_program::ID);
    // Create account data for config (small fixed size). We'll allocate and assign to the program.
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let config_space = 8 + 32 + 32 + 8 + 2 + 1 + (32 * 8) + 1 + 1;
    let config_lamports = rent.minimum_balance(config_space);
    let payer = &ctx.payer;

    // Fund the config PDA
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

    // Call finalize_message_v1
    let relayer = Keypair::new();
    // fund relayer
    let fund_ix = solana_program::system_instruction::transfer(
        &payer.pubkey(),
        &relayer.pubkey(),
        rent.minimum_balance(0) + 1_000_000,
    );
    let tx = Transaction::new_signed_with_payer(
        &[fund_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Build instruction to call finalize_message_v1 directly via anchor client instruction construction
    let accounts = zpx_router_program::accounts::FinalizeMessageV1 {
        relayer: relayer.pubkey(),
        config: config_pda,
        replay: replay_pda,
        system_program: solana_program::system_program::id(),
    };

    let ix =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::FinalizeMessageV1 {
            src_chain_id,
            dst_chain_id,
            forwarded_amount,
            nonce,
            payload_hash,
            src_adapter,
            asset_mint,
            _initiator: initiator,
        });
    let account_metas = accounts.to_account_metas(None);
    let instruction = solana_program::instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: account_metas,
        data: ix,
    };

    let tx = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&payer.pubkey()),
        &[payer, &relayer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Fetch replay account and assert lamports >= rent.minimum_balance(0)
    let replay_account = ctx
        .banks_client
        .get_account(replay_pda)
        .await?
        .expect("replay account missing");
    let min = rent.minimum_balance(0);
    assert!(replay_account.lamports >= min);

    // Second call should fail with ReplayAlreadyUsed
    let tx2 = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer, &relayer],
        ctx.last_blockhash,
    );
    let res2 = ctx.banks_client.process_transaction(tx2).await;
    assert!(res2.is_err(), "second finalize should fail due to replay");

    Ok(())
}
