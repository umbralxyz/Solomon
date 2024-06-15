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
    pub deposits: u64,
    pub reward_tally: u64,
}

#[account]
pub struct VaultState {
    pub admin: Pubkey,
    pub bump: u8,
    pub deposit_token: Pubkey,

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
    ) -> Result<()> {
        // todo
        require!(offset < 9, StakeError::BadOffset);

        ctx.accounts.vault_state.bump = ctx.bumps.vault_state;
        ctx.accounts.vault_state.deposit_token = ctx.accounts.deposit_token.key();
        ctx.accounts.vault_state.admin = admin;
        ctx.accounts.vault_state.offset = offset;
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
            ctx.accounts.vault_token_account.amount,
        );
        ctx.accounts.transfer_from_user_to_vault(amt)?;
        ctx.accounts.mint_tokens_to_user(&salt, shares)?;

        emit!(StakeEvent {
            who: ctx.accounts.user.key(),
            assets: amt,
            shares: shares,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, salt: [u8; 8], shares: u64) -> Result<()> {
        let assets = ctx.accounts.vault_state.convert_to_assets(
            shares,
            ctx.accounts.staking_token.supply,
            ctx.accounts.vault_token_account.amount,
        );

        ctx.accounts.burn_tokens_from_user(shares)?;
        ctx.accounts.transfer_from_vault_to_user(&salt, assets)?;

        emit!(UnstakeEvent {
            who: ctx.accounts.user.key(),
            shares,
            assets,
        });

        Ok(())
    }
}

impl VaultState {
    pub fn convert_to_shares(&self, assets: u64, total_supply: u64, total_assets: u64) -> u64 {
        if total_supply == 0 {
            return assets;
        }
    
        // 1e9(total_supply) + 1eINTERNAL_OFFSET
        let virtual_supply = total_supply + 10_u64.pow(self.offset as u32);
    
        // in terms of our decimals
        // shares per asset deposit
        // = 1e18(assets * supply) + 1e(9+INTERNAL_OFFSET)(assets)
        let x = assets * virtual_supply;
    
        // supply / total_assets = shares per asset
        // = 1e9(shares) + 1e(9+INTERNAL_OFFSET)(assets / total_assets)
        let x = x / (total_assets + 1);
    
        // = 1e9(shares) + 1e(INTERNAL_OFFSET)(assets / total_assets)
        x
    }
    
    pub fn convert_to_assets(&self, shares: u64, total_supply: u64, total_assets: u64) -> u64 {
        if total_supply == 0 {
            return shares;
        }
        
        let virtual_supply = total_supply + 10_u64.pow(self.offset as u32);
        let x = shares * (total_assets + 1);
        let x = x / virtual_supply;

        x
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
