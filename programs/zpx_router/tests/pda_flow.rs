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

async fn create_mint(
    banks_client: &mut solana_program_test::BanksClient,
    payer: &Keypair,
    recent_blockhash: solana_sdk::hash::Hash,
    mint: &Keypair,
) -> std::result::Result<Pubkey, TransportError> {
    // Create mint account and initialize
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

#[tokio::test]
async fn pda_vault_forward_and_admin_withdraw() -> std::result::Result<(), TransportError> {
    let program_id = zpx_router::ID;
    // Pre-create mint keypair so PDA derivations are deterministic
    let mint_kp = Keypair::new();
    let mint_pubkey = mint_kp.pubkey();

    // Derive hub protocol PDA (used for protocol vault and authority)
    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint_pubkey.to_bytes()];
    let (vault_pda, _vbump) = Pubkey::find_program_address(seeds, &program_id);

    // Derive relayer PDA
    let relayer_seeds: &[&[u8]] = &[b"hub_relayer_vault", &mint_pubkey.to_bytes()];
    let (relayer_pda, _rbump) = Pubkey::find_program_address(relayer_seeds, &program_id);

    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );

    // Prepack a token account at the vault PDA (pattern A) so the program accepts PDA-as-account
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

    // Start test environment
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Create mint
    let _mint = create_mint(&mut banks_client, &payer, recent_blockhash, &mint_kp).await?;

    // Create relayer vault as a normal token account whose authority == relayer_pda (pattern B)
    let relayer_vault = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &relayer_pda,
        &mint_pubkey,
    )
    .await?;

    // Create registry and config PDAs and initialize
    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &program_id);
    let (registry_pda, _rbump2) = Pubkey::find_program_address(&[b"hub_registry"], &program_id);

    // init config
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

    // init registry
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

    // create a spoke entry (spoke_id = 1)
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

    // Create user 'from' ATA and fund it
    let user_from = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;
    // Mint tokens to user
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

    // Create adapter target token account
    let adapter_target = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;

    // Create relayer_token_account (used if direct relayer payout)
    let relayer_token_account = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;

    // Create a small message_account system account (mutable) for forward_via_spoke
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

    // Call forward_via_spoke: this will transfer fees to hub_protocol_vault (PDA-account) and relayer vault (authority==PDA)
    let forward_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true), // user
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true), // relayer (use same payer)
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

    // After forward: check vault balances
    // Protocol fee expected: amount * protocol_fee_bps / 10_000 = 10_000 * 5 / 10_000 = 5
    let proto_expected = 5u64;
    let relayer_expected = 100u64; // 10_000 * 100 / 10_000 = 100

    let vault_account = banks_client
        .get_account(vault_pda)
        .await?
        .expect("vault not found");
    let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
    assert_eq!(vault_data.amount, proto_expected);

    let relayer_account = banks_client
        .get_account(relayer_vault)
        .await?
        .expect("relayer vault not found");
    let relayer_data = spl_token::state::Account::unpack(&relayer_account.data).unwrap();
    assert_eq!(relayer_data.amount, relayer_expected);

    // Now call admin_withdraw to move protocol fee from vault_pda -> destination
    let dest_ata = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;

    let admin_ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(vault_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(vault_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(dest_ata, false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::AdminWithdraw {
            amount: proto_expected,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[admin_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Verify destination received proto_expected
    let dest_account = banks_client
        .get_account(dest_ata)
        .await?
        .expect("dest not found");
    let dest_data = spl_token::state::Account::unpack(&dest_account.data).unwrap();
    assert_eq!(dest_data.amount, proto_expected);

    Ok(())
}
