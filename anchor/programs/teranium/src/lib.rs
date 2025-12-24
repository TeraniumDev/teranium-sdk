use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer},
};
use pyth_sdk_solana::load_price_feed_from_account_info;

declare_id!("Dx9ZBP9kFYjvZX6sY6bHKgyD3BQtTmnhU6apDpMUAMWV");

const BPS_DENOM: i128 = 10_000;
const MAX_STALENESS_SECONDS: i64 = 60;

const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

#[program]
pub mod teranium {
    use super::*;

    pub fn initialize_vault(ctx: Context<InitializeVault>, mint: Pubkey) -> Result<()> {
        require_keys_eq!(ctx.accounts.mint.key(), mint, TeraniumError::MintMismatch);

        let vault = &mut ctx.accounts.vault;
        vault.mint = mint;
        vault.bump = ctx.bumps.vault;
        vault.authority_bump = ctx.bumps.vault_authority;
        vault.total_deposits = 0;

        emit!(VaultInitialized {
            vault: vault.key(),
            mint,
            vault_bump: vault.bump,
            authority_bump: vault.authority_bump,
        });

        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(amount > 0, TeraniumError::InvalidAmount);

        let vault = &mut ctx.accounts.vault;
        require_keys_eq!(ctx.accounts.user_token_account.mint, vault.mint, TeraniumError::MintMismatch);
        require_keys_eq!(ctx.accounts.vault_token_account.mint, vault.mint, TeraniumError::MintMismatch);

        if ctx.accounts.user_position.owner == Pubkey::default() {
            ctx.accounts.user_position.owner = ctx.accounts.owner.key();
            ctx.accounts.user_position.vault = vault.key();
            ctx.accounts.user_position.deposited = 0;
        }

        require_keys_eq!(ctx.accounts.user_position.owner, ctx.accounts.owner.key(), TeraniumError::Unauthorized);
        require_keys_eq!(ctx.accounts.user_position.vault, vault.key(), TeraniumError::InvalidUserPosition);

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.vault_token_account.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                },
            ),
            amount,
        )?;

        ctx.accounts.user_position.deposited = ctx
            .accounts
            .user_position
            .deposited
            .checked_add(amount)
            .ok_or(TeraniumError::MathOverflow)?;

        vault.total_deposits = vault
            .total_deposits
            .checked_add(amount)
            .ok_or(TeraniumError::MathOverflow)?;

        emit!(Deposited {
            owner: ctx.accounts.owner.key(),
            vault: vault.key(),
            amount,
            deposited_after: ctx.accounts.user_position.deposited,
            total_deposits_after: vault.total_deposits,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, TeraniumError::InvalidAmount);

        let vault = &mut ctx.accounts.vault;
        require_keys_eq!(ctx.accounts.user_token_account.mint, vault.mint, TeraniumError::MintMismatch);
        require_keys_eq!(ctx.accounts.vault_token_account.mint, vault.mint, TeraniumError::MintMismatch);

        require_keys_eq!(ctx.accounts.user_position.owner, ctx.accounts.owner.key(), TeraniumError::Unauthorized);
        require_keys_eq!(ctx.accounts.user_position.vault, vault.key(), TeraniumError::InvalidUserPosition);

        require!(amount <= ctx.accounts.user_position.deposited, TeraniumError::InsufficientDepositedBalance);

        let authority_seeds: &[&[u8]] = &[
            b"vault_authority",
            vault.key().as_ref(),
            &[vault.authority_bump],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_token_account.to_account_info(),
                    to: ctx.accounts.user_token_account.to_account_info(),
                    authority: ctx.accounts.vault_authority.to_account_info(),
                },
                &[authority_seeds],
            ),
            amount,
        )?;

        ctx.accounts.user_position.deposited = ctx
            .accounts
            .user_position
            .deposited
            .checked_sub(amount)
            .ok_or(TeraniumError::MathOverflow)?;

        vault.total_deposits = vault
            .total_deposits
            .checked_sub(amount)
            .ok_or(TeraniumError::MathOverflow)?;

        emit!(Withdrawn {
            owner: ctx.accounts.owner.key(),
            vault: vault.key(),
            amount,
            deposited_after: ctx.accounts.user_position.deposited,
            total_deposits_after: vault.total_deposits,
        });

        Ok(())
    }

    /// Oracle-priced swap between a base mint vault and the USDC vault.
    ///
    /// - Uses a Pyth price feed (legacy price account) for base mint USD price.
    /// - Uses oracle confidence interval as a deterministic slippage bound.
    /// - Enforces staleness using publish_time.
    /// - Ensures vaults remain solvent against `total_deposits` after swap.
    pub fn oracle_swap(ctx: Context<OracleSwap>, amount: u64, max_slippage_bps: u16) -> Result<()> {
        require!(amount > 0, TeraniumError::InvalidAmount);
        require!(max_slippage_bps as i128 <= BPS_DENOM, TeraniumError::InvalidSlippageBps);

        require_keys_eq!(ctx.accounts.usdc_mint.key(), USDC_MINT, TeraniumError::InvalidUsdcMint);
        require_keys_eq!(ctx.accounts.usdc_vault.mint, ctx.accounts.usdc_mint.key(), TeraniumError::MintMismatch);
        require_keys_eq!(ctx.accounts.base_vault.mint, ctx.accounts.base_mint.key(), TeraniumError::MintMismatch);
        require!(ctx.accounts.base_vault.mint != ctx.accounts.usdc_mint.key(), TeraniumError::InvalidSwapPair);

        require_keys_eq!(ctx.accounts.base_vault_token_account.mint, ctx.accounts.base_vault.mint, TeraniumError::MintMismatch);
        require_keys_eq!(ctx.accounts.usdc_vault_token_account.mint, ctx.accounts.usdc_vault.mint, TeraniumError::MintMismatch);

        // Determine direction from token account mints.
        let from_mint = ctx.accounts.user_from_token_account.mint;
        let to_mint = ctx.accounts.user_to_token_account.mint;

        let base_mint = ctx.accounts.base_mint.key();
        let usdc_mint = ctx.accounts.usdc_mint.key();

        let base_decimals = ctx.accounts.base_mint.decimals as u32;
        let usdc_decimals = ctx.accounts.usdc_mint.decimals as u32;

        require!(
            (from_mint == base_mint && to_mint == usdc_mint) || (from_mint == usdc_mint && to_mint == base_mint),
            TeraniumError::InvalidSwapPair
        );

        // Load oracle (Pyth legacy price feed).
        let price_feed = load_price_feed_from_account_info(&ctx.accounts.pyth_price_account)?;
        let price = price_feed
            .get_current_price()
            .ok_or(TeraniumError::OracleNoPrice)?;

        // Staleness enforcement.
        let now = Clock::get()?.unix_timestamp;
        let age = now
            .checked_sub(price.publish_time)
            .ok_or(TeraniumError::OracleStale)?;
        require!(age <= MAX_STALENESS_SECONDS, TeraniumError::OracleStale);

        // Confidence-based slippage bound (conf/|price| <= max_slippage_bps).
        let px_i128: i128 = price.price as i128;
        require!(px_i128 != 0, TeraniumError::OracleInvalidPrice);
        require!(px_i128 > 0, TeraniumError::OracleInvalidPrice);

        let abs_px: i128 = px_i128;
        let conf_i128: i128 = price.conf as i128;
        let max_bps: i128 = max_slippage_bps as i128;

        require!(conf_i128 >= 0, TeraniumError::OracleInvalidConfidence);

        // conf * 10_000 <= price * max_slippage_bps
        require!(
            conf_i128
                .checked_mul(BPS_DENOM)
                .ok_or(TeraniumError::MathOverflow)?
                <= abs_px
                    .checked_mul(max_bps)
                    .ok_or(TeraniumError::MathOverflow)?,
            TeraniumError::OracleSlippageExceeded
        );

        let amount_u128: u128 = amount as u128;
        let expo: i32 = price.expo;

        let (amount_out, direction) = if from_mint == base_mint {
            // base -> usdc
            let usdc_out = base_to_usdc(amount_u128, abs_px as u128, expo, base_decimals, usdc_decimals)?;
            require!(usdc_out > 0, TeraniumError::SwapZeroOut);

            // Ensure USDC vault remains solvent against deposits after paying out.
            let post = ctx
                .accounts
                .usdc_vault_token_account
                .amount
                .checked_sub(usdc_out as u64)
                .ok_or(TeraniumError::InsufficientVaultLiquidity)?;
            require!(post >= ctx.accounts.usdc_vault.total_deposits, TeraniumError::InsufficientVaultLiquidity);

            // User pays base into base vault.
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.user_from_token_account.to_account_info(),
                        to: ctx.accounts.base_vault_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                amount,
            )?;

            // Vault pays USDC to user.
            let usdc_auth_seeds: &[&[u8]] = &[
                b"vault_authority",
                ctx.accounts.usdc_vault.key().as_ref(),
                &[ctx.accounts.usdc_vault.authority_bump],
            ];

            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.usdc_vault_token_account.to_account_info(),
                        to: ctx.accounts.user_to_token_account.to_account_info(),
                        authority: ctx.accounts.usdc_vault_authority.to_account_info(),
                    },
                    &[usdc_auth_seeds],
                ),
                usdc_out as u64,
            )?;

            (usdc_out as u64, SwapDirection::BaseToUsdc)
        } else {
            // usdc -> base
            let base_out = usdc_to_base(amount_u128, abs_px as u128, expo, base_decimals, usdc_decimals)?;
            require!(base_out > 0, TeraniumError::SwapZeroOut);

            // Ensure base vault remains solvent against deposits after paying out.
            let post = ctx
                .accounts
                .base_vault_token_account
                .amount
                .checked_sub(base_out as u64)
                .ok_or(TeraniumError::InsufficientVaultLiquidity)?;
            require!(post >= ctx.accounts.base_vault.total_deposits, TeraniumError::InsufficientVaultLiquidity);

            // User pays USDC into USDC vault.
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.user_from_token_account.to_account_info(),
                        to: ctx.accounts.usdc_vault_token_account.to_account_info(),
                        authority: ctx.accounts.user.to_account_info(),
                    },
                ),
                amount,
            )?;

            // Vault pays base to user.
            let base_auth_seeds: &[&[u8]] = &[
                b"vault_authority",
                ctx.accounts.base_vault.key().as_ref(),
                &[ctx.accounts.base_vault.authority_bump],
            ];

            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.base_vault_token_account.to_account_info(),
                        to: ctx.accounts.user_to_token_account.to_account_info(),
                        authority: ctx.accounts.base_vault_authority.to_account_info(),
                    },
                    &[base_auth_seeds],
                ),
                base_out as u64,
            )?;

            (base_out as u64, SwapDirection::UsdcToBase)
        };

        emit!(OracleSwapped {
            user: ctx.accounts.user.key(),
            base_vault: ctx.accounts.base_vault.key(),
            usdc_vault: ctx.accounts.usdc_vault.key(),
            from_mint,
            to_mint,
            amount_in: amount,
            amount_out,
            oracle_price: price.price,
            oracle_conf: price.conf,
            oracle_expo: price.expo,
            direction: direction as u8,
        });

        Ok(())
    }
}

