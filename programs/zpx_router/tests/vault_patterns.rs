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

#[tokio::test]
async fn vault_pattern_a_and_b() -> std::result::Result<(), TransportError> {
    // Pattern A: token account address equals PDA
    // Pattern B: token account owner (authority) equals PDA
    let program_id = zpx_router::ID;
    let mint_kp = Keypair::new();
    let mint_pubkey = mint_kp.pubkey();

    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint_pubkey.to_bytes()];
    let (vault_pda, _vbump) = Pubkey::find_program_address(seeds, &program_id);

    let mut program_test =
        ProgramTest::new("zpx_router", program_id, processor!(zpx_router::entry));
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );

    // Prepack a token account at the vault PDA (Pattern A)
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
        data: token_data.clone(),
        owner: spl_token::id(),
        executable: false,
        rent_epoch: 0,
    };
    program_test.add_account(vault_pda, vault_account);

    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;
    // create mint
    let _mint = create_mint(&mut banks_client, &payer, recent_blockhash, &mint_kp).await?;

    // Call validate_vault_pda_or_authority indirectly via admin_withdraw flow as in admin_withdraw.rs
    // Reuse admin_withdraw setup but focus on ensuring PDA-account pattern passes.
    // Create destination ATA
    let dest_ata_kp = Keypair::new();
    let rent = banks_client
        .get_rent()
        .await
        .map_err(|e| TransportError::Custom(format!("rent err: {:?}", e)))?;
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &dest_ata_kp.pubkey(),
        rent.minimum_balance(spl_token::state::Account::LEN),
        spl_token::state::Account::LEN as u64,
        &spl_token::id(),
    );
    let init_ix = spl_token::instruction::initialize_account(
        &spl_token::id(),
        &dest_ata_kp.pubkey(),
        &mint_pubkey,
        &payer.pubkey(),
    )
    .map_err(|e| TransportError::Custom(format!("init acct err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[&payer, &dest_ata_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Initialize config so admin_withdraw will accept admin==payer
    let (config_pda, _cbump) = Pubkey::find_program_address(&[b"zpx_config"], &program_id);
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

    // Mint some tokens into the PDA vault
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &vault_pda,
        &payer.pubkey(),
        &[],
        1234,
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

    // Call admin_withdraw which uses validate_vault_pda_or_authority and should succeed for Pattern A
    let ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(vault_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(vault_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(dest_ata_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::AdminWithdraw { amount: 100 }.data(),
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

    // Verify dest received tokens
    let dest_account = banks_client
        .get_account(dest_ata_kp.pubkey())
        .await?
        .expect("dest not found");
    let dest_data = spl_token::state::Account::unpack(&dest_account.data).unwrap();
    assert_eq!(dest_data.amount, 100u64);

    // Pattern B: create a token account whose authority == PDA (address != PDA)
    let other_vault_kp = Keypair::new();
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &other_vault_kp.pubkey(),
        rent.minimum_balance(spl_token::state::Account::LEN),
        spl_token::state::Account::LEN as u64,
        &spl_token::id(),
    );
    let init_ix = spl_token::instruction::initialize_account(
        &spl_token::id(),
        &other_vault_kp.pubkey(),
        &mint_pubkey,
        &vault_pda,
    )
    .map_err(|e| TransportError::Custom(format!("init acct err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[&payer, &other_vault_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Mint into the authority-equals-PDA vault
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        &mint_pubkey,
        &other_vault_kp.pubkey(),
        &payer.pubkey(),
        &[],
        50,
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

    // Withdraw via admin_withdraw using other_vault_kp (authority==PDA). Use hub_protocol_pda equals vault_pda as unchecked.
    let dest2_kp = Keypair::new();
    let create_ix = system_instruction::create_account(
        &payer.pubkey(),
        &dest2_kp.pubkey(),
        rent.minimum_balance(spl_token::state::Account::LEN),
        spl_token::state::Account::LEN as u64,
        &spl_token::id(),
    );
    let init_ix = spl_token::instruction::initialize_account(
        &spl_token::id(),
        &dest2_kp.pubkey(),
        &mint_pubkey,
        &payer.pubkey(),
    )
    .map_err(|e| TransportError::Custom(format!("init acct err: {:?}", e)))?;
    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[&payer, &dest2_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    let ix = Instruction {
        program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(other_vault_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(vault_pda, false),
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(dest2_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::AdminWithdraw { amount: 25 }.data(),
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

    let dest2_account = banks_client
        .get_account(dest2_kp.pubkey())
        .await?
        .expect("dest2 not found");
    let dest2_data = spl_token::state::Account::unpack(&dest2_account.data).unwrap();
    assert_eq!(dest2_data.amount, 25u64);

    Ok(())
}
