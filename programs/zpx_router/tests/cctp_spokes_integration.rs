use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account as SolAccount, pubkey::Pubkey, signature::Keypair, signer::Signer,
    system_instruction, transaction::Transaction, transport::TransportError,
};
use spl_token::state::{Account as SplTokenAccount, AccountState};

// Minimal helpers (copied from forward_event_shape.rs)
async fn create_mint(
    banks_client: &mut solana_program_test::BanksClient,
    payer: &Keypair,
    recent_blockhash: solana_sdk::hash::Hash,
    mint: &Keypair,
) -> std::result::Result<Pubkey, TransportError> {
    let rent = banks_client
        .get_rent()
        .await
        .map_err(|e| TransportError::Custom(format!("rent err: {:?}", e)))?;
    let mint_rent = rent.minimum_balance(spl_token::state::Mint::LEN);
    let create_mint_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        mint_rent,
        spl_token::state::Mint::LEN as u64,
        &spl_token::id(),
    );
    let init_mint_ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint.pubkey(),
        &payer.pubkey(),
        None,
        0,
    )
    .map_err(|e| TransportError::Custom(format!("init mint err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[create_mint_ix, init_mint_ix],
        Some(&payer.pubkey()),
        &[payer, mint],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;
    Ok(mint.pubkey())
}

async fn create_token_account_with_owner(
    banks_client: &mut solana_program_test::BanksClient,
    payer: &Keypair,
    recent_blockhash: solana_sdk::hash::Hash,
    owner: &Pubkey,
    mint: &Pubkey,
) -> std::result::Result<Pubkey, TransportError> {
    let ata = Keypair::new();
    let rent = banks_client
        .get_rent()
        .await
        .map_err(|e| TransportError::Custom(format!("rent err: {:?}", e)))?;
    let rent_lamports = rent.minimum_balance(spl_token::state::Account::LEN);
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &ata.pubkey(),
        rent_lamports,
        spl_token::state::Account::LEN as u64,
        &spl_token::id(),
    );
    let init_ix =
        spl_token::instruction::initialize_account(&spl_token::id(), &ata.pubkey(), mint, owner)
            .map_err(|e| TransportError::Custom(format!("init acct err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[payer, &ata],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;
    Ok(ata.pubkey())
}

// A simple mock adapter processor: succeed when data[0]==0, fail otherwise
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
async fn cctp_spokes_end_to_end() -> std::result::Result<(), TransportError> {
    let program_id = zpx_router::ID;

    // Create two distinct mock adapter program ids for v1 and v2
    let adapter_v1 = Pubkey::new_unique();
    let adapter_v2 = Pubkey::new_unique();

    let mint_kp = Keypair::new();
    let mint_pubkey = mint_kp.pubkey();

    // Derive PDAs
    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint_pubkey.to_bytes()];
    let (vault_pda, _vbump) = Pubkey::find_program_address(seeds, &program_id);
    let relayer_seeds: &[&[u8]] = &[b"hub_relayer_vault", &mint_pubkey.to_bytes()];
    let (relayer_pda, _rbump) = Pubkey::find_program_address(relayer_seeds, &program_id);

    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );
    // Register two mock adapters
    program_test.add_program(
        "mock_adapter_v1",
        adapter_v1,
        processor!(mock_adapter_process),
    );
    program_test.add_program(
        "mock_adapter_v2",
        adapter_v2,
        processor!(mock_adapter_process),
    );

    // Prepack vault PDA account for pattern A
    let mut token_data = vec![0u8; SplTokenAccount::LEN];
    let token_acct = SplTokenAccount {
        mint: mint_pubkey,
        owner: vault_pda,
        amount: 0u64,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0u64,
        close_authority: COption::None,
    };
    SplTokenAccount::pack_into_slice(&token_acct, &mut token_data);
    let vault_account = SolAccount {
        lamports: 1_000_000_000,
        data: token_data,
        owner: spl_token::id(),
        executable: false,
        rent_epoch: 0,
    };
    program_test.add_account(vault_pda, vault_account);

    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Create mint
    let _mint = create_mint(&mut banks_client, &payer, recent_blockhash, &mint_kp).await?;
    // create relayer vault (pattern B)
    let relayer_vault = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &relayer_pda,
        &mint_pubkey,
    )
    .await?;

    // Init config & registry
    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &program_id);
    let (registry_pda, _rbump2) = Pubkey::find_program_address(&[b"hub_registry"], &program_id);

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
            allowed_token_mint: mint_pubkey,
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

    // create spoke for adapter_v1 (spoke_id = 1)
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
            spoke_id: 1u32,
            adapter_program: adapter_v1,
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

    // create spoke for adapter_v2 (spoke_id = 2)
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
            spoke_id: 2u32,
            adapter_program: adapter_v2,
            direct_relayer_payout: false,
            version: 2u8,
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

    // user from ATA and fund it
    let user_from = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &user_from,
        &payer.pubkey(),
        &[],
        10_000,
    )
    .map_err(|e| TransportError::Custom(format!("mint_to err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[mint_to_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // adapter target and relayer token account
    let adapter_target = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;
    let relayer_token_account = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;

    // message account
    let message_kp = Keypair::new();
    let create_msg_ix = system_instruction::create_account(
        &payer.pubkey(),
        &message_kp.pubkey(),
        1_000_000,
        0,
        &program_id,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_msg_ix],
        Some(&payer.pubkey()),
        &[&payer, &message_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // create replay accounts for adapter_v1 and adapter_v2 (9 bytes, owned by adapter program)
    let rent = banks_client
        .get_rent()
        .await
        .map_err(|e| TransportError::Custom(format!("rent err: {:?}", e)))?;
    let replay_space = 9usize;
    let replay_rent = rent.minimum_balance(replay_space);
    let replay1_kp = Keypair::new();
    let create_replay1_ix = system_instruction::create_account(
        &payer.pubkey(),
        &replay1_kp.pubkey(),
        replay_rent,
        replay_space as u64,
        &adapter_v1,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_replay1_ix],
        Some(&payer.pubkey()),
        &[&payer, &replay1_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    let replay2_kp = Keypair::new();
    let create_replay2_ix = system_instruction::create_account(
        &payer.pubkey(),
        &replay2_kp.pubkey(),
        replay_rent,
        replay_space as u64,
        &adapter_v2,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_replay2_ix],
        Some(&payer.pubkey()),
        &[&payer, &replay2_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // perform forward_via_spoke for spoke 1 (adapter_v1)
    let forward_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true), // user
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true), // relayer
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(user_from, false),
            solana_program::instruction::AccountMeta::new(vault_pda, false),
            solana_program::instruction::AccountMeta::new(relayer_vault, false),
            solana_program::instruction::AccountMeta::new(relayer_token_account, false),
            solana_program::instruction::AccountMeta::new(adapter_target, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::ForwardViaSpoke {
            spoke_id: 1u32,
            amount: 10_000u64,
            dst_domain: 0u32,
            _mint_recipient: [0u8; 32],
            is_protocol_fee: true,
            is_relayer_fee: true,
            _nonce: 0u64,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[forward_ix],
        Some(&payer.pubkey()),
        &[&payer, &payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Now CPI into adapter_v1 via adapter_passthrough (success path: data[0]==0)
    let ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(adapter_v1, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new(replay1_kp.pubkey(), false),
        ],
        data: zpx_router::instruction::AdapterPassthrough {
            instruction_data: vec![0u8],
        }
        .data(),
    };
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

    // Negative case: CPI with failing payload should error
    let fail_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(adapter_v1, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new(replay1_kp.pubkey(), false),
        ],
        data: zpx_router::instruction::AdapterPassthrough {
            instruction_data: vec![1u8],
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[fail_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());

    // Replenish user_from so second forward has funds
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &user_from,
        &payer.pubkey(),
        &[],
        5_000,
    )
    .map_err(|e| TransportError::Custom(format!("mint_to err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[mint_to_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Repeat: perform forward_via_spoke for spoke 2 (adapter_v2)
    let forward_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true), // user
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true), // relayer
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(user_from, false),
            solana_program::instruction::AccountMeta::new(vault_pda, false),
            solana_program::instruction::AccountMeta::new(relayer_vault, false),
            solana_program::instruction::AccountMeta::new(relayer_token_account, false),
            solana_program::instruction::AccountMeta::new(adapter_target, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::ForwardViaSpoke {
            spoke_id: 2u32,
            amount: 5_000u64,
            dst_domain: 0u32,
            _mint_recipient: [0u8; 32],
            is_protocol_fee: true,
            is_relayer_fee: true,
            _nonce: 0u64,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[forward_ix],
        Some(&payer.pubkey()),
        &[&payer, &payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // CPI into adapter_v2 (success)
    let ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(adapter_v2, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new(replay2_kp.pubkey(), false),
        ],
        data: zpx_router::instruction::AdapterPassthrough {
            instruction_data: vec![0u8],
        }
        .data(),
    };
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

    Ok(())
}