fn pow10_u128(exp: u32) -> Result<u128> {
    // Bound to keep computation safe and deterministic.
    require!(exp <= 38, TeraniumError::MathOverflow);
    let mut v: u128 = 1;
    for _ in 0..exp {
        v = v.checked_mul(10).ok_or(TeraniumError::MathOverflow)?;
    }
    Ok(v)
}

fn base_to_usdc(amount_base: u128, price: u128, expo: i32, base_decimals: u32, usdc_decimals: u32) -> Result<u128> {
    // usdc_out = amount_base * price * 10^{usdc_decimals} * 10^{max(expo,0)} / (10^{base_decimals} * 10^{max(-expo,0)})
    let expo_pos: u32 = if expo > 0 { expo as u32 } else { 0 };
    let expo_neg: u32 = if expo < 0 { (-expo) as u32 } else { 0 };

    let num = amount_base
        .checked_mul(price)
        .ok_or(TeraniumError::MathOverflow)?
        .checked_mul(pow10_u128(usdc_decimals)?)
        .ok_or(TeraniumError::MathOverflow)?
        .checked_mul(pow10_u128(expo_pos)?)
        .ok_or(TeraniumError::MathOverflow)?;

    let denom = pow10_u128(base_decimals)?
        .checked_mul(pow10_u128(expo_neg)?)
        .ok_or(TeraniumError::MathOverflow)?;

    Ok(num.checked_div(denom).ok_or(TeraniumError::SwapZeroOut)?)
}

