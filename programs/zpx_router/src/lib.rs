// SPDX-License-Identifier: MIT
#![allow(unexpected_cfgs)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
use anchor_lang::prelude::*;
use anchor_lang::system_program::{self, CreateAccount};
use anchor_spl::token::{self as token, Mint, Token, TokenAccount, TransferChecked};

pub mod hash;
use anchor_lang::solana_program::pubkey::Pubkey;
use hash::{global_route_id, keccak256, message_hash_be};

declare_id!("Adx7Rd5zT1fRiTfat69nf3snARTHECvdqFGkirStpQdY");

const FEE_CAP_BPS: u16 = 5; // protocol fee cap (0.05%)
const RELAYER_FEE_CAP_BPS: u16 = 1000; // relayer fee cap (10%) – adjustable in config

#[program]
pub mod zpx_router {
    use super::*;

    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        admin: Pubkey,
        fee_recipient: Pubkey,
        src_chain_id: u64,
        relayer_fee_bps: u16,
    ) -> Result<()> {
        // Prevent deploying with placeholder program id
        require!(
            crate::ID.to_string() != "11111111111111111111111111111111",
            ErrorCode::PlaceholderProgramId
        );
        require!(
            relayer_fee_bps <= RELAYER_FEE_CAP_BPS,
            ErrorCode::RelayerFeeTooHigh
        );
        let cfg = &mut ctx.accounts.config;
        cfg.admin = admin;
        cfg.fee_recipient = fee_recipient;
        cfg.src_chain_id = src_chain_id;
        cfg.relayer_fee_bps = relayer_fee_bps;
        cfg.adapters_len = 0;
        cfg.adapters = [Pubkey::default(); 8];
        cfg.paused = false;
        cfg.bump = ctx.bumps.config;
        emit!(ConfigUpdated {
            admin,
            fee_recipient,
            src_chain_id,
            relayer_fee_bps
        });
        Ok(())
    }

    pub fn update_config(
        ctx: Context<UpdateConfig>,
        fee_recipient: Option<Pubkey>,
        src_chain_id: Option<u64>,
        relayer_fee_bps: Option<u16>,
        paused: Option<bool>,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        // Explicit admin check (defense in depth)
        require!(
            cfg.admin == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );
        if let Some(fr) = fee_recipient {
            cfg.fee_recipient = fr;
        }
        if let Some(s) = src_chain_id {
            cfg.src_chain_id = s;
        }
        if let Some(r) = relayer_fee_bps {
            require!(r <= RELAYER_FEE_CAP_BPS, ErrorCode::RelayerFeeTooHigh);
            cfg.relayer_fee_bps = r;
        }
        if let Some(p) = paused {
            cfg.paused = p;
        }
        emit!(ConfigUpdated {
            admin: cfg.admin,
            fee_recipient: cfg.fee_recipient,
            src_chain_id: cfg.src_chain_id,
            relayer_fee_bps: cfg.relayer_fee_bps
        });
        Ok(())
    }

    pub fn add_adapter(ctx: Context<AdminConfig>, adapter: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        // Explicit admin check (defense in depth)
        require!(
            cfg.admin == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );
        let len = cfg.adapters_len as usize;
        for i in 0..len {
            if cfg.adapters[i] == adapter {
                return err!(ErrorCode::AdapterAlreadyExists);
            }
        }
        require!(len < 8, ErrorCode::AdapterListFull);
        cfg.adapters[len] = adapter;
        cfg.adapters_len += 1;
        emit!(AdapterAdded {
            admin: cfg.admin,
            program: adapter
        });
        Ok(())
    }

    pub fn remove_adapter(ctx: Context<AdminConfig>, adapter: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        // Explicit admin check (defense in depth)
        require!(
            cfg.admin == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );
        let len = cfg.adapters_len as usize;
        let mut idx = None;
        for i in 0..len {
            if cfg.adapters[i] == adapter {
                idx = Some(i);
                break;
            }
        }
        let i = idx.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
        let last = len - 1;
        if i != last {
            cfg.adapters[i] = cfg.adapters[last];
        }
        cfg.adapters[last] = Pubkey::default();
        cfg.adapters_len -= 1;
        emit!(AdapterRemoved {
            admin: cfg.admin,
            program: adapter
        });
        Ok(())
    }

    /// Thin source-leg entrypoint (no vault logic). Pull -> skim -> forward -> emit.
    pub fn universal_bridge_transfer(
        ctx: Context<UniversalBridgeTransfer>,
        amount: u64,
        protocol_fee: u64,
        relayer_fee: u64,
        payload: Vec<u8>,
        dst_chain_id: u64,
        nonce: u64,
    ) -> Result<()> {
        let cfg = &ctx.accounts.config;
        // Chain id width guard to avoid silent truncation when emitting u16
        require!(
            cfg.src_chain_id <= u16::MAX as u64 && dst_chain_id <= u16::MAX as u64,
            ErrorCode::ChainIdOutOfRange
        );
        // Defensive: correct token program
        require!(
            ctx.accounts.token_program.key() == Token::id(),
            ErrorCode::InvalidTokenProgram
        );
        require!(!cfg.paused, ErrorCode::Paused);
        require!(cfg.src_chain_id != 0, ErrorCode::SrcChainNotSet);
        validate_common(amount, payload.len(), cfg.paused, cfg.src_chain_id)?;
        validate_payload_len(payload.len())?;
        // Adapter allowlist: ensure target is allowed
        require!(
            is_allowed_adapter_cfg(cfg, &ctx.accounts.target_adapter_program.key()),
            ErrorCode::AdapterNotAllowed
        );
        let (forward_amount, total_fees) =
            compute_fees_and_forward(amount, protocol_fee, relayer_fee, cfg.relayer_fee_bps)?;

        // Strict ATA derivation: ensure provided ATA matches expected associated account for fee recipient
        // Use the associated token program PDA derivation with token program id as parameter.
        // Expected = get_associated_token_address_with_program_id(fee_recipient, mint, token_program.key())
        let ata_seeds: &[&[u8]] = &[
            &cfg.fee_recipient.to_bytes(),
            &ctx.accounts.token_program.key().to_bytes(),
            &ctx.accounts.mint.key().to_bytes(),
        ];
        let (expected_fee_ata, _bump) =
            Pubkey::find_program_address(ata_seeds, &anchor_spl::associated_token::ID);
        require!(
            ctx.accounts.fee_recipient_ata.key() == expected_fee_ata,
            ErrorCode::InvalidFeeRecipientAta
        );
        // Extra checks for safety
        require!(
            ctx.accounts.fee_recipient_ata.owner == Token::id(),
            ErrorCode::InvalidTokenProgram
        );
        require!(
            ctx.accounts.fee_recipient_ata.mint == ctx.accounts.mint.key(),
            ErrorCode::InvalidFeeRecipientAta
        );

        // Transfer: user -> fee_recipient (fees)
        if total_fees > 0 {
            token::transfer_checked(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        mint: ctx.accounts.mint.to_account_info(),
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.fee_recipient_ata.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                total_fees,
                ctx.accounts.mint.decimals,
            )?;
        }

        // Transfer: user -> target (forward amount)
        if forward_amount > 0 {
            token::transfer_checked(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        mint: ctx.accounts.mint.to_account_info(),
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.target_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                forward_amount,
                ctx.accounts.mint.decimals,
            )?;
        }

        // Canonical hashes
        let payload_hash = keccak256(&[payload.as_slice()]);
        let src_adapter_32 = ctx.accounts.target_adapter_program.key().to_bytes(); // adapter-agnostic: target program as srcAdapter
        let recipient_32 = [0u8; 32]; // unknown on source leg (recipient resolved on dest)
        let asset_32 = ctx.accounts.mint.key().to_bytes();
        let mut amount_be = [0u8; 32];
        amount_be[16..].copy_from_slice(&(forward_amount as u128).to_be_bytes());
        let msg_hash = message_hash_be(
            cfg.src_chain_id,
            src_adapter_32,
            recipient_32,
            asset_32,
            amount_be,
            payload_hash,
            nonce,
            dst_chain_id,
        );
        let initiator_32 = ctx.accounts.user.key().to_bytes();
        let global_route = global_route_id(
            cfg.src_chain_id,
            dst_chain_id,
            initiator_32,
            msg_hash,
            nonce,
        );

        // Events per EVM schema
        emit!(BridgeInitiated {
            route_id: [0u8; 32],
            user: ctx.accounts.user.key(),
            token: ctx.accounts.mint.key(),
            target: ctx.accounts.target_adapter_program.key(),
            forwarded_amount: forward_amount,
            protocol_fee,
            relayer_fee,
            payload_hash,
            src_chain_id: cfg.src_chain_id as u16, // EVM uses u16; store u64 but emit lower 16 bits
            dst_chain_id: dst_chain_id as u16,
            nonce,
        });
        emit!(UniversalBridgeInitiated {
            route_id: [0u8; 32],
            payload_hash,
            message_hash: msg_hash,
            global_route_id: global_route,
            user: ctx.accounts.user.key(),
            token: ctx.accounts.mint.key(),
            target: ctx.accounts.target_adapter_program.key(),
            forwarded_amount: forward_amount,
            protocol_fee,
            relayer_fee,
            src_chain_id: cfg.src_chain_id as u16,
            dst_chain_id: dst_chain_id as u16,
            nonce,
        });
        if total_fees > 0 {
            emit!(FeeAppliedSource {
                message_hash: msg_hash,
                asset: ctx.accounts.mint.key(),
                payer: ctx.accounts.user.key(),
                target: ctx.accounts.target_adapter_program.key(),
                protocol_fee,
                relayer_fee,
                fee_recipient: cfg.fee_recipient,
                applied_at: Clock::get()?.unix_timestamp as u64,
            });
        }
        Ok(())
    }

    /// Destination finalize path (stateless): mark message replay and emit telemetry.
    /// No token movement. Creates a minimal PDA at [b"replay", message_hash] owned by this program.
    #[allow(clippy::too_many_arguments)]
    pub fn finalize_message_v1(
        ctx: Context<FinalizeMessageV1>,
        src_chain_id: u64,
        dst_chain_id: u64,
        forwarded_amount: u64,
        nonce: u64,
        payload_hash: [u8; 32],
        src_adapter: Pubkey,
        asset_mint: Pubkey,
        _initiator: Pubkey,
    ) -> Result<()> {
        // Build canonical message hash matching source-leg schema
        let src_adapter_32 = src_adapter.to_bytes();
        let recipient_32 = [0u8; 32];
        let asset_32 = asset_mint.to_bytes();
        let mut amount_be = [0u8; 32];
        amount_be[16..].copy_from_slice(&(forwarded_amount as u128).to_be_bytes());
        let message_hash = message_hash_be(
            src_chain_id,
            src_adapter_32,
            recipient_32,
            asset_32,
            amount_be,
            payload_hash,
            nonce,
            dst_chain_id,
        );

        // Chain id width guard to avoid truncation when emitting u16
        require!(
            src_chain_id <= u16::MAX as u64 && dst_chain_id <= u16::MAX as u64,
            ErrorCode::ChainIdOutOfRange
        );

        // Recompute canonical message_hash (parity) and derive PDA; reject if mismatch
        let recomputed = message_hash_be(
            src_chain_id,
            src_adapter_32,
            recipient_32,
            asset_32,
            amount_be,
            payload_hash,
            nonce,
            dst_chain_id,
        );
        require!(recomputed == message_hash, ErrorCode::Unauthorized);

        // Derive PDA and ensure the provided account matches it
        let (expected_pda, bump) =
            Pubkey::find_program_address(&[b"replay", &message_hash], &crate::ID);
        require_keys_eq!(
            ctx.accounts.replay.key(),
            expected_pda,
            ErrorCode::Unauthorized
        );

        // If already initialized, this is a replay
        if ctx.accounts.replay.to_account_info().lamports() > 0 {
            return err!(ErrorCode::ReplayAlreadyUsed);
        }

        // Create minimal PDA owned by this program to mark replay. Use rent-exempt balance to avoid rent collection.
        let rent = Rent::get()?;
        let lamports = rent.minimum_balance(0);
        system_program::create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                CreateAccount {
                    from: ctx.accounts.relayer.to_account_info(),
                    to: ctx.accounts.replay.to_account_info(),
                },
                &[&[b"replay", &message_hash, &[bump]]],
            ),
            lamports,
            0,
            &crate::ID,
        )?;

        // Emit telemetry event (no fee movement in v1)
        emit!(FeeAppliedDest {
            message_hash,
            src_chain_id: src_chain_id as u16,
            dst_chain_id: dst_chain_id as u16,
            router: crate::ID,
            asset: asset_mint,
            amount: forwarded_amount,
            protocol_bps: 0,
            lp_bps: 0,
            collector: ctx.accounts.config.fee_recipient,
            applied_at: Clock::get()?.unix_timestamp as u64,
        });

        Ok(())
    }
}

