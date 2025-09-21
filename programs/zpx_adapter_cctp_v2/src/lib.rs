use anchor_lang::prelude::*;
use solana_program::msg;

declare_id!("CtTpV2adAp7er222222222222222222222222222222");

const REPLAY_SEED: &[u8] = b"adapter_replay";

#[program]
pub mod zpx_adapter_cctp_v2 {
    use super::*;

    pub fn process_transfer(
        ctx: Context<ProcessTransfer>,
        _message_id: [u8; 32],
        payload: Vec<u8>,
    ) -> Result<()> {
        let replay = &mut ctx.accounts.replay;
        if replay.processed != 0 {
            return err!(AdapterError::ReplayProcessed);
        }
        // Simulate parsing CCTP v2 payload: require payload len >= 2 and payload[0]==0 and payload[1]==1
        if payload.len() < 2 || payload[0] != 0u8 || payload[1] != 1u8 {
            return err!(AdapterError::InvalidPayload);
        }
        // Simulate burn action: emit event
        msg!("CCTP v2 adapter: simulated burn and attestation flow");
        emit!(Burned {
            message_id: _message_id,
            version: 2u8,
        });
        replay.processed = 1;
        Ok(())
    }
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

#[account]
pub struct Replay {
    pub processed: u8,
}

#[event]
pub struct Burned {
    pub message_id: [u8; 32],
    pub version: u8,
}

#[error_code]
pub enum AdapterError {
    #[msg("Invalid payload")]
    InvalidPayload,
    #[msg("Replay processed")]
    ReplayProcessed,
}
