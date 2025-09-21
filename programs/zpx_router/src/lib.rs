// SPDX-License-Identifier: MIT
#![allow(unexpected_cfgs)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
// Allow some clippy lints at the crate level to keep the program ergonomic for
// now (many public entrypoints are long and Anchor's Result carries a large
// error type). These are deliberate tradeoffs for readability in this repo.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::result_large_err)]
#![allow(clippy::field_reassign_with_default)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::token::{self as token, Mint, Token, TokenAccount};
use spl_token::state::Account as SplAccount;

// Updated to use vault-program.json derived pubkey
declare_id!("zoopxFVyJcE2LAcMqDnKjWx9jv7UWDkDvqviVVypVPz");

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
        protocol_fee_bps: u16,
        relayer_pubkey: Pubkey,
        accept_any_token: bool,
        allowed_token_mint: Pubkey,
        direct_relayer_payout_default: bool,
        min_forward_amount: u64,
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
        require!(
            protocol_fee_bps <= FEE_CAP_BPS,
            ErrorCode::ProtocolFeeTooHigh
        );
        let cfg = &mut ctx.accounts.config;
        cfg.admin = admin;
        cfg.fee_recipient = fee_recipient;
        cfg.src_chain_id = src_chain_id;
        cfg.relayer_fee_bps = relayer_fee_bps;
        cfg.protocol_fee_bps = protocol_fee_bps;
        cfg.relayer_pubkey = relayer_pubkey;
        cfg.accept_any_token = accept_any_token;
        cfg.allowed_token_mint = allowed_token_mint;
        cfg.direct_relayer_payout_default = direct_relayer_payout_default;
        cfg.min_forward_amount = min_forward_amount;
        cfg.adapters_len = 0;
        cfg.adapters = [Pubkey::default(); 8];
        cfg.paused = false;
        cfg.bump = ctx.bumps.get("config").copied().unwrap();
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
        protocol_fee_bps: Option<u16>,
        relayer_pubkey: Option<Pubkey>,
        accept_any_token: Option<bool>,
        allowed_token_mint: Option<Pubkey>,
        direct_relayer_payout_default: Option<bool>,
        min_forward_amount: Option<u64>,
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
        if let Some(pfb) = protocol_fee_bps {
            require!(pfb <= FEE_CAP_BPS, ErrorCode::ProtocolFeeTooHigh);
            cfg.protocol_fee_bps = pfb;
        }
        if let Some(rp) = relayer_pubkey {
            cfg.relayer_pubkey = rp;
        }
        if let Some(aat) = accept_any_token {
            cfg.accept_any_token = aat;
        }
        if let Some(atm) = allowed_token_mint {
            cfg.allowed_token_mint = atm;
        }
        if let Some(d) = direct_relayer_payout_default {
            cfg.direct_relayer_payout_default = d;
        }
        if let Some(m) = min_forward_amount {
            cfg.min_forward_amount = m;
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

    pub fn initialize_registry(ctx: Context<InitializeRegistry>) -> Result<()> {
        let mut registry = ctx.accounts.registry.load_init()?;
        registry.spokes_len = 0;
        registry.bump = ctx.bumps.get("registry").copied().unwrap();
        Ok(())
    }

    pub fn admin_withdraw(ctx: Context<AdminWithdraw>, amount: u64) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(
            cfg.admin == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );
        // Validate vault: accept either (A) token account address == PDA, or
        // (B) token account's authority == PDA. Return the bump for signer seeds.
        let (bump, _expected_vault) = validate_vault_pda_or_authority(
            &ctx.accounts.hub_protocol_vault,
            &ctx.accounts.mint.key(),
            ctx.program_id,
        )?;

        // Use program-signed CPI to move tokens from the PDA vault to the destination
        let signer_seeds: &[&[&[u8]]] = &[&[
            b"hub_protocol_vault",
            &ctx.accounts.mint.key().to_bytes(),
            &[bump],
        ]];

        // Determine which AccountInfo to use as the authority for the CPI:
        // - If the token account itself is the PDA (address == expected_vault), use its AccountInfo
        // - Otherwise use the provided `hub_protocol_pda` AccountInfo (unchecked PDA)
        let expected_vault = Pubkey::create_program_address(
            &[
                b"hub_protocol_vault",
                &ctx.accounts.mint.key().to_bytes(),
                &[bump],
            ],
            ctx.program_id,
        )
        .ok();
        let authority_ai =
            if Some(*ctx.accounts.hub_protocol_vault.to_account_info().key) == expected_vault {
                ctx.accounts.hub_protocol_vault.to_account_info()
            } else {
                ctx.accounts.hub_protocol_pda.to_account_info()
            };

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.hub_protocol_vault.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: authority_ai.clone(),
                },
                signer_seeds,
            ),
            amount,
        )?;
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
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.fee_recipient_ata.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                total_fees,
            )?;
        }

        // Transfer: user -> target (forward amount)
        if forward_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.target_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                forward_amount,
            )?;
        }

        // Phase‑1: hashing/finalization removed. Use zeroed placeholder values where
        // tests expect a 32-byte hash to be available in emitted events.
        let payload_hash = [0u8; 32];
        let _src_adapter_32 = ctx.accounts.target_adapter_program.key().to_bytes();
        let _recipient_32 = [0u8; 32];
        let _asset_32 = ctx.accounts.mint.key().to_bytes();
        let mut amount_be = [0u8; 32];
        amount_be[16..].copy_from_slice(&(forward_amount as u128).to_be_bytes());
        let msg_hash = [0u8; 32];
        let _initiator_32 = ctx.accounts.user.key().to_bytes();
        let global_route = [0u8; 32];

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

    // Test helper: perform a CPI to the provided adapter program. Used by program-tests
    // to validate CPI failure handling and rollback semantics.
    pub fn bridge_with_adapter_cpi(ctx: Context<BridgeWithAdapterCpi>) -> Result<()> {
        // Build instruction data: adapter's `fail_now` has no args, instruction index 0
        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: ctx.accounts.adapter_program.key(),
            accounts: vec![],
            data: vec![0u8],
        };
        // Perform CPI and propagate error. Pass the adapter account info so the runtime
        // has ownership/context for the CPI.
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[ctx.accounts.adapter_program.to_account_info()],
        )
        .map_err(|_| error!(ErrorCode::Unauthorized))?;
        Ok(())
    }

    /// Phase-2: adapter passthrough CPI. This is a thin wrapper that forwards the
    /// net amount and calls the adapter program's expected entrypoint. The account
    /// layout for adapters will be formalized in Phase-2; for now this shows the
    /// intended wiring so tests and CI can exercise CPI flow.
    pub fn adapter_passthrough(
        ctx: Context<AdapterPassthrough>,
        instruction_data: Vec<u8>,
    ) -> Result<()> {
        // Forwarding to adapter is authorized by the hub's relayer/admin logic in forward_via_spoke
        // Here we simply perform a CPI into the adapter with the provided instruction data.
        // Provide the adapter with the message and replay account infos so the adapter
        // can perform replay-guard logic. The account order convention here is:
        // [message_account, replay_account]
        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: ctx.accounts.adapter_program.key(),
            accounts: vec![
                anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                    *ctx.accounts.message_account.to_account_info().key,
                    false,
                ),
                anchor_lang::solana_program::instruction::AccountMeta::new(
                    *ctx.accounts.replay_account.to_account_info().key,
                    false,
                ),
            ],
            data: instruction_data,
        };
        // Pass the message and replay account infos to the invoked program so it can
        // inspect and/or mutate the replay account.
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.message_account.to_account_info(),
                ctx.accounts.replay_account.to_account_info(),
            ],
        )
        .map_err(|_| error!(ErrorCode::Unauthorized))?;
        Ok(())
    }

    /// Hub: create a new spoke registry entry (admin-only)
    pub fn create_spoke(
        ctx: Context<CreateSpoke>,
        spoke_id: u32,
        adapter_program: Pubkey,
        direct_relayer_payout: bool,
        version: u8,
        metadata: Option<String>,
    ) -> Result<()> {
    let mut registry = ctx.accounts.registry.load_mut()?;
        // Only admin PDA or config.admin can create spokes
        let cfg = &ctx.accounts.config;
        require!(
            cfg.admin == ctx.accounts.authority.key() || ctx.accounts.admin.key() == cfg.admin,
            ErrorCode::Unauthorized
        );
        let len = registry.spokes_len as usize;
        require!(len < MAX_SPOKES, ErrorCode::AdapterListFull);
        // ensure unique spoke_id
        for i in 0..len {
            if registry.spokes[i].spoke_id == spoke_id {
                return err!(ErrorCode::AdapterAlreadyExists);
            }
        }
        let mut entry = SpokeEntry::default();
        entry.spoke_id = spoke_id;
        entry.adapter_program = adapter_program;
        entry.enabled = true;
        entry.paused = false;
        entry.direct_relayer_payout = direct_relayer_payout;
        entry.version = version;
        if let Some(m) = metadata {
            let bytes = m.as_bytes();
            let mut meta = [0u8; SPOKE_METADATA_LEN];
            meta[..bytes.len().min(SPOKE_METADATA_LEN)]
                .copy_from_slice(&bytes[..bytes.len().min(SPOKE_METADATA_LEN)]);
            entry.metadata = meta;
        }
        entry.created_at_slot = Clock::get()?.slot;
        registry.spokes[len] = entry;
        registry.spokes_len += 1;
        Ok(())
    }

    pub fn update_spoke(
        ctx: Context<UpdateSpoke>,
        spoke_id: u32,
        adapter_program: Option<Pubkey>,
        direct_relayer_payout: Option<bool>,
        paused: Option<bool>,
        metadata: Option<String>,
    ) -> Result<()> {
    let mut registry = ctx.accounts.registry.load_mut()?;
        let cfg = &ctx.accounts.config;
        require!(
            cfg.admin == ctx.accounts.authority.key() || ctx.accounts.admin.key() == cfg.admin,
            ErrorCode::Unauthorized
        );
        let len = registry.spokes_len as usize;
        let mut idx = None;
        for i in 0..len {
            if registry.spokes[i].spoke_id == spoke_id {
                idx = Some(i);
                break;
            }
        }
        let i = idx.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
        if let Some(p) = adapter_program {
            registry.spokes[i].adapter_program = p;
        }
        if let Some(d) = direct_relayer_payout {
            registry.spokes[i].direct_relayer_payout = d;
        }
        if let Some(p) = paused {
            registry.spokes[i].paused = p;
        }
        if let Some(m) = metadata {
            let bytes = m.as_bytes();
            let mut meta = [0u8; SPOKE_METADATA_LEN];
            meta[..bytes.len().min(SPOKE_METADATA_LEN)]
                .copy_from_slice(&bytes[..bytes.len().min(SPOKE_METADATA_LEN)]);
            registry.spokes[i].metadata = meta;
        }
        Ok(())
    }

    pub fn pause_spoke(ctx: Context<PauseSpoke>, spoke_id: u32) -> Result<()> {
    let mut registry = ctx.accounts.registry.load_mut()?;
        let cfg = &ctx.accounts.config;
        require!(
            cfg.admin == ctx.accounts.authority.key() || ctx.accounts.admin.key() == cfg.admin,
            ErrorCode::Unauthorized
        );
        let len = registry.spokes_len as usize;
        let mut idx = None;
        for i in 0..len {
            if registry.spokes[i].spoke_id == spoke_id {
                idx = Some(i);
                break;
            }
        }
        let i = idx.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
        registry.spokes[i].paused = true;
        Ok(())
    }

    pub fn enable_spoke(ctx: Context<PauseSpoke>, spoke_id: u32) -> Result<()> {
    let mut registry = ctx.accounts.registry.load_mut()?;
        let cfg = &ctx.accounts.config;
        require!(
            cfg.admin == ctx.accounts.authority.key() || ctx.accounts.admin.key() == cfg.admin,
            ErrorCode::Unauthorized
        );
        let len = registry.spokes_len as usize;
        let mut idx = None;
        for i in 0..len {
            if registry.spokes[i].spoke_id == spoke_id {
                idx = Some(i);
                break;
            }
        }
        let i = idx.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
        registry.spokes[i].paused = false;
        Ok(())
    }

    /// Forward via spoke: hub-level fee skimming and CPI into adapter
    #[allow(clippy::too_many_arguments)]
    pub fn forward_via_spoke(
        ctx: Context<ForwardViaSpoke>,
        spoke_id: u32,
        amount: u64,
        dst_domain: u32,
        _mint_recipient: [u8; 32],
        is_protocol_fee: bool,
        is_relayer_fee: bool,
        _nonce: u64,
    ) -> Result<()> {
        // Validate caller is relayer or admin
        let cfg = &ctx.accounts.config;
        require!(
            ctx.accounts.relayer.key() == cfg.relayer_pubkey
                || ctx.accounts.relayer.key() == cfg.admin,
            ErrorCode::Unauthorized
        );
        // Lookup spoke
    let registry = ctx.accounts.registry.load()?;
        let mut idx = None;
        for i in 0..(registry.spokes_len as usize) {
            if registry.spokes[i].spoke_id == spoke_id {
                idx = Some(i);
                break;
            }
        }
        let i = idx.ok_or_else(|| error!(ErrorCode::AdapterNotAllowed))?;
        let spoke = &registry.spokes[i];
        require!(spoke.enabled && !spoke.paused, ErrorCode::AdapterNotAllowed);

        // Enforce hub-level fee caps (configured on init/update)
        require!(
            cfg.protocol_fee_bps <= FEE_CAP_BPS,
            ErrorCode::ProtocolFeeTooHigh
        );
        require!(
            cfg.relayer_fee_bps <= RELAYER_FEE_CAP_BPS,
            ErrorCode::RelayerFeeTooHigh
        );

    // Compute fees (use hub-configured bps, and allow skipping via flags)
        require!(amount > 0, ErrorCode::ZeroAmount);
        let proto_fee = if is_protocol_fee {
            ((amount as u128) * (cfg.protocol_fee_bps as u128) / 10_000u128) as u64
        } else {
            0
        };
        let relayer_fee = if is_relayer_fee {
            ((amount as u128) * (cfg.relayer_fee_bps as u128) / 10_000u128) as u64
        } else {
            0
        };
        let total_fees = proto_fee
            .checked_add(relayer_fee)
            .ok_or(ErrorCode::MathOverflow)?;
        require!(total_fees <= amount, ErrorCode::FeesExceedAmount);
        let net_amount = amount - total_fees;
        require!(net_amount > 0, ErrorCode::ZeroAmount);

        // Unpack 'from' token account and validate ownership and mint
        let from_acc = SplAccount::unpack(&ctx.accounts.from.to_account_info().data.borrow())
            .map_err(|_| error!(ErrorCode::InvalidTokenProgram))?;
        require!(from_acc.owner == ctx.accounts.user.key(), ErrorCode::Unauthorized);
        require!(from_acc.mint == ctx.accounts.mint.key(), ErrorCode::InvalidTokenProgram);

        // Transfer fees to vaults or relayer
        // Protocol fee -> hub_protocol_fee_vault (PDA)
        // Validate vault PDAs are correct. The token accounts provided must have
        // their authority (owner field) set to the corresponding PDA and the
        // account data must be owned by the SPL Token program.
        // Validate protocol vault: accept either address==PDA or authority==PDA
        let _proto_bump = validate_vault_pda_or_authority(
            &ctx.accounts.hub_protocol_vault,
            &ctx.accounts.mint.key(),
            ctx.program_id,
        )?;
        // Validate relayer vault: accept either address==PDA or authority==PDA
        // Note: relayer vault uses seed "hub_relayer_vault". Unpack the token
        // account manually from the provided UncheckedAccount to avoid heavy
        // Anchor try_accounts logic which increases stack usage.
        let relayer_seeds: &[&[u8]] = &[b"hub_relayer_vault", &ctx.accounts.mint.key().to_bytes()];
        let (expected_relayer_vault, _rbump) = Pubkey::find_program_address(relayer_seeds, ctx.program_id);
        // Ensure SPL Token program owns relayer vault account data
        require!(ctx.accounts.hub_relayer_vault.to_account_info().owner == &token::ID, ErrorCode::InvalidTokenProgram);
        let relayer_acc = SplAccount::unpack(&ctx.accounts.hub_relayer_vault.to_account_info().data.borrow())
            .map_err(|_| error!(ErrorCode::InvalidVaultOwner))?;
        // Pattern A: relayer account address equals PDA -> check authority
        if ctx.accounts.hub_relayer_vault.to_account_info().key == &expected_relayer_vault {
            require_keys_eq!(relayer_acc.owner, expected_relayer_vault, ErrorCode::InvalidVaultOwner);
        } else {
            // Pattern B: the token account's authority equals the PDA
            require_keys_eq!(relayer_acc.owner, expected_relayer_vault, ErrorCode::InvalidVaultOwner);
        }
        if proto_fee > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.hub_protocol_vault.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                proto_fee,
            )?;
        }

        // Relayer fee -> direct payout or hub_relayer_vault
        if relayer_fee > 0 {
            if spoke.direct_relayer_payout || cfg.direct_relayer_payout_default {
                // Ensure relayer token account belongs to configured relayer pubkey
                let relayer_token_acc = SplAccount::unpack(&ctx.accounts.relayer_token_account.to_account_info().data.borrow())
                    .map_err(|_| error!(ErrorCode::Unauthorized))?;
                require!(relayer_token_acc.owner == cfg.relayer_pubkey, ErrorCode::Unauthorized);
                token::transfer(
                    CpiContext::new(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: ctx.accounts.from.to_account_info(),
                            to: ctx.accounts.relayer_token_account.to_account_info(),
                            authority: ctx.accounts.user.to_account_info(),
                        },
                    ),
                    relayer_fee,
                )?;
            } else {
                token::transfer(
                    CpiContext::new(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: ctx.accounts.from.to_account_info(),
                            to: ctx.accounts.hub_relayer_vault.to_account_info(),
                            authority: ctx.accounts.user.to_account_info(),
                        },
                    ),
                    relayer_fee,
                )?;
            }
        }

        // Transfer net amount to adapter target token account
        if net_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.from.to_account_info(),
                        to: ctx.accounts.adapter_target_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                net_amount,
            )?;
        }

        // CPI passthrough to adapter omitted in Phase 1 (TODO: add adapter CPI with explicit account layout)

        emit!(Forwarded {
            user: ctx.accounts.user.key(),
            relayer: ctx.accounts.relayer.key(),
            spoke_id,
            adapter_program: spoke.adapter_program,
            amount,
            protocol_fee: proto_fee,
            relayer_fee,
            net_amount,
            dst_domain,
            message_account: ctx.accounts.message_account.key(),
        });

        Ok(())
    }

    // Phase‑1: finalize/hash functionality removed. No entrypoint provided.
}