// ------------ Accounts / Config / Events / Errors ------------
#[account]
pub struct Config {
    pub admin: Pubkey,
    pub fee_recipient: Pubkey,
    pub src_chain_id: u64,
    pub relayer_fee_bps: u16,
    pub adapters_len: u8,
    pub adapters: [Pubkey; 8],
    pub paused: bool,
    pub bump: u8,
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + 32 + 32 + 8 + 2 + 1 + (32*8) + 1 + 1,
        seeds = [b"zpx_config"],
        bump
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds=[b"zpx_config"],
        bump=config.bump,
        constraint = config.admin == authority.key() @ ErrorCode::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct AdminConfig<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct UniversalBridgeTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(mut, constraint = from.owner == user.key(), constraint = from.mint == mint.key())]
    pub from: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = fee_recipient_ata.mint == mint.key(),
        constraint = fee_recipient_ata.owner == config.fee_recipient @ ErrorCode::InvalidFeeRecipientAta
    )]
    pub fee_recipient_ata: Account<'info, TokenAccount>,
    #[account(mut, constraint = target_token_account.mint == mint.key())]
    pub target_token_account: Account<'info, TokenAccount>,
    /// CHECK: adapter program (CPI target); we don’t execute it here, just emit identity
    pub target_adapter_program: UncheckedAccount<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct FinalizeMessageV1<'info> {
    #[account(mut)]
    pub relayer: Signer<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    /// CHECK: PDA to be created at [b"replay", message_hash]
    #[account(mut)]
    pub replay: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// SCHEMA FROZEN. Do not reorder/rename. Bump with V2 if changes are required.
