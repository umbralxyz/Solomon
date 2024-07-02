use std::collections::VecDeque;

use anchor_lang::prelude::*;
use anchor_spl::{
    token::{self, Transfer},
    metadata::{
        create_metadata_accounts_v3,
        mpl_token_metadata::types::DataV2,
        CreateMetadataAccountsV3,
    }, 
};
mod context;
use context::*;

declare_id!("36axVA5TApdCi8u7LV1ReekkEDJNGKMK2sL8akfi5e4Z");

const STAKING_TOKEN_SEED: &[u8] = b"staking-token";
const VAULT_TOKEN_ACCOUNT_SEED: &[u8] = b"vault-token-account";
const USER_DATA_SEED: &[u8] = b"user-data";
const VAULT_STATE_SEED: &[u8] = b"vault-state";

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct MetadataParams {
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[account]
pub struct UserPDA {
    pub assets_available: u64,
    pub unstake_queue: VecDeque<(u32, u64)>,
}

#[account]
pub struct Blacklisted {
    pub user: Pubkey,
    pub blacklisted: bool,
}

#[account]
pub struct VaultState {
    pub admin: Pubkey,
    pub deposit_token: Pubkey,
    pub vesting_amount: u64,
    pub total_assets: u64,
    pub min_shares: u64,
    pub last_distribution_time: u32,
    pub cooldown: u32,
    pub vesting_period: u32,
    pub bump: u8,
    pub rewarders: Vec<Pubkey>,
}

#[program]
pub mod stake {
    use super::*;

    pub fn initialize_vault_state(
        ctx: Context<InitializeVaultState>,
        admin: Pubkey,
        _salt: [u8; 8],
        cooldown: u32,
        min_shares: u64,
    ) -> Result<()> {
        ctx.accounts.vault_state.bump = ctx.bumps.vault_state;
        ctx.accounts.vault_state.deposit_token = ctx.accounts.deposit_token.key();
        ctx.accounts.vault_state.admin = admin;
        ctx.accounts.vault_state.cooldown = cooldown;
        ctx.accounts.vault_state.vesting_amount = 0;
        ctx.accounts.vault_state.last_distribution_time = 0;
        ctx.accounts.vault_state.total_assets = 0;
        ctx.accounts.vault_state.min_shares = min_shares;
        ctx.accounts.vault_state.vesting_period = 8 * 3600;

        Ok(())
    }

    pub fn initialize_program_accounts(
        ctx: Context<InitializeProgramAccounts>,
        salt: [u8; 8],
        metadata: MetadataParams,
    ) -> Result<()> {

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, &salt, &[ctx.accounts.vault_state.bump]];
        let signer = &[seeds][..];

        let token_data = DataV2 {
            name: metadata.name,
            symbol: metadata.symbol,
            uri: metadata.uri,
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        };

        let metadata_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                payer: ctx.accounts.caller.to_account_info(),
                update_authority: ctx.accounts.caller.to_account_info(),
                mint: ctx.accounts.staking_token.to_account_info(),
                metadata: ctx.accounts.metadata.to_account_info(),
                mint_authority: ctx.accounts.vault_state.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
            &signer
        );

        create_metadata_accounts_v3(
            metadata_ctx,
            token_data,
            false,
            true,
            None,
        )?;

        Ok(())
    }

    pub fn set_cooldown(ctx: Context<SetCooldown>, _salt: [u8; 8], duration: u32) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.vault_state.cooldown = duration;

        emit!(SetCooldownEvent{
            who: ctx.accounts.caller.key(),
            duration: duration,
        });

        Ok(())
    }

    pub fn set_vesting_period(ctx: Context<SetVestingPeriod>, _salt: [u8; 8], duration: u32) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.vault_state.vesting_period = duration;

        emit!(SetVestingPeriodEvent{
            who: ctx.accounts.caller.key(),
            duration: duration,
        });

        Ok(())
    }

    pub fn blacklist(ctx: Context<Blacklist>, _salt: [u8; 8], user: Pubkey) -> Result<()> {
        if ctx
            .accounts
            .blacklisted
            .blacklisted
        {
            return Err(StakeError::AlreadyBlacklisted.into());
        }

        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(StakeError::NotAdmin.into());
        }

        ctx.accounts.blacklisted.blacklisted = true;

        emit!(AddToBlacklistEvent {
            who: user,
            added_by: ctx.accounts.caller.key(),
        });

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
        if ctx.accounts.blacklisted.blacklisted {
            return Err(StakeError::Blacklisted.into());
        }
        
        let shares = ctx.accounts.vault_state.convert_to_shares(
            amt,
            ctx.accounts.staking_token.supply,
        )?;
        if shares == 0 {
            return Err(StakeError::ZeroShares.into())
        }
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

    pub fn start_unstake(ctx: Context<Unstake>, _salt: [u8; 8], shares: u64) -> Result<()> {
        if ctx.accounts.blacklisted.blacklisted {
            return Err(StakeError::Blacklisted.into());
        }

        let cd = ctx.accounts.vault_state.cooldown;
        let time = Clock::get()?.unix_timestamp as u32;
        let cd_end = time + cd;

        let assets = ctx.accounts.vault_state.convert_to_assets(
            shares,
            ctx.accounts.staking_token.supply,
        )?;

        ctx.accounts.burn_tokens_from_user(shares)?;
        ctx.accounts.vault_state.total_assets -= assets;
        ctx.accounts.user_data.unstake_queue.push_back((cd_end, assets));

        emit!(StartUnstakeEvent {
            who: ctx.accounts.user.key(),
            shares,
            assets,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, salt: [u8; 8], assets: u64) -> Result<()> {
        if ctx.accounts.blacklisted.blacklisted {
            return Err(StakeError::Blacklisted.into());
        }

        ctx.accounts.user_data.get_available_assets()?;
        let assets_available = ctx.accounts.user_data.assets_available;

        if assets > assets_available {
            return Err(StakeError::AssetsUnavailable.into());
        }

        ctx.accounts.transfer_from_vault_to_user(&salt, assets)?; 
        ctx.accounts.user_data.assets_available -= assets;

        emit!(UnstakeEvent {
            who: ctx.accounts.user.key(),
            assets,
        });

        Ok(())
    }

    pub fn reward(ctx: Context<Reward>, amt: u64, _salt: [u8; 8]) -> Result<()> {
        let state = &mut ctx.accounts.vault_state;
        if !state.rewarders.contains(&ctx.accounts.caller.key()) {
            return Err(StakeError::NotRewarder.into());
        }

        let time = Clock::get()?.unix_timestamp as u32;

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

    pub fn check_available_assets(ctx: Context<CheckAssets>, _salt: [u8; 8])-> Result<u64> {
        Ok(ctx.accounts.user_data.get_available_assets()?)
    }
}

impl VaultState {
    pub fn convert_to_shares(&self, assets: u64, total_supply: u64) -> Result<u64>  {
        if self.total_assets == 0 {
            return Ok(assets);
        }

        let total_assets = self.total_assets - self.get_unvested()?;
        let x = assets * total_supply / total_assets;
    
        Ok(x)
    }
    
    pub fn convert_to_assets(&self, shares: u64, total_supply: u64) -> Result<u64>  {
        if total_supply == 0 {
            return Ok(shares);
        }
        
        let total_assets = self.total_assets - self.get_unvested()?;
        let x = (shares * total_assets) / total_supply;

        Ok(x)
    }

    pub fn get_unvested(&self) -> Result<u64> {
        let time = Clock::get()?.unix_timestamp as u32;
        let time_passed = (time - self.last_distribution_time) as u64;
        let vesting_period = self.vesting_period as u64;
    
        if time_passed > vesting_period {
            return Ok(0);
        }

        let amt = ((vesting_period - time_passed) * self.vesting_amount) / vesting_period;

        Ok(amt)
    }

    pub fn new(&mut self) {
        self.rewarders = Vec::with_capacity(20);
    }
}

impl UserPDA {
    pub fn new() -> Self {
        Self {
            assets_available: 0,
            unstake_queue: VecDeque::with_capacity(100),
        }
    }

    pub fn get_available_assets(&mut self) -> Result<u64> {
        let time = Clock::get()?.unix_timestamp as u32;

        // Iterate over unstake_queue and add cooled-down assets to user data
        while let Some((cd, assets)) = self.unstake_queue.get(0) {
            if time >= *cd {
                self.assets_available += assets;
                self.unstake_queue.pop_front();
            } else {
                break;
            }
        }

        Ok(self.assets_available)
    }
}

#[event]
pub struct SetVestingPeriodEvent {
    who: Pubkey,
    duration: u32,
}

#[event]
pub struct SetCooldownEvent {
    who: Pubkey,
    duration: u32,
}

#[event]
pub struct StakeEvent {
    who: Pubkey,
    assets: u64,
    shares: u64,
}

#[event]
pub struct StartUnstakeEvent {
    who: Pubkey,
    shares: u64,
    assets: u64,
}   

#[event]
pub struct UnstakeEvent {
    who: Pubkey,
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
pub struct AddToBlacklistEvent {
    who: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct AdminTransferEvent {
    old_admin: Pubkey,
    new_admin: Pubkey,
}

#[error_code]
pub enum StakeError {
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
    #[msg("Insufficient assets available")]
    AssetsUnavailable,
    #[msg("Bad Staking Token Decimals, they must be gte than the deposit token")]
    BadStakingTokenDecimals,
    #[msg("Total shares are below minimum for staking or unstaking")]
    MinSharesViolation,
    #[msg("Deposit is too small to receive shares")]
    ZeroShares,
}
