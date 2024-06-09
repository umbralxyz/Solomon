use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("36axVA5TApdCi8u7LV1ReekkEDJNGKMK2sL8akfi5e4Z");

// 1e9
const VAULT_TOKEN_SCALAR: u64 = 1_000_000_000;

const STAKING_TOKEN_SEED: &[u8] = b"staking-token";
const VAULT_TOKEN_ACCOUNT_SEED: &[u8] = b"vault-token-account";
const USER_DATA_SEED: &[u8] = b"user-data";
const VAULT_STATE_SEED: &[u8] = b"vault-state";

#[account]
pub struct VaultState {
    pub admin: Pubkey,
    pub bump: u8,

    pub cooldown: u64,
    pub max_cooldown: u64,
    pub min_shares: u64,
    pub total_deposits: u64,
    pub reward_per_deposit: u64,
    pub last_distribution_amt: u64,
    pub last_distribution_time: u64,

    pub deposit_token: Pubkey,
    pub deposit_token_decimals: u8,

    pub rewarders: Vec<Pubkey>,
    pub blacklist: Vec<Pubkey>,
}

#[program]
pub mod stake {
    use anchor_spl::token::{Burn, MintTo};

    use super::*;

    // todo remove
    pub fn mint_staked_token(ctx: Context<MintToken>, amt: u64) -> Result<()> {
        let state = &ctx.accounts.vault_state;

        if ctx.accounts.authority.key() != state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        // mint tokens to recipient
        let cpi_accounts = MintTo {
            mint: ctx.accounts.staking_token.to_account_info(),
            to: ctx.accounts.recipient.to_account_info(),
            authority: ctx.accounts.staking_token.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();

        let seeds: &[&[u8]] = &[
            STAKING_TOKEN_SEED,
            state.admin.as_ref(),
            state.deposit_token.as_ref(),
            &[ctx.bumps.staking_token],
        ];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, seeds);

        token::mint_to(cpi_ctx, amt)?;

        Ok(())
    }

    pub fn initialize_vault_state(
        ctx: Context<InitializeVaultState>,
        admin: Pubkey,
        max_cooldown: u64,
    ) -> Result<()> {
        let vault_state = &mut ctx.accounts.vault_state;

        vault_state.max_cooldown = max_cooldown;
        vault_state.cooldown = max_cooldown;
        vault_state.deposit_token = ctx.accounts.deposit_token.key();
        vault_state.admin = admin;
        vault_state.rewarders = vec![admin];
        vault_state.last_distribution_time = Clock::get()?.unix_timestamp as u64;
        vault_state.bump = ctx.bumps.vault_state;

        Ok(())
    }

    pub fn initialize_user_account(_ctx: Context<InitializeUserAccount>) -> Result<()> {
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
        distribute(&mut ctx.accounts.vault_state)?;
        let auth = ctx.accounts.vault_state.to_account_info();
        let state = &mut ctx.accounts.vault_state;
        let user_data = &mut ctx.accounts.user_data;

        if state.blacklist.contains(&ctx.accounts.user.key()) {
            return Err(StakeError::Blacklisted.into());
        }

        // Transfer user's unstaked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.user_deposit_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        // Mint tokens to depositer
        let cpi_accounts = MintTo {
            mint: ctx.accounts.staking_token.to_account_info(),
            to: ctx.accounts.user_staking_token_account.to_account_info(),
            authority: auth,
        };

        let seeds: &[&[u8]] = &[
            VAULT_STATE_SEED,
            state.admin.as_ref(),
            state.deposit_token.as_ref(),
            &[state.bump],
        ];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            seeds,
        );

        token::mint_to(cpi_ctx, amt)?;

        // Update user data and vault
        let new_cd_end = Clock::get()?.unix_timestamp as u64 + state.cooldown;
        user_data.cooldowns.push((new_cd_end, amt));
        user_data.deposits += amt;
        user_data.reward_tally += state.reward_per_deposit * amt / VAULT_TOKEN_SCALAR;
        state.total_deposits += amt;

        emit!(StakeEvent {
            who: ctx.accounts.user.key(),
            amt: amt,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>) -> Result<()> {
        distribute(&mut ctx.accounts.vault_state)?;

        // Withdraws the assets that have cooled down and all yield generated
        let auth = ctx.accounts.vault_state.to_account_info();
        let state = &mut ctx.accounts.vault_state;

        let time = Clock::get()?.unix_timestamp as u64;

        let mut deposits = ctx.accounts.user_data.deposits; // TODO: consider renaming this

        for (cd, hold) in &ctx.accounts.user_data.cooldowns {
            if cd >= &time {
                deposits -= hold; // don't unstake assets that have not cooled down
            }
        }

        let user_data = &mut ctx.accounts.user_data;

        // Calculate user yields
        let yields = user_data.deposits * state.reward_per_deposit / VAULT_TOKEN_SCALAR - user_data.reward_tally;

        // Transfer token to caller
        let accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.user_deposit_token_account.to_account_info(),
            authority: auth,
        };

        let seeds: &[&[u8]] = &[
            VAULT_STATE_SEED,
            state.admin.as_ref(),
            state.deposit_token.as_ref(),
            &[state.bump],
        ];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            accounts,
            seeds,
        );

        token::transfer(cpi_ctx, deposits + yields)?;

        // Burn staked tokens that caller redeemed
        let accounts = Burn {
            mint: ctx.accounts.staking_token.to_account_info(),
            from: ctx.accounts.user_staking_token_account.to_account_info(),
            authority: ctx.accounts.staking_token.to_account_info(),
        };

