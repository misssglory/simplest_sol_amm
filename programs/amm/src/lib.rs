use anchor_lang::prelude::*;
use anchor_spl::{
    token::{Token, TokenAccount, Mint, Transfer, transfer, MintTo, mint_to, Burn, burn},
    associated_token::AssociatedToken,
};

declare_id!("HGyfHondhiRc4GUpvzWRyNRbMuWv6vJUAoEtcLP9kVoY");

#[program]
pub mod amm {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        fee_bps: u16,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.token_a_mint = ctx.accounts.token_a_mint.key();
        pool.token_b_mint = ctx.accounts.token_b_mint.key();
        pool.token_a_vault = ctx.accounts.token_a_vault.key();
        pool.token_b_vault = ctx.accounts.token_b_vault.key();
        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.fee_bps = fee_bps;
        pool.admin = ctx.accounts.admin.key();
        pool.bump = ctx.bumps.pool;
        Ok(())
    }

    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
    ) -> Result<()> {
        // Transfer token A
        let transfer_a_accounts = Transfer {
            from: ctx.accounts.user_token_a_account.to_account_info(),
            to: ctx.accounts.token_a_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_a = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_a_accounts,
        );
        transfer(cpi_ctx_a, amount_a)?;
        
        // Transfer token B
        let transfer_b_accounts = Transfer {
            from: ctx.accounts.user_token_b_account.to_account_info(),
            to: ctx.accounts.token_b_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        let cpi_ctx_b = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_b_accounts,
        );
        transfer(cpi_ctx_b, amount_b)?;
        
        // Calculate LP tokens to mint
        let lp_to_mint = if ctx.accounts.lp_mint.supply == 0 {
            // First liquidity provider gets LP tokens equal to sqrt(amount_a * amount_b)
            let product = amount_a.checked_mul(amount_b).unwrap();
            (product as f64).sqrt() as u64
        } else {
            // Calculate based on share of pool
            let total_lp = ctx.accounts.lp_mint.supply;
            let pool_a_before = ctx.accounts.token_a_vault.amount - amount_a;
            
            amount_a
                .checked_mul(total_lp)
                .unwrap()
                .checked_div(pool_a_before)
                .unwrap()
        };
        
        // Mint LP tokens
        let seeds = &[
            b"pool",
            ctx.accounts.pool.token_a_mint.as_ref(),
            ctx.accounts.pool.token_b_mint.as_ref(),
            &[ctx.accounts.pool.bump],
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

    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Determine direction
        let is_a_to_b = ctx.accounts.input_mint.key() == pool.token_a_mint;
        
        let (input_reserves, output_reserves) = if is_a_to_b {
            (ctx.accounts.token_a_vault.amount, ctx.accounts.token_b_vault.amount)
        } else {
            (ctx.accounts.token_b_vault.amount, ctx.accounts.token_a_vault.amount)
        };
        
        // Calculate output with fee
        let fee_amount = amount_in
            .checked_mul(pool.fee_bps as u64)
            .unwrap()
            .checked_div(10000)
            .unwrap();
        
        let amount_in_after_fee = amount_in.checked_sub(fee_amount).unwrap();
        
        let numerator = amount_in_after_fee
            .checked_mul(output_reserves)
            .unwrap();
        let denominator = input_reserves
            .checked_add(amount_in_after_fee)
            .unwrap();
        
        let amount_out = numerator.checked_div(denominator).unwrap();
        
        require!(
            amount_out >= min_amount_out,
            AmmError::SlippageExceeded
        );
        
        // Transfer input
        let transfer_in_accounts = Transfer {
            from: ctx.accounts.user_input_account.to_account_info(),
            to: if is_a_to_b {
                ctx.accounts.token_a_vault.to_account_info()
            } else {
                ctx.accounts.token_b_vault.to_account_info()
            },
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
        
        let transfer_out_accounts = Transfer {
            from: if is_a_to_b {
                ctx.accounts.token_b_vault.to_account_info()
            } else {
                ctx.accounts.token_a_vault.to_account_info()
            },
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

    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
    ) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        // Calculate proportional amounts
        let total_lp = ctx.accounts.lp_mint.supply;
        
        let amount_a_out = (ctx.accounts.token_a_vault.amount as u128)
            .checked_mul(lp_amount as u128)
            .unwrap()
            .checked_div(total_lp as u128)
            .unwrap() as u64;
        
        let amount_b_out = (ctx.accounts.token_b_vault.amount as u128)
            .checked_mul(lp_amount as u128)
            .unwrap()
            .checked_div(total_lp as u128)
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
        // space = 8 + std::mem::size_of::<Pool>(),
        space = 8 + Pool::INIT_SPACE,
        seeds = [b"pool", token_a_mint.key().as_ref(), token_b_mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,
    
    pub token_a_mint: Account<'info, Mint>,
    pub token_b_mint: Account<'info, Mint>,
    
    #[account(
        init_if_needed,
        payer = admin,
        token::mint = token_a_mint,
        token::authority = pool,
    )]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = admin,
        token::mint = token_b_mint,
        token::authority = pool,
    )]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = admin,
        mint::decimals = 9,
        mint::authority = pool,
        mint::freeze_authority = pool,
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
        has_one = token_a_mint,
        has_one = token_b_mint,
        has_one = token_a_vault,
        has_one = token_b_vault,
        has_one = lp_mint,
    )]
    pub pool: Account<'info, Pool>,
    
    #[account(mut)]
    pub token_a_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub token_b_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub lp_mint: Account<'info, Mint>,
    
    pub token_a_mint: Account<'info, Mint>,
    pub token_b_mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    #[account(
        mut,
        token::mint = token_a_mint,
        token::authority = user,
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        token::mint = token_b_mint,
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
        has_one = token_a_vault,
        has_one = token_b_vault,
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
    )]
    pub user_input_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_output_account.mint == output_mint.key(),
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
        has_one = token_a_vault,
        has_one = token_b_vault,
        has_one = lp_mint,
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
    )]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_token_a_account.mint == pool.token_a_mint,
    )]
    pub user_token_a_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = user_token_b_account.mint == pool.token_b_mint,
    )]
    pub user_token_b_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[account]
#[derive(Default, InitSpace)]
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