#[event]
pub struct BridgeInitiated {
    pub route_id: [u8; 32],
    pub user: Pubkey,
    pub token: Pubkey,
    pub target: Pubkey,
    pub forwarded_amount: u64,
    pub protocol_fee: u64,
    pub relayer_fee: u64,
    pub payload_hash: [u8; 32],
    pub src_chain_id: u16,
    pub dst_chain_id: u16,
    pub nonce: u64,
}

/// SCHEMA FROZEN. Do not reorder/rename. Bump with V2 if changes are required.
#[event]
pub struct UniversalBridgeInitiated {
    pub route_id: [u8; 32],
    pub payload_hash: [u8; 32],
    pub message_hash: [u8; 32],
    pub global_route_id: [u8; 32],
    pub user: Pubkey,
    pub token: Pubkey,
    pub target: Pubkey,
    pub forwarded_amount: u64,
    pub protocol_fee: u64,
    pub relayer_fee: u64,
    pub src_chain_id: u16,
    pub dst_chain_id: u16,
    pub nonce: u64,
}

/// SCHEMA FROZEN. Do not reorder/rename. Bump with V2 if changes are required.
#[event]
pub struct FeeAppliedSource {
    pub message_hash: [u8; 32],
    pub asset: Pubkey,
    pub payer: Pubkey,
    pub target: Pubkey,
    pub protocol_fee: u64,
    pub relayer_fee: u64,
    pub fee_recipient: Pubkey,
    pub applied_at: u64,
}

