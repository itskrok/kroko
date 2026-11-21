use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

// Temporary placeholder. Replace with the newly generated Mainnet Program ID.
declare_id!("11111111111111111111111111111111");

// KROKO PRESALE MAINNET CONFIGURATION
pub const ADMIN_PUBKEY: Pubkey = pubkey!("EUaCu2CTEiGsysm1mWQZE1arbSmPgT4wGjdo4823PAVk");

const TOKEN_DECIMALS: u8 = 6;
const TOKEN_SCALE: u128 = 1_000_000;
const LAMPORTS_PER_SOL: u128 = 1_000_000_000;
const PERCENT_SCALE: u128 = 100;

const SALE_SEED: &[u8] = b"sale";
const TOTAL_PRESALE_TOKENS: u64 = 500_000_000 * 1_000_000;

#[program]
pub mod kroko_presale {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        require!(
            ctx.accounts.mint.decimals == TOKEN_DECIMALS,
            PresaleError::InvalidMintDecimals
        );

        require_keys_eq!(
            ctx.accounts.token_vault.mint,
            ctx.accounts.mint.key(),
            PresaleError::InvalidVaultMint
        );

        require_keys_eq!(
            ctx.accounts.token_vault.owner,
            ctx.accounts.sale.key(),
            PresaleError::InvalidVaultAuthority
        );

        require!(
            ctx.accounts.token_vault.amount >= TOTAL_PRESALE_TOKENS,
            PresaleError::InsufficientVaultBalance
        );

        let sale = &mut ctx.accounts.sale;

        sale.admin = ctx.accounts.admin.key();
        sale.sol_receiver = ctx.accounts.admin.key();
        sale.mint = ctx.accounts.mint.key();
        sale.token_vault = ctx.accounts.token_vault.key();

        sale.stage_configs = [
            StageConfig {
                allocation: 40_000_000 * TOKEN_SCALE as u64,
                target_sol: 400 * LAMPORTS_PER_SOL as u64,
                base_price: 50_000,
                bonus_percent: 100,
                tokens_sold: 0,
                sol_raised: 0,
            },
            StageConfig {
                allocation: 175_000_000 * TOKEN_SCALE as u64,
                target_sol: 2_000 * LAMPORTS_PER_SOL as u64,
                base_price: 50_000,
                bonus_percent: 75,
                tokens_sold: 0,
                sol_raised: 0,
            },
            StageConfig {
                allocation: 285_000_000 * TOKEN_SCALE as u64,
                target_sol: 3_800 * LAMPORTS_PER_SOL as u64,
                base_price: 50_000,
                bonus_percent: 50,
                tokens_sold: 0,
                sol_raised: 0,
            },
        ];

        sale.total_transactions = 0;
        sale.total_sol_raised = 0;
        sale.total_tokens_sold = 0;
        sale.bump = ctx.bumps.sale;

        msg!("KROKO PRESALE initialized.");
        msg!("Admin and SOL Receiver: {}", sale.admin);
        msg!("Mint: {}", sale.mint);
        msg!("Token Vault: {}", sale.token_vault);

