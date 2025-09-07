// cargo test -p zoopx_router --tests
#![allow(deprecated)]

use anchor_lang::solana_program::program_pack::Pack;
use anchor_lang::{InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_program,
    transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address_with_program_id as get_ata_with_id;
use spl_memo as memo;
use spl_token::id as TOKEN_PROGRAM_ID;
use spl_token::state::Account as ClassicAccount;
use spl_token_2022::id as TOKEN_2022_PROGRAM_ID;
use zoopx_router::{self, config_pda};

// Dummy adapter program id (arbitrary constant pubkey)
const DUMMY_ADAPTER_ID: Pubkey = Pubkey::new_from_array([7u8; 32]);

// Adapter to match solana-program-test's expected processor signature
use anchor_lang::prelude::{AccountInfo as AnchorAccountInfo, Pubkey as AnchorPubkey};
use anchor_lang::solana_program::entrypoint::ProgramResult as AnchorProgramResult;
fn anchor_process<'a, 'b, 'c, 'd>(
    pid: &'a AnchorPubkey,
    accs: &'b [AnchorAccountInfo<'c>],
    data: &'d [u8],
) -> AnchorProgramResult {
    // Safety: The program-test runtime passes a slice whose element lifetimes are valid
    // for the duration of the slice borrow. Anchor's entry requires both to be the same
    // lifetime parameter. This transmute only tightens the relation and is safe here.
    let accs_same_lifetime: &'c [AnchorAccountInfo<'c>] = unsafe { std::mem::transmute(accs) };
    zoopx_router::entry(pid, accs_same_lifetime, data)
}

#[allow(clippy::too_many_arguments)]
async fn create_mint_and_atas(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    token_program: Pubkey,
    decimals: u8,
    user: &Keypair,
    fee_recipient: Pubkey,
    adapter_authority: Pubkey,
) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let mint = Keypair::new();
    let rent = banks_client.get_rent().await.unwrap();
    let mint_space = spl_token::state::Mint::LEN as u64;
    let min_rent = rent.minimum_balance(mint_space as usize);

    let mut tx = Transaction::new_with_payer(
        &[system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            min_rent,
            mint_space,
            &token_program,
        )],
        Some(&payer.pubkey()),
    );
    tx.sign(&[payer, &mint], *recent_blockhash);
    banks_client.process_transaction(tx).await.unwrap();

    // initialize mint via token-2022 helper which supports both ids
    let init_mint_ix = spl_token_2022::instruction::initialize_mint2(
        &token_program,
        &mint.pubkey(),
        &payer.pubkey(),
        None,
        decimals,
    )
    .unwrap();
    let mut tx = Transaction::new_with_payer(&[init_mint_ix], Some(&payer.pubkey()));
    tx.sign(&[payer], *recent_blockhash);
    banks_client.process_transaction(tx).await.unwrap();

    // derive and create ATAs
    let user_ata = get_ata_with_id(&user.pubkey(), &mint.pubkey(), &token_program);
    let fee_ata = get_ata_with_id(&fee_recipient, &mint.pubkey(), &token_program);
    let custody_ata = get_ata_with_id(&adapter_authority, &mint.pubkey(), &token_program);

    for owner in [user.pubkey(), fee_recipient, adapter_authority] {
        let ix = spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),
            &owner,
            &mint.pubkey(),
            &token_program,
        );
        let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
        tx.sign(&[payer], *recent_blockhash);
        banks_client.process_transaction(tx).await.unwrap();
    }

    // mint tokens to user
    let mint_to_ix = spl_token_2022::instruction::mint_to(
        &token_program,
        &mint.pubkey(),
        &user_ata,
        &payer.pubkey(),
        &[],
        1_000_000,
    )
    .unwrap();
    let mut tx = Transaction::new_with_payer(&[mint_to_ix], Some(&payer.pubkey()));
    tx.sign(&[payer], *recent_blockhash);
    banks_client.process_transaction(tx).await.unwrap();

    (mint.pubkey(), user_ata, fee_ata, custody_ata)
}

async fn get_token_amount(banks_client: &mut BanksClient, ata: Pubkey) -> u64 {
    let acc = banks_client
        .get_account(ata)
        .await
        .unwrap()
        .expect("ata exists");
    let parsed = ClassicAccount::unpack_from_slice(&acc.data).unwrap();
    parsed.amount
}

