pub mod stake;

use anchor_lang::prelude::*;
use anchor_spl::token::{self, TokenAccount};
use anchor_spl::token::{Token, MintTo, Transfer};
declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

#[program]
pub mod vault {
    use super::*;

    pub fn initialize_mint_state(
        ctx: Context<InitializeMintState>,
        max_mint_per_block: u64,
        max_redeem_per_block: u64,
        exchange_rate: u64,
    ) -> Result<()> {
        let admin = ctx.accounts.admin.key();
        let mint_state = &mut ctx.accounts.mint_state;
        //let b = ctx.bumps.mint_state;
    
        mint_state.supported_assets = vec![];
        mint_state.max_mint_per_block = max_mint_per_block;
        mint_state.max_redeem_per_block = max_redeem_per_block;
        mint_state.minted_per_block = 0;
        mint_state.redeemed_per_block = 0;
        mint_state.exchange_rate = exchange_rate;
        mint_state.approved_minters = vec![admin];
        mint_state.approved_redeemers = vec![admin];
        mint_state.admin = admin;
    
        Ok(())
    }

    pub fn mint_token(ctx: Context<MintToken>, amt: u64) -> Result<()> {
        let approved_minters = &ctx.accounts.mint_state.approved_minters;
        let authority = ctx.accounts.authority.key();
        let rate = ctx.accounts.mint_state.exchange_rate;

        if !approved_minters.contains(&authority) {
            return Err(ErrorCode::NotAnApprovedMinter.into());
        }

        // Create the MintTo struct for context
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();

        // Create the CpiContext for the request
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        // Execute with anchor's helper function
        token::mint_to(cpi_ctx, amt*rate)?; // TODO: add decimals with rate
        
        Ok(())
    }

    pub fn transfer_token(ctx: Context<TransferToken>, amt: u64) -> Result<()> {
        if ctx.accounts.from_authority.key() != ctx.accounts.mint_state.admin.key() {
            return Err(ErrorCode::NotAdmin.into());
        }

        // Create the Transfer struct for context
        let transfer_instruction = Transfer{
            from: ctx.accounts.from.to_account_info(),
            to: ctx.accounts.to.to_account_info(),
            authority: ctx.accounts.from_authority.to_account_info(),
        };
         
        let cpi_program = ctx.accounts.token_program.to_account_info();

        // Create the Context for Transfer request
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        // Execute anchor's helper function to transfer tokens
        anchor_spl::token::transfer(cpi_ctx, amt)?;
 
        Ok(())
    }

    pub fn whitelist_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if !ctx.accounts.mint_state.managers.contains(&ctx.accounts.caller.key()) {
            return Err(ErrorCode::NotManager.into());
        }

        let approved_minters = &mut ctx.accounts.mint_state.approved_minters;

        if !approved_minters.contains(&minter.clone()) {
            approved_minters.push(minter);
        } else {
            return Err(ErrorCode::AlreadyWhitelisted.into());
        }

