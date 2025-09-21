use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
    transaction::Transaction, transport::TransportError,
};

#[tokio::test]
async fn registry_create_and_duplicate_and_unauthorized() -> std::result::Result<(), TransportError>
{
    let program_id = zpx_router::ID;
    let program_test = ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));

    // Start test environment
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Derive PDAs
    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &program_id);
    let (registry_pda, _rbump) = Pubkey::find_program_address(&[b"hub_registry"], &program_id);

    // 1) Initialize config as payer
    let init_cfg_ix = Instruction {
        program_id,
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
            relayer_fee_bps: 100u16,
            protocol_fee_bps: 5u16,
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

    // 2) Initialize registry
    let init_reg_ix = Instruction {
        program_id,
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

    // 3) Create spoke (success)
    let create_spoke_ix = Instruction {
        program_id,
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
            spoke_id: 42u32,
            adapter_program: payer.pubkey(),
            direct_relayer_payout: false,
            version: 1u8,
            metadata: None,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[create_spoke_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Verify registry account contains the spoke entry we just created
    let registry_account = banks_client
        .get_account(registry_pda)
        .await?
        .expect("registry not found");
    let registry_data = registry_account.data;
    use anchor_lang::AccountDeserialize;
    let registry_acc: zpx_router::Registry =
        zpx_router::Registry::try_deserialize(&mut &registry_data[..]).expect("deserialize failed");
    // Confirm that at least one entry has spoke_id == 42
    let mut found = false;
    for i in 0..(registry_acc.spokes_len as usize) {
        if registry_acc.spokes[i].spoke_id == 42u32 {
            found = true;
            break;
        }
    }
    assert!(found, "created spoke not found in registry");

    // Minimal adapter allowlist check: config should not have any adapters yet
    let config_account = banks_client
        .get_account(config_pda)
        .await?
        .expect("config not found");
    let config_data = config_account.data;
    let cfg: zpx_router::Config =
        zpx_router::Config::try_deserialize(&mut &config_data[..]).expect("deserialize cfg failed");
    assert_eq!(
        cfg.adapters_len, 0,
        "expected no adapters in config by default"
    );

    // 4) Duplicate create_spoke should fail with AdapterAlreadyExists
    let dup_spoke_ix = Instruction {
        program_id,
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
            spoke_id: 42u32,
            adapter_program: payer.pubkey(),
            direct_relayer_payout: false,
            version: 1u8,
            metadata: None,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[dup_spoke_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());

    // 5) Unauthorized create_spoke by non-admin should fail
    let other = Keypair::new();
    // Fund other so it can pay for tx
    let ix = system_instruction::transfer(&payer.pubkey(), &other.pubkey(), 1_000_000_000);
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

    let create_spoke_unauth_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(other.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(Pubkey::new_unique(), false),
            solana_program::instruction::AccountMeta::new_readonly(
                solana_program::system_program::id(),
                false,
            ),
        ],
        data: zpx_router::instruction::CreateSpoke {
            spoke_id: 43u32,
            adapter_program: other.pubkey(),
            direct_relayer_payout: false,
            version: 1u8,
            metadata: None,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[create_spoke_unauth_ix],
        Some(&other.pubkey()),
        &[&other],
        recent_blockhash,
    );
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());

    Ok(())
}
