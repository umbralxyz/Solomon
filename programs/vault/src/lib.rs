use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount, Transfer, InitializeMint};
use std::collections::{HashMap, HashSet};

declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

#[account]
pub struct MintState {
    pub vault_token_mint: Pubkey,
    pub max_mint_per_block: u64,
    pub max_redeem_per_block: u64,
    pub minted_per_block: u64,
    pub redeemed_per_block: u64,
    pub approved_minters: Vec<Pubkey>,
    pub approved_redeemers: Vec<Pubkey>,
    pub managers: Vec<Pubkey>,
    pub admin: Pubkey,
}

#[account]
struct ExchangeRate {
    deposit_rate: u64,
    redeem_rate: u64,
}

#[program]
pub mod vault {
    use anchor_spl::token::Burn;

    use super::*;

    pub fn initialize_mint_state(
        ctx: Context<InitializeMintState>,
        max_mint_per_block: u64,
        max_redeem_per_block: u64,
    ) -> Result<()> {
        let admin = ctx.accounts.admin.key();
        let mint_state = &mut ctx.accounts.mint_state;
        //let b = ctx.bumps.mint_state;

        mint_state.max_mint_per_block = max_mint_per_block;
        mint_state.max_redeem_per_block = max_redeem_per_block;
        mint_state.minted_per_block = 0;
        mint_state.redeemed_per_block = 0;
        mint_state.approved_minters = vec![admin];
        mint_state.approved_redeemers = vec![admin];
        mint_state.admin = admin;

        let create_mint = token::Cre

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
        if ctx.accounts.authority.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        ctx.accounts.exchange_rate.deposit_rate = deposit_rate;
        ctx.accounts.exchange_rate.redeem_rate = redeem_rate;

        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, collat: u64) -> Result<()> {
        let state = &ctx.accounts.mint_state;
        let rate = ctx.accounts.exchange_rate.deposit_rate;
        let amt = collat * rate;
        if state.redeemed_per_block + amt > state.max_redeem_per_block {
            return Err(MintError::MaxRedeemExceeded.into());
        }

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

        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), transfer_instruction);

        token::transfer(cpi_ctx, collat)?;

        // mint tokens to caller
        let cpi_accounts = MintTo {
            mint: ctx.accounts.vault_token_mint.to_account_info(),
            to: ctx.accounts.caller_vault_token.to_account_info(),
            authority: ctx.accounts.vault_token_mint.to_account_info(),
        };

        // todo: deploy token

        // let cpi_program = ctx.accounts.token_program.to_account_info();

        // let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, &[&[
        //     b"mintstate".as_ref(),
        // ]]);

        // token::mint_to(cpi_ctx, amt)?;

        Ok(())
    }

    pub fn redeem(ctx: Context<Redeem>, amt: u64) -> Result<()> {
        let caller = &ctx.accounts.redeemer.key();
        let state = &mut ctx.accounts.mint_state;
        let mut rate = 0;
        let asset = ctx.accounts.collat_program.key();

        if let Some(i) = state
            .exchange_rates
            .iter()
            .position(|(pubkey, _, _)| *pubkey == asset)
        {
            rate = state.exchange_rates[i].2;
        } else {
            return Err(MintError::AssetNotSupported.into());
        }

        let collat = amt * rate;

        if state.redeemed_per_block + amt > state.max_redeem_per_block {
            return Err(MintError::MaxRedeemExceeded.into());
        }

        let approved_redeemers = &state.approved_redeemers;
        let authority = ctx.accounts.redeemer.key();

        if !approved_redeemers.contains(&authority) {
            return Err(MintError::NotAnApprovedRedeemer.into());
        }

        // Transfer "mint token" to mint vault
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_token.to_account_info(),
            to: ctx.accounts.mint_token_vault.to_account_info(),
            authority: ctx.accounts.redeemer.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        // Transfer collat to redeemer
        let transfer_instruction = Transfer {
            from: ctx.accounts.mint_collat_vault.to_account_info(),
            to: ctx.accounts.caller_collat.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };

        let cpi_program = ctx.accounts.collat_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, collat)?;

        // burn tokens that caller redeemed
        let cpi_accounts = Burn {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.mint_token_vault.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::burn(cpi_ctx, amt)?;

        state.redeemed_per_block += amt;

        emit!(RedeemEvent {
            who: *caller,
            amt: amt,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amt: u64) -> Result<()> {
        let caller = &ctx.accounts.caller.key();
        if !ctx.accounts.mint_state.managers.contains(caller) {
            return Err(MintError::NotManager.into());
        }

        // Transfer collateral
        let transfer_instruction = Transfer {
            from: ctx.accounts.mint_collat_vault.to_account_info(),
            to: ctx.accounts.caller.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        emit!(WithdrawEvent {
            who: *caller,
            amt: amt,
        });

        Ok(())
    }

    pub fn whitelist_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_minters = &mut ctx.accounts.mint_state.approved_minters;

        if approved_minters.contains(&minter.clone()) {
            return Err(MintError::AlreadyMinter.into());
        }

        approved_minters.push(minter);

        Ok(())
    }

    pub fn remove_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_minters = &mut ctx.accounts.mint_state.approved_minters;

        if let Some(i) = approved_minters.iter().position(|&x| x == minter) {
            approved_minters.swap_remove(i);
        } else {
            return Err(MintError::MinterNotWhitelisted.into());
        }

        Ok(())
    }

    pub fn whitelist_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_redeemers = &mut ctx.accounts.mint_state.approved_redeemers;

        if approved_redeemers.contains(&redeemer.clone()) {
            return Err(MintError::AlreadyRedeemer.into());
        }

        approved_redeemers.push(redeemer);

        Ok(())
    }

    pub fn remove_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let approved_redeemers = &mut ctx.accounts.mint_state.approved_redeemers;

        if let Some(i) = approved_redeemers.iter().position(|&x| x == redeemer) {
            approved_redeemers.swap_remove(i);
        } else {
            return Err(MintError::RedeemerNotWhitelisted.into());
        }

        Ok(())
    }

    pub fn add_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        let caller = ctx.accounts.caller.key();

        if caller != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.mint_state.managers;

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
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.mint_state.managers;

        if let Some(i) = managers.iter().position(|&x| x == manager) {
            managers.swap_remove(i);
        } else {
            return Err(MintError::NotManagerYet.into());
        }

        Ok(())
    }

    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.admin = new_admin;

        Ok(())
    }

    pub fn set_max_mint_per_block(ctx: Context<SetMaxMintPerBlock>, new_max: u64) -> Result<()> {
        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.max_mint_per_block = new_max;
        Ok(())
    }

    pub fn set_max_redeem_per_block(
        ctx: Context<SetMaxRedeemPerBlock>,
        new_max: u64,
    ) -> Result<()> {
        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.max_redeem_per_block = new_max;
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(asset: Pubkey)]
pub struct AddAsset<'info> {
    #[account(mut)]
    pub system_program: AccountInfo<'info>,
    #[account(mut, seeds = [b"mintstate"], bump)]
    pub mint_state: Account<'info, MintState>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init_if_needed, 
        payer = authority, 
        seeds = [b"exchangerate", asset.as_ref()],
        space = 8 + 8 + 8,
        bump
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,
}

