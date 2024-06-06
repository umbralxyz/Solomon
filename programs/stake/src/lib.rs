use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("36axVA5TApdCi8u7LV1ReekkEDJNGKMK2sL8akfi5e4Z");

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

    pub fn initialize_vault_state(ctx: Context<InitializeVaultState>, max_cooldown: u64, token: Pubkey) -> Result<()> {
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

    pub fn initialize_user_account(ctx: Context<InitializeUserAccount>) -> Result<()> {
        let user_data = &mut ctx.accounts.user_data;

        user_data.user = ctx.accounts.user.key();
        user_data.deposits = 0;
        user_data.reward_tally = 0;
        user_data.cooldowns = Vec::new();

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
        let user_data = &mut ctx.accounts.user_data;

        if state.token != ctx.accounts.token_program.key() {
            return Err(StakeError::WrongToken.into());
        }

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

        // Mint tokens to depositer
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.user_staked_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };

        let cpi_program = ctx.accounts.staked_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::mint_to(cpi_ctx, amt)?;

        // Update user data and vault
        let new_cd_end = Clock::get()?.unix_timestamp as u64 + state.cooldown;
        user_data.cooldowns.push((new_cd_end, amt));
        user_data.deposits += amt;
        user_data.reward_tally += state.reward_per_deposit + amt;
        state.total_deposits += amt;

        emit!(StakeEvent {
            who: ctx.accounts.user.key(),
            amt: amt,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        // Withdraws the assets that have cooled down and all yield generated
        let state = &mut ctx.accounts.vault_state;

        let clock = Clock::get()?.unix_timestamp as u64;

        let mut deposits = ctx.accounts.user_data.deposits; // TODO: consider renaming this
        
        for (cd, hold) in &ctx.accounts.user_data.cooldowns {
            if cd >= &clock {
                deposits -= hold; // don't unstake assets that have not cooled down
            }
        }

        let user_data = &mut ctx.accounts.user_data;

        // Calculate user yields 
        let yields = user_data.deposits * state.reward_per_deposit - user_data.reward_tally;

        // Transfer staked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.user_staked_account.to_account_info(),
            to: ctx.accounts.vault_staked_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_program = ctx.accounts.staked_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, deposits)?;

        // Transfer token to caller
        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, deposits + yields)?;

        // Burn staked tokens that caller redeemed
        let cpi_accounts = Burn {
            mint: ctx.accounts.staked_mint.to_account_info(),
            from: ctx.accounts.vault_staked_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::burn(cpi_ctx, deposits)?;

        // Clear deposits that were unstaked and update user reward tally and deposits
        user_data.cooldowns.retain(|&(cd, _)| cd >= clock);
        user_data.reward_tally = user_data.deposits * state.reward_per_deposit; // TODO: double check this line and below
        user_data.reward_tally -= state.reward_per_deposit * deposits;
        user_data.deposits -= deposits;

        // Update vault state
        state.total_deposits -= deposits;

        emit!(UnstakeEvent {
            who: ctx.accounts.user.key(),
            amt: deposits,
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

        if state.rewarders.contains(&ctx.accounts.caller.key()) {
            // TODO: uncomment after problem is understood
            //return Err(StakeError::NotRewarder.into());
        }

        // Transfer unstaked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.caller.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        state.reward_per_deposit = (state.reward_per_deposit + amt) / state.total_deposits;

        state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        ctx.accounts.vault_state.last_distribution_time = Clock::get()?.unix_timestamp as u64;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeVaultState<'info> {
    #[account(init_if_needed, payer = admin, space = 1024, seeds = [b"vault-state"], bump)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeUserAccount<'info> {
    #[account(init, payer = user, space = 8 + 8 + 32 + (8 * 10), seeds = [b"user_data", user.key().as_ref()], bump)]
    pub user_data: Account<'info, UserPDA>,
    #[account(mut)]
    pub user: Signer<'info>,
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
    #[account(mut)]
    pub caller_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub user_data: Account<'info, UserPDA>,
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
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub user_data: Account<'info, UserPDA>,
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
    #[account(mut)]
    pub staked_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub staked_program: Program<'info, Token>,
}

#[account]
pub struct VaultState {
    pub cooldown: u64,
    pub max_cooldown: u64,
    pub min_shares: u64,
    pub total_deposits: u64,
    pub reward_per_deposit: u64,
    pub last_distribution_amt: u64,
    pub last_distribution_time: u64,
    pub vault_bump: u8,
    pub rewarders: Vec<Pubkey>,
    pub admin: Pubkey,
    pub token: Pubkey,
    pub blacklist: Vec<Pubkey>,
}

#[account]
pub struct UserPDA {
    pub user: Pubkey,
    pub cooldowns: Vec<(u64, u64)>, // (cooldown_end timestamp, amount of deposit)
    pub deposits: u64,
    pub reward_tally: u64, 
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
    #[msg("That token is not available for staking")]
    WrongToken,
}