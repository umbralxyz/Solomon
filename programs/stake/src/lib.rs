use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
mod context;
use context::*;

declare_id!("36axVA5TApdCi8u7LV1ReekkEDJNGKMK2sL8akfi5e4Z");

const STAKING_TOKEN_SEED: &[u8] = b"staking-token";
const VAULT_TOKEN_ACCOUNT_SEED: &[u8] = b"vault-token-account";
const USER_DATA_SEED: &[u8] = b"user-data";
const VAULT_STATE_SEED: &[u8] = b"vault-state";

#[account]
pub struct UserPDA {
    pub user: Pubkey,
    pub cooldown_end: u64,
}

#[account]
pub struct VaultState {
    pub admin: Pubkey,
    pub bump: u8,
    pub deposit_token: Pubkey,
    pub cooldown: u64,
    pub vesting_amount: u64,
    pub last_distribution_time: u64,
    pub total_assets: u64,
    pub vesting_period: u64,

    /// See [`https://blog.openzeppelin.com/a-novel-defense-against-erc4626-inflation-attacks`]
    pub offset: u8,

    pub rewarders: Vec<Pubkey>,
    pub blacklist: Vec<Pubkey>,
}

#[program]
pub mod stake {
    use super::*;
    use std::vec;

    pub fn initialize_vault_state(
        ctx: Context<InitializeVaultState>,
        admin: Pubkey,
        _salt: [u8; 8],
        offset: u8,
        cooldown: u64,
    ) -> Result<()> {
        // todo
        require!(offset < 9, StakeError::BadOffset);

        ctx.accounts.vault_state.bump = ctx.bumps.vault_state;
        ctx.accounts.vault_state.deposit_token = ctx.accounts.deposit_token.key();
        ctx.accounts.vault_state.admin = admin;
        ctx.accounts.vault_state.offset = offset;
        ctx.accounts.vault_state.cooldown = cooldown;
        ctx.accounts.vault_state.vesting_amount = 0;
        ctx.accounts.vault_state.last_distribution_time = 0;
        ctx.accounts.vault_state.total_assets = 0;
        ctx.accounts.vault_state.vesting_period = 8 * 3600;
        ctx.accounts.vault_state.blacklist = vec![];

        Ok(())
    }

    pub fn initialize_program_accounts(
        _ctx: Context<InitializeProgramAccounts>,
        _salt: [u8; 8],
    ) -> Result<()> {
        Ok(())
    }

    pub fn initialize_user_account(
        _ctx: Context<InitializeUserAccount>,
        _salt: [u8; 8],
    ) -> Result<()> {
        Ok(())
    }

    pub fn set_cooldown(ctx: Context<SetCooldown>, _salt: [u8; 8], duration: u64) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.vault_state.cooldown = duration;

