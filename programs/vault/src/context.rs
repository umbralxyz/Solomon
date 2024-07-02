use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use super::*;

#[derive(Accounts)]
#[instruction(admin: Pubkey)]
pub struct InitializeVaultState<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub token_metadata_program: Program<'info, Metadata>,

    // todo: space
    #[account(
        init, 
        payer = signer, 
        space = 8 + 8 + (2 + 20 + 20 + 50) * 32,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init, 
        payer = signer, 
        mint::decimals = 9, 
        mint::authority = vault_state,
        seeds = [MINT_SEED], 
        bump
    )]
    pub vault_token: Account<'info, Mint>,

    /// CHECK: New Metaplex Account creation
    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), vault_token.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    pub metadata: UncheckedAccount<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(asset: Pubkey)]
pub struct UpdateAsset<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    #[account(
        init_if_needed, 
        payer = authority, 
        seeds = [EXCHANGE_RATE_SEED, asset.as_ref()],
        space = 8 + 8 + 8 + 32,
        bump
    )]
    pub exchange_rate: Account<'info, ExchangeRate>,

    /// The program owned collateral
    #[account(
        init_if_needed,
        seeds = [TOKEN_ATA_SEED, asset.as_ref()],
        bump,
        payer = authority,
        token::mint = collateral_token_mint,
        token::authority = vault_state,
    )]
    pub program_collateral: Account<'info, TokenAccount>,

    #[account(mut)]
    pub collateral_token_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
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
        seeds = [TOKEN_ATA_SEED, collateral_token_mint.key().as_ref()],
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
        mut,
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
        seeds = [VAULT_STATE_SEED, minter.key().as_ref()],
        bump
    )]
    pub user_permissions: Account<'info, Permissions>,
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
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
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
        seeds = [TOKEN_ATA_SEED, collateral_token_mint.key().as_ref()],
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
        seeds = [VAULT_STATE_SEED, redeemer.key().as_ref()],
        bump
    )]
    pub user_permissions: Account<'info, Permissions>,
    #[account(
        mut,
        constraint = vault_token_mint.key() == vault_state.vault_token_mint,
        seeds = [MINT_SEED],
        bump,
    )]
    pub vault_token_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump,
    )]
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
        seeds = [TOKEN_ATA_SEED, collat_mint.key().as_ref()],
        bump
    )]
    pub program_collat: Account<'info, TokenAccount>,

    #[account(mut)]
    pub destination: Account<'info, TokenAccount>,
    #[account(mut)]
    pub collat_mint: Account<'info, Mint>,
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct UserPermissions<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        init_if_needed, 
        space = 8 + 32 + 1 + 1,
        payer = caller,
        seeds = [VAULT_STATE_SEED, user.as_ref()],
        bump
    )]
    pub user_permissions: Account<'info, Permissions>,
    #[account(mut)]
    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawAddresses<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct Managers<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    #[account(
        mut,
        seeds = [VAULT_STATE_SEED],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut)]
    pub caller: Signer<'info>,
}