// skipped legacy Anchor TS e2e; Rust program-test covers integration
import * as anchor from "@coral-xyz/anchor";
const { BN } = anchor;
type AnchorProvider = anchor.AnchorProvider;
type Idl = anchor.Idl;
import * as web3 from "@solana/web3.js";
import {
  getOrCreateAssociatedTokenAccount,
  createMint,
  mintTo,
  TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
} from "@solana/spl-token";
import fs from "fs";
import path from "path";
import { expect } from "chai";

// Memo program for target CPI
const MEMO_PROGRAM_ID = new web3.PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

// Types
type AnyIdl = Idl & { metadata?: { address?: string } } & { address?: string };

describe.skip("zoopx_router token_interface e2e", function () {
  this.timeout(1_000_000);

  const provider = (() => {
    // Ensure Anchor has a wallet and RPC URL in env for NodeWallet and Provider
    const home = process.env.HOME || process.env.USERPROFILE || "";
    if (!process.env.ANCHOR_WALLET && home) {
      process.env.ANCHOR_WALLET = path.join(home, ".config/solana/id.json");
    }
    if (!process.env.ANCHOR_PROVIDER_URL) {
      process.env.ANCHOR_PROVIDER_URL = "http://127.0.0.1:8899";
    }
    return anchor.AnchorProvider.env();
  })();
  anchor.setProvider(provider);
  const connection = provider.connection;
  const payer = (provider.wallet as anchor.Wallet).payer ?? (provider.wallet as any); // anchor-cli wallet

  let program: any;
  let programId: web3.PublicKey;
  let configPda: web3.PublicKey;
  let configBump: number; // not directly needed, but useful to keep

  // Common config
  const decimals = 6;
  const userMintAmount = 1_000_000n; // 1.0 token (6 decimals)
  const amount = 100_000n; // 0.1 tokens
  const protocolFee = 10_000n; // 0.01 tokens
  const payload = Buffer.from("hello");

  const confirmOpts: web3.ConfirmOptions = { commitment: "confirmed" };

  before(async () => {
  ({ program, programId } = await loadProgram());

    // Derive config PDA
  [configPda, configBump] = web3.PublicKey.findProgramAddressSync(
      [Buffer.from("zoopx_config")],
      programId
    );

    // Airdrop payer
  await airdropIfNeeded(provider, payer.publicKey, 2 * web3.LAMPORTS_PER_SOL);

    // Initialize or update config with a fresh fee recipient
    const info = await connection.getAccountInfo(configPda, confirmOpts.commitment);
    const feeRecipient = web3.Keypair.generate();
    await airdropIfNeeded(provider, feeRecipient.publicKey, 0.5 * web3.LAMPORTS_PER_SOL);
    if (!info) {
      await program.methods
        .initializeConfig(feeRecipient.publicKey)
        .accounts({
          payer: payer.publicKey,
          config: configPda,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([payer])
        .rpc(confirmOpts);
    } else {
      await program.methods
        .updateConfig(feeRecipient.publicKey)
        .accounts({
          authority: payer.publicKey,
          config: configPda,
        })
        .signers([payer])
        .rpc(confirmOpts);
    }
    // Stash on global for later use
    (globalThis as any).__feeRecipientPubkey = feeRecipient.publicKey;
  });

  it("routes via SPL Token (classic)", async () => {
  const tokenProgram = TOKEN_PROGRAM_ID;

    // Create mint
    const mint = await createMint(
  connection,
  payer,
  payer.publicKey,
  null,
  decimals,
  undefined,
  confirmOpts,
  tokenProgram
    );

    // ATAs
  const user = payer; // use payer as user
  const feeRecipientOwner = (globalThis as any).__feeRecipientPubkey as web3.PublicKey;
  const cpiTargetOwner = web3.Keypair.generate();
  await airdropIfNeeded(provider, cpiTargetOwner.publicKey, 0.5 * web3.LAMPORTS_PER_SOL);

    const userAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      user.publicKey,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );
    const feeRecipientAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      feeRecipientOwner,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );
    const cpiTargetAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      cpiTargetOwner.publicKey,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );

    // Mint to user
    await mintTo(
  connection,
  payer,
  mint,
  userAta.address,
  payer,
  Number(userMintAmount),
  [],
  confirmOpts,
  tokenProgram
    );

    // Pre balances
    const preUser = await getBalance(connection, userAta.address);
    const preFee = await getBalance(connection, feeRecipientAta.address);
    const preCpi = await getBalance(connection, cpiTargetAta.address);

    // Call router
    await program.methods
      .universalBridgeTransfer(new BN(amount.toString()), new BN(protocolFee.toString()), Buffer.from(payload))
      .accounts({
        user: user.publicKey,
        mint,
        from: userAta.address,
        feeRecipientToken: feeRecipientAta.address,
        cpiTargetTokenAccount: cpiTargetAta.address,
        targetAdapterProgram: MEMO_PROGRAM_ID,
        tokenProgram: tokenProgram,
        config: configPda,
      })
      .remainingAccounts([
        { pubkey: cpiTargetAta.address, isWritable: true, isSigner: false },
        { pubkey: MEMO_PROGRAM_ID, isWritable: false, isSigner: false },
      ])
      .signers([user])
      .rpc(confirmOpts);

    // Post balances
    const postUser = await getBalance(connection, userAta.address);
    const postFee = await getBalance(connection, feeRecipientAta.address);
    const postCpi = await getBalance(connection, cpiTargetAta.address);

    expect(preFee + protocolFee).to.equal(postFee);
    expect(preCpi + (amount - protocolFee)).to.equal(postCpi);
    expect(preUser - amount).to.equal(postUser);

    // Owner assertions
    await assertOwnerProgram(connection, userAta.address, tokenProgram);
    await assertOwnerProgram(connection, feeRecipientAta.address, tokenProgram);
    await assertOwnerProgram(connection, cpiTargetAta.address, tokenProgram);
  });

  it("routes via Token-2022", async () => {
  const tokenProgram = TOKEN_2022_PROGRAM_ID;

    // Create mint
    const mint = await createMint(
  connection,
  payer,
  payer.publicKey,
  null,
  decimals,
  undefined,
  confirmOpts,
  tokenProgram
    );

    // ATAs
  const user = payer; // use payer as user
  const feeRecipientOwner = (globalThis as any).__feeRecipientPubkey as web3.PublicKey;
  const cpiTargetOwner = web3.Keypair.generate();
  await airdropIfNeeded(provider, cpiTargetOwner.publicKey, 0.5 * web3.LAMPORTS_PER_SOL);

    const userAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      user.publicKey,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );
    const feeRecipientAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      feeRecipientOwner,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );
    const cpiTargetAta = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      cpiTargetOwner.publicKey,
      true,
  confirmOpts.commitment,
  confirmOpts,
  tokenProgram
    );

    // Mint to user
    await mintTo(
  connection,
  payer,
  mint,
  userAta.address,
  payer,
  Number(userMintAmount),
  [],
  confirmOpts,
  tokenProgram
    );

    // Pre balances
    const preUser = await getBalance(connection, userAta.address);
    const preFee = await getBalance(connection, feeRecipientAta.address);
    const preCpi = await getBalance(connection, cpiTargetAta.address);

    // Call router
    await program.methods
      .universalBridgeTransfer(new BN(amount.toString()), new BN(protocolFee.toString()), Buffer.from(payload))
      .accounts({
        user: user.publicKey,
        mint,
        from: userAta.address,
        feeRecipientToken: feeRecipientAta.address,
        cpiTargetTokenAccount: cpiTargetAta.address,
        targetAdapterProgram: MEMO_PROGRAM_ID,
        tokenProgram: tokenProgram,
        config: configPda,
      })
      .remainingAccounts([
        { pubkey: cpiTargetAta.address, isWritable: true, isSigner: false },
        { pubkey: MEMO_PROGRAM_ID, isWritable: false, isSigner: false },
      ])
      .signers([user])
      .rpc(confirmOpts);

    // Post balances
    const postUser = await getBalance(connection, userAta.address);
    const postFee = await getBalance(connection, feeRecipientAta.address);
    const postCpi = await getBalance(connection, cpiTargetAta.address);

    expect(preFee + protocolFee).to.equal(postFee);
    expect(preCpi + (amount - protocolFee)).to.equal(postCpi);
    expect(preUser - amount).to.equal(postUser);

    // Owner assertions
    await assertOwnerProgram(connection, userAta.address, tokenProgram);
    await assertOwnerProgram(connection, feeRecipientAta.address, tokenProgram);
    await assertOwnerProgram(connection, cpiTargetAta.address, tokenProgram);
  });

  // Helpers
  function loadProgram(): { program: any; programId: web3.PublicKey } {
    const idlPath = path.resolve(__dirname, "../target/idl/zoopx_router.json");
    const raw = fs.readFileSync(idlPath, "utf8");
    const idl = JSON.parse(raw) as Idl & { metadata?: { address?: string } };

  // Note: Some Anchor IDLs on 0.31.x omit account struct layouts in idl.accounts.
  // We'll proceed even if idl.accounts is missing or empty to avoid Program constructor crashes.
    if (!idl.metadata?.address) {
      const topAddr = (idl as any).address as string | undefined;
      if (topAddr) {
        (idl as any).metadata = { ...(idl as any).metadata, address: topAddr } as any;
      } else {
        throw new Error("IDL missing metadata.address. Run `anchor deploy` then `anchor build`.");
      }
    }

  const provider = anchor.AnchorProvider.local();
    anchor.setProvider(provider);
    // Repair/inline account type layouts so Anchor can compute sizes
    const anyIdl: any = idl as any;
    if (Array.isArray(anyIdl.accounts) && Array.isArray(anyIdl.types)) {
      for (const acc of anyIdl.accounts) {
        const linkName: string | undefined = acc?.name;
        const accType = acc?.type;
        // Try to resolve defined references or missing types by name
        const resolveToInline = (name: string) => {
          const def = anyIdl.types.find((t: any) => t.name === name);
          if (def && def.type) {
            acc.type = def.type; // inline struct so coder has a concrete layout
          }
        };

        if (!accType && linkName) {
          resolveToInline(linkName);
        } else if (accType && (accType.defined as string | undefined)) {
          resolveToInline(accType.defined as string);
        }
      }
    }
  // As a final guard, remove accounts namespace to avoid Program building AccountClient with missing coder
  delete anyIdl.accounts;
  // Debugging: verify IDL shape
  // eslint-disable-next-line no-console
  console.log("IDL keys:", Object.keys(idl));
  // eslint-disable-next-line no-console
  console.log("IDL instructions count:", Array.isArray((idl as any).instructions) ? (idl as any).instructions.length : "none");

  const programId = new web3.PublicKey((idl as any).metadata.address as string);
  const program = new (anchor as any).Program(idl as any, programId as any, provider as any);
  // eslint-disable-next-line no-console
  console.log("Program coder present:", !!(program as any).coder, "instruction coder:", !!(program as any).coder?.instruction);
    return { program, programId };
  }

  async function airdropIfNeeded(provider: AnchorProvider, pubkey: web3.PublicKey, lamports: number) {
    const bal = await provider.connection.getBalance(pubkey, "processed");
    if (bal >= lamports / 2) return; // already funded
    const sig = await provider.connection.requestAirdrop(pubkey, lamports);
    await provider.connection.confirmTransaction(sig, "confirmed");
  }

  async function getBalance(conn: web3.Connection, ata: web3.PublicKey): Promise<bigint> {
    const res = await conn.getTokenAccountBalance(ata, "confirmed");
    return BigInt(res.value.amount);
  }

  async function assertOwnerProgram(conn: web3.Connection, ata: web3.PublicKey, ownerProgram: web3.PublicKey) {
    const info = await conn.getAccountInfo(ata, "confirmed");
    if (!info) throw new Error("Account not found: " + ata.toBase58());
    expect(info.owner.toBase58()).to.equal(ownerProgram.toBase58());
  }
});