fn usdc_to_base(amount_usdc: u128, price: u128, expo: i32, base_decimals: u32, usdc_decimals: u32) -> Result<u128> {
    // base_out = amount_usdc * 10^{base_decimals} * 10^{max(-expo,0)} / (price * 10^{usdc_decimals} * 10^{max(expo,0)})
    let expo_pos: u32 = if expo > 0 { expo as u32 } else { 0 };
    let expo_neg: u32 = if expo < 0 { (-expo) as u32 } else { 0 };

    let num = amount_usdc
        .checked_mul(pow10_u128(base_decimals)?)
        .ok_or(TeraniumError::MathOverflow)?
        .checked_mul(pow10_u128(expo_neg)?)
        .ok_or(TeraniumError::MathOverflow)?;

    let denom = price
        .checked_mul(pow10_u128(usdc_decimals)?)
        .ok_or(TeraniumError::MathOverflow)?
        .checked_mul(pow10_u128(expo_pos)?)
        .ok_or(TeraniumError::MathOverflow)?;

    Ok(num.checked_div(denom).ok_or(TeraniumError::SwapZeroOut)?)
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct InitializeVault<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = 8 + VaultAccount::INIT_SPACE,
        seeds = [b"vault", mint.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, VaultAccount>,

    /// CHECK: PDA signer only; validated by seeds.
    #[account(
        seeds = [b"vault_authority", vault.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = vault_authority
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault.mint.as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, VaultAccount>,

    /// CHECK: PDA signer only; validated by seeds.
    #[account(
        seeds = [b"vault_authority", vault.key().as_ref()],
        bump = vault.authority_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + UserPosition::INIT_SPACE,
        seeds = [b"user_position", vault.key().as_ref(), owner.key().as_ref()],
        bump
    )]
    pub user_position: Account<'info, UserPosition>,

    #[account(
        mut,
        constraint = user_token_account.owner == owner.key() @ TeraniumError::Unauthorized
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = vault_token_account.owner == vault_authority.key() @ TeraniumError::Unauthorized
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault.mint.as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, VaultAccount>,

    /// CHECK: PDA signer only; validated by seeds.
    #[account(
        seeds = [b"vault_authority", vault.key().as_ref()],
        bump = vault.authority_bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"user_position", vault.key().as_ref(), owner.key().as_ref()],
        bump,
        constraint = user_position.owner == owner.key() @ TeraniumError::Unauthorized,
        constraint = user_position.vault == vault.key() @ TeraniumError::InvalidUserPosition
    )]
    pub user_position: Account<'info, UserPosition>,

    #[account(
        mut,
        constraint = user_token_account.owner == owner.key() @ TeraniumError::Unauthorized
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = vault_token_account.owner == vault_authority.key() @ TeraniumError::Unauthorized
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct OracleSwap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", base_vault.mint.as_ref()],
        bump = base_vault.bump
    )]
    pub base_vault: Account<'info, VaultAccount>,

    /// CHECK: PDA signer only; validated by seeds.
    #[account(
        seeds = [b"vault_authority", base_vault.key().as_ref()],
        bump = base_vault.authority_bump
    )]
    pub base_vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = base_vault_token_account.owner == base_vault_authority.key() @ TeraniumError::Unauthorized
    )]
    pub base_vault_token_account: Account<'info, TokenAccount>,

    pub base_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [b"vault", usdc_vault.mint.as_ref()],
        bump = usdc_vault.bump
    )]
    pub usdc_vault: Account<'info, VaultAccount>,

    /// CHECK: PDA signer only; validated by seeds.
    #[account(
        seeds = [b"vault_authority", usdc_vault.key().as_ref()],
        bump = usdc_vault.authority_bump
    )]
    pub usdc_vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = usdc_vault_token_account.owner == usdc_vault_authority.key() @ TeraniumError::Unauthorized
    )]
    pub usdc_vault_token_account: Account<'info, TokenAccount>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = user_from_token_account.owner == user.key() @ TeraniumError::Unauthorized
    )]
    pub user_from_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_to_token_account.owner == user.key() @ TeraniumError::Unauthorized
    )]
    pub user_to_token_account: Account<'info, TokenAccount>,

    /// CHECK: validated by parsing Pyth price feed header.
    pub pyth_price_account: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct VaultAccount {
    pub mint: Pubkey,
    pub bump: u8,
    pub authority_bump: u8,
    pub total_deposits: u64,
}