        Ok(())
    }

    pub fn remove_minter(ctx: Context<Minters>, minter: Pubkey) -> Result<()> {
        if !ctx.accounts.mint_state.managers.contains(&ctx.accounts.caller.key()) {
            return Err(ErrorCode::NotManager.into());
        }

        let approved_minters = &mut ctx.accounts.mint_state.approved_minters;

        if let Some(i) = approved_minters.iter().position(|&x| x == minter) {
            approved_minters.swap_remove(i);
        } else {
            return Err(ErrorCode::MinterNotWhitelisted.into())
        }
        
        Ok(())
    }

    pub fn whitelist_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if !ctx.accounts.mint_state.managers.contains(&ctx.accounts.caller.key()) {
            return Err(ErrorCode::NotManager.into());
        }

        let approved_redeemers = &mut ctx.accounts.mint_state.approved_redeemers;

        if !approved_redeemers.contains(&redeemer.clone()) {
            approved_redeemers.push(redeemer);
        } else {
            return Err(ErrorCode::AlreadyWhitelisted.into());
        }

        Ok(())
    }

    pub fn remove_redeemer(ctx: Context<Redeemers>, redeemer: Pubkey) -> Result<()> {
        if !ctx.accounts.mint_state.managers.contains(&ctx.accounts.caller.key()) {
            return Err(ErrorCode::NotManager.into());
        }

        let approved_redeemers = &mut ctx.accounts.mint_state.approved_redeemers;

        if let Some(i) = approved_redeemers.iter().position(|&x| x == redeemer) {
            approved_redeemers.swap_remove(i);
        } else {
            return Err(ErrorCode::RedeemerNotWhitelisted.into())
        }
        
        Ok(())
    }

    pub fn add_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(ErrorCode::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.mint_state.managers;

        if !managers.contains(&manager) {
            managers.push(manager);
        } else {
            return Err(ErrorCode::AlreadyManager.into());
        }

        Ok(())
    }

    pub fn remove_manager(ctx: Context<Managers>, manager: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(ErrorCode::NotAdmin.into());
        }

        let managers = &mut ctx.accounts.mint_state.managers;

        if let Some(i) = managers.iter().position(|&x| x == manager) {
            managers.swap_remove(i);
        } else {
            return Err(ErrorCode::NotManagerYet.into());
        }

        Ok(())
    }

    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        if ctx.accounts.caller.key() != ctx.accounts.mint_state.admin {
            return Err(ErrorCode::NotAdmin.into());
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

    pub fn set_max_redeem_per_block(ctx: Context<SetMaxRedeemPerBlock>, new_max: u64) -> Result<()> {
        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.max_redeem_per_block = new_max;
        Ok(())
    }

  
}

#[derive(Accounts)]
pub struct MintToken<'info> {
    /// CHECK: This is the token that we want to mint
    #[account(mut)]
    pub mint: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    /// CHECK: This is the token account that we want to mint tokens to
    #[account(mut)]
    pub token_account: UncheckedAccount<'info>,
    /// CHECK: the authority of the mint account
    #[account(mut)]
    pub authority: AccountInfo<'info>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
}

#[derive(Accounts)]
pub struct TransferToken<'info> {
    pub token_program: Program<'info, Token>,
    #[account(mut)]
    pub from: Account<'info, TokenAccount>,
    /// CHECK: The associated token account that we are transferring the token to
    #[account(mut)]
    pub to: AccountInfo<'info>,
    // the authority of the from account 
    pub from_authority: Signer<'info>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
}

#[derive(Accounts)]
pub struct Minters<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    /// CHECK: The owner of the token contract
    #[account(signer)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Redeemers<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    /// CHECK: The owner of the token contract
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
    pub caller: Signer<'info>
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

#[account]
pub struct MintState {
    pub supported_assets: Vec<Pubkey>,
    pub max_mint_per_block: u64,
    pub max_redeem_per_block: u64,
    pub minted_per_block: u64,
    pub redeemed_per_block: u64,
    pub exchange_rate: u64, 
    pub approved_minters: Vec<Pubkey>,
    pub approved_redeemers: Vec<Pubkey>,
    pub managers: Vec<Pubkey>,
    pub admin: Pubkey,
}

#[derive(Accounts)]
pub struct InitializeMintState<'info> {
    #[account(init_if_needed, payer = admin, space = 1024, seeds = [b"mint-state"], bump)]
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



#[error_code]
pub enum ErrorCode {
    #[msg("The provided account is not an approved minter")]
    NotAnApprovedMinter,
    #[msg("The minter is already whitelisted")]
    AlreadyWhitelisted,
    #[msg("The minter is not whitelisted")]
    MinterNotWhitelisted,
    #[msg("The caller is not an admin")]
    NotAdmin,
    #[msg("Max mint for this block has been exceeded")]
    MaxMintPerBlockExceeded,
    #[msg("The redeemer is not whitelisted")]
    RedeemerNotWhitelisted,
    #[msg("The provided key is not yet a manager")]
    NotManagerYet,
    #[msg("The provided key is already a manager")]
    AlreadyManager,
    #[msg("The caller is not a manager")]
    NotManager,
}