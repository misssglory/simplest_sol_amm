use anchor_lang::prelude::*;
use anchor_spl::{
    token::{Token, TokenAccount, Mint, Transfer, transfer},
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
        
        // Transfer tokens from user to vaults
        let transfer_a_instruction = Transfer {
            from: ctx.accounts.user_token_a_account.to_account_info(),
            to: ctx.accounts.token_a_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_a = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_a_instruction,
        );
        transfer(cpi_ctx_a, amount_a)?;
        
        let transfer_b_instruction = Transfer {
            from: ctx.accounts.user_token_b_account.to_account_info(),
            to: ctx.accounts.token_b_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_b = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_b_instruction,
        );
        transfer(cpi_ctx_b, amount_b)?;
        
        // Mint LP tokens to user
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        let mint_lp_instruction = anchor_spl::token::MintTo {
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.user_lp_account.to_account_info(),
            authority: pool.to_account_info(),
        };
        
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            mint_lp_instruction,
            signer,
        );
        
        // Calculate LP tokens to mint based on first deposit
        let lp_to_mint = if ctx.accounts.lp_mint.supply == 0 {
            amount_a // Initial mint based on token A amount
        } else {
            // Calculate proportional LP tokens
            let total_lp_supply = ctx.accounts.lp_mint.supply;
            let pool_a_balance = ctx.accounts.token_a_vault.amount;
            let lp_amount = amount_a
                .checked_mul(total_lp_supply)
                .unwrap()
                .checked_div(pool_a_balance)
                .unwrap();
            lp_amount
        };
        
        anchor_spl::token::mint_to(cpi_ctx, lp_to_mint)?;
        
        Ok(())
    }

    // Swap tokens
    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64,
        minimum_amount_out: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Calculate output amount using constant product formula
        let input_reserves = if ctx.accounts.input_mint.key() == pool.token_a_mint {
            ctx.accounts.token_a_vault.amount
        } else {
            ctx.accounts.token_b_vault.amount
        };
        
        let output_reserves = if ctx.accounts.output_mint.key() == pool.token_a_mint {
            ctx.accounts.token_a_vault.amount
        } else {
            ctx.accounts.token_b_vault.amount
        };
        
        // Constant product formula: x * y = k
        let amount_in_with_fee = amount_in
            .checked_mul(10000u64.checked_sub(pool.fee_bps as u64).unwrap())
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
        
        require!(
            amount_out >= minimum_amount_out,
            AmmError::SlippageExceeded
        );
        
        // Transfer input tokens from user to vault
        let transfer_in_instruction = Transfer {
            from: ctx.accounts.user_input_account.to_account_info(),
            to: if ctx.accounts.input_mint.key() == pool.token_a_mint {
                ctx.accounts.token_a_vault.to_account_info()
            } else {
                ctx.accounts.token_b_vault.to_account_info()
            },
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_in = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_in_instruction,
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
        
        let transfer_out_instruction = Transfer {
            from: if ctx.accounts.output_mint.key() == pool.token_a_mint {
                ctx.accounts.token_a_vault.to_account_info()
            } else {
                ctx.accounts.token_b_vault.to_account_info()
            },
            to: ctx.accounts.user_output_account.to_account_info(),
            authority: pool.to_account_info(),
        };
        
        let cpi_ctx_out = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_out_instruction,
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
        
        // Burn LP tokens
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        let burn_instruction = anchor_spl::token::Burn {
            mint: ctx.accounts.lp_mint.to_account_info(),
            from: ctx.accounts.user_lp_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_burn = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            burn_instruction,
        );
        anchor_spl::token::burn(cpi_ctx_burn, lp_amount)?;
        
        // Calculate proportional share of pool
        let total_lp_supply = ctx.accounts.lp_mint.supply;
        let share = lp_amount as f64 / total_lp_supply as f64;
        
        let amount_a_out = (ctx.accounts.token_a_vault.amount as f64 * share) as u64;
        let amount_b_out = (ctx.accounts.token_b_vault.amount as f64 * share) as u64;
        
        // Transfer tokens from vaults to user
        let seeds = &[
            b"pool",
            pool.token_a_mint.as_ref(),
            pool.token_b_mint.as_ref(),
            &[pool.bump],
        ];
        let signer = &[&seeds[..]];
        
        // Transfer token A
        let transfer_a_instruction = Transfer {
            from: ctx.accounts.token_a_vault.to_account_info(),
            to: ctx.accounts.user_token_a_account.to_account_info(),
            authority: pool.to_account_info(),
        };
        
        let cpi_ctx_a = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_a_instruction,
            signer,
        );
        transfer(cpi_ctx_a, amount_a_out)?;
        
        // Transfer token B
        let transfer_b_instruction = Transfer {
            from: ctx.accounts.token_b_vault.to_account_info(),
            to: ctx.accounts.user_token_b_account.to_account_info(),
            authority: pool.to_account_info(),
        };
        
        let cpi_ctx_b = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_b_instruction,
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
        space = 8 + size_of::<Pool>(),
        seeds = [b"pool", token_a_mint.key().as_ref(), token_b_mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,
    
    pub token_a_mint: Account<'info, Mint>,
    pub token_b_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = admin,
        associated_token::mint = token_a_mint,
        associated_token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = admin,
        associated_token::mint = token_b_mint,
        associated_token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = admin,
        mint::decimals = 6,
        mint::authority = pool,
        mint::freeze_authority = pool,
    )]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub admin: Signer<'info>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
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
    
    #[account(mut)]
    pub token_a_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = user_token_a_account.mint == pool.token_a_mint,
        constraint = user_token_a_account.owner == user.key(),
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_token_b_account.mint == pool.token_b_mint,
        constraint = user_token_b_account.owner == user.key(),
    )]
    pub user_token_b_account: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = lp_mint,
        associated_token::authority = user,
    )]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump = pool.bump,
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(mut)]
    pub token_a_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub input_mint: Account<'info, Mint>,
    pub output_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        constraint = user_input_account.mint == input_mint.key(),
        constraint = user_input_account.owner == user.key(),
    )]
    pub user_input_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_output_account.mint == output_mint.key(),
        constraint = user_output_account.owner == user.key(),
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
    
    #[account(mut)]
    pub token_a_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        constraint = user_lp_account.mint == lp_mint.key(),
        constraint = user_lp_account.owner == user.key(),
    )]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_token_a_account.mint == pool.token_a_mint,
        constraint = user_token_a_account.owner == user.key(),
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_token_b_account.mint == pool.token_b_mint,
        constraint = user_token_b_account.owner == user.key(),
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
    pub fee_bps: u16, // basis points (0.01%)
    pub admin: Pubkey,
    pub bump: u8,
}

#[error_code]
pub enum AmmError {
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,
    #[msg("Invalid pool")]
    InvalidPool,
    #[msg("Insufficient liquidity")]
    InsufficientLiquidity,
}