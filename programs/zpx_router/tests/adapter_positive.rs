use anchor_lang::InstructionData;
use solana_program::instruction::Instruction;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, msg};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
    transaction::Transaction, transport::TransportError,
};
use spl_token::state::{Account as SplTokenAccount, AccountState};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

// Minimal mock adapter processor: accepts data[0]==0 (success) else returns error.
static PROCESSED: OnceLock<Mutex<HashSet<Pubkey>>> = OnceLock::new();

fn mock_adapter_process(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    msg!("mock adapter invoked len={}", data.len());
    msg!("mock adapter accounts len = {}", accounts.len());
    for (i, ai) in accounts.iter().enumerate() {
        msg!(
            "acct[{}] = {:?}, owner={:?}, len={}",
            i,
            ai.key,
            ai.owner,
            ai.data_len()
        );
    }
    // Prefer using an on-chain replay account if provided as account[1]
    if accounts.len() >= 2 {
        let message_key = *accounts[0].key;
        let replay_ai = &accounts[1];
        msg!(
            "mock adapter message key = {:?}, replay_ai.len={}",
            message_key,
            replay_ai.data_len()
        );
        // Read existing processed flag if present
        if replay_ai.data_len() >= 9 {
            let d = replay_ai.try_borrow_data()?;
            let processed = d[8];
            msg!("replay account processed flag = {}", processed);
            if processed != 0u8 {
                msg!("replay seen on-chain for {:?}", message_key);
                return Err(solana_program::program_error::ProgramError::Custom(2));
            }
        }
        // Now attempt to set processed flag by mutably writing to the replay account
        if replay_ai.data_len() >= 9 {
            let mut d = replay_ai.try_borrow_mut_data()?;
            d[8] = 1u8;
            msg!("wrote processed flag on-chain");
        }
    } else if accounts.len() >= 1 {
        // Fallback: use in-memory set to simulate replay persistence across invocations of the mock.
        let message_key = *accounts[0].key;
        msg!("mock adapter message key = {:?}", message_key);
        let lock = PROCESSED.get_or_init(|| Mutex::new(HashSet::new()));
        let mut set = lock.lock().unwrap();
        msg!("processed set size before = {}", set.len());
        if set.contains(&message_key) {
            msg!("replay seen for {:?}", message_key);
            return Err(solana_program::program_error::ProgramError::Custom(2));
        }
        set.insert(message_key);
        msg!("processed set size after = {}", set.len());
    }
    if data.len() > 0 && data[0] == 0u8 {
        Ok(())
    } else {
        Err(solana_program::program_error::ProgramError::Custom(1))
    }
}

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

