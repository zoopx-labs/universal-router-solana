// Temporary allow: Anchor/solana-program macros emit cfg probes (custom-heap, custom-panic, anchor-debug, etc.)
// that surface as `unexpected_cfgs` under newer rustc check-cfg linting. Until dependency
// versions are upgraded, suppress them here so workspace clippy with `-D warnings` passes.
#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
declare_id!("11111111111111111111111111111111");
#[program]
pub mod zpx_lp_vaults {
    use super::*;
    pub fn ping(_ctx: Context<Ping>) -> Result<()> {
        Ok(())
    }
}
#[derive(Accounts)]
pub struct Ping<'info> {
    pub _signer: Signer<'info>,
}
