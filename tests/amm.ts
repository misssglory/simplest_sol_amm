import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { Amm } from '../target/types/amm';
import {
  TOKEN_PROGRAM_ID,
  createMint,
  getAssociatedTokenAddress,
  createAssociatedTokenAccount,
  mintTo,
  getAccount,
  getMint,
  createAccount,
  getOrCreateAssociatedTokenAccount,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';
// import {
//   Keypair,
//   Transaction
// } from '@solana/web3.js';
import { assert } from 'chai';

describe('amm', () => {
  const provider = anchor.AnchorProvider.env();
  const connection = provider.connection;
  anchor.setProvider(provider);

  const program = anchor.workspace.Amm as Program<Amm>;
  const admin = provider.wallet;

  let tokenAMint: anchor.web3.PublicKey;
  let tokenBMint: anchor.web3.PublicKey;
  let pool: anchor.web3.PublicKey;
  let poolBump: number;
  let lpMint: anchor.web3.Keypair;
  let tokenAVault: anchor.web3.Keypair;
  let tokenBVault: anchor.web3.Keypair;
  const decimals: bigint = 9n;

  before(async () => {
    // Create test tokens
    tokenAMint = await createMint(
      provider.connection,
      admin.payer,
      admin.publicKey,
      null,
      Number(decimals)
    );

    tokenBMint = await createMint(
      provider.connection,
      admin.payer,
      admin.publicKey,
      null,
      Number(decimals)
    );

    // Find pool PDA
    [pool, poolBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("pool"),
        tokenAMint.toBuffer(),
        tokenBMint.toBuffer(),
      ],
      program.programId
    );

    tokenAVault = anchor.web3.Keypair.generate();
    tokenBVault = anchor.web3.Keypair.generate();
  });

  it('Initialize AMM Pool', async () => {
    // Create LP Mint FIRST
    console.log("Initialize AMM Pool");
    console.log(admin.publicKey);
    console.log(TOKEN_PROGRAM_ID);
    console.log(anchor.web3.SystemProgram.programId);


    lpMint = anchor.web3.Keypair.generate();
    console.log("LP Mint:", lpMint.publicKey.toBase58());

    // Create vault accounts manually
    console.log("Token A Vault:", tokenAVault.publicKey.toBase58());

    console.log("Token B Vault: ", tokenBVault.publicKey.toBase58());
    console.log("Token A Mint: ", tokenAMint.toBase58());
    console.log("Token B Mint: ", tokenBMint.toBase58());
    console.log("Pool: ", pool.toBase58());

    console.log("admin: ", admin.publicKey.toBase58());
    console.log("tokenProgram: ", TOKEN_PROGRAM_ID.toBase58());
    console.log("systemProgram: ", anchor.web3.SystemProgram.programId.toBase58());
    console.log("rent: ", anchor.web3.SYSVAR_RENT_PUBKEY.toBase58());

    // const transaction = new Transaction();
    // // .add(transferInstruction);
    // transaction.add()

    // const transactionSignature = await sendAndConfirmTransaction(
    //   connection,
    //   transaction,
    //   [admin.payer], // signer
    // );

    // Initialize pool with ALL required accounts
    // const test_admin = anchor.web3.Keypair.generate();
    // console.log('test admin ', test_admin.publicKey.toBase58());
    const tx = await program.methods
      .initializePool(30) // 0.3% fee
      .accounts({
        pool: pool,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
        tokenAVault: tokenAVault.publicKey,
        tokenBVault: tokenBVault.publicKey,
        lpMint: lpMint.publicKey,
        admin: admin.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      // .signers([admin.payer])
      .signers([tokenAVault, tokenBVault, lpMint, admin.payer])
      .rpc();

    console.log("Initialize pool transaction:", tx);

    const poolAccount = await program.account.pool.fetch(pool);
    assert.equal(poolAccount.tokenAMint.toBase58(), tokenAMint.toBase58());
    assert.equal(poolAccount.tokenBMint.toBase58(), tokenBMint.toBase58());
    assert.equal(poolAccount.feeBps, 30);
    assert.equal(poolAccount.lpMint.toBase58(), lpMint.publicKey.toBase58());
    assert.equal(poolAccount.tokenAVault.toBase58(), tokenAVault.publicKey.toBase58());
    assert.equal(poolAccount.tokenBVault.toBase58(), tokenBVault.publicKey.toBase58());

    console.log("Pool initialized successfully");
  });

  it('Add Liquidity', async () => {
    // Create user token accounts

    // Check if user token accounts exist, create if not
    const getOrCreateATASimple = async (
      payer: anchor.web3.Signer, 
      mint: anchor.web3.PublicKey, 
      owner: anchor.web3.PublicKey
    ): Promise<anchor.web3.PublicKey> => {
      const ata = await getAssociatedTokenAddress(mint, owner);
      try {
        await getAccount(provider.connection, ata);
        console.log("got ATA: ", ata.toBase58());
      } catch {
        await createAssociatedTokenAccount(
          provider.connection,
          payer,
          mint,
          owner
        );
        console.log("created ATA: ", ata.toBase58());
      }
      return ata;
    }

    const userTokenAAccount = await getOrCreateATASimple(
      admin.payer,
      tokenAMint,
      admin.publicKey
    );

    const userTokenBAccount = await getOrCreateATASimple(
      admin.payer,
      tokenBMint,
      admin.publicKey
    );

    // Mint test tokens to user
    await mintTo(
      provider.connection,
      admin.payer,
      tokenAMint,
      userTokenAAccount,
      admin.publicKey,
      1000n * 10n ** decimals // 1000 tokens
    );

    await mintTo(
      provider.connection,
      admin.payer,
      tokenBMint,
      userTokenBAccount,
      admin.publicKey,
      1000n * 10n ** decimals // 1000 tokens
    );

    // Create user LP account
    const userLpAccount = await getOrCreateATASimple(
      admin.payer,
      lpMint.publicKey,
      admin.publicKey
    );

    // Add liquidity with all required accounts
    const tx = await program.methods
      .addLiquidity(
        new anchor.BN(100n * 10n ** decimals), // 100 token A
        new anchor.BN(100n * 10n ** decimals)  // 100 token B
      )
      .accounts({
        pool: pool,
        tokenAVault: tokenAVault.publicKey,
        tokenBVault: tokenBVault.publicKey,
        lpMint: lpMint.publicKey,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
        user: admin.publicKey,
        userTokenAAccount: userTokenAAccount,
        userTokenBAccount: userTokenBAccount,
        userLpAccount: userLpAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      // .signers([admin.payer])
      // .signers([lpMint, tokenAVault, tokenBVault])
      .signers([admin.payer])
      .rpc();

    console.log("Add liquidity transaction:", tx);

    // Check balances
    const vaultABalance = await getAccount(provider.connection, tokenAVault.publicKey,);
    const vaultBBalance = await getAccount(provider.connection, tokenBVault.publicKey,);
    const userLpBalance = await getAccount(provider.connection, userLpAccount,);
    console.log("Vault A Balance:", vaultABalance.amount.toString());
    console.log("Vault B Balance:", vaultBBalance.amount.toString());
    console.log("User LP Balance:", userLpBalance.amount.toString());

    assert.equal(vaultABalance.amount.toString(), (100n * 10n ** decimals).toString());
    assert.equal(vaultBBalance.amount.toString(), (100n * 10n ** decimals).toString());
    assert(new anchor.BN(0).lt(new anchor.BN(userLpBalance.amount)));
  });

  it('Swap Tokens', async () => {
    const userTokenAAccount = await getAssociatedTokenAddress(
      tokenAMint,
      admin.publicKey
    );

    const userTokenBAccount = await getAssociatedTokenAddress(
      tokenBMint,
      admin.publicKey
    );

    // Get balances before swap
    const beforeUserA = await getAccount(provider.connection, userTokenAAccount);
    const beforeUserB = await getAccount(provider.connection, userTokenBAccount);
    const beforeVaultA = await getAccount(provider.connection, tokenAVault.publicKey);
    const beforeVaultB = await getAccount(provider.connection, tokenBVault.publicKey);

    console.log("Before swap - User A:", beforeUserA.amount.toString());
    console.log("Before swap - User B:", beforeUserB.amount.toString());
    console.log("Before swap - Vault A:", beforeVaultA.amount.toString());
    console.log("Before swap - Vault B:", beforeVaultB.amount.toString());

    // Swap A for B
    const swapAmount = new anchor.BN(10n * 10n ** decimals); // 10 token A

    const tx = await program.methods
      .swap(
        swapAmount,
        new anchor.BN(0) // Minimum output
      )
      .accounts({
        pool: pool,
        tokenAVault: tokenAVault,
        tokenBVault: tokenBVault,
        user: admin.publicKey,
        inputMint: tokenAMint,
        outputMint: tokenBMint,
        userInputAccount: userTokenAAccount,
        userOutputAccount: userTokenBAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([admin.payer])
      .rpc();

    console.log("Swap transaction:", tx);

    // Get balances after swap
    const afterUserA = await getAccount(provider.connection, userTokenAAccount);
    const afterUserB = await getAccount(provider.connection, userTokenBAccount);
    const afterVaultA = await getAccount(provider.connection, tokenAVault.publicKey);
    const afterVaultB = await getAccount(provider.connection, tokenBVault.publicKey);

    console.log("After swap - User A:", afterUserA.amount.toString());
    console.log("After swap - User B:", afterUserB.amount.toString());
    console.log("After swap - Vault A:", afterVaultA.amount.toString());
    console.log("After swap - Vault B:", afterVaultB.amount.toString());

    // Verify swap
    const amountBeforeA = new anchor.BN(beforeUserA.amount);
    const amountAfterA = new anchor.BN(afterUserA.amount);
    const amountBeforeB = new anchor.BN(beforeUserB.amount);
    const amountAfterB = new anchor.BN(afterUserB.amount);

    const aSpent = amountBeforeA.sub(amountAfterA);
    const bReceived = amountAfterB.sub(amountBeforeB);

    console.log("A spent:", aSpent.toString());
    console.log("B received:", bReceived.toString());

    assert(aSpent.eq(swapAmount));
    assert(bReceived.gt(new anchor.BN(0)));
  });

  it('Remove Liquidity', async () => {
    const userLpAccount = await getAssociatedTokenAddress(
      lpMint.publicKey,
      admin.publicKey
    );

    const userTokenAAccount = await getAssociatedTokenAddress(
      tokenAMint,
      admin.publicKey
    );

    const userTokenBAccount = await getAssociatedTokenAddress(
      tokenBMint,
      admin.publicKey
    );

    // Get balances before removal
    const beforeLp = await getAccount(provider.connection, userLpAccount);
    const beforeUserA = await getAccount(provider.connection, userTokenAAccount);
    const beforeUserB = await getAccount(provider.connection, userTokenBAccount);
    const beforeVaultA = await getAccount(provider.connection, tokenAVault.publicKey);
    const beforeVaultB = await getAccount(provider.connection, tokenBVault.publicKey);

    console.log("Before removal - LP:", beforeLp.amount.toString());
    console.log("Before removal - Vault A:", beforeVaultA.amount.toString());
    console.log("Before removal - Vault B:", beforeVaultB.amount.toString());

    // Remove 50% of LP tokens
    const lpToRemove = new anchor.BN(beforeLp.amount).div(new anchor.BN(2));

    const tx = await program.methods
      .removeLiquidity(lpToRemove)
      .accounts({
        pool: pool,
        tokenAVault: tokenAVault,
        tokenBVault: tokenBVault,
        lpMint: lpMint,
        user: admin.publicKey,
        userLpAccount: userLpAccount,
        userTokenAAccount: userTokenAAccount,
        userTokenBAccount: userTokenBAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([admin.payer])
      .rpc();

    console.log("Remove liquidity transaction:", tx);

    // Get balances after removal
    const afterLp = await getAccount(provider.connection, userLpAccount);
    const afterUserA = await getAccount(provider.connection, userTokenAAccount);
    const afterUserB = await getAccount(provider.connection, userTokenBAccount);
    const afterVaultA = await getAccount(provider.connection, tokenAVault.publicKey);
    const afterVaultB = await getAccount(provider.connection, tokenBVault.publicKey);

    console.log("After removal - LP:", afterLp.amount.toString());
    console.log("After removal - Vault A:", afterVaultA.amount.toString());
    console.log("After removal - Vault B:", afterVaultB.amount.toString());

    // Verify removal
    const lpBurned = new anchor.BN(beforeLp.amount).sub(
      new anchor.BN(afterLp.amount));
    const aReceived = new anchor.BN(afterUserA.amount).sub(
      new anchor.BN(beforeUserA.amount));
    const bReceived = new anchor.BN(afterUserB.amount).sub(
      new anchor.BN(beforeUserB.amount));

    console.log("LP burned:", lpBurned.toString());
    console.log("A received:", aReceived.toString());
    console.log("B received:", bReceived.toString());

    assert(lpBurned.eq(lpToRemove));
    assert(aReceived.gt(new anchor.BN(0)));
    assert(bReceived.gt(new anchor.BN(0)));
  });
});