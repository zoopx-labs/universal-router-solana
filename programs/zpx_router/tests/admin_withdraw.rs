use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program::program_pack::Pack;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account as SolAccount, pubkey::Pubkey, signature::Keypair, signer::Signer,
    system_instruction, transaction::Transaction, transport::TransportError,
};

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
async fn admin_withdraw_from_hub_pda() -> std::result::Result<(), TransportError> {
    let program_id = zpx_router::ID;
    // Pre-create a mint keypair so vault PDA is deterministic before starting ProgramTest
    let mint_kp = Keypair::new();
    let mint_pubkey = mint_kp.pubkey();
    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint_pubkey.to_bytes()];
    let (vault_pda, _bump) = Pubkey::find_program_address(seeds, &program_id);

    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));
    // Register spl-token processor
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );
    // Prepack an SPL token Account at the vault PDA so the program's PDA checks succeed
    use solana_program::program_option::COption;
    use spl_token::state::{Account as SplTokenAccount, AccountState};
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

    // Create the mint using the prepared keypair
    let _mint_pubkey = create_mint(&mut banks_client, &payer, recent_blockhash, &mint_kp)
        .await
        .map_err(|e| TransportError::Custom(format!("mint create failed: {:?}", e)))?;

    // vault_pda is used directly as the SPL token account for the protocol vault (pre-seeded above).
    let vault_ata = vault_pda;

    // Create destination token account owned by payer
    let dest_ata = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await
    .map_err(|_| TransportError::Custom("create dest ata failed".into()))?;

    // Mint some tokens into the vault ATA
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &vault_ata,
        &payer.pubkey(),
        &[],
        1_000,
    )
    .map_err(|e| TransportError::Custom(format!("mint_to instr err: {:?}", e)))?;
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

    // Call admin_withdraw instruction: need to construct instruction data for anchor
    let admin = payer.pubkey();
    // Initialize router config so admin check passes (call initialize_config)
    let (config_pda, _config_bump) = Pubkey::find_program_address(&[b"zpx_config"], &program_id);
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
            relayer_fee_bps: 0u16,
            protocol_fee_bps: 0u16,
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

    let ix = Instruction {
        program_id,
        accounts: vec![
            // authority (Signer)
            solana_program::instruction::AccountMeta::new_readonly(admin, true),
            // config (seeds zpx_config)
            solana_program::instruction::AccountMeta::new(config_pda, false),
            // hub_protocol_vault (source token account)
            solana_program::instruction::AccountMeta::new(vault_ata, false),
            // hub_protocol_pda (unchecked PDA authority)
            solana_program::instruction::AccountMeta::new_readonly(vault_pda, false),
            // mint
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            // destination
            solana_program::instruction::AccountMeta::new(dest_ata, false),
            // token program
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::AdminWithdraw { amount: 500 }.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await?;

    // Read destination account balance to assert received tokens
    let dest_account = banks_client
        .get_account(dest_ata)
        .await
        .map_err(|e| TransportError::Custom(format!("get_account err: {:?}", e)))?
        .expect("dest account not found");
    let dest_data = spl_token::state::Account::unpack(&dest_account.data).unwrap();
    assert_eq!(dest_data.amount, 500);

    Ok(())
}
