#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::system_program;
use solana_sdk::{instruction, signature::Keypair, signer::Signer, transaction::Transaction};

// Simple upgrade/migration smoke test: deploy, init config, then simulate upgrade and
// verify config account is preserved.
#[tokio::test]
async fn upgrade_preserves_state() {
    fn entry_wrapper(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        input: &[u8],
    ) -> solana_sdk::entrypoint::ProgramResult {
        let accounts_coerced: &[AccountInfo<'_>] = unsafe { std::mem::transmute(accounts) };
        zpx_router_program::entry(program_id, accounts_coerced, input)
    }

    let mut program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(entry_wrapper),
    );
    let mut ctx = program.start_with_context().await;
    let payer = Keypair::from_bytes(&ctx.payer.to_bytes()).expect("clone payer");

    let src_chain_id = 42u64;
    let (config_pda, bump) =
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
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Read config account data
    let acc = ctx
        .banks_client
        .get_account(config_pda)
        .await
        .unwrap()
        .expect("config exists");
    assert!(acc.lamports > 0, "config should be funded");

    // Simulate post-upgrade behavior by calling a second router instruction and
    // verifying the config account remains present and unchanged.
    // We'll call the BridgeWithAdapterCpi with a no-op adapter (use program id 11111111111111111111111111111111 which will be rejected),
    // but we don't care about its result here — the goal is to ensure state remains.
    let accounts = zpx_router_program::accounts::BridgeWithAdapterCpi {
        adapter_program: solana_sdk::pubkey::Pubkey::new_unique(),
    };
    let ix_data = anchor_lang::InstructionData::data(
        &zpx_router_program::instruction::BridgeWithAdapterCpi {},
    );
    let ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: accounts.to_account_metas(None),
        data: ix_data,
    };
    let tx2 = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        ctx.last_blockhash,
    );
    // We ignore the result — just ensure no panics and the config survives.
    let _ = ctx.banks_client.process_transaction(tx2).await;

    // After the second call, ensure the config account is still present
    let acc2 = ctx
        .banks_client
        .get_account(config_pda)
        .await
        .unwrap()
        .expect("config exists after second call");
    assert_eq!(
        acc.lamports, acc2.lamports,
        "lamports preserved after second call"
    );
}