fn program_test() -> ProgramTest {
    let mut pt = ProgramTest::default();
    // Register router under test
    pt.add_program(
        "zoopx_router",
        zoopx_router::id(),
        processor!(anchor_process),
    );
    // Register a dummy adapter program that accepts any CPI without signatures
    fn dummy_adapter_process<'a, 'b, 'c, 'd>(
        _pid: &'a AnchorPubkey,
        _accs: &'b [AnchorAccountInfo<'c>],
        _data: &'d [u8],
    ) -> AnchorProgramResult {
        Ok(())
    }
    pt.add_program(
        "dummy_adapter",
        DUMMY_ADAPTER_ID,
        processor!(dummy_adapter_process),
    );
    pt
}

#[tokio::test]
async fn happy_path_classic() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let lamports = 2_000_000_000;
    let tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &payer.pubkey(),
            &user.pubkey(),
            lamports,
        )],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_PROGRAM_ID();

    let (mint, user_ata, fee_ata, custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    // invoke transfer
    let amount = 100_000u64;
    // Respect fee cap (0.05% => 50 on 100_000)
    let protocol_fee = 50u64;
    let payload = b"hi".to_vec();
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: custody_ata,
            target_adapter_program: DUMMY_ADAPTER_ID,
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount,
            protocol_fee,
            relayer_fee: 0,
            payload: payload.clone(),
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };

    let mut metas = ix.accounts.clone();
    // Include the adapter program id and custody ATA in remaining accounts per program checks
    metas.push(AccountMeta::new_readonly(DUMMY_ADAPTER_ID, false));
    metas.push(AccountMeta::new(custody_ata, false));
    let ix = Instruction {
        accounts: metas,
        ..ix
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let user_after = get_token_amount(&mut banks_client, user_ata).await;
    let fee_after = get_token_amount(&mut banks_client, fee_ata).await;
    let custody_after = get_token_amount(&mut banks_client, custody_ata).await;

    assert_eq!(user_after, 1_000_000 - amount);
    assert_eq!(fee_after, protocol_fee);
    assert_eq!(custody_after, amount - protocol_fee);
}

#[tokio::test]
async fn happy_path_token2022() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let lamports = 2_000_000_000;
    let tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &payer.pubkey(),
            &user.pubkey(),
            lamports,
        )],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_2022_PROGRAM_ID();

    let (mint, user_ata, fee_ata, custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    // invoke transfer
    let amount = 100_000u64;
    // Respect fee cap (0.05% => 50 on 100_000)
    let protocol_fee = 50u64;
    let payload = b"hi".to_vec();
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: custody_ata,
            target_adapter_program: DUMMY_ADAPTER_ID,
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount,
            protocol_fee,
            relayer_fee: 0,
            payload: payload.clone(),
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };

    let mut metas = ix.accounts.clone();
    // Include the adapter program id and custody ATA in remaining accounts per program checks
    metas.push(AccountMeta::new_readonly(DUMMY_ADAPTER_ID, false));
    metas.push(AccountMeta::new(custody_ata, false));
    let ix = Instruction {
        accounts: metas,
        ..ix
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let user_after = get_token_amount(&mut banks_client, user_ata).await;
    let fee_after = get_token_amount(&mut banks_client, fee_ata).await;
    let custody_after = get_token_amount(&mut banks_client, custody_ata).await;

    assert_eq!(user_after, 1_000_000 - amount);
    assert_eq!(fee_after, protocol_fee);
    assert_eq!(custody_after, amount - protocol_fee);
}

#[tokio::test]
async fn happy_path_with_relayer_fee_classic() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let lamports = 2_000_000_000;
    let tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &payer.pubkey(),
            &user.pubkey(),
            lamports,
        )],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_PROGRAM_ID();

    let (mint, user_ata, fee_ata, custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    // invoke transfer with both protocol_fee and relayer_fee
    let amount = 100_000u64;
    let protocol_fee = 50u64; // within 5 bps cap
    let relayer_fee = 25u64; // additional fee, no cap besides total <= amount
    let total_fee = protocol_fee + relayer_fee;
    let payload = b"hi".to_vec();
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: custody_ata,
            target_adapter_program: DUMMY_ADAPTER_ID,
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount,
            protocol_fee,
            relayer_fee,
            payload: payload.clone(),
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };

    let mut metas = ix.accounts.clone();
    metas.push(AccountMeta::new_readonly(DUMMY_ADAPTER_ID, false));
    metas.push(AccountMeta::new(custody_ata, false));
    let ix = Instruction { accounts: metas, ..ix };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let user_after = get_token_amount(&mut banks_client, user_ata).await;
    let fee_after = get_token_amount(&mut banks_client, fee_ata).await;
    let custody_after = get_token_amount(&mut banks_client, custody_ata).await;

    assert_eq!(user_after, 1_000_000 - amount);
    assert_eq!(fee_after, total_fee);
    assert_eq!(custody_after, amount - total_fee);
}

