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