        let seeds: &[&[u8]] = &[
            VAULT_STATE_SEED,
            state.admin.as_ref(),
            state.deposit_token.as_ref(),
            &[state.bump],
        ];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            accounts,
            seeds,
        );

        token::burn(cpi_ctx, deposits)?;

        // Clear deposits that were unstaked and update user reward tally and deposits
        user_data.cooldowns.retain(|&(cd, _)| cd >= time);
        user_data.reward_tally -= state.reward_per_deposit * deposits / VAULT_TOKEN_SCALAR;
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

        emit!(AddRewarderEvent {
            who: rewarder,
            added_by: ctx.accounts.caller.key(),
        });

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

        emit!(RemoveRewarderEvent {
            who: rewarder,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn reward(ctx: Context<Reward>, amt: u64) -> Result<()> {
        distribute(&mut ctx.accounts.vault_state)?;

        let state = &mut ctx.accounts.vault_state;
        if !state.rewarders.contains(&ctx.accounts.caller.key()) {
            return Err(StakeError::NotRewarder.into());
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

        ctx.accounts.vault_state.last_distribution_amt += amt;

        Ok(())
    }
}

pub fn distribute(state: &mut VaultState) -> Result<u64> {
    // called before the first reward, so the clock will not be 0
    if state.last_distribution_amt == 0 {
        state.last_distribution_time = Clock::get()?.unix_timestamp as u64;
        return Ok(0);
    }

    let time = Clock::get()?.unix_timestamp as u64;
    let time_passed = time - state.last_distribution_time;

    let scalar = 1000000000 as u64; // 10^9
    let mut percentage = scalar;

    // get the percentage of 8 hours passed since last distribution
    // if greater than 8 hours, percentage = 100%
    if time_passed < (8 * 60 * 60) {
        percentage = (scalar * time_passed) / (8 * 60 * 60);
    }

    let amt = (state.last_distribution_amt * percentage) / scalar;

    state.last_distribution_amt -= amt;
    state.reward_per_deposit += amt * VAULT_TOKEN_SCALAR / state.total_deposits;
    state.last_distribution_time = time;

    Ok(amt)
}

#[derive(Accounts)]
#[instruction(admin: Pubkey, max_cooldown: u64)]
pub struct InitializeVaultState<'info> {
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,

    /// The vault state for this deposit token and admin
    #[account(
        init, 
        payer = caller, 
        // todo check space
        space = 1024, 
        seeds = [VAULT_STATE_SEED, admin.as_ref(), deposit_token.key().as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init,
        payer = caller,
        seeds = [STAKING_TOKEN_SEED, admin.as_ref(), deposit_token.key().as_ref()],
        mint::decimals = 9,
        mint::authority = vault_state,
        bump
    )]
    pub staking_token: Account<'info, Mint>,

    /// The deposit token ATA for this vault and admin
    #[account(
        init, 
        payer = caller, 
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, admin.as_ref(), deposit_token.key().as_ref()],
        token::mint = deposit_token,
        token::authority = vault_state,
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub deposit_token: Account<'info, Mint>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct InitializeUserAccount<'info> {
    pub system_program: Program<'info, System>,

    #[account(
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init, 
        payer = user, 
        space = 8 + 8 + 32 + (8 * 10), 
        seeds = [USER_DATA_SEED, user.key().as_ref(), 
        vault_state.key().as_ref()], 
        bump
    )]
    pub user_data: Account<'info, UserPDA>,

    #[account(mut)]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct MintToken<'info> {
    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        seeds = [STAKING_TOKEN_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.key().as_ref()],
        bump
    )]
    pub staking_token: Account<'info, Mint>,

    /// Warning!: We dont check if the recipient has the same authority as the caller
    #[account(
        mut,
        token::mint = staking_token,
    )]
    pub recipient: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
}

#[derive(Accounts)]
pub struct Reward<'info> {
    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    /// The callers deposit token account
    #[account(
        mut,
        token::mint = vault_state.deposit_token,
        token::authority = caller,
    )]
    pub caller_token_account: Account<'info, TokenAccount>,

    /// The vaults ATA for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    pub token_program: Program<'info, Token>,
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        mut,
        seeds = [STAKING_TOKEN_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.key().as_ref()],
        bump
    )]
    pub staking_token: Account<'info, Mint>,
    #[account(
        mut,
        seeds = [USER_DATA_SEED, user.key().as_ref(), vault_state.key().as_ref()], 
        bump
    )]
    pub user_data: Account<'info, UserPDA>,
    /// THe user deposit token account, were going to transfer from this
    #[account(
        mut,
        token::mint = vault_state.deposit_token,
        token::authority = user,
    )]
    pub user_deposit_token_account: Account<'info, TokenAccount>,
    /// The users staking token account, were going to mint to this
    #[account(
        mut,
        token::mint = staking_token,
        token::authority = user,
    )]
    pub user_staking_token_account: Account<'info, TokenAccount>,
    /// The vaults ATA for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [STAKING_TOKEN_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.key().as_ref()],
        bump
    )]
    pub staking_token: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [USER_DATA_SEED, user.key().as_ref(), vault_state.key().as_ref()], 
        bump
    )]
    pub user_data: Account<'info, UserPDA>,

    #[account(
        mut,
        token::mint = staking_token,
        token::authority = user,
    )]
    pub user_staking_token_account: Account<'info, TokenAccount>,

    /// The user deposit account were going to send collateral too
    #[account(
        mut,
        token::mint = vault_state.deposit_token,
        token::authority = user,
    )]
    pub user_deposit_token_account: Account<'info, TokenAccount>,

    /// The vaults ATA for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetCooldownDuration<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Rewarders<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, vault_state.admin.as_ref(), vault_state.deposit_token.as_ref()],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
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

#[event]
pub struct AddRewarderEvent {
    who: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct RemoveRewarderEvent {
    who: Pubkey,
    removed_by: Pubkey,
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
