#![cfg(feature = "program-test")]

use ::zpx_router as zpx_router_program;
use anchor_lang::prelude::*;
use solana_program_test::*;
use solana_sdk::system_program;
use solana_sdk::{instruction, signature::Keypair, signer::Signer, transaction::Transaction};

// Basic CPI failure test: router calls the adapter which returns an error; router should propagate
#[tokio::test]
async fn cpi_failure_propagates() {
    // Wrap entry as native processor
    fn entry_wrapper(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        input: &[u8],
    ) -> solana_sdk::entrypoint::ProgramResult {
        let accounts_coerced: &[AccountInfo<'_>] = unsafe { std::mem::transmute(accounts) };
        zpx_router_program::entry(program_id, accounts_coerced, input)
    }

    // Create an inline mock adapter processor that always fails. Register it with ProgramTest.
    let mock_program_id = "MockCpi111111111111111111111111111111111111"
        .parse()
        .unwrap();

    // Inline failing entry for mock adapter
    fn mock_adapter_entry(
        _program_id: &Pubkey,
        _accounts: &[solana_sdk::account_info::AccountInfo],
        _input: &[u8],
    ) -> solana_sdk::entrypoint::ProgramResult {
        // Return a custom error so CPI fails
        Err(anchor_lang::solana_program::program_error::ProgramError::Custom(0xDEAD))
    }

    let mut program = ProgramTest::new(
        "zpx_router",
        zpx_router_program::ID,
        processor!(entry_wrapper),
    );
    program.add_program("mock_cpi", mock_program_id, processor!(mock_adapter_entry));

    let mut ctx = program.start_with_context().await;

    let payer = Keypair::from_bytes(&ctx.payer.to_bytes()).expect("clone payer");

    // Build accounts and instruction for BridgeWithAdapterCpi
    let accounts = zpx_router_program::accounts::BridgeWithAdapterCpi {
        adapter_program: mock_program_id,
    };
    let ix_data = anchor_lang::InstructionData::data(
        &zpx_router_program::instruction::BridgeWithAdapterCpi {},
    );
    let ix = instruction::Instruction {
        program_id: zpx_router_program::ID,
        accounts: accounts.to_account_metas(None),
        data: ix_data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        ctx.last_blockhash,
    );
    // Expect processing to fail (adapter returns error). process_transaction returns Err.
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(
        res.is_err(),
        "expected CPI to failing adapter to cause transaction failure"
    );
}
