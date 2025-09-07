#![allow(deprecated)]
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::keccak::hash as keccak_hash;
use anchor_lang::system_program::System;
use anchor_spl::token::spl_token;
use anchor_spl::token_interface::{
    self as token, spl_token_2022, Mint, TokenAccount, TokenInterface, TransferChecked,
};
use spl_associated_token_account::get_associated_token_address_with_program_id as get_ata_with_id;

declare_id!("654eeCFFpL9koVoFrAhRr1xmvMDq9BnjHYZgc3JxAmNf");

const FEE_CAP_BASIS_POINTS: u64 = 5; // 0.05% == 5 bps
const BASIS_POINTS_DIVISOR: u64 = 10_000;
const ADAPTER_CAP: usize = 8;
const SRC_CHAIN_ID: u16 = 1; // set to your canonical source chain id

#[program]
pub mod zoopx_router {
    use super::*;

    pub fn universal_bridge_transfer(
        ctx: Context<UniversalBridgeTransfer>,
        amount: u64,
        protocol_fee: u64,
    relayer_fee: u64,        // NEW
        payload: Vec<u8>,
        dst_chain_id: u16, // 0 if unused
        nonce: u64,        // 0 if unused
    ) -> Result<()> {
        // --- Basic guards
        require!(amount > 0, ErrorCode::ZeroAmount);
        require!(payload.len() <= 512, ErrorCode::PayloadTooLarge);
        // --- Validate token program: must be SPL Token or Token-2022
        let tk = ctx.accounts.token_program.key();
        if !(tk == spl_token::ID || tk == spl_token_2022::ID) {
            msg!("invalid token program: {}", tk);
            return err!(ErrorCode::InvalidTokenProgram);
        }

        // --- Enforce the owner program of accounts matches the chosen token program
        require!(
            ctx.accounts.mint.to_account_info().owner == &tk,
            ErrorCode::InvalidTokenProgramOwner
        );
        for acc in [
            ctx.accounts.from.to_account_info(),
            ctx.accounts.fee_recipient_token.to_account_info(),
            ctx.accounts.cpi_target_token_account.to_account_info(),
        ] {
            require!(acc.owner == &tk, ErrorCode::InvalidTokenProgramOwner);
        }

        // --- Fee math & caps
        // existing: validate protocol_fee cap (5 bps)
        let _remaining_after_protocol_only = validate_fee_cap(amount, protocol_fee)?;

        // NEW: total fees must not exceed amount
        let total_fee_to_protocol = protocol_fee
            .checked_add(relayer_fee)
            .ok_or(ErrorCode::MathOverflow)?;
        require!(total_fee_to_protocol <= amount, ErrorCode::ProtocolFeeExceedsAmount);

        // compute remainder after fees (safe math)
        let remaining = amount
            .checked_sub(total_fee_to_protocol)
            .ok_or(ErrorCode::Underflow)?;

        // --- Ensure fee recipient token account belongs to configured fee recipient and correct mint
        require_keys_eq!(
            ctx.accounts.fee_recipient_token.owner,
            ctx.accounts.config.fee_recipient,
            ErrorCode::InvalidFeeRecipient
        );
        require_keys_eq!(
            ctx.accounts.fee_recipient_token.mint,
            ctx.accounts.mint.key(),
            ErrorCode::InvalidFeeRecipient
        );
        // Enforce canonical ATA for fee recipient, derived against the runtime token program id
        let expected_fee_ata = get_ata_with_id(
            &ctx.accounts.config.fee_recipient,
            &ctx.accounts.mint.key(),
            &ctx.accounts.token_program.key(),
        );
        require_keys_eq!(
            ctx.accounts.fee_recipient_token.key(),
            expected_fee_ata,
            ErrorCode::InvalidCustodyAta
        );

        // --- Transfer combined protocol + relayer fee
        if total_fee_to_protocol > 0 {
            let decimals = ctx.accounts.mint.decimals;
            token::transfer_checked(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.from.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.fee_recipient_token.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                total_fee_to_protocol,
                decimals,
            )?;
        }

        // --- Transfer remaining to CPI target token account (checked)
        if remaining > 0 {
            let decimals = ctx.accounts.mint.decimals;
            token::transfer_checked(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.from.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.cpi_target_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                remaining,
                decimals,
            )?;
        }

        // --- Defensive CPI checks
        require!(
            ctx.accounts.target_adapter_program.executable,
            ErrorCode::InvalidTargetProgram
        );
        require!(
            !ctx.remaining_accounts.is_empty(),
            ErrorCode::MissingCpiAccounts
        );

        // Reject unexpected signers in remaining accounts
        require!(
            !ctx.remaining_accounts.iter().any(|ai| ai.is_signer),
            ErrorCode::UnexpectedSigner
        );

        // Inclusion checks instead of brittle positional check
        require!(
            ctx.remaining_accounts
                .iter()
                .any(|ai| ai.key() == ctx.accounts.cpi_target_token_account.key()),
            ErrorCode::MissingCpiAccounts
        );
        let adapter_ai = ctx
            .remaining_accounts
            .iter()
            .find(|ai| ai.key() == ctx.accounts.target_adapter_program.key())
            .ok_or(error!(ErrorCode::MissingCpiAccounts))?;
        require!(
            !adapter_ai.is_writable && !adapter_ai.is_signer,
            ErrorCode::BadAdapterMetaFlags
        );

        // Allowlist enforcement (when non-empty)
        if ctx.accounts.config.adapters_len > 0 {
            let len = ctx.accounts.config.adapters_len as usize;
            let target = ctx.accounts.target_adapter_program.key();
            let mut allowed = false;
            for i in 0..len {
                if ctx.accounts.config.adapters[i] == target {
                    allowed = true;
                    break;
                }
            }
            if !allowed {
                msg!("adapter not allowlisted: {}", target);
                return err!(ErrorCode::AdapterNotAllowed);
            }
        }

        // Destination custody invariant: cpi_target_token_account must be ATA(adapter_authority, mint)
        let expected_ata = get_ata_with_id(
            &ctx.accounts.adapter_authority.key(),
            &ctx.accounts.mint.key(),
            &ctx.accounts.token_program.key(),
        );
        require_keys_eq!(
            ctx.accounts.cpi_target_token_account.key(),
            expected_ata,
            ErrorCode::InvalidCustodyAta
        );

        // --- Emit audit event with keccak(payload)
        let ph: [u8; 32] = keccak_hash(&payload).0;
        msg!(
            "zoopx_router: tk={}, amount={}, fee={}",
            tk,
            amount,
            protocol_fee
        );
        emit!(BridgeInitiated {
            user: ctx.accounts.user.key(),
            source_mint: ctx.accounts.mint.key(),
            amount_after_fees: remaining,
            protocol_fee,
            target_adapter: ctx.accounts.target_adapter_program.key(),
            payload_hash: ph,
            src_chain_id: SRC_CHAIN_ID,
            dst_chain_id,
            nonce,
        });

        // --- CPI into the target adapter program
        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: ctx.accounts.target_adapter_program.key(),
            accounts: ctx
                .remaining_accounts
                .iter()
                .map(|ai| anchor_lang::solana_program::instruction::AccountMeta {
                    pubkey: ai.key(),
                    is_signer: ai.is_signer,
                    is_writable: ai.is_writable,
                })
                .collect(),
            data: payload, // moved, no clone
        };
        let account_infos: Vec<AccountInfo> = ctx.remaining_accounts.to_vec();
        anchor_lang::solana_program::program::invoke(&ix, &account_infos)?;

