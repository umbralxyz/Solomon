use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

#[program]
pub mod stake {
    use anchor_spl::token::{Burn, MintTo};

    use super::*;

    pub fn mint_staked_token(ctx: Context<MintToken>, amt: u64) -> Result<()> {
        let state = &ctx.accounts.vault_state;

        if ctx.accounts.authority.key() != state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        // mint tokens to recipient
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.recipient.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::mint_to(cpi_ctx, amt)?; 

        Ok(())
    }

    pub fn initialize(ctx: Context<Initialize>, max_cooldown: u64, token: Pubkey) -> Result<()> {
        let vault_state = &mut ctx.accounts.vault_state;

        // TODO: min shares donation attack protection
        vault_state.max_cooldown = max_cooldown;
        vault_state.cooldown = max_cooldown;
        vault_state.token = token;
        vault_state.admin = ctx.accounts.admin.key();
        vault_state.rewarders = vec![ctx.accounts.admin.key()];
        vault_state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        Ok(())
    }

    pub fn set_cooldown_duration(ctx: Context<SetCooldownDuration>, duration: u64) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;

        if ctx.accounts.caller.key() != state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        require!(duration <= state.max_cooldown, StakeError::TooSoon);
        state.cooldown = duration;

        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amt: u64) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        
        if state.blacklist.contains(&ctx.accounts.user.key()) {
            return Err(StakeError::Blacklisted.into());
        }

        // Transfer user's unstaked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        // mint tokens to depositer
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.user_staked_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.staked_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::mint_to(cpi_ctx, amt)?; 

        // Update user data or add new user data
        let new_cd_end = Clock::get()?.unix_timestamp as u64 + state.cooldown;
        
        if let Some(i) = state.user_data.iter().position(|x| x.user == ctx.accounts.user.key()) {
            state.user_data[i].cooldown_end = new_cd_end;
            state.user_data[i].pending_deposits.push(amt);
        } else {
            state.user_data.push(UserData {
                user: ctx.accounts.user.key(),
                pending_deposits: vec![amt],
                deposits: 0,
                yields: 0,
                cooldown_end: new_cd_end,
            })
        }

        emit!(StakeEvent {
            who: ctx.accounts.user.key(),
            amt: amt,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        let user_data: &mut UserData;
        
        if let Some(i) = state.user_data.iter().position(|x| x.user == ctx.accounts.user.key()) {
            user_data = &mut state.user_data[i];
        } else {
            return Err(StakeError::UserNotFound.into())
        }

        let clock = Clock::get()?;

        if (clock.unix_timestamp as u64) < user_data.cooldown_end {
            return Err(StakeError::TooSoon.into());
        }

        let to_vault = user_data.deposits;
        let to_user = user_data.deposits + user_data.yields;

        // Transfer staked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.user_staked_account.to_account_info(),
            to: ctx.accounts.vault_staked_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_program = ctx.accounts.staked_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, to_vault)?;

        // Transfer token to caller
        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, to_user)?;

        // Burn staked tokens that caller redeemed
        let cpi_accounts = Burn {
            mint: ctx.accounts.staked_program.to_account_info(),
            from: ctx.accounts.vault_staked_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::burn(cpi_ctx, to_vault)?;

        {
            user_data.cooldown_end = 0;
            user_data.deposits = 0;
            user_data.yields = 0;
        }

        state.total_deposits -= to_vault;

        emit!(UnstakeEvent {
            who: ctx.accounts.user.key(),
            amt: to_vault,
        });

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

    pub fn reward(ctx: Context<Reward>, amt: u64) -> Result<()> {
        // amt = total yield to distribute
        
        let state = &mut ctx.accounts.vault_state;

        let total_deposits = state.total_deposits;
        let mut new_deposits = 0;

        if state.rewarders.contains(&ctx.accounts.caller.key()) {
            return Err(StakeError::NotRewarder.into());
        }

        let user_data = &mut state.user_data;

        // Update each user's data
        for user in user_data {
            let yield_portion = (user.deposits / total_deposits) * amt; // TODO: should it be (user.yields + user.deposits) ?
            user.yields += yield_portion;

            for deposit in user.pending_deposits.drain(..) {
                user.deposits += deposit;
                new_deposits += deposit;
            }
        }

        state.total_deposits += new_deposits;
        state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        ctx.accounts.vault_state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init_if_needed, payer = admin, space = 168)] // space might need to be much larger for user data?
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintToken<'info> {
    /// CHECK: the token to mint
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    /// CHECK: the token account to mint tokens to
    #[account(mut)]
    pub recipient: Account<'info, TokenAccount>,
    /// CHECK: the authority of the mint account
    #[account(signer)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
}

#[derive(Accounts)]
pub struct SetCooldownDuration<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(signer)]
    pub caller: Signer<'info>,
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
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(seeds = [b"vault-state"], bump)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub vault: Signer<'info>,
    #[account(mut)]
    pub user_staked_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_staked_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    /// CHECK: the token to mint
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub staked_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(seeds = [b"vault-state"], bump)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub vault: Signer<'info>,
    #[account(mut)]
    pub user_staked_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_staked_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub staked_program: Program<'info, Token>,
}

#[account]
pub struct VaultState {
    pub cooldown: u64,
    pub max_cooldown: u64,
    pub min_shares: u64,
    pub total_deposits: u64,
    pub last_distribution_time: u64,
    pub vault_bump: u8,
    pub rewarders: Vec<Pubkey>,
    pub admin: Pubkey,
    pub token: Pubkey,
    pub blacklist: Vec<Pubkey>,
    pub user_data: Vec<UserData>, 
}

#[account]
pub struct UserData {
    pub user: Pubkey,
    pub pending_deposits: Vec<u64>, // to be added to deposits at next reward
    pub deposits: u64,
    pub yields: u64, 
    pub cooldown_end: u64,
}

#[event]
pub struct StakeEvent {
    who: Pubkey,
    amt: u64,
}

#[event]
pub struct UnstakeEvent {
    who: Pubkey,
    amt: u64,
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
    #[msg("The user is prohibited from staking")]
    Blacklisted,
    #[msg("The user was not found in current stakers")]
    UserNotFound,
}