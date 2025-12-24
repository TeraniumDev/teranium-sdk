import { PublicKey } from "@solana/web3.js";

export function findVaultPda(programId: PublicKey, mint: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([Buffer.from("vault"), mint.toBuffer()], programId);
}

export function findVaultAuthorityPda(programId: PublicKey, vault: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([Buffer.from("vault_authority"), vault.toBuffer()], programId);
}

export function findUserPositionPda(programId: PublicKey, vault: PublicKey, owner: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("user_position"), vault.toBuffer(), owner.toBuffer()],
    programId,
  );
}
