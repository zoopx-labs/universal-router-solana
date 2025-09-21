use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account as SolAccount, pubkey::Pubkey, signature::Keypair, signer::Signer,
    system_instruction, transaction::Transaction, transport::TransportError,
};

// A minimal mock adapter entrypoint that returns Ok when called with data[0]==0
// and returns an error otherwise. We'll register it as a program in ProgramTest.
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey as SPubkey,
};

#[allow(unused)]
fn mock_adapter_process(
    _program_id: &SPubkey,
    _accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    msg!("mock_adapter invoked with len={}", data.len());
    if data.len() > 0 && data[0] == 0u8 {
        Ok(())
    } else {
        Err(solana_program::program_error::ProgramError::Custom(1))
    }
}

#[tokio::test]
async fn adapter_cpi_invocation_roundtrip() -> std::result::Result<(), TransportError> {
    let program_id = zpx_router::ID;
    // Use the real adapter program id declared in the crate
    let adapter_program_id = Pubkey::new_unique();

    // Create ProgramTest and register zpx_router and the mock adapter program
    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));
    // Register mock adapter as a program with a simple processor
    program_test.add_program(
        "mock_adapter",
        adapter_program_id,
        processor!(mock_adapter_process),
    );

    // Start environment
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Build instruction to call bridge_with_adapter_cpi with adapter_program = adapter_program_id
    let ix = Instruction {
        program_id,
        accounts: vec![solana_program::instruction::AccountMeta::new_readonly(
            adapter_program_id,
            false,
        )],
        data: zpx_router::instruction::BridgeWithAdapterCpi {}.data(),
    };

    // 1) Case: adapter program exists but its CPI call inside bridge_with_adapter_cpi will attempt to invoke the adapter
    // The router's bridge_with_adapter_cpi builds an instruction with data vec![0u8], which our mock accepts.
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // 2) Negative case: call adapter_passthrough with a failing payload (non-zero) -- but adapter_passthrough is separate entrypoint.
    let fail_ix = Instruction {
        program_id,
        accounts: vec![solana_program::instruction::AccountMeta::new_readonly(
            adapter_program_id,
            false,
        )],
        data: zpx_router::instruction::AdapterPassthrough {
            instruction_data: vec![1u8, 2u8],
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[fail_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    // This should return an error (adapter returns custom error) -> process_transaction will error
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());

    Ok(())
}