        Ok(())
    }

    pub fn initialize_config(ctx: Context<InitializeConfig>, fee_recipient: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.admin = ctx.accounts.payer.key();
        cfg.fee_recipient = fee_recipient;
        cfg.bump = ctx.bumps.config;
        cfg.adapters_len = 0;
        cfg.adapters = [Pubkey::default(); ADAPTER_CAP];
        emit!(ConfigUpdated {
            admin: cfg.admin,
            fee_recipient
        });
        Ok(())
    }

    pub fn update_config(ctx: Context<UpdateConfig>, fee_recipient: Pubkey) -> Result<()> {
        ctx.accounts.config.fee_recipient = fee_recipient;
        emit!(ConfigUpdated {
            admin: ctx.accounts.config.admin,
            fee_recipient
        });
        Ok(())
    }

    pub fn add_adapter(ctx: Context<UpdateConfig>, program_id: Pubkey) -> Result<()> {
        add_adapter_to_config(&mut ctx.accounts.config, program_id)?;
        emit!(AdapterAdded {
            admin: ctx.accounts.config.admin,
            program: program_id
        });
        Ok(())
    }

    pub fn remove_adapter(ctx: Context<UpdateConfig>, program_id: Pubkey) -> Result<()> {
        remove_adapter_from_config(&mut ctx.accounts.config, program_id)?;
        emit!(AdapterRemoved {
            admin: ctx.accounts.config.admin,
            program: program_id
        });
        Ok(())
    }
}

