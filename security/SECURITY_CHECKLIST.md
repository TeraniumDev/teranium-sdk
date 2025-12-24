# Teranium Audit & Threat Model Checklist

## Vault Security

- PDA seed derivations
  - Vault PDA seeds are exactly `["vault", mint]`.
  - Vault authority PDA seeds are exactly `["vault_authority", vault_pda]`.
  - User position PDA seeds are exactly `["user_position", vault_pda, owner]`.
  - Verify no alternative seed path exists for the same semantic account.

- One vault per mint
  - `initialize_vault` uses PDA `["vault", mint]` and `init`; second init fails deterministically.
  - Vault stores `mint` and is immutable after init.

- Authority isolation
  - All vault outflows (withdraw + swap outleg) are signed by the Vault Authority PDA.
  - Verify no instruction performs a vault-owned token transfer without PDA signer.

- Account validation
  - Token accounts enforce `mint == vault.mint` for deposit/withdraw.
  - Token accounts enforce `owner == expected authority`.
  - UserPosition validates `owner` and `vault` link.

- Overflow/underflow protection
  - All state arithmetic uses checked add/sub.
  - Validate that u64 conversions from u128 swap math are safe.

- Accounting invariants
  - `vault.total_deposits` equals sum of all `UserPosition.deposited` for that vault.
  - Withdraw enforces `amount <= user_position.deposited`.

## Oracle Risks

- Price staleness
  - Swap rejects oracle prices older than configured staleness window.
  - Uses `publish_time` vs current cluster time.

- Oracle validity
  - Swap rejects `price <= 0` and missing price.
  - Validate the oracle account data parses as a valid feed.

- Oracle manipulation assumptions
  - Document assumptions on oracle resiliency and on-chain update cadence.
  - Consider correlated failure modes: network halt, delayed updates, or publisher outages.

- Single-oracle fallback risks
  - If only one oracle feed is used, review whether any secondary safety bounds are required.
  - Consider “halt swaps when oracle unhealthy” as the correct behavior.

## Swap Risks

- Slippage enforcement
  - Swap uses deterministic confidence bound: `conf/price <= max_slippage_bps`.
  - Ensure max slippage input is bounded to <= 10,000 bps.

- Deterministic execution
  - No floating point.
  - Fixed-point conversion uses integer arithmetic and explicit rounding via integer division.

- Solvency & withdrawals
  - Swap must not reduce a vault token balance below `vault.total_deposits`.
  - Verify this check occurs before the outflow transfer.

- MEV surface analysis
  - Oracle staleness + confidence bounds can be targeted by timing.
  - Assess transaction ordering sensitivity: users should set conservative `max_slippage_bps`.

## User Safety

- Withdrawal guarantees
  - Withdraw path should always succeed if user has deposited and the vault is solvent.
  - Swap solvency checks must preserve withdrawability for depositors.

- Accounting consistency
  - Deposit/withdraw updates to UserPosition and VaultAccount must be atomic within the same instruction.

- No hidden admin paths
  - No privileged signer branches.
  - No hard-coded upgrade authority assumptions in instruction logic.

## SDK Risks

- Transaction construction correctness
  - PDA derivations must match on-chain seeds exactly.
  - Vault token accounts should use ATAs for vault authority.

- PDA mismatch protection
  - SDK should derive addresses rather than accepting user-supplied PDAs.
  - When accepting an override, verify it matches derived values.

- Wallet signing assumptions
  - Deposit requires user signature for transfer.
  - Withdraw requires only user signature; vault outflow is PDA-signed.
  - Swap requires user signature and PDA signatures for outflow leg.