// ------------ Accounts / Config / Events / Errors ------------
#[account]
pub struct Config {
    pub admin: Pubkey,
    pub fee_recipient: Pubkey,
    pub src_chain_id: u64,
    pub relayer_fee_bps: u16,
    pub protocol_fee_bps: u16,
    pub relayer_pubkey: Pubkey,
    pub accept_any_token: bool,
    pub allowed_token_mint: Pubkey,
    pub direct_relayer_payout_default: bool,
    pub min_forward_amount: u64,
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
        // space calc: discriminator(8) + admin(32) + fee_recipient(32) + src_chain_id(8) + relayer_fee_bps(2)
        // + protocol_fee_bps(2) + relayer_pubkey(32) + accept_any_token(1) + allowed_token_mint(32)
        // + direct_relayer_payout_default(1) + min_forward_amount(8) + adapters_len(1) + adapters(32*8) + paused(1) + bump(1)
        space = 8 + 32 + 32 + 8 + 2 + 2 + 32 + 1 + 32 + 1 + 8 + 1 + (32*8) + 1 + 1,
        seeds = [b"zpx_config"],
        bump
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminWithdraw<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub hub_protocol_vault: Account<'info, TokenAccount>,
    /// CHECK: PDA for the hub protocol authority (used when token account authority==PDA)
    pub hub_protocol_pda: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(mut, constraint = destination.mint == mint.key())]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InitializeRegistry<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        // space calc: discriminator(8) + spokes_len(1) + spokes(MAX_SPOKES * per-spoke) + bump(1)
        // per-spoke conservative estimate: spoke_id(4) + adapter_program(32) + enabled(1) + paused(1)
        // + direct_relayer_payout(1) + version(1) + metadata(SPOKE_METADATA_LEN) + created_at_slot(8)
        // => ~64 bytes; use 80 to be conservative for padding/alignment
        space = 8 + 1 + (80 * MAX_SPOKES) + 1,
        seeds = [b"hub_registry"],
        bump
    )]
    pub registry: AccountLoader<'info, Registry>,
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
    #[account(mut, seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct CreateSpoke<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds=[b"hub_registry"], bump=registry.load()?.bump)]
    pub registry: AccountLoader<'info, Registry>,
    /// CHECK: admin PDA (optional)
    pub admin: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateSpoke<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds=[b"hub_registry"], bump=registry.load()?.bump)]
    pub registry: AccountLoader<'info, Registry>,
    /// CHECK: admin PDA (optional)
    pub admin: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct PauseSpoke<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds=[b"hub_registry"], bump=registry.load()?.bump)]
    pub registry: AccountLoader<'info, Registry>,
    /// CHECK: admin PDA (optional)
    pub admin: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ForwardViaSpoke<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    /// CHECK: relayer EOA invoking the forward
    pub relayer: Signer<'info>,
    pub mint: UncheckedAccount<'info>,
    #[account(mut)]
    pub from: UncheckedAccount<'info>,
    #[account(mut)]
    pub hub_protocol_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub hub_relayer_vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub relayer_token_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub adapter_target_token_account: UncheckedAccount<'info>,
    #[account(mut, seeds=[b"hub_registry"], bump=registry.load()?.bump)]
    pub registry: AccountLoader<'info, Registry>,
    #[account(seeds=[b"zpx_config"], bump=config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub message_account: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
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
pub struct BridgeWithAdapterCpi<'info> {
    /// CHECK: adapter program to CPI into
    pub adapter_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct AdapterPassthrough<'info> {
    /// CHECK: adapter program to CPI into
    pub adapter_program: UncheckedAccount<'info>,
    /// CHECK: message account passed to adapter
    pub message_account: UncheckedAccount<'info>,
    /// CHECK: replay PDA account the adapter will write to
    #[account(mut)]
    pub replay_account: UncheckedAccount<'info>,
}

