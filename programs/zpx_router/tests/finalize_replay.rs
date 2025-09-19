// SPDX-License-Identifier: MIT
// Program-test that asserts `finalize_message_v1` creates a rent-exempt replay PDA and
// that the PDA remains owned and funded (durability vs rent-collection). This test is
// gated behind `--features program-test` to avoid pulling heavy dev-deps into normal
// developer runs.
#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use ::zpx_router::hash::{keccak256, message_hash_be};
use anchor_lang::prelude::*;
use anchor_lang::Discriminator; // needed for Replay::DISCRIMINATOR
use anchor_lang::ToAccountMetas;
use solana_program_test::*;
use solana_sdk::entrypoint::ProgramResult;
use solana_sdk::{
    instruction, signature::Keypair, signer::Signer, system_instruction, system_program,
    transaction::Transaction, transport::TransportError,
};

#[tokio::test]
#[ignore]
async fn finalize_replay_marks_and_prevents_reuse() -> std::result::Result<(), TransportError> {
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
    println!("TEST-DEBUG msg_hash={:?}", msg_hash);
    println!("TEST-DEBUG replay_pda={}", replay_pda);

    // Boot a program-test environment with the router program
    // wrapper to normalize the entry fn signature for ProgramTest expectations
    fn entry_wrapper(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
        // test-only shim: coerce lifetimes to match Anchor's expected signature
        let accounts_coerced: &[AccountInfo<'_>] = unsafe { std::mem::transmute(accounts) };
        zpx_router_program::entry(program_id, accounts_coerced, input)
    }

    let program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(entry_wrapper),
    );
    let mut ctx = program.start_with_context().await;

    // Initialize the config account via the program's `initialize_config` entrypoint so Anchor
    // correctly creates the PDA with the right owner and account data. Creating a PDA via
    // `system_instruction::create_account` would mark the new account as a required signer
    // (causing NotEnoughSigners), so we must use the program's init flow.
    let config_seed = b"zpx_config";
    let (config_pda, config_bump) =
        Pubkey::find_program_address(&[config_seed], &zpx_router_program::ID);
    // Create account data for config (small fixed size). We'll allocate and assign to the program.
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let config_space = 8 + 32 + 32 + 8 + 2 + 1 + (32 * 8) + 1 + 1;
    let payer = &ctx.payer;

    // Build and send the initialize_config instruction (program will `init` the PDA)
    let init_accounts = zpx_router_program::accounts::InitializeConfig {
        payer: payer.pubkey(),
        config: config_pda,
        system_program: system_program::id(),
    };
    let init_ix_data =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::InitializeConfig {
            admin: payer.pubkey(),
            fee_recipient: payer.pubkey(),
            src_chain_id,
            relayer_fee_bps: 0u16,
        });
    let init_metas = init_accounts.to_account_metas(None);
    let init_ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: init_metas,
        data: init_ix_data,
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Call finalize_message_v1
    let relayer = Keypair::new();
    // fund relayer
    let fund_ix = system_instruction::transfer(
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
    // First, add src_adapter to the config adapters list so finalize's allowlist check passes.
    let add_accounts = zpx_router_program::accounts::AdminConfig {
        authority: payer.pubkey(),
        config: config_pda,
    };
    let add_ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: add_accounts.to_account_metas(None),
        data: anchor_lang::InstructionData::data(&zpx_router_program::instruction::AddAdapter {
            adapter: src_adapter,
        }),
    };
    let tx_add = Transaction::new_signed_with_payer(
        &[add_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx_add).await?;

    // Fetch and decode the config account to assert the adapter was recorded.
    let cfg_acc = ctx
        .banks_client
        .get_account(config_pda)
        .await?
        .expect("config missing");
    // Decode with Anchor's AccountDeserialize
    use anchor_lang::AccountDeserialize;
    let cfg: zpx_router_program::Config =
        zpx_router_program::Config::try_deserialize(&mut cfg_acc.data.as_slice()).unwrap();
    println!(
        "DEBUG cfg.adapters_len={} adapter0={}",
        cfg.adapters_len, cfg.adapters[0]
    );

    let accounts = zpx_router_program::accounts::FinalizeMessageV1 {
        relayer: relayer.pubkey(),
        config: config_pda,
        replay: replay_pda,
        system_program: system_program::id(),
    };

    let ix_data =
        anchor_lang::InstructionData::data(&zpx_router_program::instruction::FinalizeMessageV1 {
            message_hash: msg_hash,
            src_chain_id,
            dst_chain_id,
            forwarded_amount,
            nonce,
            payload_hash,
            src_adapter,
            asset_mint,
            _initiator: initiator,
        });
    println!("TEST-DEBUG ix_data_len={}", ix_data.len());
    let account_metas = accounts.to_account_metas(None);
    // (Debug output removed for CI stability)
    let instruction = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: account_metas,
        data: ix_data.clone(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&payer.pubkey()),
        &[payer, &relayer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await?;

    // Fetch replay account and assert lamports >= rent.minimum_balance(1)
    // Poll for the replay account for a few slots to tolerate transient ledger visibility
    let mut replay_account_opt = None;
    for _ in 0..5u8 {
        if let Some(acc) = ctx.banks_client.get_account(replay_pda).await? {
            replay_account_opt = Some(acc);
            break;
        }
        // Inject a tiny barrier transaction to advance slot/commit state
        let noop_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
        let noop_tx = Transaction::new_signed_with_payer(
            &[noop_ix],
            Some(&payer.pubkey()),
            &[payer],
            ctx.last_blockhash,
        );
        ctx.banks_client.process_transaction(noop_tx).await?;
    }
    let replay_account = replay_account_opt.expect("replay account missing after retries");
    let min = rent.minimum_balance(9);
    assert!(
        replay_account.lamports >= min,
        "replay not funded as expected"
    );
    assert_eq!(
        replay_account.owner,
        zpx_router_program::ID,
        "replay owner mismatch"
    );
    assert!(replay_account.data.len() >= 9, "replay data too small");
    let expected_disc = zpx_router_program::Replay::discriminator();
    assert_eq!(
        &replay_account.data[0..8],
        &expected_disc,
        "discriminator mismatch"
    );
    assert_eq!(replay_account.data[8], 1u8, "processed flag not set");

    // Ensure the ledger state is committed and the runtime sees the created PDA.
    // Inject a tiny no-op transaction (transfer 1 lamport payer->payer) as a barrier.
    let noop_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
    let noop_tx = Transaction::new_signed_with_payer(
        &[noop_ix],
        Some(&payer.pubkey()),
        &[payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(noop_tx).await?;

    // Second call should fail with ReplayAlreadyUsed. Rebuild the instruction so
    // account metas are regenerated and the runtime resolves the current account state.
    // IMPORTANT: Refresh blockhash to avoid banks_client treating identical tx as duplicate (which would short-circuit execution).
    ctx.last_blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let account_metas2 = zpx_router_program::accounts::FinalizeMessageV1 {
        relayer: relayer.pubkey(),
        config: config_pda,
        replay: replay_pda,
        system_program: system_program::id(),
    }
    .to_account_metas(None);
    let instruction2 = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: account_metas2,
        data: ix_data.clone(),
    };
    let tx2 = Transaction::new_signed_with_payer(
        &[instruction2],
        Some(&payer.pubkey()),
        &[payer, &relayer],
        ctx.last_blockhash,
    );
    let res2 = ctx.banks_client.process_transaction(tx2).await;
    match res2 {
        Ok(_) => panic!("second finalize should fail due to replay"),
        Err(BanksClientError::TransactionError(
            solana_sdk::transaction::TransactionError::InstructionError(
                _,
                solana_sdk::instruction::InstructionError::Custom(code),
            ),
        )) => {
            assert_eq!(
                code, 6019,
                "unexpected custom error code (expected ReplayAlreadyProcessed=6019)"
            );
        }
        Err(e) => panic!("unexpected error variant: {:?}", e),
    }

    Ok(())
}