impl Space for VaultAccount {
    const INIT_SPACE: usize = 32 + 1 + 1 + 8;
}

#[account]
pub struct UserPosition {
    pub owner: Pubkey,
    pub vault: Pubkey,
    pub deposited: u64,
}

impl Space for UserPosition {
    const INIT_SPACE: usize = 32 + 32 + 8;
}

#[event]
pub struct VaultInitialized {
    pub vault: Pubkey,
    pub mint: Pubkey,
    pub vault_bump: u8,
    pub authority_bump: u8,
}

#[event]
pub struct Deposited {
    pub owner: Pubkey,
    pub vault: Pubkey,
    pub amount: u64,
    pub deposited_after: u64,
    pub total_deposits_after: u64,
}

#[event]
pub struct Withdrawn {
    pub owner: Pubkey,
    pub vault: Pubkey,
    pub amount: u64,
    pub deposited_after: u64,
    pub total_deposits_after: u64,
}

#[repr(u8)]
pub enum SwapDirection {
    BaseToUsdc = 0,
    UsdcToBase = 1,
}

#[event]
pub struct OracleSwapped {
    pub user: Pubkey,
    pub base_vault: Pubkey,
    pub usdc_vault: Pubkey,
    pub from_mint: Pubkey,
    pub to_mint: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub oracle_price: i64,
    pub oracle_conf: u64,
    pub oracle_expo: i32,
    pub direction: u8,
}

#[error_code]
pub enum TeraniumError {
    #[msg("Invalid amount")]
    InvalidAmount,

    #[msg("Mint mismatch")]
    MintMismatch,

    #[msg("Unauthorized")]
    Unauthorized,

    #[msg("Invalid user position")]
    InvalidUserPosition,

    #[msg("Insufficient deposited balance")]
    InsufficientDepositedBalance,

    #[msg("Math overflow")]
    MathOverflow,

    #[msg("Invalid slippage bps")]
    InvalidSlippageBps,

    #[msg("Oracle has no price")]
    OracleNoPrice,

    #[msg("Oracle price is stale")]
    OracleStale,

    #[msg("Oracle price invalid")]
    OracleInvalidPrice,

    #[msg("Oracle confidence invalid")]
    OracleInvalidConfidence,

    #[msg("Oracle slippage exceeded")]
    OracleSlippageExceeded,

    #[msg("Invalid USDC mint")]
    InvalidUsdcMint,

    #[msg("Invalid swap pair")]
    InvalidSwapPair,

    #[msg("Swap would produce zero output")]
    SwapZeroOut,

    #[msg("Insufficient vault liquidity")]
    InsufficientVaultLiquidity,
}
