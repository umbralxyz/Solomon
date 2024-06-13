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
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let user: anchor.web3.Keypair;
  let vaultAuthority: anchor.web3.Keypair;
  let vaultUnstaked: anchor.web3.PublicKey;
  let userUnstaked: anchor.web3.PublicKey;
  let userStaked: anchor.web3.PublicKey;
  let unstakedMint: anchor.web3.Keypair;
  const salt: number[] = Array.from({ length: 8 }, () => Math.floor(Math.random() * 256));
  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state"), Buffer.from(salt)],
    program.programId
  );
  const newCD = 3;

  before(async () => {
    user = anchor.web3.Keypair.generate();
    console.log("User key: ", user.publicKey.toString());
    vaultAuthority = anchor.web3.Keypair.generate();
    unstakedMint = anchor.web3.Keypair.generate();

    await anchor.AnchorProvider.env().connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
  });

  it("Initialize vault, add user, set cooldown", async () => {
    vaultUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, vaultAuthority.publicKey);
    userUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, user.publicKey);

    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(MINT_SIZE);
    // Create mint for UserToken (unstaked token)
    const unstakedMintTx = new anchor.web3.Transaction().add(
      // Create an account from the user mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: unstakedMint.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create unstaked mint account that is controlled by anchor wallet
      createInitializeMintInstruction(unstakedMint.publicKey, 0, adminKey, adminKey),
      // Create the ATA account that is associated with unstaked mint on anchor wallet
      createAssociatedTokenAccountInstruction(adminKey, userUnstaked, user.publicKey, unstakedMint.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(unstakedMintTx, [unstakedMint]);

    const mintAmount = 10000000;

    // Mint unstaked tokens to user
    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(unstakedMint.publicKey, userUnstaked, adminKey, mintAmount),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    const depositerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const userUnstakedBal = depositerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("Admin: ", adminKey.toString());

    // Initialize staking vault and token accounts
    await program.methods.initializeVaultState(adminKey, new anchor.BN(100000), salt).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));

    await program.methods.initializeProgramAccounts(salt).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));

    // Add user
    await program.methods.initializeUserAccount(salt).accounts({
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    // Initialize user staked account
    const [stakingTokenPDA, stakingTokenBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("staking-token"), vaultStatePDA.toBuffer()],
      program.programId
    );

    userStaked = await getAssociatedTokenAddress(stakingTokenPDA, user.publicKey);
    const initUserStaked = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, userStaked, user.publicKey, stakingTokenPDA),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(initUserStaked, []);

    await program.methods.setCooldownDuration(new anchor.BN(newCD), salt).rpc();
    console.log("Set staking cooldown to: ", newCD.toString());
  });
  
  
  it("Stake test", async () => {
    const stake = new anchor.BN(1000000);

    // Get balances of user before
    let callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Stake as user
    await program.methods.stake(stake, salt).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    // Get balances of user after
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

    // Add rewarder
    await program.methods.addRewarder(user.publicKey, salt).accounts({
      caller: adminKey
    }).rpc().catch(e => console.error(e));

    console.log("Added rewarder: ", user.publicKey.toString());

    // Reward unstaked tokens to vault as user
    await program.methods.reward(reward, salt).accounts({
      callerTokenAccount: userUnstaked,
      caller: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    console.log("Rewarded tokens: ", reward.toString());

    // Remove rewarder
    await program.methods.removeRewarder(user.publicKey, salt).accounts({
      caller: adminKey
    }).rpc().catch(e => console.error(e));

    console.log("Removed rewarder: ", user.publicKey.toString());
  });

  
  it("Unstake test", async () => {
    // Get balances of user before
    let callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Unstake as user before CD
    await program.methods.unstake(salt).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey
    }).signers([user]).rpc().catch(e => console.error(e));

    // Get balances of user after (no deposits withdrawn because insufficent time has passed)
    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    await sleep(1000 * (newCD));

    // Unstake as user after CD
    await program.methods.unstake(salt).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey
    }).signers([user]).rpc().catch(e => console.error(e));

    // Get balances of user after cooldown has passed
    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfterCD = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfterCD = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("User unstaked tokens before unstaking: ", unstakedBefore);
    console.log("User staked tokens before unstaking: ", stakedBefore);
    console.log("User unstaked tokens after unstaking (before CD end): ", unstakedAfter);
    console.log("User staked tokens after unstaking (before CD end): ", stakedAfter);
    console.log("User unstaked tokens after unstaking (after CD end): ", unstakedAfterCD);
    console.log("User staked tokens after unstaking (after CD end): ", stakedAfterCD);
  });
  
  it("Transfer admin back and forth", async () => {
    await program.methods.transferAdmin(user.publicKey, salt).rpc()
    console.log("Transfered admin to: ", user.publicKey.toString());

    await program.methods.transferAdmin(adminKey, salt).accounts({
      caller: user.publicKey,
    }).signers([user]).rpc();
    console.log("Transfered admin back to: ", adminKey.toString());
  });
});

function sleep(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms));
}