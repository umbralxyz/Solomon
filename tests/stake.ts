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
  //console.log("Salt: ", salt);

  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state"), Buffer.from(salt)],
    program.programId
  );

  const [vaultTokenAccountPDA, vaultTokenAccountBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-token-account")],
    program.programId
  );

  before(async () => {
    user = anchor.web3.Keypair.generate();
    console.log("User key: ", user.publicKey);
    vaultAuthority = anchor.web3.Keypair.generate();
    unstakedMint = anchor.web3.Keypair.generate();

    await anchor.AnchorProvider.env().connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    
  });

  it("Initialize vault and add user", async () => {
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

    const mintAmount = 10000000;

    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(unstakedMint.publicKey, userUnstaked, adminKey, mintAmount),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    const depositerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const userUnstakedBal = depositerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("admin: ", adminKey);

    // Initialize staking vault
    await program.methods.initializeVaultState(adminKey, new anchor.BN(0)).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));

    // Add user
    await program.methods.initializeUserAccount().accounts({
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    // Initialize user staked account
    const [stakingTokenPDA, stakingTokenBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("staking-token"),
        //adminKey.toBuffer(), 
        //unstakedMint.publicKey.toBuffer()
      ],
      program.programId
    );

    userStaked = await getAssociatedTokenAddress(stakingTokenPDA, user.publicKey);
    const initUserStaked = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, userStaked, user.publicKey, stakingTokenPDA),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(initUserStaked, []);

    const stakingTokenAccountInfo = await program.provider.connection.getAccountInfo(stakingTokenPDA);
    if (stakingTokenAccountInfo) {
      console.log("Staking token account is initialized");
    }
  });
  
  
  it("Stake test", async () => {
    const stake = new anchor.BN(1000000);

    let callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    await program.methods.stake(stake).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("User unstaked tokens before staking: ", unstakedBefore);
    console.log("User staked tokens before staking: ", stakedBefore);
    console.log("User unstaked tokens after staking: ", unstakedAfter);
    console.log("User staked tokens after staking: ", stakedAfter);
  });

  it("Reward test", async () => {
    const reward = new anchor.BN(1000000);

    await program.methods.addRewarder(user.publicKey).accounts({
      caller: adminKey
    }).rpc().catch(e => console.error(e));

    console.log("Added rewarder: ", user.publicKey);

    await program.methods.reward(reward).accounts({
      callerTokenAccount: userUnstaked,
      caller: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    console.log("Rewarded tokens: ", reward);
  });

  it("Unstake test", async () => {
    let callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    await program.methods.unstake().accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("User unstaked tokens before unstaking: ", unstakedBefore);
    console.log("User staked tokens before unstaking: ", stakedBefore);
    console.log("User unstaked tokens after unstaking: ", unstakedAfter);
    console.log("User staked tokens after unstaking: ", stakedAfter);
  });
});
