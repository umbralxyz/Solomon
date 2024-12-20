use anchor_lang::prelude::*;
use anchor_spl::{
    token::{self, MintTo, Transfer, Burn},
    metadata::{
        create_metadata_accounts_v3,
        mpl_token_metadata::types::DataV2,
        CreateMetadataAccountsV3, 
        Metadata,
    }, 
};
mod context;
use context::*;

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

const DECIMALS_SCALAR: u128 = 1_000_000_000;
const MAX_WITHDRAW_ADRESSES: usize = 50;
const MAX_MANAGER_ADDRESSES: usize = 20;
const MINT_SEED: &[u8] = b"mint";
const TOKEN_ACCOUNT_SEED: &[u8] = b"token-account";
const EXCHANGE_RATE_SEED: &[u8] = b"exchange-rate";
const VAULT_STATE_SEED: &[u8] = b"vault-state";

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct MetadataParams {
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[account]
pub struct VaultState {
    pub vault_token_mint: Pubkey,
    pub asset_managers: Vec<Pubkey>,
    pub role_managers: Vec<Pubkey>,
    pub withdraw_addresses: Vec<Pubkey>,
    pub admin: Pubkey,
    pub bump: u8,
}

#[account]
pub struct ExchangeRate {
    asset: Pubkey,
    /// The deposit rate is defined in scaled units of stable coin per asset coin
    /// (1e9)
    deposit_rate: u64,
    /// The redeem rate is defined in scaled units of asset coin per stable coin
    redeem_rate: u64,
}

#[account]
pub struct Permissions {
    key: Pubkey,
    can_mint: bool,
    can_redeem: bool,
}

#[program]
pub mod vault {
    use super::*;

    pub fn initialize_vault_state(
        ctx: Context<InitializeVaultState>,
        admin: Pubkey,
        metadata: MetadataParams,
    ) -> Result<()> {
        let seeds = &[VAULT_STATE_SEED, &[ctx.bumps.vault_state]];
        let signer = [&seeds[..]];

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
                payer: ctx.accounts.signer.to_account_info(),
                update_authority: ctx.accounts.signer.to_account_info(),
                mint: ctx.accounts.vault_token.to_account_info(),
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

        ctx.accounts.vault_state.admin = admin;
        ctx.accounts.vault_state.vault_token_mint = ctx.accounts.vault_token.key();
        ctx.accounts.vault_state.bump = ctx.bumps.vault_state;

        Ok(())
    }

    // The deposit rate is (stable units / asset units) [how many stable coins a depositer will get per asset coin]
    // The redeem rate is (asset units / stable units) [how many asset coins a depositer will get per stable coin]
    pub fn update_asset(
        ctx: Context<UpdateAsset>,
        asset: Pubkey,
        deposit_rate: u64,
        redeem_rate: u64,
    ) -> Result<()> {
        if ctx.accounts.authority.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        ctx.accounts.exchange_rate.asset = asset;
        ctx.accounts.exchange_rate.deposit_rate = deposit_rate;
        ctx.accounts.exchange_rate.redeem_rate = redeem_rate;

        emit!(AssetModifiedEvent{
            who: ctx.accounts.authority.key(),
            asset: asset,
            deposit_rate,
            redeem_rate,
        });

        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, collat: u64) -> Result<()> {
        let rate = ctx.accounts.exchange_rate.deposit_rate as u128;
        let decimals = ctx.accounts.collateral_token_mint.decimals;
        let collat_adjusted =  if decimals < 9 {
            collat * 10_u64.pow(9 - decimals as u32)
        } else {
            collat
        };
        let amt: u64 = (collat_adjusted as u128 * rate / DECIMALS_SCALAR).try_into().unwrap();

        if rate == 0 {
            return Err(MintError::AssetNotSupported.into());
        }
        
        if !ctx.accounts.user_permissions.can_mint {
            return Err(MintError::NotAnApprovedMinter.into());
        }

        // Transfer collat to mint vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_collateral.to_account_info(),
            to: ctx.accounts.program_collateral.to_account_info(),
            authority: ctx.accounts.minter.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
        );
        token::transfer(cpi_ctx, collat)?;

        // Mint tokens to caller
        let cpi_accounts = MintTo {
            mint: ctx.accounts.vault_token_mint.to_account_info(),
            to: ctx.accounts.caller_vault_token.to_account_info(),
            authority: ctx.accounts.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED.as_ref(), &[ctx.accounts.vault_state.bump]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            seeds,
        );
        token::mint_to(cpi_ctx, amt)?;

        emit!(DepositEvent{
            who: ctx.accounts.minter.key(),
            token_mint: ctx.accounts.collateral_token_mint.key(),
            amt: collat,
        });

        Ok(())
    }

    pub fn redeem(ctx: Context<Redeem>, amt: u64) -> Result<()> {
        let rate = ctx.accounts.exchange_rate.redeem_rate as u128;
        let decimals = ctx.accounts.collateral_token_mint.decimals;
        let collat_raw: u64 = (amt as u128 * rate / DECIMALS_SCALAR).try_into().unwrap();
        let collat = if decimals < 9 {
            collat_raw / 10_u64.pow(9 - decimals as u32)
        } else {
            collat_raw
        };

        if rate == 0 {
            return Err(MintError::AssetNotSupported.into());
        }

        if !ctx.accounts.user_permissions.can_redeem {
            return Err(MintError::NotAnApprovedRedeemer.into());
        }

        // Transfer collateral to the caller
        let transfer_instruction = Transfer {
            from: ctx.accounts.program_collateral.to_account_info(),
            to: ctx.accounts.caller_collateral.to_account_info(),
            authority: ctx.accounts.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, &[ctx.bumps.vault_state]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
            seeds,
        );
        token::transfer(cpi_ctx, collat)?;

        // Burn staked tokens that caller redeemed
        let cpi_accounts = Burn {
            mint: ctx.accounts.vault_token_mint.to_account_info(),
            from: ctx.accounts.caller_vault_token.to_account_info(),
            authority: ctx.accounts.redeemer.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::burn(cpi_ctx, amt)?;

        emit!(RedeemEvent{
            who: ctx.accounts.redeemer.key(),
            token_mint: ctx.accounts.collateral_token_mint.key(),
            amt: amt,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amt: u64) -> Result<()> {
        if !ctx.accounts.vault_state.asset_managers.contains(&ctx.accounts.caller.key()) {
            return Err(MintError::NotManager.into());
        }

        let destination = &ctx.accounts.destination.key();
        if !ctx.accounts.vault_state.withdraw_addresses.contains(destination) {
            return Err(MintError::NotWithdrawer.into());
        }

        // Transfer collateral
        let transfer_instruction = Transfer {
            from: ctx.accounts.program_collat.to_account_info(),
            to: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, &[ctx.accounts.vault_state.bump]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
            seeds,
        );

        token::transfer(cpi_ctx, amt)?;

        emit!(WithdrawEvent {
            who: *destination,
            token_mint: ctx.accounts.collat_mint.key(),
            amt: amt,
        });

        Ok(())
    }

    pub fn whitelist_minter(ctx: Context<UserPermissions>, user: Pubkey) -> Result<()> {
        if !ctx.accounts.vault_state.role_managers.contains(&ctx.accounts.caller.key()) {
            return Err(MintError::NotManager.into());
        }

        ctx.accounts.user_permissions.key = user;
        ctx.accounts.user_permissions.can_mint = true;

        emit!(NewMinterEvent{
            new_minter: user,
            added_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn remove_minter(ctx: Context<UserPermissions>, user: Pubkey) -> Result<()> {
        if !ctx.accounts.vault_state.role_managers.contains(&ctx.accounts.caller.key()) {
            return Err(MintError::NotManager.into());
        }

        ctx.accounts.user_permissions.can_mint = false;

        emit!(MinterRemovedEvent{
            removed: user,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn whitelist_redeemer(ctx: Context<UserPermissions>, user: Pubkey) -> Result<()> {
        if !ctx.accounts.vault_state.role_managers.contains(&ctx.accounts.caller.key()) {
            return Err(MintError::NotManager.into());
        }

        ctx.accounts.user_permissions.key = user;
        ctx.accounts.user_permissions.can_redeem = true;

        emit!(NewRedeemerEvent{
            new_redeemer: user,
            added_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn remove_redeemer(ctx: Context<UserPermissions>, user: Pubkey) -> Result<()> {
        if !ctx.accounts.vault_state.role_managers.contains(&ctx.accounts.caller.key()) {
            return Err(MintError::NotManager.into());
        }

        ctx.accounts.user_permissions.can_redeem = false;

        emit!(RedeemerRemovedEvent{
            removed: user,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn add_asset_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        let caller = ctx.accounts.caller.key();

        if caller != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.asset_managers;

        if managers.len() > MAX_MANAGER_ADDRESSES {
            return Err(MintError::MaxArrayLength.into());
        }

        if !managers.contains(&manager) {
            managers.push(manager);
        } else {
            return Err(MintError::AlreadyAssetManager.into());
        }

        emit!(NewAssetManagerEvent {
            new_asset_manager: manager,
            added_by: caller,
        });

        Ok(())
    }

    pub fn add_role_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        let caller = ctx.accounts.caller.key();

        if caller != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.role_managers;

        if managers.len() > MAX_MANAGER_ADDRESSES {
            return Err(MintError::MaxArrayLength.into());
        }

        if !managers.contains(&manager) {
            managers.push(manager);
        } else {
            return Err(MintError::AlreadyRoleManager.into());
        }

        emit!(NewRoleManagerEvent {
            new_role_manager: manager,
            added_by: caller,
        });

        Ok(())
    }

    pub fn remove_asset_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.asset_managers;

        if let Some(i) = managers.iter().position(|&x| x == manager) {
            managers.swap_remove(i);
        } else {
            return Err(MintError::NotManagerYet.into());
        }

        emit!(AssetManagerRemovedEvent{
            asset_manager_removed: manager,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn remove_role_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.role_managers;

        if let Some(i) = managers.iter().position(|&x| x == manager) {
            managers.swap_remove(i);
        } else {
            return Err(MintError::NotManagerYet.into());
        }

        emit!(RoleManagerRemovedEvent{
            role_manager_removed: manager,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }

    pub fn add_withdraw_address(ctx: Context<WithdrawAddresses>, address: Pubkey) -> Result<()> {
        let caller = ctx.accounts.caller.key();

        if caller != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let withdraw_addresses = &mut ctx.accounts.vault_state.withdraw_addresses;

        if withdraw_addresses.len() > MAX_WITHDRAW_ADRESSES {
            return Err(MintError::MaxArrayLength.into());
        }

        if !withdraw_addresses.contains(&address) {
            withdraw_addresses.push(address);
        } else {
            return Err(MintError::AlreadyWithdrawer.into());
        }

        emit!(WithdrawAddressAdded {
            address: address,
            added_by: caller,
        });

        Ok(())
    }

    pub fn remove_withdraw_address(ctx: Context<WithdrawAddresses>, address: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let withdraw_addresses = &mut ctx.accounts.vault_state.withdraw_addresses;

        if let Some(i) = withdraw_addresses.iter().position(|&x| x == address) {
            withdraw_addresses.swap_remove(i);
        } else {
            return Err(MintError::NotWithdrawerYet.into());
        }

        emit!(WithdrawAddressRemoved{
            address: address,
            removed_by: ctx.accounts.caller.key(),
        });

        Ok(())
    }


    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let vault_state = &mut ctx.accounts.vault_state;
        vault_state.admin = new_admin;

        emit!(AdminTransferEvent{
            old_admin: ctx.accounts.caller.key(),
            new_admin: new_admin,
        });

        Ok(())
    }
}

impl VaultState {
    pub fn new(&mut self) {
        self.asset_managers = Vec::with_capacity(20);
        self.role_managers = Vec::with_capacity(20);
        self.withdraw_addresses = Vec::with_capacity(50);
    }
}

#[event]
pub struct AssetModifiedEvent {
    who: Pubkey,
    asset: Pubkey,
    deposit_rate: u64,
    redeem_rate: u64,
}

#[event]
pub struct DepositEvent {
    who: Pubkey,
    token_mint: Pubkey,
    amt: u64,
}

#[event]
pub struct WithdrawEvent {
    who: Pubkey,
    token_mint: Pubkey,
    amt: u64,
}

#[event]
pub struct RedeemEvent {
    who: Pubkey,
    token_mint: Pubkey,
    amt: u64,
}

#[event]
pub struct NewMinterEvent {
    new_minter: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct WithdrawAddressAdded {
    address: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct WithdrawAddressRemoved {
    address: Pubkey,
    removed_by: Pubkey,
}

#[event]
pub struct MinterRemovedEvent {
    removed: Pubkey,
    removed_by: Pubkey,
}

#[event]
pub struct NewRedeemerEvent {
    new_redeemer: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct RedeemerRemovedEvent {
    removed: Pubkey,
    removed_by: Pubkey,
}

#[event]
pub struct NewAssetManagerEvent {
    new_asset_manager: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct NewRoleManagerEvent {
    new_role_manager: Pubkey,
    added_by: Pubkey,
}

#[event]
pub struct AssetManagerRemovedEvent {
    asset_manager_removed: Pubkey,
    removed_by: Pubkey,
}

#[event]
pub struct RoleManagerRemovedEvent {
    role_manager_removed: Pubkey,
    removed_by: Pubkey,
}

#[event]
pub struct AdminTransferEvent{
    old_admin: Pubkey,
    new_admin: Pubkey,
}

#[error_code]
pub enum MintError {
    #[msg("The provided account is not an approved minter")]
    NotAnApprovedMinter,
    #[msg("The provided account is not an approved redeemer")]
    NotAnApprovedRedeemer,
    #[msg("The caller is not a manager")]
    NotManager,
    #[msg("The caller is not an admin")]
    NotAdmin,
    #[msg("The provided address is not a whitelisted withdraw address")]
    NotWithdrawer,
    #[msg("The minter is already whitelisted")]
    AlreadyMinter,
    #[msg("The redeemer is already whitelisted")]
    AlreadyRedeemer,
    #[msg("The withdraw address is already whitelisted")]
    AlreadyWithdrawer,
    #[msg("The provided key is already an asset manager")]
    AlreadyAssetManager,
    #[msg("The provided key is already a role manager")]
    AlreadyRoleManager,
    #[msg("The minter is not whitelisted")]
    MinterNotWhitelisted,
    #[msg("The withdraw address is not whitelisted")]
    AddressNotWhitelisted,
    #[msg("The redeemer is not whitelisted")]
    RedeemerNotWhitelisted,
    #[msg("The provided key is not yet a manager")]
    NotManagerYet,
    #[msg("The provided key is not yet a whitelisted withdraw address")]
    NotWithdrawerYet,
    #[msg("Max mint for this block has been exceeded")]
    MaxMintExceeded,
    #[msg("Max redeem for this block has been exceeded")]
    MaxRedeemExceeded,
    #[msg("Asset not supported by mint vault")]
    AssetNotSupported,
    #[msg("Asset already supported by mint vault")]
    AssetAlreadySupported,
    #[msg("Max array length has been exceeded")]
    MaxArrayLength,
}