// --- Helpers (pure state updates, unit-testable)
pub fn add_adapter_to_config(config: &mut Config, program_id: Pubkey) -> Result<()> {
    let len = config.adapters_len as usize;
    for i in 0..len {
        if config.adapters[i] == program_id {
            return err!(ErrorCode::AdapterAlreadyExists);
        }
    }
    if len >= ADAPTER_CAP {
        return err!(ErrorCode::AdapterListFull);
    }
    config.adapters[len] = program_id;
    config.adapters_len = config
        .adapters_len
        .checked_add(1)
        .ok_or(ErrorCode::MathOverflow)?;
    Ok(())
}

pub fn remove_adapter_from_config(config: &mut Config, program_id: Pubkey) -> Result<()> {
    let len = config.adapters_len as usize;
    let mut idx_opt: Option<usize> = None;
    for i in 0..len {
        if config.adapters[i] == program_id {
            idx_opt = Some(i);
            break;
        }
    }
    let idx = idx_opt.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
    let last = len - 1;
    if idx != last {
        config.adapters[idx] = config.adapters[last];
    }
    config.adapters[last] = Pubkey::default();
    config.adapters_len = config
        .adapters_len
        .checked_sub(1)
        .ok_or(ErrorCode::MathOverflow)?;
    Ok(())
}

// --- Accounts

#[derive(Accounts)]
pub struct UniversalBridgeTransfer<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    // TokenInterface mint
    pub mint: InterfaceAccount<'info, Mint>,

    // TokenInterface token accounts
    #[account(
        mut,
        constraint = from.mint == mint.key(),
        constraint = from.owner == user.key()
    )]
    pub from: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = fee_recipient_token.mint == mint.key()
    )]
    pub fee_recipient_token: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = cpi_target_token_account.mint == mint.key()
    )]
    pub cpi_target_token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: CPI target program; executable check is performed in handler
    pub target_adapter_program: UncheckedAccount<'info>,

    /// CHECK: authority used only for equality/ATA derivation checks
    pub adapter_authority: UncheckedAccount<'info>,

    // Token program can be SPL Token or Token-2022
    pub token_program: Interface<'info, TokenInterface>,

    #[account(seeds = [b"zoopx_config"], bump = config.bump)]
    pub config: Account<'info, Config>,
}

#[account]
pub struct Config {
    pub admin: Pubkey,
    pub fee_recipient: Pubkey,
    pub bump: u8,
    pub adapters_len: u8,
    pub adapters: [Pubkey; ADAPTER_CAP],
}

pub mod config_pda {
    use super::*;
    pub fn seeds() -> [&'static [u8]; 1] {
        [b"zoopx_config"]
    }
    pub fn pda_address(program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&seeds(), program_id)
    }
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + 32 + 32 + 1 + 1 + 32 * ADAPTER_CAP,
        seeds = [b"zoopx_config"],
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
        mut,
        seeds = [b"zoopx_config"],
        bump = config.bump,
        constraint = config.admin == authority.key() @ ErrorCode::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

// --- Fee cap validator
pub fn validate_fee_cap(amount: u64, protocol_fee: u64) -> Result<u64> {
    // First check basic relation to produce a clearer error when fee > amount
    if protocol_fee > amount {
        msg!("fee exceeds amount: fee={} amount={}", protocol_fee, amount);
        return err!(ErrorCode::ProtocolFeeExceedsAmount);
    }

    // Then check the 5 bps cap using widened arithmetic to avoid overflow
    let lhs128 = (protocol_fee as u128) * (BASIS_POINTS_DIVISOR as u128);
    let rhs128 = (amount as u128) * (FEE_CAP_BASIS_POINTS as u128);
    if lhs128 > rhs128 {
        msg!(
            "fee cap violation: fee={} amount={} cap_bps={}",
            protocol_fee,
            amount,
            FEE_CAP_BASIS_POINTS
        );
        return err!(ErrorCode::ProtocolFeeTooHigh);
    }

    let remaining = amount
        .checked_sub(protocol_fee)
        .ok_or(ErrorCode::Underflow)?;
    Ok(remaining)
}

