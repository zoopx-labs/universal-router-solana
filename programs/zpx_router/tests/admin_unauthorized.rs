// SPDX-License-Identifier: MIT
#![cfg(feature = "program-test")]

use anchor_lang::prelude::*;
use anchor_lang::ToAccountMetas;
use solana_program_test::*;
use solana_sdk::{
    entrypoint::ProgramResult, instruction, signature::Keypair, signer::Signer, system_instruction,
    system_program, transaction::Transaction, transport::TransportError,
};

use ::zpx_router as zpx_router_program;

#[tokio::test]
#[ignore]
async fn unauthorized_admin_calls_are_rejected() -> std::result::Result<(), TransportError> {
    // Boot program-test environment
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

    // Create config PDA so add_adapter constraint can find it
    let config_seed = b"zpx_config";
    let (config_pda, _bump) = Pubkey::find_program_address(&[config_seed], &zpx_router_program::ID);

    // Initialize config account using the program's `initialize_config` so Anchor creates
    // the PDA and account data correctly. This ensures subsequent AdminConfig checks
    // will operate on a properly initialized account.
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let payer = &ctx.payer;
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

    // Non-admin signer attempts to call add_adapter
    let rogue = Keypair::new();
    // fund rogue
    let fund_ix = system_instruction::transfer(
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
    use solana_sdk::instruction::AccountMeta as SdkAccountMeta;

    // Reconstruct the Anchor accounts struct so we can derive account metas
    let accounts = zpx_router_program::accounts::AdminConfig {
        authority: rogue.pubkey(),
        config: config_pda,
    };

    // Convert Anchor account metas to solana_sdk AccountMeta preserving signer/writable flags
    let anchor_metas = accounts.to_account_metas(None);
    let metas: Vec<SdkAccountMeta> = anchor_metas
        .into_iter()
        .map(|m| {
            if m.is_writable {
                SdkAccountMeta::new(m.pubkey, m.is_signer)
            } else {
                SdkAccountMeta::new_readonly(m.pubkey, m.is_signer)
            }
        })
        .collect();

    let instruction = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: metas,
        data: ix_data,
    };

    // Send transaction signed by both payer and rogue (rogue is not the configured admin) — expect program-level failure
    let mut tx = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
    tx.sign(&[payer, &rogue], ctx.last_blockhash);
    let res = ctx.banks_client.process_transaction(tx).await;

    // The program should return an error due to unauthorized admin (rogue isn't the configured admin)
    assert!(res.is_err(), "unauthorized admin call should fail");

    Ok(())
}
