use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount, Transfer};

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

const DECIMALS_SCALAR: u64 = 1_000_000_000;
const MINT_SEED: &[u8] = b"mint";
const TOKEN_ATA_SEED: &[u8] = b"token-account";
const EXCHANGE_RATE_SEED: &[u8] = b"exchange-rate";
const VAULT_STATE_SEED: &[u8] = b"vault-state";

#[account]
pub struct VaultState {
    pub vault_token_mint: Pubkey,
    pub minted_per_block: u64,
    pub redeemed_per_block: u64,
    pub approved_minters: Vec<Pubkey>,
    pub approved_redeemers: Vec<Pubkey>,
    pub managers: Vec<Pubkey>,
    pub admin: Pubkey,
    pub bump: u8,
}

#[account]
pub struct ExchangeRate {
    /// The deposit rate is defined in sclaed units of stable coin per asset coin
    /// (1e9)
    deposit_rate: u64,
    /// The redeem rate is defined in scaled units of asset coin per stable coin
    redeem_rate: u64,
}

#[program]
pub mod vault {
    use anchor_spl::token::Burn;

    use super::*;

    pub fn initialize_vault_state(
        ctx: Context<InitializeVaultState>,
        admin: Pubkey,
    ) -> Result<()> {
        ctx.accounts.vault_state.minted_per_block = 0;
        ctx.accounts.vault_state.redeemed_per_block = 0;
        ctx.accounts.vault_state.approved_minters = vec![admin];
        ctx.accounts.vault_state.approved_redeemers = vec![admin];
        ctx.accounts.vault_state.admin = admin;
        ctx.accounts.vault_state.vault_token_mint = ctx.accounts.vault_token.key();
        ctx.accounts.vault_state.bump = ctx.accounts.vault_state.bump;

        Ok(())
    }

    // The deposit rate is (stable units / asset units) [how many stable coins a depositer will get per asset coin]
    // The redeem rate is (asset units / stable units) [how many asset coins a depositer will get per stable coin]
    pub fn add_asset(
        ctx: Context<AddAsset>,
        _asset: Pubkey,
        deposit_rate: u64,
        redeem_rate: u64,
    ) -> Result<()> {
        if ctx.accounts.authority.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        ctx.accounts.exchange_rate.deposit_rate = deposit_rate;
        ctx.accounts.exchange_rate.redeem_rate = redeem_rate;

        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, collat: u64) -> Result<()> {
        let state = &ctx.accounts.vault_state;
        let rate = ctx.accounts.exchange_rate.deposit_rate;
        let amt = collat * rate / DECIMALS_SCALAR;

        let approved_minters = &state.approved_minters;
        if !approved_minters.contains(&ctx.accounts.minter.key()) {
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

        // mint tokens to caller
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

        Ok(())
    }

    pub fn redeem(ctx: Context<Redeem>, amt: u64) -> Result<()> {
        let state = &ctx.accounts.vault_state;
        let rate = ctx.accounts.exchange_rate.redeem_rate;
        let decimals = ctx.accounts.collateral_token_mint.decimals;
        let collat = amt * rate / 10_u64.pow(decimals as u32);

        let approved_minters = &state.approved_minters;
        if !approved_minters.contains(&ctx.accounts.redeemer.key()) {
            return Err(MintError::NotAnApprovedRedeemer.into());
        }

        // Transfer collateral to the caller
        let transfer_instruction = Transfer {
            from: ctx.accounts.program_collateral.to_account_info(),
            to: ctx.accounts.caller_collateral.to_account_info(),
            authority: ctx.accounts.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, &[ctx.accounts.vault_state.bump]];
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
            authority: ctx.accounts.vault_token_mint.to_account_info(),
        };

        let seeds: &[&[u8]] = &[MINT_SEED.as_ref(), &[ctx.bumps.vault_token_mint]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            seeds,
        );
        token::burn(cpi_ctx, amt)?;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amt: u64) -> Result<()> {
        let caller = &ctx.accounts.caller.key();
        if !ctx.accounts.vault_state.managers.contains(caller) {
            return Err(MintError::NotManager.into());
        }

        // Transfer collateral
        let transfer_instruction = Transfer {
            from: ctx.accounts.program_collat.to_account_info(),
            to: ctx.accounts.caller.to_account_info(),
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
            who: *caller,
            amt: amt,
        });

        Ok(())
    }

    pub fn whitelist_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_minters = &mut ctx.accounts.vault_state.approved_minters;

        if approved_minters.contains(&minter.clone()) {
            return Err(MintError::AlreadyMinter.into());
        }

        approved_minters.push(minter);

        Ok(())
    }

    pub fn remove_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_minters = &mut ctx.accounts.vault_state.approved_minters;

        if let Some(i) = approved_minters.iter().position(|&x| x == minter) {
            approved_minters.swap_remove(i);
        } else {
            return Err(MintError::MinterNotWhitelisted.into());
        }

        Ok(())
    }

    pub fn whitelist_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_redeemers = &mut ctx.accounts.vault_state.approved_redeemers;

        if approved_redeemers.contains(&redeemer.clone()) {
            return Err(MintError::AlreadyRedeemer.into());
        }

        approved_redeemers.push(redeemer);

        Ok(())
    }

    pub fn remove_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_redeemers = &mut ctx.accounts.vault_state.approved_redeemers;

        if let Some(i) = approved_redeemers.iter().position(|&x| x == redeemer) {
            approved_redeemers.swap_remove(i);
        } else {
            return Err(MintError::RedeemerNotWhitelisted.into());
        }

        Ok(())
    }

    pub fn add_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        let caller = ctx.accounts.caller.key();

        if caller != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.managers;

        if !managers.contains(&manager) {
            managers.push(manager);
        } else {
            return Err(MintError::AlreadyManager.into());
        }

        emit!(NewManagerEvent {
            new_manager: manager,
            added_by: caller,
        });

        Ok(())
    }

    pub fn remove_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.vault_state.managers;

        if let Some(i) = managers.iter().position(|&x| x == manager) {
            managers.swap_remove(i);
        } else {
            return Err(MintError::NotManagerYet.into());
        }

        Ok(())
    }

    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.vault_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let vault_state = &mut ctx.accounts.vault_state;
        vault_state.admin = new_admin;

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(admin: Pubkey)]
pub struct InitializeVaultState<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    // todo: space
    #[account(
        init, 
        payer = signer, 
        space = 256,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init, 
        payer = signer, 
        mint::decimals = 9, 
        mint::authority = vault_token,
        seeds = [MINT_SEED], 
        bump
    )]
    pub vault_token: Account<'info, Mint>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(asset: Pubkey)]
