use anchor_lang::prelude::*;

declare_id!("MockCpi111111111111111111111111111111111111");

#[program]
pub mod mock_cpi {
    use super::*;

    // Instruction that always fails with a custom error
    pub fn fail_now(_ctx: Context<FailNow>) -> Result<()> {
        err!(MockError::AlwaysFail)
    }
}

#[derive(Accounts)]
pub struct FailNow {}

#[error_code]
pub enum MockError {
    #[msg("Mock CPI always fails")]
    AlwaysFail,
}
