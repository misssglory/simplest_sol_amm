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
} from '@solana/spl-token';
import { assert } from 'chai';

describe('amm', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  
  const program = anchor.workspace.Amm as Program<Amm>;
  const admin = provider.wallet;
  
  let tokenAMint: anchor.web3.PublicKey;
  let tokenBMint: anchor.web3.PublicKey;
  let pool: anchor.web3.PublicKey;
  let poolBump: number;
  let lpMint: anchor.web3.PublicKey;
  let tokenAVault: anchor.web3.PublicKey;
  let tokenBVault: anchor.web3.PublicKey;

  before(async () => {
    // Create test tokens
    tokenAMint = await createMint(
      provider.connection,
      admin.payer,
      admin.publicKey,
      null,
      6
    );
    
    tokenBMint = await createMint(
      provider.connection,
      admin.payer,
      admin.publicKey,
      null,
      6
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
  });

  it('Initialize AMM Pool', async () => {
    // Get vault addresses
    tokenAVault = await getAssociatedTokenAddress(tokenAMint, pool, true);
    tokenBVault = await getAssociatedTokenAddress(tokenBMint, pool, true);
    
    // Initialize pool
    const tx = await program.methods
      .initializePool(30) // 0.3% fee
      .accounts({
        pool: pool,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
        tokenAVault: tokenAVault,
        tokenBVault: tokenBVault,
        admin: admin.publicKey,
      })
      .signers([admin.payer])
      .rpc();
    
    console.log("Initialize pool transaction:", tx);
    
    // Fetch pool account
    const poolAccount = await program.account.pool.fetch(pool);
    assert.equal(poolAccount.tokenAMint.toBase58(), tokenAMint.toBase58());
    assert.equal(poolAccount.tokenBMint.toBase58(), tokenBMint.toBase58());
    assert.equal(poolAccount.feeBps, 30);
    
    // Get LP mint from pool
    lpMint = poolAccount.lpMint;
    console.log("LP Mint:", lpMint.toBase58());
  });

  it('Add Liquidity', async () => {
    // Create user token accounts
    const userTokenAAccount = await getAssociatedTokenAddress(
      tokenAMint,
      admin.publicKey
    );
    
    const userTokenBAccount = await getAssociatedTokenAddress(
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
      1000 * 10 ** 6 // 1000 tokens
    );
    
    await mintTo(
      provider.connection,
      admin.payer,
      tokenBMint,
      userTokenBAccount,
      admin.publicKey,
      1000 * 10 ** 6 // 1000 tokens
    );
    
    // Create user LP account
    const userLpAccount = await getAssociatedTokenAddress(
      lpMint,
      admin.publicKey
    );
    
    // Add liquidity
    const tx = await program.methods
      .addLiquidity(
        new anchor.BN(100 * 10 ** 6), // 100 token A
        new anchor.BN(100 * 10 ** 6)  // 100 token B
      )
      .accounts({
        pool: pool,
        tokenAVault: tokenAVault,
        tokenBVault: tokenBVault,
        lpMint: lpMint,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
        user: admin.publicKey,
        userTokenAAccount: userTokenAAccount,
        userTokenBAccount: userTokenBAccount,
        userLpAccount: userLpAccount,
      })
      .signers([admin.payer])
      .rpc();
    
    console.log("Add liquidity transaction:", tx);
    
    // Check balances
    const vaultABalance = await getAccount(provider.connection, tokenAVault);
    const vaultBBalance = await getAccount(provider.connection, tokenBVault);
    const userLpBalance = await getAccount(provider.connection, userLpAccount);
    
    console.log("Vault A Balance:", vaultABalance.amount.toString());
    console.log("Vault B Balance:", vaultBBalance.amount.toString());
    console.log("User LP Balance:", userLpBalance.amount.toString());
    
    assert.equal(vaultABalance.amount.toString(), (100 * 10 ** 6).toString());
    assert.equal(vaultBBalance.amount.toString(), (100 * 10 ** 6).toString());
    assert(userLpBalance.amount.gt(new anchor.BN(0)));
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
    const beforeVaultA = await getAccount(provider.connection, tokenAVault);
    const beforeVaultB = await getAccount(provider.connection, tokenBVault);
    
    console.log("Before swap - User A:", beforeUserA.amount.toString());
    console.log("Before swap - User B:", beforeUserB.amount.toString());
    console.log("Before swap - Vault A:", beforeVaultA.amount.toString());
    console.log("Before swap - Vault B:", beforeVaultB.amount.toString());
    
    // Swap A for B
    const swapAmount = new anchor.BN(10 * 10 ** 6); // 10 token A
    
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
      })
      .signers([admin.payer])
      .rpc();
    
    console.log("Swap transaction:", tx);
    
    // Get balances after swap
    const afterUserA = await getAccount(provider.connection, userTokenAAccount);
    const afterUserB = await getAccount(provider.connection, userTokenBAccount);
    const afterVaultA = await getAccount(provider.connection, tokenAVault);
    const afterVaultB = await getAccount(provider.connection, tokenBVault);
    
    console.log("After swap - User A:", afterUserA.amount.toString());
    console.log("After swap - User B:", afterUserB.amount.toString());
    console.log("After swap - Vault A:", afterVaultA.amount.toString());
    console.log("After swap - Vault B:", afterVaultB.amount.toString());
    
    // Verify swap
    const aSpent = beforeUserA.amount.sub(afterUserA.amount);
    const bReceived = afterUserB.amount.sub(beforeUserB.amount);
    
    console.log("A spent:", aSpent.toString());
    console.log("B received:", bReceived.toString());
    
    assert(aSpent.eq(swapAmount));
    assert(bReceived.gt(new anchor.BN(0)));
  });

  it('Remove Liquidity', async () => {
    const userLpAccount = await getAssociatedTokenAddress(
      lpMint,
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
    const beforeVaultA = await getAccount(provider.connection, tokenAVault);
    const beforeVaultB = await getAccount(provider.connection, tokenBVault);
    
    console.log("Before removal - LP:", beforeLp.amount.toString());
    console.log("Before removal - Vault A:", beforeVaultA.amount.toString());
    console.log("Before removal - Vault B:", beforeVaultB.amount.toString());
    
    // Remove 50% of LP tokens
    const lpToRemove = beforeLp.amount.div(new anchor.BN(2));
    
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
      })
      .signers([admin.payer])
      .rpc();
    
    console.log("Remove liquidity transaction:", tx);
    
    // Get balances after removal
    const afterLp = await getAccount(provider.connection, userLpAccount);
    const afterUserA = await getAccount(provider.connection, userTokenAAccount);
    const afterUserB = await getAccount(provider.connection, userTokenBAccount);
    const afterVaultA = await getAccount(provider.connection, tokenAVault);
    const afterVaultB = await getAccount(provider.connection, tokenBVault);
    
    console.log("After removal - LP:", afterLp.amount.toString());
    console.log("After removal - Vault A:", afterVaultA.amount.toString());
    console.log("After removal - Vault B:", afterVaultB.amount.toString());
    
    // Verify removal
    const lpBurned = beforeLp.amount.sub(afterLp.amount);
    const aReceived = afterUserA.amount.sub(beforeUserA.amount);
    const bReceived = afterUserB.amount.sub(beforeUserB.amount);
    
    console.log("LP burned:", lpBurned.toString());
    console.log("A received:", aReceived.toString());
    console.log("B received:", bReceived.toString());
    
    assert(lpBurned.eq(lpToRemove));
    assert(aReceived.gt(new anchor.BN(0)));
    assert(bReceived.gt(new anchor.BN(0)));
  });
});