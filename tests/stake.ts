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
  createTransferInstruction,
} from "@solana/spl-token"; 
import { assert } from "chai";
import { BN } from "bn.js";

describe("stake", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.Stake as Program<Stake>;
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let vaultAuthority: anchor.web3.Keypair;
  let vaultUnstaked: anchor.web3.PublicKey;
  let user: anchor.web3.Keypair;
  let userUnstaked: anchor.web3.PublicKey;
  let userStaked: anchor.web3.PublicKey;
  let userTwo: anchor.web3.Keypair;
  let userTwoUnstaked: anchor.web3.PublicKey;
  let userTwoStaked: anchor.web3.PublicKey;
  let unstakedMint: anchor.web3.Keypair;
  const salt: number[] = Array.from({ length: 8 }, () => Math.floor(Math.random() * 256));
  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state"), Buffer.from(salt)],
    program.programId
  );
  const [stakingTokenPDA, stakingTokenBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("staking-token"), vaultStatePDA.toBuffer()],
    program.programId
  );
  const cd = 1;
  const vestingPeriod = 10;
  const minShares = new anchor.BN(100);

  before(async () => {
    user = anchor.web3.Keypair.generate();
    userTwo = anchor.web3.Keypair.generate();
    console.log("User one key: ", user.publicKey.toString());
    console.log("User two key: ", userTwo.publicKey.toString());
    vaultAuthority = anchor.web3.Keypair.generate();
    unstakedMint = anchor.web3.Keypair.generate();

    await anchor.AnchorProvider.env().connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(userTwo.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
  });

  it("Initialize vault, add user, set cooldown", async () => {
    vaultUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, vaultAuthority.publicKey);
    userUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, user.publicKey);
    userTwoUnstaked = await getAssociatedTokenAddress(unstakedMint.publicKey, userTwo.publicKey);

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
      // Create user one unstaked ATA
      createAssociatedTokenAccountInstruction(adminKey, userUnstaked, user.publicKey, unstakedMint.publicKey),
      // Create user two unstaked ATA
      createAssociatedTokenAccountInstruction(adminKey, userTwoUnstaked, userTwo.publicKey, unstakedMint.publicKey),
    );
    await anchor.AnchorProvider.env().sendAndConfirm(unstakedMintTx, [unstakedMint]);

    // Mint unstaked tokens to user
    const mintAmount = 100000;
    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(unstakedMint.publicKey, userUnstaked, adminKey, mintAmount),
    );
    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    // Initialize staking vault and token accounts
    await program.methods.initializeVaultState(adminKey, salt, 0, minShares).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));
    console.log("Admin: ", adminKey.toString());

    await program.methods.initializeProgramAccounts(salt).accounts({
      depositToken: unstakedMint.publicKey,
      caller: adminKey,
    }).rpc().catch(e => console.error(e));

    // Initialize user staked account
    userStaked = await getAssociatedTokenAddress(stakingTokenPDA, user.publicKey);
    const initUserStaked = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, userStaked, user.publicKey, stakingTokenPDA),
    );
    await anchor.AnchorProvider.env().sendAndConfirm(initUserStaked, []);

    // Initialize user two staked account
    userTwoStaked = await getAssociatedTokenAddress(stakingTokenPDA, userTwo.publicKey);
    const initUserTwoStaked = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, userTwoStaked, userTwo.publicKey, stakingTokenPDA),
    );
    await anchor.AnchorProvider.env().sendAndConfirm(initUserTwoStaked, []);

    // Set CD and vesting period
    await program.methods.setCooldown(salt, cd).rpc();
    console.log("Set staking cooldown to: ", cd.toString());

    await program.methods.setVestingPeriod(salt, vestingPeriod).rpc();
    console.log("Set vesting period to: ", vestingPeriod.toString());
  });
  
  
  it("Stake test", async () => {
    const one = new anchor.BN(1)
    const stake = new anchor.BN(99999);

    let mintAccountInfo = await program.provider.connection.getParsedAccountInfo(stakingTokenPDA);
    let totalSupply = mintAccountInfo.value.data.parsed.info.supply;
    console.log("Total supply of staking token before staking: ", totalSupply);

    // Get balances of user before
    let callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Stake as user
    await program.methods.stake(salt, one).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    // Stake as user
    await program.methods.stake(salt, stake).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey,
    }).signers([user]).rpc().catch(e => console.error(e));

    // Get balances of user after
    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    mintAccountInfo = await program.provider.connection.getParsedAccountInfo(stakingTokenPDA);
    totalSupply = mintAccountInfo.value.data.parsed.info.supply;

    console.log("User unstaked tokens before staking: ", unstakedBefore);
    console.log("User staked tokens before staking: ", stakedBefore);
    console.log("Total supply of staking token after staking: ", totalSupply);
    console.log("User unstaked tokens after staking: ", unstakedAfter);
    console.log("User staked tokens after staking: ", stakedAfter);
  });

  it("Reward test", async () => {
    const reward = new anchor.BN(10000);

    // Add rewarder
    await program.methods.addRewarder(user.publicKey, salt).accounts({
      caller: adminKey
    }).rpc().catch(e => console.error(e));

    console.log("Added rewarder: ", user.publicKey.toString());

    // Mint collat tokens to user one for rewarding to vault
    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(unstakedMint.publicKey, userUnstaked, adminKey, 10000),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

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



    const unstake = new anchor.BN(50000);
    const unstakeTwo = new anchor.BN(25000);

    //await sleep(1000 * (newCD));

    // Unstake half as user one
    await program.methods.startUnstake(salt, unstake).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey
    }).signers([user]).rpc().catch(e => console.error(e));

    // Should fail when commented out or removed
    await sleep(1000 * (cd));

    await program.methods.unstake(salt, unstake).accounts({
      userDepositTokenAccount: userUnstaked,
      userStakingTokenAccount: userStaked,
      user: user.publicKey
    }).signers([user]).rpc().catch(e => console.error(e));

    // Get balances of user after (no deposits withdrawn because insufficent time has passed)
    callerInfo = await program.provider.connection.getParsedAccountInfo(userUnstaked);
    const unstakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userStaked);
    const stakedAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Transfer other half to user two
    const transfer = new anchor.web3.Transaction().add(
      createTransferInstruction(userStaked, userTwoStaked, user.publicKey, unstakedAfter)
    );
    await anchor.AnchorProvider.env().sendAndConfirm(transfer, [user]);

    // Get balances of user two after transfer / before unstake
    callerInfo = await program.provider.connection.getParsedAccountInfo(userTwoUnstaked);
    const unstakedTwoBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userTwoStaked);
    const stakedTwoBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Unstake some as user two
    await program.methods.startUnstake(salt, unstakeTwo).accounts({
      userDepositTokenAccount: userTwoUnstaked,
      userStakingTokenAccount: userTwoStaked,
      user: userTwo.publicKey
    }).signers([userTwo]).rpc().catch(e => console.error(e));

    // Should fail when commented out or removed
    await sleep(1000 * (cd));

    await program.methods.unstake(salt, unstakeTwo).accounts({
      userDepositTokenAccount: userTwoUnstaked,
      userStakingTokenAccount: userTwoStaked,
      user: userTwo.publicKey
    }).signers([userTwo]).rpc().catch(e => console.error(e));

    // Unstake remainder as user two (should fail due to cooldown)
    await program.methods.startUnstake(salt, unstakeTwo).accounts({
      userDepositTokenAccount: userTwoUnstaked,
      userStakingTokenAccount: userTwoStaked,
      user: userTwo.publicKey
    }).signers([userTwo]).rpc().catch(e => console.error(e));

    // Should fail when commented out or removed
    await sleep(1000 * (cd));

    await program.methods.unstake(salt, unstakeTwo).accounts({
      userDepositTokenAccount: userTwoUnstaked,
      userStakingTokenAccount: userTwoStaked,
      user: userTwo.publicKey
    }).signers([userTwo]).rpc().catch(e => console.error(e));

    // Get balances of user after cooldown has passed
    callerInfo = await program.provider.connection.getParsedAccountInfo(userTwoUnstaked);
    const unstakedTwoAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userTwoStaked);
    const stakedTwoAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("User one unstaked tokens before unstaking: ", unstakedBefore);
    console.log("User one staked tokens before unstaking: ", stakedBefore);
    console.log("User one unstaked tokens after unstaking: ", unstakedAfter);
    console.log("User one staked tokens after unstaking: ", stakedAfter);
    console.log("User two unstaked tokens before unstaking: ", unstakedTwoBefore);
    console.log("User two staked tokens before unstaking: ", stakedTwoBefore);
    console.log("User two unstaked tokens after unstaking: ", unstakedTwoAfter);
    console.log("User two staked tokens after unstaking: ", stakedTwoAfter);
  });
});

function sleep(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms));
}