#[account]
pub struct Replay {
    pub processed: u8,
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
    #[msg("Invalid fee recipient ATA")]
    InvalidFeeRecipientAta,
    #[msg("Placeholder program id used; replace with real id")]
    PlaceholderProgramId,
    // New replay-guard specific errors
    #[msg("Replay PDA does not match expected seeds")]
    InvalidReplayPda,
    #[msg("Replay account not owned by program")]
    InvalidReplayOwner,
    #[msg("Replay account too small")]
    ReplayAccountTooSmall,
    #[msg("Message has already been finalized (replay)")]
    ReplayAlreadyProcessed,
    #[msg("Computed hash mismatch")]
    HashMismatch,
    #[msg("Vault PDA does not match expected seeds")]
    InvalidVaultPda,
    #[msg("Vault account not owned by program")]
    InvalidVaultOwner,
    // Phase 1 intentionally removed finalize/hash surface; no FeatureRemoved variant retained.
}

// Phase‑1: canonical hashing and finalization removed. No local helpers retained.

// Hub-and-spoke constants
const MAX_SPOKES: usize = 32;
// Reduce spoke metadata length to shrink stack/frame sizes in Anchor-generated code
// and SBF verifier frame estimates. 16 bytes should be sufficient for small tags
// used in tests and reduces per-spoke storage from 64 -> 16.
const SPOKE_METADATA_LEN: usize = 16;

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

