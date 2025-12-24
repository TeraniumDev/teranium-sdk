import {
  AnchorProvider,
  Program,
  type Idl,
} from "@coral-xyz/anchor";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  PublicKey,
  type Transaction,
  type VersionedTransaction,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  type Connection,
  type TransactionSignature,
} from "@solana/web3.js";

import { TERANIUM_IDL } from "./idl";
import { TERANIUM_PROGRAM_ID, USDC_MINT } from "./constants";
import { findUserPositionPda, findVaultAuthorityPda, findVaultPda } from "./pdas";

export type Commitment = "processed" | "confirmed" | "finalized";

export type TeraniumWallet = {
  publicKey: PublicKey;
  signTransaction: <T extends Transaction | VersionedTransaction>(transaction: T) => Promise<T>;
  signAllTransactions: <T extends Transaction | VersionedTransaction>(transactions: T[]) => Promise<T[]>;
};

export type TeraniumConfig = {
  connection: Connection;
  wallet: TeraniumWallet;
  programId?: PublicKey;
  commitment?: Commitment;
};

export type InitializeVaultParams = {
  mint: PublicKey;
};

export type DepositParams = {
  mint: PublicKey;
  amount: bigint;
  userTokenAccount?: PublicKey;
};

export type WithdrawParams = {
  mint: PublicKey;
  amount: bigint;
  userTokenAccount?: PublicKey;
};

export type OracleSwapDirection = "baseToUsdc" | "usdcToBase";

export type OracleSwapParams = {
  baseMint: PublicKey;
  direction: OracleSwapDirection;
  amount: bigint;
  maxSlippageBps: number;
  pythPriceAccount: PublicKey;
  userBaseTokenAccount?: PublicKey;
  userUsdcTokenAccount?: PublicKey;
};

function toU64(amount: bigint): bigint {
  if (amount <= 0n) throw new Error("amount must be > 0");
  const max = (1n << 64n) - 1n;
  if (amount > max) throw new Error("amount exceeds u64");
  return amount;
}

function toU16(value: number): number {
  if (!Number.isInteger(value)) throw new Error("maxSlippageBps must be integer");
  if (value < 0 || value > 10_000) throw new Error("maxSlippageBps out of range");
  return value;
}

export class Teranium {
  readonly programId: PublicKey;
  readonly provider: AnchorProvider;
  readonly program: Program<Idl>;

  readonly vault: {
    initializeVault: (params: InitializeVaultParams) => Promise<TransactionSignature>;
    deposit: (params: DepositParams) => Promise<TransactionSignature>;
    withdraw: (params: WithdrawParams) => Promise<TransactionSignature>;
  };

  readonly swap: {
    execute: (params: OracleSwapParams) => Promise<TransactionSignature>;
  };

  constructor(cfg: TeraniumConfig) {
    this.programId = cfg.programId ?? TERANIUM_PROGRAM_ID;

    this.provider = new AnchorProvider(cfg.connection, cfg.wallet, {
      commitment: cfg.commitment ?? "confirmed",
    });

    const idlWithAddress = {
      ...(TERANIUM_IDL as unknown as Record<string, unknown>),
      address: this.programId.toBase58(),
    } as unknown as Idl;

    this.program = new Program(idlWithAddress, this.provider);

    this.vault = {
      initializeVault: async ({ mint }) => {
        const [vault] = findVaultPda(this.programId, mint);
        const [vaultAuthority] = findVaultAuthorityPda(this.programId, vault);
        const vaultTokenAccount = getAssociatedTokenAddressSync(mint, vaultAuthority, true);

        return await this.program.methods
          .initializeVault(mint)
          .accounts({
            payer: this.provider.wallet.publicKey,
            mint,
            vault,
            vaultAuthority,
            vaultTokenAccount,
            systemProgram: SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            rent: SYSVAR_RENT_PUBKEY,
          })
          .rpc();
      },

      deposit: async ({ mint, amount, userTokenAccount }) => {
        const u64 = toU64(amount);
        const owner = this.provider.wallet.publicKey;

        const [vault] = findVaultPda(this.programId, mint);
        const [vaultAuthority] = findVaultAuthorityPda(this.programId, vault);
        const [userPosition] = findUserPositionPda(this.programId, vault, owner);

        const userAta = userTokenAccount ?? getAssociatedTokenAddressSync(mint, owner, false);
        const vaultAta = getAssociatedTokenAddressSync(mint, vaultAuthority, true);

        return await this.program.methods
          .deposit(u64)
          .accounts({
            owner,
            vault,
            vaultAuthority,
            userPosition,
            userTokenAccount: userAta,
            vaultTokenAccount: vaultAta,
            systemProgram: SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            rent: SYSVAR_RENT_PUBKEY,
          })
          .rpc();
      },

      withdraw: async ({ mint, amount, userTokenAccount }) => {
        const u64 = toU64(amount);
        const owner = this.provider.wallet.publicKey;

        const [vault] = findVaultPda(this.programId, mint);
        const [vaultAuthority] = findVaultAuthorityPda(this.programId, vault);
        const [userPosition] = findUserPositionPda(this.programId, vault, owner);

        const userAta = userTokenAccount ?? getAssociatedTokenAddressSync(mint, owner, false);
        const vaultAta = getAssociatedTokenAddressSync(mint, vaultAuthority, true);

        return await this.program.methods
          .withdraw(u64)
          .accounts({
            owner,
            vault,
            vaultAuthority,
            userPosition,
            userTokenAccount: userAta,
            vaultTokenAccount: vaultAta,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .rpc();
      },
    };

    this.swap = {
      execute: async ({
        baseMint,
        direction,
        amount,
        maxSlippageBps,
        pythPriceAccount,
        userBaseTokenAccount,
        userUsdcTokenAccount,
      }) => {
        const u64 = toU64(amount);
        const u16 = toU16(maxSlippageBps);
        const user = this.provider.wallet.publicKey;

        const [baseVault] = findVaultPda(this.programId, baseMint);
        const [baseVaultAuthority] = findVaultAuthorityPda(this.programId, baseVault);
        const baseVaultTokenAccount = getAssociatedTokenAddressSync(baseMint, baseVaultAuthority, true);

        const [usdcVault] = findVaultPda(this.programId, USDC_MINT);
        const [usdcVaultAuthority] = findVaultAuthorityPda(this.programId, usdcVault);
        const usdcVaultTokenAccount = getAssociatedTokenAddressSync(USDC_MINT, usdcVaultAuthority, true);

        const userBaseAta = userBaseTokenAccount ?? getAssociatedTokenAddressSync(baseMint, user, false);
        const userUsdcAta = userUsdcTokenAccount ?? getAssociatedTokenAddressSync(USDC_MINT, user, false);

        const userFromTokenAccount = direction === "baseToUsdc" ? userBaseAta : userUsdcAta;
        const userToTokenAccount = direction === "baseToUsdc" ? userUsdcAta : userBaseAta;

        return await this.program.methods
          .oracleSwap(u64, u16)
          .accounts({
            user,
            baseVault,
            baseVaultAuthority,
            baseVaultTokenAccount,
            baseMint,
            usdcVault,
            usdcVaultAuthority,
            usdcVaultTokenAccount,
            usdcMint: USDC_MINT,
            userFromTokenAccount,
            userToTokenAccount,
            pythPriceAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .rpc();
      },
    };
  }
}