/// SCHEMA FROZEN. Do not reorder/rename. Bump with V2 if changes are required.
#[event]
pub struct FeeAppliedDest {
    pub message_hash: [u8; 32],
    pub src_chain_id: u16,
    pub dst_chain_id: u16,
    pub router: Pubkey,
    pub asset: Pubkey,
    pub amount: u64,
    pub protocol_bps: u16,
    pub lp_bps: u16,
    pub collector: Pubkey,
    pub applied_at: u64,
}

#[event]
pub struct AdapterAdded {
    pub admin: Pubkey,
    pub program: Pubkey,
}
#[event]
pub struct AdapterRemoved {
    pub admin: Pubkey,
    pub program: Pubkey,
}
#[event]
pub struct ConfigUpdated {
    pub admin: Pubkey,
    pub fee_recipient: Pubkey,
    pub src_chain_id: u64,
    pub relayer_fee_bps: u16,
}

/// Exposed schema snapshots (field names and order) for tests and tooling
pub const BRIDGE_INITIATED_FIELDS: &[&str] = &[
    "route_id",
    "user",
    "token",
    "target",
    "forwarded_amount",
    "protocol_fee",
    "relayer_fee",
    "payload_hash",
    "src_chain_id",
    "dst_chain_id",
    "nonce",
];

