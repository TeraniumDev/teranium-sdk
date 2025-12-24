"use client";

import { useMemo, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";

import { Teranium } from "@teranium/sdk";

function parsePubkey(value: string): PublicKey {
  return new PublicKey(value.trim());
}

function parseU64(value: string): bigint {
  const v = BigInt(value.trim());
  if (v <= 0n) throw new Error("amount must be > 0");
  return v;
}

export function AppShell() {
  const { connection } = useConnection();
  const wallet = useWallet();

  const teranium = useMemo(() => {
    if (!wallet.publicKey || !wallet.signTransaction || !wallet.signAllTransactions) return null;
    return new Teranium({
      connection,
      wallet: {
        publicKey: wallet.publicKey,
        signTransaction: wallet.signTransaction,
        signAllTransactions: wallet.signAllTransactions,
      },
    });
  }, [connection, wallet.publicKey, wallet.signTransaction, wallet.signAllTransactions]);

  const [mint, setMint] = useState("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
  const [amount, setAmount] = useState("1");

  const [baseMint, setBaseMint] = useState("");
  const [swapDir, setSwapDir] = useState<"baseToUsdc" | "usdcToBase">("baseToUsdc");
  const [maxSlippageBps, setMaxSlippageBps] = useState("50");
  const [pythPrice, setPythPrice] = useState("");

  const [txSig, setTxSig] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run<T>(fn: () => Promise<T>) {
    setErr(null);
    setTxSig(null);
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ minHeight: "100vh", padding: 24, maxWidth: 880, margin: "0 auto" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 24 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 700 }}>Teranium Integrator</h1>
        <WalletMultiButton />
      </div>

      {!wallet.publicKey ? (
        <div style={{ padding: 16, border: "1px solid #e5e7eb", borderRadius: 12 }}>
          Connect a wallet to continue.
        </div>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "1fr", gap: 16 }}>
          <section style={{ padding: 16, border: "1px solid #e5e7eb", borderRadius: 12 }}>
            <h2 style={{ marginTop: 0 }}>Vault</h2>

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Mint</label>
            <input
              value={mint}
              onChange={(e) => setMint(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
              placeholder="Token mint"
            />

            <div style={{ height: 10 }} />

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Amount (raw, u64)</label>
            <input
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
              placeholder="1"
            />

            <div style={{ display: "flex", gap: 12, marginTop: 12, flexWrap: "wrap" }}>
              <button
                disabled={!teranium || busy}
                onClick={() =>
                  run(async () => {
                    const sig = await teranium!.vault.deposit({
                      mint: parsePubkey(mint),
                      amount: parseU64(amount),
                    });
                    setTxSig(sig);
                  })
                }
                style={{ padding: "10px 12px", borderRadius: 10, border: "1px solid #111827", background: "#111827", color: "white" }}
              >
                Deposit
              </button>

              <button
                disabled={!teranium || busy}
                onClick={() =>
                  run(async () => {
                    const sig = await teranium!.vault.withdraw({
                      mint: parsePubkey(mint),
                      amount: parseU64(amount),
                    });
                    setTxSig(sig);
                  })
                }
                style={{ padding: "10px 12px", borderRadius: 10, border: "1px solid #111827", background: "white", color: "#111827" }}
              >
                Withdraw
              </button>
            </div>
          </section>

          <section style={{ padding: 16, border: "1px solid #e5e7eb", borderRadius: 12 }}>
            <h2 style={{ marginTop: 0 }}>Oracle Swap</h2>

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Base Mint</label>
            <input
              value={baseMint}
              onChange={(e) => setBaseMint(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
              placeholder="Base mint (non-USDC)"
            />

            <div style={{ height: 10 }} />

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Pyth Price Account (legacy)</label>
            <input
              value={pythPrice}
              onChange={(e) => setPythPrice(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
              placeholder="Pyth price account pubkey"
            />

            <div style={{ height: 10 }} />

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Direction</label>
            <select
              value={swapDir}
              onChange={(e) => setSwapDir(e.target.value as any)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
            >
              <option value="baseToUsdc">Base → USDC</option>
              <option value="usdcToBase">USDC → Base</option>
            </select>

            <div style={{ height: 10 }} />

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Amount (raw, u64)</label>
            <input
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
            />

            <div style={{ height: 10 }} />

            <label style={{ display: "block", fontSize: 12, marginBottom: 6 }}>Max slippage (bps)</label>
            <input
              value={maxSlippageBps}
              onChange={(e) => setMaxSlippageBps(e.target.value)}
              style={{ width: "100%", padding: 10, border: "1px solid #e5e7eb", borderRadius: 10 }}
              placeholder="50"
            />

            <div style={{ display: "flex", gap: 12, marginTop: 12, flexWrap: "wrap" }}>
              <button
                disabled={!teranium || busy}
                onClick={() =>
                  run(async () => {
                    const sig = await teranium!.swap.execute({
                      baseMint: parsePubkey(baseMint),
                      direction: swapDir,
                      amount: parseU64(amount),
                      maxSlippageBps: Number(maxSlippageBps),
                      pythPriceAccount: parsePubkey(pythPrice),
                    });
                    setTxSig(sig);
                  })
                }
                style={{ padding: "10px 12px", borderRadius: 10, border: "1px solid #111827", background: "#111827", color: "white" }}
              >
                Execute Swap
              </button>
            </div>
          </section>

          {(err || txSig) && (
            <section style={{ padding: 16, border: "1px solid #e5e7eb", borderRadius: 12 }}>
              <h2 style={{ marginTop: 0 }}>Status</h2>
              {err && <div style={{ color: "#b91c1c" }}>{err}</div>}
              {txSig && (
                <div>
                  Signature: <a href={`https://solscan.io/tx/${txSig}`} target="_blank" rel="noreferrer">{txSig}</a>
                </div>
              )}
            </section>
          )}
        </div>
      )}
    </div>
  );
}