        Ok(())
    }

    pub fn set_vesting_period(ctx: Context<SetVestingPeriod>, _salt: [u8; 8], duration: u64) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.vault_state.vesting_period = duration;

        Ok(())
    }

    pub fn blacklist(ctx: Context<Blacklist>, _salt: [u8; 8], user: Pubkey) -> Result<()> {
        if ctx
            .accounts
            .vault_state
            .blacklist
            .contains(&user)
        {
            return Err(StakeError::AlreadyBlacklisted.into());
        }

        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.vault_state.blacklist.push(user);

        Ok(())
    }

    pub fn add_rewarder(ctx: Context<Rewarders>, rewarder: Pubkey, _salt: [u8; 8]) -> Result<()> {
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

    pub fn remove_rewarder(ctx: Context<Rewarders>, rewarder: Pubkey, _salt: [u8; 8]) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        let rewarders = &mut ctx.accounts.vault_state.rewarders;

        if let Some(i) = rewarders.iter().position(|&x| x == rewarder) {
            rewarders.swap_remove(i);
        } else {
            return Err(StakeError::NotRewarderYet.into());
        }

        emit!(RemoveRewarderEvent{
            who: rewarder,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, salt: [u8; 8], amt: u64) -> Result<()> {
        if ctx
            .accounts
            .vault_state
            .blacklist
            .contains(&ctx.accounts.user.key())
        {
            return Err(StakeError::Blacklisted.into());
        }

        let shares = ctx.accounts.vault_state.convert_to_shares(
            amt,
            ctx.accounts.staking_token.supply,
        )?;
        ctx.accounts.transfer_from_user_to_vault(amt)?;
        ctx.accounts.mint_tokens_to_user(&salt, shares)?;
        ctx.accounts.vault_state.total_assets += amt;

        emit!(StakeEvent {
            who: ctx.accounts.user.key(),
            assets: amt,
            shares: shares,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, salt: [u8; 8], shares: u64) -> Result<()> {
        if ctx
            .accounts
            .vault_state
            .blacklist
            .contains(&ctx.accounts.user.key())
        {
            return Err(StakeError::Blacklisted.into());
        }

        let time = Clock::get()?.unix_timestamp as u64;

        if ctx.accounts.user_data.cooldown_end > time {
            return Err(StakeError::TooSoon.into());
        }

        let assets = ctx.accounts.vault_state.convert_to_assets(
            shares,
            ctx.accounts.staking_token.supply,
        )?;

        ctx.accounts.burn_tokens_from_user(shares)?;
        ctx.accounts.transfer_from_vault_to_user(&salt, assets)?; 
        let cd = ctx.accounts.vault_state.cooldown;
        ctx.accounts.user_data.cooldown_end = time + cd;

        emit!(UnstakeEvent {
            who: ctx.accounts.user.key(),
            shares,
            assets,
        });

        Ok(())
    }

    pub fn reward(ctx: Context<Reward>, amt: u64, _salt: [u8; 8]) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        if !state.rewarders.contains(&ctx.accounts.caller.key()) {
            return Err(StakeError::NotRewarder.into());
        }

        let time = Clock::get()?.unix_timestamp as u64;

        // Transfer unstaked tokens to vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.caller.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
        );
        token::transfer(cpi_ctx, amt)?;

        ctx.accounts.vault_state.last_distribution_time = time;
        ctx.accounts.vault_state.total_assets += amt;
        ctx.accounts.vault_state.vesting_amount = amt;

        emit!(RewardEvent{
            who: ctx.accounts.caller.key(),
            amt: amt,
        });

        Ok(())
    }
}

impl VaultState {
    pub fn convert_to_shares(&self, assets: u64, total_supply: u64) -> Result<u64>  {
        // 1e9(total_supply) + 1eINTERNAL_OFFSET
        let virtual_supply = total_supply + 10_u64.pow(self.offset as u32);
    
        // in terms of our decimals
        // shares per asset deposit
        // = 1e18(assets * supply) + 1e(9+INTERNAL_OFFSET)(assets)
        let x = assets * virtual_supply;
    
        // supply / total_assets = shares per asset
        // = 1e9(shares) + 1e(9+INTERNAL_OFFSET)(assets / total_assets)
        let total_assets = self.total_assets - self.get_unvested()?;
        let x = x / (total_assets + 1);
    
        // = 1e9(shares) + 1e(INTERNAL_OFFSET)(assets / total_assets)
        Ok(x)
    }
    
    pub fn convert_to_assets(&self, shares: u64, total_supply: u64) -> Result<u64>  {
        if total_supply == 0 {
            return Ok(shares);
        }
        
        let total_assets = self.total_assets - self.get_unvested()?;
        let virtual_supply = total_supply + 10_u64.pow(self.offset as u32);
        let x = shares * (total_assets + 1);
        let x = x / virtual_supply;

        Ok(x)
    }

    pub fn get_unvested(&self) -> Result<u64> {
        let time = Clock::get()?.unix_timestamp as u64;
        let time_passed = time - self.last_distribution_time;
    
        // Get the percentage of 8 hours passed since last distribution
        // If greater than 8 hours, percentage = 100%
        if time_passed > self.vesting_period {
            return Ok(0);
        }

        let amt = ((self.vesting_period - time_passed) * self.vesting_amount) / self.vesting_period;

        Ok(amt)
    }
}

#[event]
pub struct StakeEvent {
    who: Pubkey,
    assets: u64,
    shares: u64,
}

#[event]
pub struct UnstakeEvent {
    who: Pubkey,
    shares: u64,
    assets: u64,
}   

#[event]
pub struct RewardEvent {
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

#[event]
pub struct AdminTransferEvent {
    old_admin: Pubkey,
    new_admin: Pubkey,
}

#[error_code]
pub enum StakeError {
    #[msg("Too many tokens requested before cooldown expires")]
    TooSoon,
    #[msg("The provided key is not yet a rewarder")]
    NotRewarderYet,
    #[msg("The provided key is already a rewarder")]
    AlreadyRewarder,
    #[msg("The provided key is already blacklisted")]
    AlreadyBlacklisted,
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
    #[msg("The vault is below the minimum shares required for staking")]
    MinSharesViolation,
    #[msg("Bad Staking Token Decimals, they must be gte than the deposit token")]
    BadStakingTokenDecimals,
    #[msg("Offset too high")]
    BadOffset
}