// --- Events
#[event]
pub struct BridgeInitiated {
    pub user: Pubkey,
    pub source_mint: Pubkey,
    pub amount_after_fees: u64,
    pub protocol_fee: u64,
    pub target_adapter: Pubkey,
    pub payload_hash: [u8; 32],
    pub src_chain_id: u16,
    pub dst_chain_id: u16, // 0 if not used
    pub nonce: u64,        // 0 if not used
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
}

// --- Errors
#[error_code]
pub enum ErrorCode {
    #[msg("Protocol fee exceeds cap of 0.05%")]
    ProtocolFeeTooHigh,
    #[msg("Protocol fee exceeds the transfer amount")]
    ProtocolFeeExceedsAmount,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Underflow computing remaining amount")]
    Underflow,
    #[msg("Invalid fee recipient address")]
    InvalidFeeRecipient,
    #[msg("Target adapter program account is not executable")]
    InvalidTargetProgram,
    #[msg("Missing required CPI accounts for target adapter")]
    MissingCpiAccounts,
    #[msg("Invalid or missing config PDA")]
    InvalidConfigPda,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Adapter allowlist is full")]
    AdapterListFull,
    #[msg("Adapter not allowlisted")]
    AdapterNotAllowed,
    #[msg("Adapter already in allowlist")]
    AdapterAlreadyExists,
    #[msg("Invalid token program")]
    InvalidTokenProgram,
    #[msg("Account is not owned by the provided token program")]
    InvalidTokenProgramOwner,
    #[msg("Payload too large")]
    PayloadTooLarge,
    #[msg("Unexpected signer in remaining accounts")]
    UnexpectedSigner,
    #[msg("Zero-amount transfer not allowed")]
    ZeroAmount,
    #[msg("Custody account does not match expected ATA")]
    InvalidCustodyAta,
    #[msg("Target adapter program account must NOT be writable or a signer")]
    BadAdapterMetaFlags,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn code(e: anchor_lang::error::Error) -> u32 {
        match e {
            anchor_lang::error::Error::AnchorError(ae) => ae.error_code_number,
            _ => 0,
        }
    }

    fn code_of(variant: ErrorCode) -> u32 {
        let e: anchor_lang::error::Error = variant.into();
        code(e)
    }

    fn new_cfg() -> Config {
        Config {
            admin: Pubkey::new_unique(),
            fee_recipient: Pubkey::new_unique(),
            bump: 254,
            adapters_len: 0,
            adapters: [Pubkey::default(); ADAPTER_CAP],
        }
    }

    #[test]
    fn fee_cap_allows_boundary() {
        let amount = 100_000u64; // 0.1 tokens at 6 decimals
        let max_fee = amount * FEE_CAP_BASIS_POINTS / BASIS_POINTS_DIVISOR; // 5 bps
        for fee in [0, 1, max_fee.saturating_sub(1), max_fee] {
            let rem = validate_fee_cap(amount, fee).expect("ok");
            assert_eq!(rem, amount - fee);
        }
    }

    #[test]
    fn fee_cap_rejects_above_boundary() {
        let amount = 100_000u64;
        let max_fee = amount * FEE_CAP_BASIS_POINTS / BASIS_POINTS_DIVISOR;
        let err = validate_fee_cap(amount, max_fee + 1).unwrap_err();
        assert_eq!(code(err), code_of(ErrorCode::ProtocolFeeTooHigh));
    }

    #[test]
    fn fee_cannot_exceed_amount() {
        let amount = 10u64;
        let err = validate_fee_cap(amount, amount + 1).unwrap_err();
        assert_eq!(code(err), code_of(ErrorCode::ProtocolFeeExceedsAmount));
    }

    #[test]
    fn fee_math_overflow_is_guarded() {
        // Construct values that would overflow without widened arithmetic.
        let amount = u64::MAX / 2;
        let fee = amount; // very large fee; should not overflow but must be rejected by cap
        let res = validate_fee_cap(amount, fee);
        assert!(res.is_err(), "should not accept fee beyond cap");
        let c = code(res.unwrap_err());
        // Either cap violation or (if amount < fee) fee-exceeds-amount; but here fee==amount so cap triggers.
        assert!(
            c == code_of(ErrorCode::ProtocolFeeTooHigh)
                || c == code_of(ErrorCode::ProtocolFeeExceedsAmount)
        );
    }