#[tokio::test]
async fn negative_wrong_custody_ata() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_PROGRAM_ID();

    let (mint, user_ata, fee_ata, _custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let wrong_custody = Pubkey::new_unique();

    let amount = 100_000u64;
    let protocol_fee = 10_000u64;
    let payload = b"hi".to_vec();
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: wrong_custody,
            target_adapter_program: memo::id(),
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount,
            protocol_fee,
            relayer_fee: 0,
            payload: payload.clone(),
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };
    let mut metas = ix.accounts.clone();
    metas.push(AccountMeta::new_readonly(memo::id(), false));
    metas.push(AccountMeta::new(wrong_custody, false));
    let ix = Instruction {
        accounts: metas,
        ..ix
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user],
        recent_blockhash,
    );
    let err = banks_client.process_transaction(tx).await.unwrap_err();
    assert!(format!("{:?}", err).contains("Custom"));
}

#[tokio::test]
async fn negative_payload_too_large() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_PROGRAM_ID();

    let (mint, user_ata, fee_ata, custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let payload = vec![0u8; 513];
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: custody_ata,
            target_adapter_program: memo::id(),
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount: 1,
            protocol_fee: 0,
            relayer_fee: 0,
            payload,
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };
    let mut metas = ix.accounts.clone();
    metas.push(AccountMeta::new_readonly(memo::id(), false));
    metas.push(AccountMeta::new(custody_ata, false));
    let ix = Instruction {
        accounts: metas,
        ..ix
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user],
        recent_blockhash,
    );
    let err = banks_client.process_transaction(tx).await.unwrap_err();
    assert!(format!("{:?}", err).contains("Custom"));
}

#[tokio::test]
async fn negative_unexpected_signer_in_remaining() {
    let pt = program_test();
    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let user = Keypair::new();
    let fee_recipient = Pubkey::new_unique();
    let adapter_authority = Pubkey::new_unique();
    let token_program = TOKEN_PROGRAM_ID();

    let (mint, user_ata, fee_ata, custody_ata) = create_mint_and_atas(
        &mut banks_client,
        &payer,
        &recent_blockhash,
        token_program,
        6,
        &user,
        fee_recipient,
        adapter_authority,
    )
    .await;

    // init config
    let (config, _bump) = config_pda::pda_address(&zoopx_router::id());
    let ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::InitializeConfig {
            payer: payer.pubkey(),
            config,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::InitializeConfig { fee_recipient }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let mut ix = Instruction {
        program_id: zoopx_router::id(),
        accounts: zoopx_router::accounts::UniversalBridgeTransfer {
            user: user.pubkey(),
            mint,
            from: user_ata,
            fee_recipient_token: fee_ata,
            cpi_target_token_account: custody_ata,
            target_adapter_program: memo::id(),
            adapter_authority,
            token_program,
            config,
        }
        .to_account_metas(None),
        data: zoopx_router::instruction::UniversalBridgeTransfer {
            amount: 1,
            protocol_fee: 0,
            relayer_fee: 0,
            payload: vec![],
            dst_chain_id: 0,
            nonce: 0,
        }
        .data(),
    };

    // Add an unexpected signer in remaining accounts; sign tx with it so the runtime flags it as a signer
    let stray = Keypair::new();
    ix.accounts.push(AccountMeta {
        pubkey: stray.pubkey(),
        is_signer: true,
        is_writable: false,
    });
    ix.accounts
        .push(AccountMeta::new_readonly(memo::id(), false));
    ix.accounts.push(AccountMeta::new(custody_ata, false));

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &user, &stray],
        recent_blockhash,
    );
    let err = banks_client.process_transaction(tx).await.unwrap_err();
    assert!(format!("{:?}", err).contains("Custom"));
}
