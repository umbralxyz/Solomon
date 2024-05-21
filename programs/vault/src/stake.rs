use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

#[program]
pub mod stake {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, max_cooldown: u64, admin: Pubkey) -> Result<()> {
        let stake_state = &mut ctx.accounts.stake_state;
        stake_state.max_cooldown = max_cooldown;
        stake_state.cooldown = max_cooldown;
        stake_state.admin = admin;
        Ok(())
    }

    pub fn set_cooldown_duration(ctx: Context<SetCooldownDuration>, duration: u64) -> Result<()> {
        let stake_state = &mut ctx.accounts.stake_state;
        require!(duration <= stake_state.max_cooldown, StakeError::InvalidCooldown);
        stake_state.cooldown = duration;
        Ok(())
    }

    pub fn cooldown_assets(ctx: Context<Cooldown>, assets: u64, owner: Pubkey) -> Result<()> {
        let cooldown = &mut ctx.accounts.user_cooldown;
        let clock = Clock::get()?;
        cooldown.cooldown_end = clock.unix_timestamp as u64 + ctx.accounts.stake_state.cooldown;
        cooldown.underlying_amount += assets;
        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        let cooldown = &mut ctx.accounts.user_cooldown;
        let clock = Clock::get()?;
        if clock.unix_timestamp as u64 >= cooldown.cooldown_end {
            let assets = cooldown.underlying_amount;
            cooldown.cooldown_end = 0;
            cooldown.underlying_amount = 0;

            let cpi_accounts = Transfer {
                from: ctx.accounts.user_staked_token_account.to_account_info(),
                to: ctx.accounts.vault_staked_token_account.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
            token::transfer(cpi_ctx, assets)?;

            let cpi_accounts = Transfer {
                from: ctx.accounts.vault_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let seeds = &[b"vault".as_ref(), &[ctx.accounts.stake_state.vault_bump]];
            let signer = &[&seeds[..]];
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, assets)?;
        } else {
            return Err(StakeError::InvalidCooldown.into());
        }
        Ok(())
    }

    
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = admin, space = 8 + 128)]
    pub stake_state: Account<'info, StakeState>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetCooldownDuration<'info> {
    #[account(mut)]
    pub stake_state: Account<'info, StakeState>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct Cooldown<'info> {
    #[account(mut)]
    pub stake_state: Account<'info, StakeState>,
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + 16,
        seeds = [b"user_cooldown", user.key().as_ref()],
        bump
    )]
    pub user_cooldown: Account<'info, UserCooldown>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(mut)]
    pub stake_state: Account<'info, StakeState>,
    #[account(mut)]
    pub user_cooldown: Account<'info, UserCooldown>,
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut)]
    pub user_staked_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_staked_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct StakeState {
    pub cooldown: u64,
    pub max_cooldown: u64,
    pub min_shares: u64,
    pub vesting_period: u64,
    pub vesting_amount: u64,
    pub last_distribution_time: u64,
    pub rewarders: Vec<Pubkey>,
    pub admin: Pubkey,
}

#[account]
pub struct UserCooldown {
    pub cooldown_end: u64,
    pub underlying_amount: u64,
}

#[error_code]
pub enum StakeError {
    #[msg("Invalid cooldown duration")]
    InvalidCooldown,
    #[msg("Invalid cooldown state")]
    InvalidCooldownState,
}