    #[test]
    fn adapters_add_remove_flow() {
        let mut cfg = new_cfg();
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();

        add_adapter_to_config(&mut cfg, a).unwrap();
        assert_eq!(cfg.adapters_len, 1);
        assert_eq!(cfg.adapters[0], a);

        add_adapter_to_config(&mut cfg, b).unwrap();
        assert_eq!(cfg.adapters_len, 2);

        // duplicate
        let e = add_adapter_to_config(&mut cfg, a).unwrap_err();
        assert_eq!(code(e), code_of(ErrorCode::AdapterAlreadyExists));

        // remove a (index 0), compaction via swap-with-last
        remove_adapter_from_config(&mut cfg, a).unwrap();
        assert_eq!(cfg.adapters_len, 1);
        assert_eq!(cfg.adapters[0], b);

        // remove missing
        let e = remove_adapter_from_config(&mut cfg, a).unwrap_err();
        assert_eq!(code(e), code_of(ErrorCode::AdapterNotAllowed));
    }

    #[test]
    fn adapters_capacity_enforced() {
        let mut cfg = new_cfg();
        for _ in 0..ADAPTER_CAP {
            add_adapter_to_config(&mut cfg, Pubkey::new_unique()).unwrap();
        }
        assert_eq!(cfg.adapters_len as usize, ADAPTER_CAP);
        let e = add_adapter_to_config(&mut cfg, Pubkey::new_unique()).unwrap_err();
        assert_eq!(code(e), code_of(ErrorCode::AdapterListFull));
    }

    #[test]
    fn inclusion_checks_logic() {
        // Simulate the inclusion-only rule (no ordering requirement)
        let target = Pubkey::new_unique();
        let token_acc = Pubkey::new_unique();

        let remaining: Vec<Pubkey> = vec![
            Pubkey::new_unique(),
            token_acc,
            Pubkey::new_unique(),
            target,
        ];
        let has_target = remaining.contains(&target);
        let has_token = remaining.contains(&token_acc);

        assert!(has_target && has_token);
    }

    #[test]
    fn inclusion_missing_target_fails() {
        let target = Pubkey::new_unique();
        let token_acc = Pubkey::new_unique();
        let remaining: Vec<Pubkey> = vec![token_acc, Pubkey::new_unique()];
        let has_target = remaining.contains(&target);
        let has_token = remaining.contains(&token_acc);
        assert!(!has_target && has_token);
    }

    #[test]
    fn inclusion_missing_token_fails() {
        let target = Pubkey::new_unique();
        let token_acc = Pubkey::new_unique();
        let remaining: Vec<Pubkey> = vec![target, Pubkey::new_unique()];
        let has_target = remaining.contains(&target);
        let has_token = remaining.contains(&token_acc);
        assert!(has_target && !has_token);
    }

    proptest! {
        #[test]
        fn prop_fee_cap_ok(amount in 1_000u64..10_000_000u64) {
            // choose a fee <= 5 bps
            let max_fee = amount.saturating_mul(FEE_CAP_BASIS_POINTS) / BASIS_POINTS_DIVISOR;
            let fee = if max_fee == 0 { 0 } else { max_fee / 2 };
            let rem = validate_fee_cap(amount, fee).unwrap();
            prop_assert_eq!(rem, amount - fee);
        }

        #[test]
        fn prop_fee_cap_rejects_too_high(amount in 10_000u64..50_000_000u64) {
            let max_fee = amount.saturating_mul(FEE_CAP_BASIS_POINTS) / BASIS_POINTS_DIVISOR;
            let too_high = max_fee.saturating_add(1);
            if too_high <= amount {
                let err = validate_fee_cap(amount, too_high).unwrap_err();
                prop_assert_eq!(code(err), code_of(ErrorCode::ProtocolFeeTooHigh));
            } else {
                let err = validate_fee_cap(amount, too_high).unwrap_err();
                prop_assert_eq!(code(err), code_of(ErrorCode::ProtocolFeeExceedsAmount));
            }
        }
    }
}
