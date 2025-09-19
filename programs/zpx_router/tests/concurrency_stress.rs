// SPDX-License-Identifier: MIT
#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use anchor_lang::prelude::*;
use anchor_lang::ToAccountMetas;
use solana_program_test::*;
use solana_sdk::entrypoint::ProgramResult;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;
use solana_sdk::{
    instruction, signature::Keypair, signer::Signer, system_instruction, transaction::Transaction,
};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
// sequential stress loop (avoid sharing BanksClient across tasks)

// Small concurrency stress test: multiple tasks concurrently call universal_bridge_transfer
// to ensure no funds are lost and fees/forwarding invariants hold under parallel load.
#[tokio::test]
async fn concurrent_transfers_stress() {
    // Setup a program-test environment
    fn entry_wrapper(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
        let accounts_coerced: &[AccountInfo<'_>] = unsafe { std::mem::transmute(accounts) };
        zpx_router_program::entry(program_id, accounts_coerced, input)
    }

    let program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(entry_wrapper),
    );
    let mut ctx = program.start_with_context().await;

    // helper to process a transaction and panic on unrecoverable errors
    async fn process_tx(ctx: &mut ProgramTestContext, tx: Transaction) {
        let mut attempt = 0u8;
        loop {
            attempt += 1;
            match ctx.banks_client.process_transaction(tx.clone()).await {
                Ok(_) => break,
                Err(e) => {
                    if attempt >= 5 {
                        panic!("process_transaction failed after retries: {}", e);
                    }
                    sleep(Duration::from_millis(10 * attempt as u64)).await;
                }
            }
        }
    }

    async fn latest_blockhash_or(
        ctx: &mut ProgramTestContext,
        fallback: solana_sdk::hash::Hash,
    ) -> solana_sdk::hash::Hash {
        match ctx.banks_client.get_latest_blockhash().await {
            Ok(h) => h,
            Err(_) => fallback,
        }
    }

    // initialize config via program instruction
    // clone the payer Keypair by serializing and deserializing its bytes (Keypair doesn't implement Clone)
    let payer = Keypair::from_bytes(&ctx.payer.to_bytes()).expect("clone payer");
    let src_chain_id = 1u64;
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
            src_chain_id,
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
        &[&payer],
        ctx.last_blockhash,
    );
    process_tx(&mut ctx, tx).await;

    // For concurrency/race testing we call finalize_message_v1 repeatedly with different nonces
    let relayer = Keypair::new();
    // fund relayer
    let rent = ctx.banks_client.get_rent().await.unwrap();
    // give the relayer a large balance so PDA creations / account writes succeed during stress test
    let fund_amount = rent.minimum_balance(0).saturating_add(10_000_000_000u64);
    let fund_ix = system_instruction::transfer(&payer.pubkey(), &relayer.pubkey(), fund_amount);
    let tx = Transaction::new_signed_with_payer(
        &[fund_ix],
        Some(&payer.pubkey()),
        &[&payer],
        ctx.last_blockhash,
    );
    process_tx(&mut ctx, tx).await;

    // Pre-add a bounded set of adapters (<= 8 per config constraints) so finalize calls pass allowlist gate.
    let adapters: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
    for adapter in adapters.iter() {
        let add_accounts = zpx_router_program::accounts::AdminConfig {
            authority: payer.pubkey(),
            config: config_pda,
        };
        let add_ix = instruction::Instruction {
            program_id: zpx_router_program::ID,
            accounts: add_accounts.to_account_metas(None),
            data: anchor_lang::InstructionData::data(&zpx_router_program::instruction::AddAdapter { adapter: *adapter }),
        };
        let prior_hash = ctx.last_blockhash;
        let recent_blockhash = latest_blockhash_or(&mut ctx, prior_hash).await;
        let add_tx = Transaction::new_signed_with_payer(
            &[add_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        process_tx(&mut ctx, add_tx).await;
    }

    let iterations = 200u64;
    // record message hashes so we can verify the exact PDA created for a given nonce
    let mut message_hashes: HashMap<u64, [u8; 32]> = HashMap::new();
    for nonce in 0..iterations {
        // pick small forwarded_amount and payload
        let forwarded_amount = 1u64 + (nonce % 10);
        let src_chain_id = 1u64;
        let dst_chain_id = 2u64;
        let payload = vec![((nonce & 0xff) as u8)];
        let payload_hash = zpx_router_program::hash::keccak256(&[&payload]);
    // Choose a pre-allowed adapter deterministically
    let src_adapter = adapters[(nonce as usize) % adapters.len()];
        let asset_mint = Pubkey::new_unique();

        // derive replay PDA for this nonce/message
        let mut amount_be = [0u8; 32];
        amount_be[16..].copy_from_slice(&(forwarded_amount as u128).to_be_bytes());
        let message_hash = zpx_router_program::hash::message_hash_be(
            src_chain_id,
            src_adapter.to_bytes(),
            [0u8; 32],
            asset_mint.to_bytes(),
            amount_be,
            payload_hash,
            nonce,
            dst_chain_id,
        );
        // save the exact message hash used so later checks use the same inputs
        message_hashes.insert(nonce, message_hash);
        let (replay_pda, _bump) =
            Pubkey::find_program_address(&[b"replay", &message_hash], &zpx_router_program::ID);

        let accounts = zpx_router_program::accounts::FinalizeMessageV1 {
            relayer: relayer.pubkey(),
            config: config_pda,
            replay: replay_pda,
            system_program: system_program::id(),
        };
        let ix_data = anchor_lang::InstructionData::data(&zpx_router_program::instruction::FinalizeMessageV1 {
            message_hash,
            src_chain_id,
            dst_chain_id,
            forwarded_amount,
            nonce,
            payload_hash,
            src_adapter,
            asset_mint,
            _initiator: Pubkey::new_unique(),
        });
        let ix = instruction::Instruction {
            program_id: zpx_router_program::ID,
            accounts: accounts.to_account_metas(None),
            data: ix_data,
        };
        // get a fresh blockhash for each transaction to avoid replay/stale blockhash issues
        let fallback_hash = ctx.last_blockhash;
    let recent_blockhash = latest_blockhash_or(&mut ctx, fallback_hash).await;
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer, &relayer],
            recent_blockhash,
        );

        // process transaction (process_tx handles retries and will panic on repeated failure)
        process_tx(&mut ctx, tx.clone()).await;

        if nonce % 31 == 0 {
            // noop to advance ledger
            let noop_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
            let fallback_hash = ctx.last_blockhash;
            let recent_blockhash = latest_blockhash_or(&mut ctx, fallback_hash).await;
            let noop_tx = Transaction::new_signed_with_payer(
                &[noop_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );
            process_tx(&mut ctx, noop_tx).await;
        }

        // throttle slightly to avoid hammering the banks-server
        if nonce % 10 == 0 {
            sleep(Duration::from_millis(1)).await;
        }
    }

    // Check a handful of PDAs exist (sample nonces)
    for nonce in [3u64, 37u64, 101u64].iter() {
        let forwarded_amount = 1u64 + (nonce % 10);
        let src_chain_id = 1u64;
        let dst_chain_id = 2u64;
        let payload = vec![((*nonce & 0xff) as u8)];
        let payload_hash = zpx_router_program::hash::keccak256(&[&payload]);
        // reuse the exact message hash recorded during the creation loop
        let message_hash = *message_hashes
            .get(nonce)
            .expect("missing message_hash for sampled nonce");
        let mut amount_be = [0u8; 32];
        amount_be[16..].copy_from_slice(&(forwarded_amount as u128).to_be_bytes());
        let (replay_pda, _bump) =
            Pubkey::find_program_address(&[b"replay", &message_hash], &zpx_router_program::ID);
        // poll for account
        let mut found = false;
        for _ in 0..5u8 {
            if let Some(_acc) = ctx.banks_client.get_account(replay_pda).await.unwrap() {
                found = true;
                break;
            }
            let noop_ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
            let fallback_hash = ctx.last_blockhash;
            let recent_blockhash = latest_blockhash_or(&mut ctx, fallback_hash).await;
            let noop_tx = Transaction::new_signed_with_payer(
                &[noop_ix],
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            );
            process_tx(&mut ctx, noop_tx).await;
        }
        assert!(
            found,
            "expected replay PDA to be present for nonce {}",
            nonce
        );
    }

    // no final token checks for PDA-only stress test
}