pub const UNIVERSAL_BRIDGE_INITIATED_FIELDS: &[&str] = &[
    "route_id",
    "payload_hash",
    "message_hash",
    "global_route_id",
    "user",
    "token",
    "target",
    "forwarded_amount",
    "protocol_fee",
    "relayer_fee",
    "src_chain_id",
    "dst_chain_id",
    "nonce",
];

pub const FEE_APPLIED_SOURCE_FIELDS: &[&str] = &[
    "message_hash",
    "asset",
    "payer",
    "target",
    "protocol_fee",
    "relayer_fee",
    "fee_recipient",
    "applied_at",
];

pub const FEE_APPLIED_DEST_FIELDS: &[&str] = &[
    "message_hash",
    "src_chain_id",
    "dst_chain_id",
    "router",
    "asset",
    "amount",
    "protocol_bps",
    "lp_bps",
    "collector",
    "applied_at",
];

#[error_code]
pub enum ErrorCode {
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Paused")]
    Paused,
    #[msg("Source chain id not set")]
    SrcChainNotSet,
    #[msg("Zero-amount not allowed")]
    ZeroAmount,
    #[msg("Payload too large")]
    PayloadTooLarge,
    #[msg("Protocol fee too high")]
    ProtocolFeeTooHigh,
    #[msg("Relayer fee too high")]
    RelayerFeeTooHigh,
    #[msg("Fees exceed amount")]
    FeesExceedAmount,
    #[msg("Adapter already exists")]
    AdapterAlreadyExists,
    #[msg("Adapter not allowed")]
    AdapterNotAllowed,
    #[msg("Adapter list full")]
    AdapterListFull,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Invalid token program")]
    InvalidTokenProgram,
    #[msg("Chain id out of range for u16 emission")]
    ChainIdOutOfRange,
    #[msg("Replay already used")]
    ReplayAlreadyUsed,
    #[msg("Invalid fee recipient ATA")]
    InvalidFeeRecipientAta,
    #[msg("Placeholder program id used; replace with real id")]
    PlaceholderProgramId,
}

/// Compute and validate fees per caps; returns (forward_amount, total_fees)
pub fn compute_fees_and_forward(
    amount: u64,
    protocol_fee: u64,
    relayer_fee: u64,
    relayer_bps_cap: u16,
) -> Result<(u64, u64)> {
    require!(amount > 0, ErrorCode::ZeroAmount);
    // Protocol fee cap: 5 bps of amount
    require!(
        (protocol_fee as u128) * 10_000u128 <= (amount as u128) * (FEE_CAP_BPS as u128),
        ErrorCode::ProtocolFeeTooHigh
    );
    if relayer_bps_cap > 0 {
        require!(
            (relayer_fee as u128) * 10_000u128 <= (amount as u128) * (relayer_bps_cap as u128),
            ErrorCode::RelayerFeeTooHigh
        );
    }
    let total_fees = protocol_fee
        .checked_add(relayer_fee)
        .ok_or(ErrorCode::MathOverflow)?;
    require!(total_fees <= amount, ErrorCode::FeesExceedAmount);
    let forward_amount = amount - total_fees;
    Ok((forward_amount, total_fees))
}

fn is_allowed_adapter_cfg(cfg: &Config, program: &Pubkey) -> bool {
    let len = cfg.adapters_len as usize;
    for i in 0..len {
        if cfg.adapters[i] == *program {
            return true;
        }
    }
    false
}

/// Validate common preconditions used by UBT
pub fn validate_common(
    amount: u64,
    payload_len: usize,
    paused: bool,
    src_chain_id: u64,
) -> Result<()> {
    require!(!paused, ErrorCode::Paused);
    require!(src_chain_id != 0, ErrorCode::SrcChainNotSet);
    require!(amount > 0, ErrorCode::ZeroAmount);
    require!(payload_len <= 512, ErrorCode::PayloadTooLarge);
    Ok(())
}

/// Validate payload size only (exposed for tests)
pub fn validate_payload_len(payload_len: usize) -> Result<()> {
    require!(payload_len <= 512, ErrorCode::PayloadTooLarge);
    Ok(())
}