#[derive(Accounts)]
#[instruction(collat: u64)]
pub struct Deposit<'info> {
    /// Requiered by init
    #[account(mut)]
    pub system_program: AccountInfo<'info>,
    #[account(mut)]
    pub token_program: AccountInfo<'info>,
    /// The program owned collateral
    #[account(
        init_if_needed,
        seeds = [b"tokenaccount", collateral_token_mint.key().as_ref()],
        bump,
        token::mint = collateral_token_mint,
        token::authority = mint_state,
        payer = minter,
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
        mut,
        token::mint = vault_token_mint,
        token::authority = minter,
    )]
    pub caller_vault_token: Account<'info, TokenAccount>,

    /// The minter
    #[account(
        seeds = [b"tokenaccount", collateral_token_mint.key().as_ref()],
        bump,
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(mut)]
    pub minter: Signer<'info>,
    /// todo check is valid mint
    #[account(mut)]
    pub collateral_token_mint: Account<'info, Mint>,
    /// Stable coin
    /// todo check is correct token
    #[account(mut)]
    pub vault_token_mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    pub token_program: Program<'info, Token>,
    pub collat_program: Program<'info, Token>,
    /// CHECK: the mint's collateral vault
    #[account(mut)]
    pub mint_collat_vault: Account<'info, TokenAccount>,
    /// CHECK: the mint's "mint token" vault
    #[account(mut)]
    pub mint_token_vault: Account<'info, TokenAccount>,
    /// CHECK: the redeemer's collateral account
    #[account(mut)]
    pub caller_collat: Account<'info, TokenAccount>,
    /// CHECK: the redeemer's "mint token" account
    #[account(mut)]
    pub caller_token: Account<'info, TokenAccount>,
    pub redeemer: Signer<'info>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    pub vault_authority: Signer<'info>,
    #[account(mut)]
    pub mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub token_program: Program<'info, Token>,
    /// CHECK: the token account to withdraw from
    #[account(mut)]
    pub mint_collat_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub caller: Account<'info, TokenAccount>,
    /// CHECK: an individual allowed to withdraw
    pub vault_authority: Signer<'info>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
}

#[derive(Accounts)]
pub struct Minters<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Redeemers<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Managers<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetMaxMintPerBlock<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetMaxRedeemPerBlock<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct InitializeMintState<'info> {
    #[account(init, payer = admin, space = 1024, seeds = [b"mintstate"], bump)]
    pub mint_state: Account<'info, MintState>,
    #[account(mut, signer)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Debug)]
struct TokenMetadata {
    ticker: String,
    name: String,
    address: Pubkey,
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