pub struct AddAsset<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    #[account(
        init_if_needed, 
        payer = authority, 
        seeds = [EXCHANGE_RATE_SEED, asset.as_ref()],
        space = 8 + 8 + 8,
        bump
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,

    /// The program owned collateral
    #[account(
        init_if_needed,
        seeds = [TOKEN_ATA_SEED],
        bump,
        payer = authority,
        token::mint = collateral_token_mint,
        token::authority = vault_state,
    )]
    pub program_collateral: Account<'info, TokenAccount>,

    #[account(mut)]
    pub collateral_token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(collat: u64)]
pub struct Deposit<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    /// The program owned collateral
    #[account(
        mut,
        seeds = [TOKEN_ATA_SEED],
        bump,
    )]
    pub program_collateral: Account<'info, TokenAccount>,
    /// The caller owned collateral
    #[account(
        mut,
        token::mint = collateral_token_mint,
        token::authority = minter,
    )]
    pub caller_collateral: Account<'info, TokenAccount>,
    /// The caller owned vault token ATA
    #[account(
        token::mint = vault_token_mint,
        token::authority = minter,
    )]
    pub caller_vault_token: Account<'info, TokenAccount>,
    #[account(
        seeds = [EXCHANGE_RATE_SEED, collateral_token_mint.key().as_ref()],
        bump,
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,
    #[account(
        mut,
        constraint = vault_token_mint.key() == vault_state.vault_token_mint,
        seeds = [MINT_SEED],
        bump
    )]
    pub vault_token_mint: Account<'info, Mint>,

    /// The collateral token mint address,
    /// we dont need any contraints here becauase we also need an exchange rate address
    /// that is owned by this program and associated with this mint
    #[account(mut)]
    pub collateral_token_mint: Account<'info, Mint>,
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub minter: Signer<'info>,
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    /// The program owned collateral
    #[account(
        mut,
        seeds = [TOKEN_ATA_SEED],
        bump,
    )]
    pub program_collateral: Account<'info, TokenAccount>,
    /// The caller owned collateral
    #[account(
        mut,
        token::mint = collateral_token_mint,
        token::authority = redeemer,
    )]
    pub caller_collateral: Account<'info, TokenAccount>,
    /// The caller owned vault token ATA
    #[account(
        mut,
        token::mint = vault_token_mint,
        token::authority = redeemer,
    )]
    pub caller_vault_token: Account<'info, TokenAccount>,
    #[account(
        seeds = [EXCHANGE_RATE_SEED, collateral_token_mint.key().as_ref()],
        bump,
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,
    #[account(
        mut,
        constraint = vault_token_mint.key() == vault_state.vault_token_mint,
        seeds = [MINT_SEED],
        bump
    )]
    pub vault_token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub redeemer: Signer<'info>,
    /// The collateral token mint address,
    /// we dont need any contraints here becauase we also need an exchange rate address
    /// that is owned by this program and associated with this mint
    #[account(mut)]
    pub collateral_token_mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        seeds = [TOKEN_ATA_SEED],
        bump
    )]
    pub program_collat: Account<'info, TokenAccount>,

    #[account(mut)]
    pub caller: Account<'info, TokenAccount>,
    #[account(mut)]
    pub collat_mint: Account<'info, Mint>,
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
}

#[derive(Accounts)]
pub struct Minters<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Redeemers<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Managers<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetMaxMintPerBlock<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetMaxRedeemPerBlock<'info> {
    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[event]
pub struct TransferEvent {
    from: Pubkey,
    to: Pubkey,
    amt: u64,
}

#[event]
pub struct DepositEvent {
    who: Pubkey,
    amt: u64,
}

#[event]
pub struct WithdrawEvent {
    who: Pubkey,
    amt: u64,
}

#[event]
pub struct RedeemEvent {
    who: Pubkey,
    amt: u64,
}

#[event]
pub struct NewManagerEvent {
    new_manager: Pubkey,
    added_by: Pubkey,
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
    #[msg("The minter is already whitelisted")]
    AlreadyMinter,
    #[msg("The redeemer is already whitelisted")]
    AlreadyRedeemer,
    #[msg("The provided key is already a manager")]
    AlreadyManager,
    #[msg("The minter is not whitelisted")]
    MinterNotWhitelisted,
    #[msg("The redeemer is not whitelisted")]
    RedeemerNotWhitelisted,
    #[msg("The provided key is not yet a manager")]
    NotManagerYet,
    #[msg("Max mint for this block has been exceeded")]
    MaxMintExceeded,
    #[msg("Max redeem for this block has been exceeded")]
    MaxRedeemExceeded,
    #[msg("Asset not supported by mint vault")]
    AssetNotSupported,
    #[msg("Asset already supported by mint vault")]
    AssetAlreadySupported,
}
