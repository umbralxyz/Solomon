import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Stake } from "../target/types/stake";
import {
  TOKEN_PROGRAM_ID,
  MINT_SIZE,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddress,
  createInitializeMintInstruction,
} from "@solana/spl-token"; 
import { assert } from "chai";
import { BN } from "bn.js";
import { publicKey } from "@coral-xyz/anchor/dist/cjs/utils";

describe("stake", () => {
  // Configure client to use local cluster.
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.Stake as Program<Stake>;
  // Generate a random keypair to represent token
  let vaultKey = anchor.web3.Keypair.generate();
  let associatedTokenAccount = undefined;
  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state")],
    program.programId
  );
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let user: anchor.web3.Keypair;
  let vaultAuthority: anchor.web3.Keypair;
  let vaultUnstaked: anchor.web3.PublicKey;
  let vaultStaked: anchor.web3.PublicKey;
  let userUnstaked: anchor.web3.PublicKey;
  let userStaked: anchor.web3.PublicKey;


  before(async () => {
    // Initialize any necessary setup before tests
    user = anchor.web3.Keypair.generate();
    vaultAuthority = anchor.web3.Keypair.generate();

    // Fund user and vaultAuthority accounts
    await anchor.AnchorProvider.env().connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Initialize mint state account
    console.log("admin: ", adminKey);
    await program.methods.initializeVaultState(new BN(0), TOKEN_PROGRAM_ID).rpc()

    // Remove admin from rewarders (added by default)
    await program.methods.removeRewarder(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Removed rewarder: ", adminKey);

    // Add admin back as rewarder
    await program.methods.addRewarder(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Added rewarder: ", adminKey);
  });
 
  it("Initialize vault and mint staked tokens", async () => {
    // Get anchor's wallet's public key
    const key = adminKey;

    // Create a new mint keypair
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

    const res = await anchor.AnchorProvider.env().sendAndConfirm(mint_tx, [vaultKey]);

    console.log(
      await program.provider.connection.getParsedAccountInfo(vaultKey.publicKey)
    );

    console.log("Account: ", res);
    console.log("Staking Vault key: ", vaultKey.publicKey.toString());
    console.log("User: ", key.toString());

    // Executes code to mint token into specified ATA
    const tx = await program.methods.mintStakedToken(new anchor.BN(10)).accounts({
      mint: vaultKey.publicKey,
      recipient: associatedTokenAccount,
      authority: key,
      vaultState: vaultStatePDA,
    }).rpc();

    console.log("StakedToken minting signature: ", tx);

    // Get minted token amount on the ATA for anchor wallet
    const accountInfo = await program.provider.connection.getParsedAccountInfo(associatedTokenAccount);
    if (accountInfo.value && accountInfo.value.data.parsed) {
      const minted = accountInfo.value.data.parsed.info.tokenAmount.amount;
      assert.equal(minted, 10);
    } else {
      throw new Error("Failed to retrieve parsed account data");
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
  });

  it("Stake test", async () => {
    const amt = new anchor.BN(1);

    const stakeTx = await program.methods.stake(amt).accounts({
      userStakedAccount: userStaked,
      userTokenAccount: userUnstaked,
      vaultStakedAccount: vaultKey.publicKey,
      vaultTokenAccount: vaultAuthority.publicKey,
      mint: vaultKey.publicKey,
      vaultState: vaultStatePDA,
    }).signers([user, vaultAuthority]).rpc();

    console.log("Stake signature: ", stakeTx);
  });
 
  it("Unstake test", async () => {
    
  });
 });