/// Spoke registry stored separately from Config. Fixed-size array-based registry for simplicity.
// Use zero-copy account for Registry to avoid large stack allocations during
// Anchor's generated `try_accounts` deserialization. Zero-copy requires fixed-size
// layouts and repr(C).
use anchor_lang::prelude::AccountLoader;

#[account(zero_copy)]
#[repr(C)]
pub struct Registry {
    pub spokes_len: u8,
    pub spokes: [SpokeEntry; MAX_SPOKES],
    pub bump: u8,
}

// Zero-copy struct for a spoke entry. Keep it repr(C) and Copy so it can be
// safely used in zero-copy accounts. Note: zero-copy structs must avoid
// variable-length types and implement Default manually.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SpokeEntry {
    pub spoke_id: u32,
    pub adapter_program: Pubkey,
    pub enabled: bool,
    pub paused: bool,
    pub direct_relayer_payout: bool,
    pub version: u8,
    pub metadata: [u8; SPOKE_METADATA_LEN],
    pub created_at_slot: u64,
}

/// Event emitted whenever a forward is executed via a spoke
#[event]
pub struct Forwarded {
    pub user: Pubkey,
    pub relayer: Pubkey,
    pub spoke_id: u32,
    pub adapter_program: Pubkey,
    pub amount: u64,
    pub protocol_fee: u64,
    pub relayer_fee: u64,
    pub net_amount: u64,
    pub dst_domain: u32,
    pub message_account: Pubkey,
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

/// Validate a hub vault token account accepting two patterns:
/// 1) The token account's pubkey equals the PDA derived from ["hub_protocol_vault", mint]
/// 2) The token account's authority (owner field inside TokenAccount) equals the PDA
///
/// In both cases the token account's account owner must be the SPL Token program.
///
/// Returns the bump for the PDA (for signer seeds) on success.
pub fn validate_vault_pda_or_authority(
    vault: &Account<TokenAccount>,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> Result<(u8, Pubkey)> {
    let seeds: &[&[u8]] = &[b"hub_protocol_vault", &mint.to_bytes()];
    let (expected_vault, bump) = Pubkey::find_program_address(seeds, program_id);
    // Ensure the SPL Token program owns the account data
    require!(
        vault.to_account_info().owner == &token::ID,
        ErrorCode::InvalidTokenProgram
    );
    // Pattern A: vault account address equals PDA
    if vault.to_account_info().key == &expected_vault {
        // Also ensure the token account's authority (owner field) equals the PDA
        require_keys_eq!(vault.owner, expected_vault, ErrorCode::InvalidVaultOwner);
        return Ok((bump, expected_vault));
    }
    // Pattern B: token account's authority equals PDA (account address differs)
    if vault.owner == expected_vault {
        return Ok((bump, expected_vault));
    }
    // neither pattern matched
    err!(ErrorCode::InvalidVaultPda)
}

/// Validate payload size only (exposed for tests)
pub fn validate_payload_len(payload_len: usize) -> Result<()> {
    require!(payload_len <= 512, ErrorCode::PayloadTooLarge);
    Ok(())
}

// Extended unit tests to increase coverage for fee logic, PDA derivation, and validators.
#[cfg(test)]
mod extended_tests {
    use super::*;
    use anchor_lang::solana_program::pubkey::Pubkey;

    #[test]
    fn compute_fees_and_forward_ok() {
        let amount = 100_000u64;
        let protocol_fee = 5u64;
        let relayer_fee = 50u64;
        let (forward, total) =
            compute_fees_and_forward(amount, protocol_fee, relayer_fee, 1000).unwrap();
        assert_eq!(total, protocol_fee + relayer_fee);
        assert_eq!(forward, amount - total);
    }

    #[test]
    fn compute_fees_and_forward_protocol_too_high() {
        let amount = 10_000u64;
        // Make protocol_fee exceed the allowed cap by computation
        let protocol_fee = ((amount as u128) * (FEE_CAP_BPS as u128) / 10_000u128) as u64 + 1;
        let res = compute_fees_and_forward(amount, protocol_fee, 0, RELAYER_FEE_CAP_BPS);
        assert!(res.is_err());
    }

    #[test]
    fn payload_len_validation() {
        assert!(validate_payload_len(0).is_ok());
        assert!(validate_payload_len(512).is_ok());
        assert!(validate_payload_len(513).is_err());
    }

    #[test]
    fn adapter_allowlist_behavior() {
        let program = Pubkey::new_unique();
        let mut cfg = Config {
            admin: Pubkey::default(),
            fee_recipient: Pubkey::default(),
            src_chain_id: 1,
            relayer_fee_bps: 0,
            protocol_fee_bps: 0,
            relayer_pubkey: Pubkey::default(),
            accept_any_token: false,
            allowed_token_mint: Pubkey::default(),
            direct_relayer_payout_default: false,
            min_forward_amount: 0,
            adapters_len: 0,
            adapters: [Pubkey::default(); 8],
            paused: false,
            bump: 0,
        };
        assert!(!is_allowed_adapter_cfg(&cfg, &program));
        cfg.adapters[0] = program;
        cfg.adapters_len = 1;
        assert!(is_allowed_adapter_cfg(&cfg, &program));
    }

    #[test]
    fn pda_derivation_stable() {
        let mint = Pubkey::new_unique();
        let (a, _) =
            Pubkey::find_program_address(&[b"hub_protocol_vault", &mint.to_bytes()], &crate::ID);
        let (b, _) =
            Pubkey::find_program_address(&[b"hub_protocol_vault", &mint.to_bytes()], &crate::ID);
        assert_eq!(a, b);
    }

    #[test]
    fn compute_fees_edge_exact_amount() {
        // A relayer fee that equals nearly the full amount will violate the relayer cap
        // and should return an error.
        let amount = 10_000u64;
        let protocol_fee = 5u64;
        let relayer_fee = amount - protocol_fee;
        let res = compute_fees_and_forward(amount, protocol_fee, relayer_fee, RELAYER_FEE_CAP_BPS);
        assert!(res.is_err());
    }

    #[test]
    fn emitted_schema_field_counts() {
        // Quick sanity check: the field slices reflect the declared event sizes
        assert!(BRIDGE_INITIATED_FIELDS.len() >= 10);
        assert!(UNIVERSAL_BRIDGE_INITIATED_FIELDS.len() >= 12);
        assert!(FEE_APPLIED_SOURCE_FIELDS.len() >= 8);
    }
}
