use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, MintTo, Mint};
declare_id!("A3p6U1p5jjZQbu346LrJb1asrTjkEPhDkfH4CXCYgpEd");

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
        mint_state.exchange_rates = vec![];
        mint_state.approved_minters = vec![admin];
        mint_state.approved_redeemers = vec![admin];
        mint_state.admin = admin;
    
        Ok(())
    }

    // The deposit rate is (stable units / asset units) [how many stable coins a depositer will get per asset coin]
    // The redeem rate is (asset units / stable units) [how many asset coins a depositer will get per stable coin]
    pub fn add_asset(ctx: Context<Asset>, asset: Pubkey, deposit_rate: u64, redeem_rate: u64) -> Result<()> {
        if ctx.accounts.authority.key() != ctx.accounts.mint_state.admin {
            return Err(MintError::NotAdmin.into());
        }

        let state = &mut ctx.accounts.mint_state;

        if state.exchange_rates.iter().any(|(pubkey, _, _)| *pubkey == asset) {
            return Err(MintError::AssetAlreadySupported.into());
        }
        
        state.exchange_rates.push((asset, deposit_rate, redeem_rate));

        Ok(())
    }

    pub fn mint_token(ctx: Context<MintToken>, amt: u64) -> Result<()> {
        let state = &mut ctx.accounts.mint_state;

        if state.minted_per_block + amt > state.max_mint_per_block {
            return Err(MintError::MaxMintExceeded.into())
        }
        
        let authority = ctx.accounts.authority.key();

        if authority != state.admin {
            return Err(MintError::NotAdmin.into());
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

        state.minted_per_block += amt;
        
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, collat: u64) -> Result<()> {
        let state = &ctx.accounts.mint_state;
        let mut rate = 0;
        let asset = ctx.accounts.collat_program.key();

        if let Some(i) = state.exchange_rates.iter().position(|(pubkey, _, _)| *pubkey == asset) {
            rate = state.exchange_rates[i].1;
        } else {
            return Err(MintError::AssetNotSupported.into());
        }

        let amt = collat * rate;

        if state.redeemed_per_block + amt > state.max_redeem_per_block {
            return Err(MintError::MaxRedeemExceeded.into())
        }

        let approved_minters = &state.approved_minters;
        let authority = ctx.accounts.depositer.key();

        if !approved_minters.contains(&authority) {
            return Err(MintError::NotAnApprovedMinter.into());
        }

        // Transfer collat to mint vault
        let transfer_instruction = Transfer{
            from: ctx.accounts.caller_collat.to_account_info(),
            to: ctx.accounts.mint_collat_vault.to_account_info(),
            authority: ctx.accounts.depositer.to_account_info(),
        };
         
        let cpi_program = ctx.accounts.collat_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, collat)?;

        // mint tokens to caller
        
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.caller_token.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::mint_to(cpi_ctx, amt)?; 

        Ok(())
    }

    pub fn redeem(ctx: Context<Redeem>, amt: u64) -> Result<()> {
        let caller = &ctx.accounts.redeemer.key();
        let state = &mut ctx.accounts.mint_state;
        let mut rate = 0;
        let asset = ctx.accounts.collat_program.key();

        if let Some(i) = state.exchange_rates.iter().position(|(pubkey, _, _)| *pubkey == asset) {
            rate = state.exchange_rates[i].2;
        } else {
            return Err(MintError::AssetNotSupported.into());
        }

        let collat = amt * rate;

        if state.redeemed_per_block + amt > state.max_redeem_per_block {
            return Err(MintError::MaxRedeemExceeded.into())
        }

        let approved_redeemers = &state.approved_redeemers;
        let authority = ctx.accounts.redeemer.key();

        if !approved_redeemers.contains(&authority) {
            return Err(MintError::NotAnApprovedRedeemer.into());
        }

        // Transfer "mint token" to mint vault
        let transfer_instruction = Transfer{
            from: ctx.accounts.caller_token.to_account_info(),
            to: ctx.accounts.mint_token_vault.to_account_info(),
            authority: ctx.accounts.redeemer.to_account_info(),
        };
         
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, transfer_instruction);

        token::transfer(cpi_ctx, amt)?;

        // Transfer collat to redeemer
        let transfer_instruction = Transfer{
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
        let transfer_instruction = Transfer{
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
            return Err(MintError::MinterNotWhitelisted.into())
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
            return Err(MintError::RedeemerNotWhitelisted.into())
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

    pub fn set_max_redeem_per_block(ctx: Context<SetMaxRedeemPerBlock>, new_max: u64) -> Result<()> {
        let mint_state = &mut ctx.accounts.mint_state;
        mint_state.max_redeem_per_block = new_max;
        Ok(())
    }

  
}

#[derive(Accounts)]
pub struct Asset<'info> {
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub authority: Signer<'info>,
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
    pub mint_state: Account<'info, MintState>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
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
    pub depositer: Signer<'info>,
    #[account(mut)]
    pub mint_state: Account<'info, MintState>,
    #[account(signer)]
    pub vault_authority: Signer<'info>,
    /// CHECK: the token to mint
    #[account(mut)]
    pub mint: Account<'info, Mint>,
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
    pub max_mint_per_block: u64,
    pub max_redeem_per_block: u64,
    pub minted_per_block: u64,
    pub redeemed_per_block: u64,
    pub exchange_rates: Vec<(Pubkey, u64, u64)>, 
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