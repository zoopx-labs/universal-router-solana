#![allow(unexpected_cfgs)]
#![allow(clippy::result_large_err)]
use anchor_lang::prelude::*;

declare_id!("ZPX9cXPQjrCtprRCgahAgL6sMQxsSrymJ7VatC6BA99");

const REPLAY_SEED: &[u8] = b"adapter_replay";

#[program]
pub mod zpx_adapter {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }

    /// Process a transfer message. Enforces a simple replay guard.
    pub fn process_transfer(
        ctx: Context<ProcessTransfer>,
        _message_id: [u8; 32],
        payload: Vec<u8>,
    ) -> Result<()> {
        let replay = &mut ctx.accounts.replay;
        if replay.processed != 0 {
            return err!(AdapterError::ReplayProcessed);
        }
        if payload.is_empty() {
            return err!(AdapterError::InvalidPayload);
        }

        // For tests: payload[0] == 0 => accept, 1 => refund
        match payload[0] {
            0 => emit!(TransferAccepted {
                message_id: _message_id,
                amount: 0
            }),
            1 => emit!(TransferRefunded {
                message_id: _message_id,
                reason: 1
            }),
            _ => return err!(AdapterError::InvalidPayload),
        }

        replay.processed = 1;
        Ok(())
    }

    pub fn accept(_ctx: Context<Accept>) -> Result<()> {
        Ok(())
    }
    pub fn refund(_ctx: Context<Refund>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ProcessTransfer<'info> {
    /// CHECK: message account arbitrary
    pub message: UncheckedAccount<'info>,
    /// Replay PDA derived from message id
    #[account(init_if_needed, payer = payer, space = 8 + 1, seeds = [REPLAY_SEED, &message.key().to_bytes()], bump)]
    pub replay: Account<'info, Replay>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Accept<'info> {
    pub caller: UncheckedAccount<'info>,
}
#[derive(Accounts)]
pub struct Refund<'info> {
    pub caller: UncheckedAccount<'info>,
}

#[account]
pub struct Replay {
    pub processed: u8,
}

#[event]
pub struct TransferAccepted {
    pub message_id: [u8; 32],
    pub amount: u64,
}
#[event]
pub struct TransferRefunded {
    pub message_id: [u8; 32],
    pub reason: u8,
}

#[error_code]
pub enum AdapterError {
    #[msg("Invalid payload")]
    InvalidPayload,
    #[msg("Replay processed")]
    ReplayProcessed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::InstructionData;
    use solana_program_test::{processor, ProgramTest};
    use solana_sdk::instruction::Instruction;
    use solana_sdk::{
        pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction,
    };

    #[tokio::test]
    async fn process_transfer_accepts_and_blocks_replay() {
        let program_id = crate::ID;
        let pt = ProgramTest::new("zpx_adapter", program_id, processor!(crate::entry));
        let (mut banks_client, payer, recent_blockhash) = pt.start().await;

        let message = Keypair::new();

        // derive the replay PDA the program expects
        let (replay_pda, _bump) =
            Pubkey::find_program_address(&[REPLAY_SEED, &message.pubkey().to_bytes()], &program_id);

        let ix = Instruction {
            program_id,
            accounts: vec![
                anchor_lang::prelude::AccountMeta::new(message.pubkey(), false),
                anchor_lang::prelude::AccountMeta::new(replay_pda, false),
                anchor_lang::prelude::AccountMeta::new(payer.pubkey(), true),
                anchor_lang::prelude::AccountMeta::new_readonly(
                    solana_program::system_program::id(),
                    false,
                ),
            ],
            // Anchor generates instruction data field names; generated struct may have leading underscore prefixes.
            data: crate::instruction::ProcessTransfer {
                _message_id: [0u8; 32],
                payload: vec![0u8],
            }
            .data(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        println!("first tx sig: {}", tx.signatures[0]);
        banks_client.process_transaction(tx).await.unwrap();

        // verify the replay account was written
        let acct = banks_client
            .get_account(replay_pda)
            .await
            .unwrap()
            .expect("replay account missing");
        // anchor discriminator is 8 bytes; processed u8 is at offset 8
        assert_eq!(acct.data[8], 1u8);

        // fund a fresh signer so the second transaction signature differs
        let payer2 = Keypair::new();
        let fund_ix = solana_sdk::system_instruction::transfer(
            &payer.pubkey(),
            &payer2.pubkey(),
            1_000_000_000,
        );
        let fund_tx = Transaction::new_signed_with_payer(
            &[fund_ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        banks_client.process_transaction(fund_tx).await.unwrap();

        // second invocation should fail due to replay; sign with payer2
        let ix = Instruction {
            program_id,
            accounts: vec![
                anchor_lang::prelude::AccountMeta::new(message.pubkey(), false),
                anchor_lang::prelude::AccountMeta::new(replay_pda, false),
                anchor_lang::prelude::AccountMeta::new(payer2.pubkey(), true),
                anchor_lang::prelude::AccountMeta::new_readonly(
                    solana_program::system_program::id(),
                    false,
                ),
            ],
            data: crate::instruction::ProcessTransfer {
                _message_id: [0u8; 32],
                payload: vec![0u8],
            }
            .data(),
        };
        let second_blockhash = banks_client.get_latest_blockhash().await.unwrap();
        println!("sending second invocation signed by payer2");
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer2.pubkey()),
            &[&payer2],
            second_blockhash,
        );
        let res = banks_client.process_transaction(tx).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn process_transfer_refund_and_invalid_payload() {
        let program_id = crate::ID;
        let pt = ProgramTest::new("zpx_adapter", program_id, processor!(crate::entry));
        let (mut banks_client, payer, recent_blockhash) = pt.start().await;

        let message = Keypair::new();
        let (replay_pda, _bump) =
            Pubkey::find_program_address(&[REPLAY_SEED, &message.pubkey().to_bytes()], &program_id);

        // refund path: payload[0] == 1
        let ix = Instruction {
            program_id,
            accounts: vec![
                anchor_lang::prelude::AccountMeta::new(message.pubkey(), false),
                anchor_lang::prelude::AccountMeta::new(replay_pda, false),
                anchor_lang::prelude::AccountMeta::new(payer.pubkey(), true),
                anchor_lang::prelude::AccountMeta::new_readonly(
                    solana_program::system_program::id(),
                    false,
                ),
            ],
            data: crate::instruction::ProcessTransfer {
                _message_id: [1u8; 32],
                payload: vec![1u8],
            }
            .data(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        banks_client.process_transaction(tx).await.unwrap();

        // invalid payload (empty) should error
        let ix = Instruction {
            program_id,
            accounts: vec![
                anchor_lang::prelude::AccountMeta::new(message.pubkey(), false),
                anchor_lang::prelude::AccountMeta::new(replay_pda, false),
                anchor_lang::prelude::AccountMeta::new(payer.pubkey(), true),
                anchor_lang::prelude::AccountMeta::new_readonly(
                    solana_program::system_program::id(),
                    false,
                ),
            ],
            data: crate::instruction::ProcessTransfer {
                _message_id: [2u8; 32],
                payload: vec![],
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
    }
}
