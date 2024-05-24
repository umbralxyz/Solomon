use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

#[program]
pub mod stake {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, max_cooldown: u64) -> Result<()> {
        let vault_state = &mut ctx.accounts.vault_state;
        vault_state.max_cooldown = max_cooldown;
        vault_state.cooldown = max_cooldown;
        vault_state.admin = ctx.accounts.admin.key();
        Ok(())
    }

    pub fn set_cooldown_duration(ctx: Context<SetCooldownDuration>, duration: u64) -> Result<()> {
        let vault_state = &mut ctx.accounts.vault_state;
        require!(duration <= vault_state.max_cooldown, StakeError::TooSoon);
        vault_state.cooldown = duration;
        Ok(())
    }

    pub fn cooldown_assets(ctx: Context<Cooldown>, assets: u64) -> Result<()> {
        let cooldown = &mut ctx.accounts.user_cooldown;
        let clock = Clock::get()?;
        cooldown.cooldown_end = clock.unix_timestamp as u64 + ctx.accounts.vault_state.cooldown;
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
            let seeds = &[b"vault".as_ref(), &[ctx.accounts.vault_state.vault_bump]];
            let signer = &[&seeds[..]];
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, assets)?;
        } else {
            return Err(StakeError::TooSoon.into());
        }
        Ok(())
    }

    pub fn add_rewarder(ctx: Context<Rewarders>, rewarder: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        let rewarders = &mut ctx.accounts.vault_state.rewarders;

        if !rewarders.contains(&rewarder) {
            rewarders.push(rewarder);
        } else {
            return Err(StakeError::AlreadyRewarder.into());
        }

        Ok(())
    }

    pub fn remove_rewarder(ctx: Context<Rewarders>, rewarder: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        let rewarders = &mut ctx.accounts.vault_state.rewarders;

        if let Some(i) = rewarders.iter().position(|&x| x == rewarder) {
            rewarders.swap_remove(i);
        } else {
            return Err(StakeError::NotRewarderYet.into());
        }

        Ok(())
    }

    pub fn reward(ctx: Context<Reward>) -> Result<()> {
        if !ctx.accounts.vault_state.rewarders.contains(&ctx.accounts.caller.key()) {
            return Err(StakeError::NotRewarder.into());
        }

        // TODO

        ctx.accounts.vault_state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init_if_needed, payer = admin, space = 168)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetCooldownDuration<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Cooldown<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        init_if_needed,
        payer = user,
        space = 24,
        seeds = [b"user_cooldown", user.key().as_ref()],
        bump
    )]
    pub user_cooldown: Account<'info, UserCooldown>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Rewarders<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)] 
pub struct Reward<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(signer)]
    pub caller: Signer<'info>,
    // TODO
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(seeds = [b"vault-state"], bump)]
    pub vault_state: Account<'info, VaultState>,
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
    /// CHECK: vault account to authorize unstaking
    #[account(seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct VaultState {
    pub cooldown: u64,
    pub max_cooldown: u64,
    pub min_shares: u64,
    pub vesting_period: u64,
    pub vesting_amount: u64,
    pub last_distribution_time: u64,
    pub vault_bump: u8,
    pub rewarders: Vec<Pubkey>,
    pub admin: Pubkey,
    pub token: Pubkey,
}

#[account]
pub struct UserCooldown {
    pub cooldown_end: u64,
    pub underlying_amount: u64,
}

#[error_code]
pub enum StakeError {
    #[msg("Unstake cooldown has not passed")]
    TooSoon,
    #[msg("The provided key is not yet a rewarder")]
    NotRewarderYet,
    #[msg("The provided key is already a rewarder")]
    AlreadyRewarder,
    #[msg("The caller is not an admin")]
    NotAdmin,
    #[msg("The caller is not a rewarder")]
    NotRewarder,
}