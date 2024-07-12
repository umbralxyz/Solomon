use anchor_lang::prelude::*;
use anchor_spl::{metadata::Metadata, token::{Burn, Mint, MintTo, Token, TokenAccount, Transfer}};
use super::*;

#[derive(Accounts)]
#[instruction(admin: Pubkey, salt: [u8; 8], cooldown: u32)]
pub struct InitializeVaultState<'info> {
    /// The vault state for this deposit token and admin
    #[account(
        init, 
        payer = caller, 
        space = 8 + (3 * 32) + (3 * 8) + (3 * 4) + 1 + (32 * 20), 
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Box<Account<'info, VaultState>>,

    #[account(mut)]
    pub deposit_token: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8])]
pub struct InitializeProgramAccounts<'info> {
    /// The vault state for this deposit token and admin
    #[account(
        seeds = [VAULT_STATE_SEED, salt.as_ref()],
        bump,
        has_one = deposit_token @ StakeError::BadDepositToken
    )]
    pub vault_state: Box<Account<'info, VaultState>>,

    #[account(
        init,
        payer = caller,
        seeds = [STAKING_TOKEN_SEED, vault_state.key().as_ref()],
        mint::decimals = deposit_token.decimals,
        mint::authority = vault_state,
        bump
    )]
    pub staking_token: Box<Account<'info, Mint>>,

    /// The deposit token account for this vault and admin
    #[account(
        init, 
        payer = caller, 
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.key().as_ref()],
        token::mint = deposit_token,
        token::authority = vault_state,
        bump
    )]
    pub vault_token_account: Box<Account<'info, TokenAccount>>,
    /// CHECK: New Metaplex Account creation
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), staking_token.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    pub metadata: UncheckedAccount<'info>,
    #[account(mut)]
    pub deposit_token: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub caller: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub token_metadata_program: Program<'info, Metadata>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8], amt: u64)]
pub struct Stake<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        mut,
        seeds = [STAKING_TOKEN_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub staking_token: Account<'info, Mint>,
    /// The user deposit token account, we're going to transfer from this
    #[account(
        mut,
        token::mint = vault_state.deposit_token,
        token::authority = user,
    )]
    pub user_deposit_token_account: Account<'info, TokenAccount>,
    /// The users staking token account, we're going to mint to this
    #[account(
        mut,
        token::mint = staking_token,
        token::authority = user,
    )]
    pub user_staking_token_account: Account<'info, TokenAccount>,
    /// The vault's account for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        init_if_needed, 
        payer = user,
        space = 8 + 32 + 1, 
        seeds = [VAULT_STATE_SEED, salt.as_ref(), user.key().as_ref()], 
        bump
    )]
    pub blacklisted: Account<'info, Blacklisted>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

impl<'info> Stake<'info> {
    pub fn transfer_from_user_to_vault(
        &self,
        amount: u64,
    ) -> Result<()> {
        let cpi_accounts = Transfer {
            from: self.user_deposit_token_account.to_account_info(),
            to: self.vault_token_account.to_account_info(),
            authority: self.user.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();
        let cpi = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi, amount)
    }

    pub fn mint_tokens_to_user(
        &self,
        salt: &[u8; 8],
        amount: u64,
    ) -> Result<()> {
        // Mint tokens to depositer
        let cpi_accounts = MintTo {
            mint: self.staking_token.to_account_info(),
            to: self.user_staking_token_account.to_account_info(),
            authority: self.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, salt, &[self.vault_state.bump]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            self.token_program.to_account_info(),
            cpi_accounts,
            seeds,
        );

        token::mint_to(cpi_ctx, amount)
    }

    pub fn check_min_shares(&self) -> Result<()> {
        let shares = self.staking_token.supply;
        if self.user.key() != self.vault_state.admin && shares > 0 && shares < self.vault_state.min_shares {
            return Err(StakeError::MinSharesViolation.into())
        }

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8])]
pub struct Unstake<'info> {
    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [STAKING_TOKEN_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub staking_token: Account<'info, Mint>,

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

    /// The vault's token account for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(
        init_if_needed, 
        payer = user,
        space = 8 + 32 + 1, 
        seeds = [VAULT_STATE_SEED, salt.as_ref(), user.key().as_ref()], 
        bump
    )]
    pub blacklisted: Account<'info, Blacklisted>,

    #[account(
        init_if_needed, 
        payer = user, 
        space = 8 + 8 + 100 * 12, 
        seeds = [USER_DATA_SEED, user.key().as_ref(), vault_state.key().as_ref()], 
        bump
    )]
    pub user_data: Account<'info, UserPDA>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> Unstake<'info> {
    pub fn transfer_from_vault_to_user(
        &self,
        salt: &[u8; 8],
        amount: u64,
    ) -> Result<()> {
         let accounts = Transfer {
            from: self.vault_token_account.to_account_info(),
            to: self.user_deposit_token_account.to_account_info(),
            authority: self.vault_state.to_account_info(),
        };

        let seeds: &[&[u8]] = &[VAULT_STATE_SEED, salt.as_ref(), &[self.vault_state.bump]];
        let seeds = &[seeds][..];
        let cpi_ctx = CpiContext::new_with_signer(
            self.token_program.to_account_info(),
            accounts,
            seeds,
        );

        token::transfer(cpi_ctx, amount)
    }

    pub fn burn_tokens_from_user(
        &self,
        amount: u64,
    ) -> Result<()> {
        let burn_instruction = Burn {
            mint: self.staking_token.to_account_info(),
            from: self.user_staking_token_account.to_account_info(),
            authority: self.user.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            self.token_program.to_account_info(),
            burn_instruction,
        );

        token::burn(cpi_ctx, amount)
    }

    pub fn check_min_shares(&self) -> Result<()> {
        let shares = self.staking_token.supply;
        if self.user.key() != self.vault_state.admin && shares > 0 && shares < self.vault_state.min_shares {
            return Err(StakeError::MinSharesViolation.into())
        }

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(amt: u64, salt: [u8; 8])]
pub struct Reward<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
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

    /// The vault's token account for the deposit token
    #[account(
        mut,
        seeds = [VAULT_TOKEN_ACCOUNT_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8], duration: u32)]
pub struct SetCooldown<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8], duration: u32)]
pub struct SetVestingPeriod<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8], user: Pubkey)]
pub struct Blacklist<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        init_if_needed, 
        payer = caller,
        space = 8 + 32 + 1, 
        seeds = [VAULT_STATE_SEED, salt.as_ref(), user.as_ref()], 
        bump
    )]
    pub blacklisted: Account<'info, Blacklisted>,
    #[account(mut)]
    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(rewarder: Pubkey, salt: [u8; 8])]
pub struct Rewarders<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(new_admin: Pubkey, salt: [u8; 8])]
pub struct TransferAdmin<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(salt: [u8; 8])]
pub struct CheckAssets<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED, salt.as_ref()], 
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [USER_DATA_SEED, user.key().as_ref(), vault_state.key().as_ref()], 
        bump
    )]
    pub user_data: Account<'info, UserPDA>,

    #[account(mut)]
    pub user: Signer<'info>,
}