#[tokio::test]
async fn forward_to_registered_adapter_and_replay_guard() -> std::result::Result<(), TransportError>
{
    let router_program_id = zpx_router::ID;
    let adapter_program_id = Pubkey::new_unique();

    // Pre-create mint keypair so PDA derivations are deterministic
    let mint_kp = Keypair::new();
    let mint_pubkey = mint_kp.pubkey();

    // Derive hub protocol vault PDA for this mint and prepack a token account there (pattern A)
    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint_pubkey.to_bytes()];
    let (vault_pda, _vbump) = Pubkey::find_program_address(seeds, &router_program_id);
    // Derive relayer PDA
    let relayer_seeds: &[&[u8]] = &[b"hub_relayer_vault", &mint_pubkey.to_bytes()];
    let (relayer_pda, _rbump) = Pubkey::find_program_address(relayer_seeds, &router_program_id);

    let mut program_test = ProgramTest::new(
        "zpx_router",
        router_program_id,
        processor!(zpx_router::entry),
    );
    // Prepack a token account at the vault PDA so the program accepts PDA-as-account
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
    let vault_account = solana_sdk::account::Account {
        lamports: 1_000_000,
        data: token_data,
        owner: spl_token::id(),
        executable: false,
        rent_epoch: 0,
    };
    program_test.add_account(vault_pda, vault_account);
    // register mock adapter
    program_test.add_program(
        "mock_adapter",
        adapter_program_id,
        processor!(mock_adapter_process),
    );
    program_test.add_program(
        "spl_token",
        anchor_spl::token::ID,
        processor!(spl_token::processor::Processor::process),
    );

    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Create mint and token accounts (mint uses the pre-created keypair so PDA derivation matches)
    let mint_pubkey = create_mint(&mut banks_client, &payer, recent_blockhash, &mint_kp).await?;
    let user_from = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;
    // Mint some tokens to user_from so transfers succeed
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
    // vault is the PDA we prepacked above
    let vault = vault_pda;

    // Initialize config and registry
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

    // Debug: check owners of config and registry PDAs and adapter program account
    let cfg_acct = banks_client
        .get_account(config_pda)
        .await
        .map_err(|e| TransportError::Custom(format!("get_account cfg err: {:?}", e)))?;
    let reg_acct = banks_client
        .get_account(registry_pda)
        .await
        .map_err(|e| TransportError::Custom(format!("get_account reg err: {:?}", e)))?;
    let adapter_acct = banks_client
        .get_account(adapter_program_id)
        .await
        .map_err(|e| TransportError::Custom(format!("get_account adapter err: {:?}", e)))?;
    println!("config owner = {:?}", cfg_acct.as_ref().map(|a| a.owner));
    println!("registry owner = {:?}", reg_acct.as_ref().map(|a| a.owner));
    println!(
        "adapter account owner = {:?}",
        adapter_acct.as_ref().map(|a| a.owner)
    );

    // Create spoke that points to adapter_program_id
    let create_spoke_ix = Instruction {
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
            spoke_id: 42u32,
            adapter_program: adapter_program_id,
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

    // Prepare message account used as unique message id for replay PDA
    let message_kp = Keypair::new();
    let create_msg_ix = system_instruction::create_account(
        &payer.pubkey(),
        &message_kp.pubkey(),
        1_000_000,
        0,
        &router_program_id,
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

    // Create replay account owned by adapter program: allocate 9 bytes (8 discriminator + 1 processed flag)
    let replay_kp = Keypair::new();
    let replay_space = 9usize;
    let rent = banks_client
        .get_rent()
        .await
        .map_err(|e| TransportError::Custom(format!("rent err: {:?}", e)))?;
    let lamports = rent.minimum_balance(replay_space);
    let create_replay_ix = system_instruction::create_account(
        &payer.pubkey(),
        &replay_kp.pubkey(),
        lamports,
        replay_space as u64,
        &adapter_program_id,
    );
    let tx = Transaction::new_signed_with_payer(
        &[create_replay_ix],
        Some(&payer.pubkey()),
        &[&payer, &replay_kp],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Do forward_via_spoke (should succeed because spoke points to adapter)
    let relayer_vault = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &relayer_pda,
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
    let adapter_target_account = create_token_account_with_owner(
        &mut banks_client,
        &payer,
        recent_blockhash,
        &payer.pubkey(),
        &mint_pubkey,
    )
    .await?;

    let forward_ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new(payer.pubkey(), true), // user
            solana_program::instruction::AccountMeta::new_readonly(payer.pubkey(), true), // relayer
            solana_program::instruction::AccountMeta::new_readonly(mint_pubkey, false),
            solana_program::instruction::AccountMeta::new(user_from, false),
            solana_program::instruction::AccountMeta::new(vault, false),
            solana_program::instruction::AccountMeta::new(relayer_vault, false),
            solana_program::instruction::AccountMeta::new(relayer_token_account, false),
            solana_program::instruction::AccountMeta::new(adapter_target_account, false),
            solana_program::instruction::AccountMeta::new(registry_pda, false),
            solana_program::instruction::AccountMeta::new(config_pda, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: zpx_router::instruction::ForwardViaSpoke {
            spoke_id: 42u32,
            amount: 1u64,
            dst_domain: 0u32,
            _mint_recipient: [0u8; 32],
            is_protocol_fee: false,
            is_relayer_fee: false,
            _nonce: 0u64,
        }
        .data(),
    };

    // First forward should succeed
    let tx = Transaction::new_signed_with_payer(
        &[forward_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer, &payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Simulate adapter CPI invocation: call adapter_passthrough with message and replay accounts
    let passthrough_ix = Instruction {
        program_id: router_program_id,
        accounts: vec![
            solana_program::instruction::AccountMeta::new_readonly(adapter_program_id, false),
            solana_program::instruction::AccountMeta::new(message_kp.pubkey(), false),
            solana_program::instruction::AccountMeta::new(replay_kp.pubkey(), false),
        ],
        data: zpx_router::instruction::AdapterPassthrough {
            instruction_data: vec![0u8],
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[passthrough_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client
        .process_transaction(tx)
        .await
        .map_err(TransportError::from)?;

    // Verify replay account was written on-chain
    let replay_acct = banks_client
        .get_account(replay_kp.pubkey())
        .await
        .map_err(|e| TransportError::Custom(format!("get replay acct err: {:?}", e)))?
        .expect("replay account missing");
    assert_eq!(replay_acct.data[8], 1u8);

    // Second passthrough (same message/replay) should now be rejected due to replay flag
    let latest_blockhash = banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[passthrough_ix.clone()],
        Some(&payer.pubkey()),
        &[&payer],
        latest_blockhash,
    );
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_err());

    Ok(())
}
