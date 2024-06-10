import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Stake } from "../target/types/stake";
import {
  TOKEN_PROGRAM_ID,
  MINT_SIZE,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddress,
  createInitializeMintInstruction,
  createMintToInstruction,
} from "@solana/spl-token"; 
import { assert } from "chai";
import { BN } from "bn.js";

describe("stake", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.Stake as Program<Stake>;
  let vaultKey = anchor.web3.Keypair.generate();
  let associatedTokenAccount = undefined;
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let user: anchor.web3.Keypair;
  let vaultAuthority: anchor.web3.Keypair;
  let vaultUnstaked: anchor.web3.PublicKey;
  let vaultStaked: anchor.web3.PublicKey;
  let userUnstaked: anchor.web3.PublicKey;
  let userStaked: anchor.web3.PublicKey;
  let userPDA: anchor.web3.PublicKey;
  let userPDABump: number;
  let unstakedMint: anchor.web3.Keypair;
  const salt: number[] = Array.from({ length: 32 }, () => Math.floor(Math.random() * 256));
  console.log("Salt: ", salt);

  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state"), Buffer.from(salt)],
    program.programId
  );

  const [stakingTokenPDA, stakingTokenBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("staking-token")],
    program.programId
  );

  const [vaultTokenAccountPDA, vaultTokenAccountBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-token-account")],
    program.programId
  );

  before(async () => {
    user = anchor.web3.Keypair.generate();
    vaultAuthority = anchor.web3.Keypair.generate();
    unstakedMint = anchor.web3.Keypair.generate();

    await anchor.AnchorProvider.env().connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    
  });

  it("Initialize vault and add users", async () => {
    vaultUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, vaultAuthority.publicKey);
    userUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, user.publicKey);
    //let callerStaked = await getAssociatedTokenAddress(vaultMint, user.publicKey);

    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(MINT_SIZE);
    // Create mint for UserToken
    const unstakedMintTx = new anchor.web3.Transaction().add(
      // Create an account from the user mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: unstakedMint.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create collat mint account that is controlled by anchor wallet
      createInitializeMintInstruction(unstakedMint.publicKey, 0, adminKey, adminKey),
      // Create the ATA account that is associated with collat mint on anchor wallet
      createAssociatedTokenAccountInstruction(adminKey, userUnstaked, user.publicKey, unstakedMint.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(unstakedMintTx, [unstakedMint]);

    console.log("admin: ", adminKey);
    await program.methods.initializeVaultState(adminKey, new anchor.BN(10), salt).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));
  });
  /*
  it("Initialize vault and mint staked tokens", async () => {
    const key = adminKey;
    vaultKey = anchor.web3.Keypair.generate();

    // Get the amount of SOL needed to pay rent for our Token Mint
    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(
      MINT_SIZE
    );

    // Get the ATA for a token and the account that will own the ATA
    associatedTokenAccount = await getAssociatedTokenAddress(
      vaultKey.publicKey,
      key
    );

    // Check if the account already exists
    const accountInfo = await program.provider.connection.getAccountInfo(vaultKey.publicKey);
    if (!accountInfo) {
      const mint_tx = new anchor.web3.Transaction().add(
        // Create an account from the mint key
        anchor.web3.SystemProgram.createAccount({
          fromPubkey: key,
          newAccountPubkey: vaultKey.publicKey,
          space: MINT_SIZE,
          programId: TOKEN_PROGRAM_ID,
          lamports,
        }),
        // Create mint account that is controlled by anchor wallet
        createInitializeMintInstruction(
          vaultKey.publicKey, 0, key, key
        ),
        // Create the ATA account that is associated with mint on anchor wallet
        createAssociatedTokenAccountInstruction(
          key, associatedTokenAccount, key, vaultKey.publicKey
        )
      );

      await anchor.AnchorProvider.env().sendAndConfirm(mint_tx, [vaultKey]);

      console.log(
        await program.provider.connection.getParsedAccountInfo(vaultKey.publicKey)
      );

      console.log("Account: ", vaultKey.publicKey.toString());
      console.log("Staking Vault key: ", vaultKey.publicKey.toString());
      console.log("User: ", key.toString());
    }

    // Mint tokens to the associated token account
    const tx = await program.methods.mintStakedToken(new anchor.BN(10)).accounts({
      recipient: associatedTokenAccount,
      authority: key,
    }).rpc();

    console.log("StakedToken minting signature: ", tx);

    // Get minted token amount on the ATA for anchor wallet
    const mintedAccountInfo = await program.provider.connection.getParsedAccountInfo(associatedTokenAccount);
    if (mintedAccountInfo.value && mintedAccountInfo.value.data.parsed) {
      const minted = mintedAccountInfo.value.data.parsed.info.tokenAmount.amount;
      assert.equal(minted, 10);
    } else {
      throw new Error("Failed to retrieve parsed account data");
    }

    // Create mint for UserToken
    const userTokenAccountInfo = await program.provider.connection.getAccountInfo(user.publicKey);
    if (!userTokenAccountInfo) {
      const userTokenMintTx = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.createAccount({
          fromPubkey: adminKey,
          newAccountPubkey: user.publicKey,
          space: MINT_SIZE,
          programId: TOKEN_PROGRAM_ID,
          lamports,
        }),
        createInitializeMintInstruction(user.publicKey, 0, adminKey, adminKey)
      );

      await anchor.AnchorProvider.env().sendAndConfirm(userTokenMintTx, [user]);
    }

    vaultUnstaked = vaultAuthority.publicKey;
    vaultStaked = await getAssociatedTokenAddress(vaultKey.publicKey, vaultAuthority.publicKey);
    userUnstaked = user.publicKey;
    userStaked = await getAssociatedTokenAddress(vaultKey.publicKey, user.publicKey);

    const createVaultsTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, vaultStaked, vaultAuthority.publicKey, vaultKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, userStaked, user.publicKey, vaultKey.publicKey)
    );

    await anchor.AnchorProvider.env().sendAndConfirm(createVaultsTx, []);
    
    // Mint tokens to user
    const mint_tx = await program.methods.mintStakedToken(new anchor.BN(10)).accounts({
      recipient: userStaked,
      authority: key,
    }).rpc();

    console.log("UserToken minting signature: ", mint_tx);
  });

  it("Initialize user account", async () => {
    [userPDA, userPDABump] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("user_data"), user.publicKey.toBuffer()],
      program.programId
    );

    const initializeUserTx = await program.methods.initializeUserAccount().accounts({
      user: user.publicKey,
    }).signers([user]).rpc();

    console.log("User account initialization signature: ", initializeUserTx);
  });
  /*
  it("Stake and reward test", async () => {
    const amt = new anchor.BN(1);

    const stakeTx = await program.methods.stake(amt).accounts({
      userStakedAccount: userStaked,
      userTokenAccount: userStaked,  
      vaultStakedAccount: vaultStaked,
      vaultTokenAccount: vaultStaked,
      mint: vaultKey.publicKey,
      vaultState: vaultStatePDA,
      userData: userPDA,
      user: user.publicKey,
      vault: adminKey,
    }).signers([user]).rpc();

    console.log("Stake signature: ", stakeTx);

    await program.methods.addRewarder(associatedTokenAccount).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Added rewarder: ", associatedTokenAccount);

    // TODO: figure out why rewarder not found
    const rewardTx = await program.methods.reward(amt).accounts({
      vaultState: vaultStatePDA,
      vaultTokenAccount: vaultStaked,
      callerTokenAccount: associatedTokenAccount,
    }).signers([]).rpc();

    console.log("Reward signature: ", rewardTx);
  });

  it("Unstake test", async () => {


    await program.methods.removeRewarder(adminKey).rpc()
    console.log("Removed rewarder: ", adminKey);

    await program.methods.addRewarder(adminKey).rpc()
    console.log("Added rewarder: ", adminKey);

    const userStakedInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const vaultStakedInfo = await program.provider.connection.getParsedAccountInfo(vaultStaked);

    console.log("User Staked Account Info: ", userStakedInfo);
    console.log("Vault Staked Account Info: ", vaultStakedInfo);
    const stakeTx = await program.methods.unstake().accounts({
      userStakedAccount: userStaked,
      userTokenAccount: userStaked,  
      vaultStakedAccount: vaultStaked,
      vaultTokenAccount: vaultStaked,
      vaultState: vaultStatePDA,
      userData: userPDA,
      user: user.publicKey,
      vault: vaultAuthority.publicKey,
      stakedMint: vaultKey.publicKey,
    }).signers([user, vaultAuthority]).rpc();
  });
  */
});
