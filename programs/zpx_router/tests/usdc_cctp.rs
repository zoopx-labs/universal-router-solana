use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
    transaction::Transaction, transport::TransportError,
};

#[tokio::test]
async fn usdc_only_spoke_rejects_other_mint() -> std::result::Result<(), TransportError> {
    let router_program_id = zpx_router::ID;
    let mut program_test = ProgramTest::new(
        "zpx_router",
        router_program_id,
        processor!(zpx_router::entry),
    );
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );

    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Set up two mints: usdc_mint and other_mint
    let usdc_kp = Keypair::new();
    let other_kp = Keypair::new();
    let usdc_mint = usdc_kp.pubkey();
    let other_mint = other_kp.pubkey();

    // Initialize config with allowed_token_mint = usdc_mint
    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &router_program_id);
    let init_cfg_ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::InitializeConfig {
            admin: payer.pubkey(),
            fee_recipient: payer.pubkey(),
            src_chain_id: 1u64,
            relayer_fee_bps: 0u16,
            protocol_fee_bps: 0u16,
            relayer_pubkey: payer.pubkey(),
            accept_any_token: false,
            allowed_token_mint: usdc_mint,
            direct_relayer_payout_default: false,
            min_forward_amount: 0u64,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_cfg_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Attempt to forward using other_mint (should be rejected)
    let ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new_readonly(other_mint, false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::UniversalBridgeTransfer {
            amount: 1u64,
            protocol_fee: 0u64,
            relayer_fee: 0u64,
            payload: vec![],
            dst_chain_id: 0u64,
            nonce: 0u64,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());
    Ok(())
}

#[tokio::test]
async fn cctp_spoke_shapes_smoke() -> std::result::Result<(), TransportError> {
    // This test is a scaffold that verifies we can create spokes intended for CCTP v1 and v2
    // and ensures the router accepts their creation. Detailed payload parsing remains in adapter tests.
    let router_program_id = zpx_router::ID;
    let mut program_test = ProgramTest::new(
        "zpx_router",
        router_program_id,
        processor!(zpx_router::entry),
    );
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &router_program_id);
    let (registry_pda, _rbump) =
        Pubkey::find_program_address(&[b"hub_registry"], &router_program_id);

    let init_cfg_ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::InitializeConfig {
            admin: payer.pubkey(),
            fee_recipient: payer.pubkey(),
            src_chain_id: 1u64,
            relayer_fee_bps: 0u16,
            protocol_fee_bps: 0u16,
            relayer_pubkey: payer.pubkey(),
            accept_any_token: true,
            allowed_token_mint: Pubkey::new_unique(),
            direct_relayer_payout_default: false,
            min_forward_amount: 0u64,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_cfg_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    let init_reg_ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::InitializeRegistry {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[init_reg_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Create spokes for v1 and v2 (no adapter behavior asserted here)
    let create_spoke_v1 = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(Pubkey::new_unique(), false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::CreateSpoke {
            spoke_id: 1u32,
            adapter_program: payer.pubkey(),
            direct_relayer_payout: false,
            version: 1u8,
            metadata: None,
        }
        .data(),
    };
    let create_spoke_v2 = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(Pubkey::new_unique(), false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::CreateSpoke {
            spoke_id: 2u32,
            adapter_program: payer.pubkey(),
            direct_relayer_payout: false,
            version: 2u8,
            metadata: None,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[create_spoke_v1, create_spoke_v2],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    Ok(())
}
