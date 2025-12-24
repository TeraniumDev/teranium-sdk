# Teranium (Anchor Program + SDK + Integrator)
![Teranium](assets/banner.png)
Teranium is a PDA-authorized Solana program implementing per-mint vaults (deposit/withdraw) and deterministic oracle-priced swaps against the USDC vault. This repository contains:

- On-chain Anchor program: [anchor/programs/teranium/src/lib.rs](anchor/programs/teranium/src/lib.rs)
- Program IDL: [idl/teranium.json](idl/teranium.json)
- TypeScript SDK: [packages/sdk/src/teranium.ts](packages/sdk/src/teranium.ts)
- Next.js App Router integrator example: [apps/integrator/app/components/AppShell.tsx](apps/integrator/app/components/AppShell.tsx)
- Security checklist / threat model: [security/SECURITY_CHECKLIST.md](security/SECURITY_CHECKLIST.md)

## On-chain Program

### Program ID

Mainnet program id (also used in localnet/devnet configs in this repo):

- Dx9ZBP9kFYjvZX6sY6bHKgyD3BQtTmnhU6apDpMUAMWV

Config: [anchor/Anchor.toml](anchor/Anchor.toml)

### Authority Model (Deterministic PDAs)

All token outflows are signed by a PDA (no custody keys). PDAs:

- Vault PDA (one per mint):
	- seeds: ["vault", mint]
- Vault authority PDA (token authority for the vault ATA):
	- seeds: ["vault_authority", vault_pda]
- User position PDA:
	- seeds: ["user_position", vault_pda, user]

PDA derivations (SDK): [packages/sdk/src/pdas.ts](packages/sdk/src/pdas.ts)

### State

VaultAccount

- mint: Pubkey
- bump: u8
- authority_bump: u8
- total_deposits: u64

UserPosition

- owner: Pubkey
- vault: Pubkey
- deposited: u64

IDL types: [idl/teranium.json](idl/teranium.json)

### Instructions

#### initialize_vault(mint: Pubkey)

Creates:

- Vault PDA using ["vault", mint]
- Vault authority PDA using ["vault_authority", vault]
- Vault ATA for the vault authority

Enforces:

- One vault per mint (PDA init is unique)
- Stored mint immutability (vault.mint is set at init)

#### deposit(amount: u64)

Transfers tokens from user ATA to vault ATA.

Enforces:

- amount > 0
- user token account mint equals vault.mint
- vault token account mint equals vault.mint
- UserPosition PDA is created if missing (init_if_needed)

Updates:

- user_position.deposited += amount (checked)
- vault.total_deposits += amount (checked)

#### withdraw(amount: u64)

Transfers tokens from vault ATA to user ATA.

Enforces:

- amount > 0
- amount <= user_position.deposited
- vault authority PDA signs the token transfer

Updates:

- user_position.deposited -= amount (checked)
- vault.total_deposits -= amount (checked)

#### oracle_swap(amount: u64, max_slippage_bps: u16)

This implementation is deterministic and does not run AMM math.

Swap pair model:

- Base mint vault <-> USDC vault (mainnet USDC mint fixed):
	- EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v

Oracle model:

- Reads a Pyth legacy price account for base mint USD price.
- Rejects stale prices using publish_time.
- Uses oracle confidence interval as a deterministic slippage guard:
	- conf / price <= max_slippage_bps / 10_000

Settlement model:

- If user swaps Base -> USDC:
	- user transfers Base into Base vault
	- USDC vault transfers USDC out to user (PDA-signed)
- If user swaps USDC -> Base:
	- user transfers USDC into USDC vault
	- Base vault transfers Base out to user (PDA-signed)

Solvency invariant enforced at execution time:

- The paying vault’s post-transfer token balance must remain >= vault.total_deposits.
	- This preserves deposit-backed withdrawability.

### Events

- VaultInitialized
- Deposited
- Withdrawn
- OracleSwapped

Event schemas are in the IDL: [idl/teranium.json](idl/teranium.json)

### Errors

All program errors are defined in the IDL errors list: [idl/teranium.json](idl/teranium.json)

## TypeScript SDK

SDK entrypoint:

- [packages/sdk/src/index.ts](packages/sdk/src/index.ts)

Key properties:

- Bundles the IDL from [idl/teranium.json](idl/teranium.json) to guarantee deterministic instruction layout.
- Derives PDAs internally; does not require integrators to pass PDAs.
- Uses ATAs for vault authority token accounts.

### Installation (workspace)

From repo root:

```bash
npm install
```

### SDK Usage

```ts
import { Connection, PublicKey } from "@solana/web3.js";
import { Teranium } from "@teranium/sdk";

const connection = new Connection("https://api.mainnet-beta.solana.com", "confirmed");

// wallet must implement: publicKey, signTransaction, signAllTransactions
const teranium = new Teranium({ connection, wallet });

// Deposit raw token units (u64)
await teranium.vault.deposit({
	mint: new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
	amount: 1n,
});

// Withdraw
await teranium.vault.withdraw({
	mint: new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
	amount: 1n,
});

// Oracle swap (base <-> USDC)
await teranium.swap.execute({
	baseMint: new PublicKey("So11111111111111111111111111111111111111112"),
	direction: "baseToUsdc",
	amount: 1n,
	maxSlippageBps: 50,
	pythPriceAccount: new PublicKey("<pyth_legacy_price_account>")
});
```

## Next.js Integrator Example

The integrator is a real App Router example using Solana Wallet Adapter and the Teranium SDK:

- [apps/integrator/app/components/AppShell.tsx](apps/integrator/app/components/AppShell.tsx)

### Run

```bash
npm run dev
```

Environment:

- NEXT_PUBLIC_RPC_URL (optional; defaults to mainnet-beta public RPC)

## Build / Tooling

### JS/TS build

```bash
npm run build
```

### On-chain build prerequisites

This repo includes Anchor sources, but local compilation requires installing Solana + Rust + Anchor on your machine:

- Rust toolchain
- Solana CLI
- Anchor CLI

Anchor workspace is under [anchor](anchor).

## Security Notes

See the audit checklist: [security/SECURITY_CHECKLIST.md](security/SECURITY_CHECKLIST.md)

Key invariants:

- All vault outflows are PDA-signed
- Integer-only math (no floats)
- Checked arithmetic
- Explicit account constraints
- Swap never reduces a vault below its deposit liabilities (token balance >= total_deposits)
