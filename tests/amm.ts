import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { Amm } from '../target/types/amm';
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  createAccount, 
  mintTo,
  getAccount,
  getAssociatedTokenAddress,
} from '@solana/spl-token';

describe('amm', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  
  const program = anchor.workspace.Amm as Program<Amm>;
  
  let tokenAMint: anchor.web3.PublicKey;
  let tokenBMint: anchor.web3.PublicKey;
  let pool: anchor.web3.PublicKey;
  
  it('Initialize AMM Pool', async () => {
    // Create token mints
    tokenAMint = await createMint(
      provider.connection,
      provider.wallet.payer,
      provider.wallet.publicKey,
      null,
      6
    );
    
    tokenBMint = await createMint(
      provider.connection,
      provider.wallet.payer,
      provider.wallet.publicKey,
      null,
      6
    );
    
    const [poolPda] = await anchor.web3.PublicKey.findProgramAddress(
      [
        Buffer.from("pool"),
        tokenAMint.toBuffer(),
        tokenBMint.toBuffer(),
      ],
      program.programId
    );
    
    pool = poolPda;
    
    // Initialize pool
    await program.methods
      .initializePool(30) // 0.3% fee
      .accounts({
        pool: pool,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
        admin: provider.wallet.publicKey,
      })
      .rpc();
  });
  
  it('Add Liquidity', async () => {
    const userTokenAAccount = await getAssociatedTokenAddress(
      tokenAMint,
      provider.wallet.publicKey
    );
    
    const userTokenBAccount = await getAssociatedTokenAddress(
      tokenBMint,
      provider.wallet.publicKey
    );
    
    // Mint tokens to user
    await mintTo(
      provider.connection,
      provider.wallet.payer,
      tokenAMint,
      userTokenAAccount,
      provider.wallet.publicKey,
      1000000000 // 1000 tokens
    );
    
    await mintTo(
      provider.connection,
      provider.wallet.payer,
      tokenBMint,
      userTokenBAccount,
      provider.wallet.publicKey,
      1000000000 // 1000 tokens
    );
    
    // Add liquidity
    await program.methods
      .addLiquidity(
        new anchor.BN(100000000), // 100 token A
        new anchor.BN(100000000)  // 100 token B
      )
      .accounts({
        pool: pool,
        user: provider.wallet.publicKey,
        tokenAMint: tokenAMint,
        tokenBMint: tokenBMint,
      })
      .rpc();
  });
  
  it('Swap Tokens', async () => {
    const userTokenAAccount = await getAssociatedTokenAddress(
      tokenAMint,
      provider.wallet.publicKey
    );
    
    const userTokenBAccount = await getAssociatedTokenAddress(
      tokenBMint,
      provider.wallet.publicKey
    );
    
    await program.methods
      .swap(
        new anchor.BN(1000000), // 1 token A in
        new anchor.BN(990000)   // minimum 0.99 token B out
      )
      .accounts({
        pool: pool,
        user: provider.wallet.publicKey,
        inputMint: tokenAMint,
        outputMint: tokenBMint,
        userInputAccount: userTokenAAccount,
        userOutputAccount: userTokenBAccount,
      })
      .rpc();
  });
  
  it('Remove Liquidity', async () => {
    const userLpAccount = await getAssociatedTokenAddress(
      (await program.account.pool.fetch(pool)).lpMint,
      provider.wallet.publicKey
    );
    
    await program.methods
      .removeLiquidity(new anchor.BN(50000000)) // Remove 50 LP tokens
      .accounts({
        pool: pool,
        user: provider.wallet.publicKey,
      })
      .rpc();
  });
});