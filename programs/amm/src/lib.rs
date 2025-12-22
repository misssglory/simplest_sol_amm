use anchor_lang::prelude::*;
use anchor_spl::{
    token::{Token, Mint, TokenAccount, Transfer, transfer, MintTo, mint_to, Burn, burn},
    associated_token::AssociatedToken,
};
use std::mem::size_of;

declare_id!("6wypgzPHKssHj8tVrofH8FzDq9fib2QLWsAkMnDFsSCv");

#[program]
pub mod amm {
    use super::*;

    // Initialize AMM pool
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_fee_bps: u16,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.token_a_mint = ctx.accounts.token_a_mint.key();
        pool.token_b_mint = ctx.accounts.token_b_mint.key();
        pool.token_a_vault = ctx.accounts.token_a_vault.key();
        pool.token_b_vault = ctx.accounts.token_b_vault.key();
        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.fee_bps = pool_fee_bps;
        pool.admin = ctx.accounts.admin.key();
        pool.bump = ctx.bumps.pool;
        Ok(())
    }

    // Add liquidity to the pool
    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Transfer token A from user to vault
        let cpi_accounts_a = Transfer {
            from: ctx.accounts.user_token_a_account.to_account_info(),
            to: ctx.accounts.token_a_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx_a = CpiContext::new(cpi_program.clone(), cpi_accounts_a);
        transfer(cpi_ctx_a, amount_a)?;
        
        // Transfer token B from user to vault
        let cpi_accounts_b = Transfer {
            from: ctx.accounts.user_token_b_account.to_account_info(),
            to: ctx.accounts.token_b_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_b = CpiContext::new(cpi_program.clone(), cpi_accounts_b);
        transfer(cpi_ctx_b, amount_b)?;
        
        // Calculate LP tokens to mint
        let lp_to_mint = if ctx.accounts.lp_mint.supply == 0 {
            // First liquidity provider gets LP tokens equal to sqrt(amount_a * amount_b)
            let product = amount_a.checked_mul(amount_b).unwrap();
            (product as f64).sqrt() as u64
        } else {
            // Calculate proportional LP tokens based on token A contribution
            let total_lp_supply = ctx.accounts.lp_mint.supply;
            let pool_a_balance = ctx.accounts.token_a_vault.amount.checked_sub(amount_a).unwrap();
            amount_a
                .checked_mul(total_lp_supply)
                .unwrap()
                .checked_div(pool_a_balance)
                .unwrap()
        };
        
        // Mint LP tokens to user
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        let mint_accounts = MintTo {
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.user_lp_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            mint_accounts,
            signer,
        );
        mint_to(cpi_ctx, lp_to_mint)?;
        
        Ok(())
    }

    // Swap tokens
    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64,
        minimum_amount_out: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Determine swap direction
        let is_a_to_b = ctx.accounts.input_mint.key() == pool.token_a_mint;
        
        let (input_reserves, output_reserves) = if is_a_to_b {
            (ctx.accounts.token_a_vault.amount, ctx.accounts.token_b_vault.amount)
        } else {
            (ctx.accounts.token_b_vault.amount, ctx.accounts.token_a_vault.amount)
        };
        
        // Calculate output amount using constant product formula (x * y = k)
        let amount_in_with_fee = amount_in
            .checked_mul(10000u64.saturating_sub(pool.fee_bps as u64))
            .unwrap()
            .checked_div(10000u64)
            .unwrap();
        
        let numerator = amount_in_with_fee
            .checked_mul(output_reserves)
            .unwrap();
        let denominator = input_reserves
            .checked_add(amount_in_with_fee)
            .unwrap();
        
        let amount_out = numerator
            .checked_div(denominator)
            .unwrap();
        
        // Check slippage
        require!(
            amount_out >= minimum_amount_out,
            AmmError::SlippageExceeded
        );
        
        // Transfer input tokens from user to vault
        let input_vault = if is_a_to_b {
            ctx.accounts.token_a_vault.to_account_info()
        } else {
            ctx.accounts.token_b_vault.to_account_info()
        };
        
        let transfer_in_accounts = Transfer {
            from: ctx.accounts.user_input_account.to_account_info(),
            to: input_vault,
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_in = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_in_accounts,
        );
        transfer(cpi_ctx_in, amount_in)?;
        
        // Transfer output tokens from vault to user
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        let output_vault = if is_a_to_b {
            ctx.accounts.token_b_vault.to_account_info()
        } else {
            ctx.accounts.token_a_vault.to_account_info()
        };
        
        let transfer_out_accounts = Transfer {
            from: output_vault,
            to: ctx.accounts.user_output_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        
        let cpi_ctx_out = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_out_accounts,
            signer,
        );
        transfer(cpi_ctx_out, amount_out)?;
        
        Ok(())
    }

    // Remove liquidity
    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Calculate proportional share
        let total_lp_supply = ctx.accounts.lp_mint.supply;
        require!(total_lp_supply > 0, AmmError::InsufficientLiquidity);
        
        let share_numerator = lp_amount as u128;
        let share_denominator = total_lp_supply as u128;
        
        let amount_a_out = (ctx.accounts.token_a_vault.amount as u128)
            .checked_mul(share_numerator)
            .unwrap()
            .checked_div(share_denominator)
            .unwrap() as u64;
        
        let amount_b_out = (ctx.accounts.token_b_vault.amount as u128)
            .checked_mul(share_numerator)
            .unwrap()
            .checked_div(share_denominator)
            .unwrap() as u64;
        
        // Burn LP tokens
        let burn_accounts = Burn {
            mint: ctx.accounts.lp_mint.to_account_info(),
            from: ctx.accounts.user_lp_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_burn = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            burn_accounts,
        );
        burn(cpi_ctx_burn, lp_amount)?;
        
        // Transfer tokens from vaults to user
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        // Transfer token A
        let transfer_a_accounts = Transfer {
            from: ctx.accounts.token_a_vault.to_account_info(),
            to: ctx.accounts.user_token_a_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        
        let cpi_ctx_a = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_a_accounts,
            signer,
        );
        transfer(cpi_ctx_a, amount_a_out)?;
        
        // Transfer token B
        let transfer_b_accounts = Transfer {
            from: ctx.accounts.token_b_vault.to_account_info(),
            to: ctx.accounts.user_token_b_account.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        
        let cpi_ctx_b = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_b_accounts,
            signer,
        );
        transfer(cpi_ctx_b, amount_b_out)?;
        
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + Pool::LEN,
        seeds = [b"pool", token_a_mint.key().as_ref(), token_b_mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,
    
    pub token_a_mint: Account<'info, Mint>,
    pub token_b_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = admin,
        token::mint = token_a_mint,
        token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = admin,
        token::mint = token_b_mint,
        token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = admin,
        mint::decimals = 9,
        mint::authority = pool,
    )]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        mut,
        token::mint = pool.token_a_mint,
        token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_b_mint,
        token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        mint::authority = pool,
    )]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        token::mint = pool.token_a_mint,
        token::authority = user,
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_b_mint,
        token::authority = user,
    )]
    pub user_token_b_account: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = user,
        token::mint = lp_mint,
        token::authority = user,
    )]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        mut,
        token::mint = pool.token_a_mint,
        token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_b_mint,
        token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub input_mint: Account<'info, Mint>,
    pub output_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        token::mint = input_mint,
        token::authority = user,
    )]
    pub user_input_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = output_mint,
        token::authority = user,
    )]
    pub user_output_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(
        mut,
        token::mint = pool.token_a_mint,
        token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_b_mint,
        token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        mint::authority = pool,
    )]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        token::mint = lp_mint,
        token::authority = user,
    )]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_a_mint,
        token::authority = user,
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = pool.token_b_mint,
        token::authority = user,
    )]
    pub user_token_b_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Pool {
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub fee_bps: u16,
    pub admin: Pubkey,
    pub bump: u8,
}

impl Pool {
    pub const LEN: usize = 32 * 4 + 2 + 32 + 1; // 4 Pubkeys + u16 + Pubkey + u8
}

#[error_code]
pub enum AmmError {
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,
    #[msg("Invalid pool")]
    InvalidPool,
    #[msg("Insufficient liquidity")]
    InsufficientLiquidity,
    #[msg("Invalid token pair")]
    InvalidTokenPair,
    #[msg("Math overflow")]
    MathOverflow,
}