        Ok(())
    }

    pub fn buy(ctx: Context<Buy>, lamports: u64) -> Result<()> {
        require!(lamports > 0, PresaleError::InvalidAmount);

        let mut remaining_lamports = lamports;
        let mut tokens_out: u64 = 0;

        for stage in ctx.accounts.sale.stage_configs.iter_mut() {
            if remaining_lamports == 0 {
                break;
            }

            let remaining_tokens = stage
                .allocation
                .checked_sub(stage.tokens_sold)
                .ok_or(PresaleError::MathOverflow)?;

            if remaining_tokens == 0 {
                continue;
            }

            let numerator = (stage.base_price as u128)
                .checked_mul(100u128 + stage.bonus_percent as u128)
                .and_then(|value| value.checked_mul(TOKEN_SCALE))
                .ok_or(PresaleError::MathOverflow)?;

            let denominator = LAMPORTS_PER_SOL
                .checked_mul(PERCENT_SCALE)
                .ok_or(PresaleError::MathOverflow)?;

            // Minimum payment required to complete the current stage.
            let required_product = (remaining_tokens as u128)
                .checked_mul(denominator)
                .ok_or(PresaleError::MathOverflow)?;

            let required_lamports = required_product
                .checked_add(numerator - 1)
                .ok_or(PresaleError::MathOverflow)?
                / numerator;

            if (remaining_lamports as u128) >= required_lamports {
                // Complete this stage and carry the remaining payment forward.
                let used_lamports = u64::try_from(required_lamports)
                    .map_err(|_| error!(PresaleError::MathOverflow))?;

                stage.tokens_sold = stage
                    .tokens_sold
                    .checked_add(remaining_tokens)
                    .ok_or(PresaleError::MathOverflow)?;

                stage.sol_raised = stage
                    .sol_raised
                    .checked_add(used_lamports)
                    .ok_or(PresaleError::MathOverflow)?;

                tokens_out = tokens_out
                    .checked_add(remaining_tokens)
                    .ok_or(PresaleError::MathOverflow)?;

                remaining_lamports = remaining_lamports
                    .checked_sub(used_lamports)
                    .ok_or(PresaleError::MathOverflow)?;
            } else {
                // Partial purchase in the current stage.
                let token_product = (remaining_lamports as u128)
                    .checked_mul(numerator)
                    .ok_or(PresaleError::MathOverflow)?;

                let stage_tokens = u64::try_from(token_product / denominator)
                    .map_err(|_| error!(PresaleError::MathOverflow))?;

                require!(stage_tokens > 0, PresaleError::PurchaseTooSmall);

                stage.tokens_sold = stage
                    .tokens_sold
                    .checked_add(stage_tokens)
                    .ok_or(PresaleError::MathOverflow)?;

                stage.sol_raised = stage
                    .sol_raised
                    .checked_add(remaining_lamports)
                    .ok_or(PresaleError::MathOverflow)?;

                tokens_out = tokens_out
                    .checked_add(stage_tokens)
                    .ok_or(PresaleError::MathOverflow)?;

                remaining_lamports = 0;
                break;
            }
        }

        // Reject payments exceeding the remaining presale allocation.
        require!(remaining_lamports == 0, PresaleError::PresaleSoldOut);

        require!(tokens_out > 0, PresaleError::PurchaseTooSmall);

        // Update aggregate sale statistics.
        let sale = &mut ctx.accounts.sale;

        sale.total_transactions = sale
            .total_transactions
            .checked_add(1)
            .ok_or(PresaleError::MathOverflow)?;

        sale.total_sol_raised = sale
            .total_sol_raised
            .checked_add(lamports)
            .ok_or(PresaleError::MathOverflow)?;

        sale.total_tokens_sold = sale
            .total_tokens_sold
            .checked_add(tokens_out)
            .ok_or(PresaleError::MathOverflow)?;

        // Transfer SOL to the fixed SOL Receiver.
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.buyer.to_account_info(),
                    to: ctx.accounts.sol_receiver.to_account_info(),
                },
            ),
            lamports,
        )?;

        // Transfer KROK from the vault using the Sale PDA authority.
        let bump_seed = [sale.bump];
        let signer_seeds: &[&[u8]] = &[SALE_SEED, &bump_seed];
        let signer = &[signer_seeds];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.buyer_token_account.to_account_info(),
                    authority: sale.to_account_info(),
                },
                signer,
            ),
            tokens_out,
        )?;

        emit!(PurchaseEvent {
            buyer: ctx.accounts.buyer.key(),
            sol_paid: lamports,
            tokens_received: tokens_out,
            total_transactions: sale.total_transactions,
            total_sol_raised: sale.total_sol_raised,
            total_tokens_sold: sale.total_tokens_sold,
        });

        msg!("KROKO purchase completed.");
        msg!("SOL paid (lamports): {}", lamports);
        msg!("KROK received (smallest units): {}", tokens_out);

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = Sale::LEN,
        seeds = [SALE_SEED],
        bump
    )]
    pub sale: Account<'info, Sale>,

    #[account(
        mut,
        address = ADMIN_PUBKEY
    )]
    pub admin: Signer<'info>,

    pub mint: Account<'info, Mint>,

    pub token_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Buy<'info> {
    #[account(
        mut,
        seeds = [SALE_SEED],
        bump = sale.bump
    )]
    pub sale: Account<'info, Sale>,

    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(address = sale.mint)]
    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        address = sale.token_vault,
        token::mint = mint,
        token::authority = sale
    )]
    pub token_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = buyer
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = sale.sol_receiver
    )]
    pub sol_receiver: SystemAccount<'info>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct StageConfig {
    pub allocation: u64,
    pub target_sol: u64,
    pub base_price: u64,
    pub bonus_percent: u16,
    pub tokens_sold: u64,
    pub sol_raised: u64,
}

#[account]
pub struct Sale {
    pub admin: Pubkey,
    pub sol_receiver: Pubkey,
    pub mint: Pubkey,
    pub token_vault: Pubkey,
    pub stage_configs: [StageConfig; 3],
    pub total_transactions: u64,
    pub total_sol_raised: u64,
    pub total_tokens_sold: u64,
    pub bump: u8,
}

impl Sale {
    pub const LEN: usize = 8 +       // Account discriminator
        (32 * 4) + // Four public keys
        (42 * 3) + // Three StageConfig entries
        (8 * 3) +  // Aggregate statistics
        1; // PDA bump
}

#[event]
pub struct PurchaseEvent {
    pub buyer: Pubkey,
    pub sol_paid: u64,
    pub tokens_received: u64,
    pub total_transactions: u64,
    pub total_sol_raised: u64,
    pub total_tokens_sold: u64,
}

#[error_code]
pub enum PresaleError {
    #[msg("The purchase amount must be greater than zero.")]
    InvalidAmount,

    #[msg("The purchase is too small to receive one smallest KROK unit.")]
    PurchaseTooSmall,

    #[msg("The presale allocation is sold out or the purchase exceeds the remaining allocation.")]
    PresaleSoldOut,

    #[msg("The mint must use exactly six decimals.")]
    InvalidMintDecimals,

    #[msg("The token vault mint does not match the presale mint.")]
    InvalidVaultMint,

    #[msg("The token vault authority must be the Sale PDA.")]
    InvalidVaultAuthority,

    #[msg("The token vault must contain at least 500 million KROK.")]
    InsufficientVaultBalance,

    #[msg("A mathematical calculation overflow occurred.")]
    MathOverflow,
}
