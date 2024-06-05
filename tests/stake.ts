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
  let mintKey = anchor.web3.Keypair.generate();
  let associatedTokenAccount = undefined;
  const [mintStatePDA, mintStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("mint-state")],
    program.programId
  );
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let userTokenMintKey: anchor.web3.Keypair;
  let depositer: anchor.web3.Keypair;
  let vaultAuthority: anchor.web3.Keypair;
  let mintCollatVault: anchor.web3.PublicKey;
  let mintTokenVault: anchor.web3.PublicKey;
  let callerCollat: anchor.web3.PublicKey;
  let callerToken: anchor.web3.PublicKey;


  before(async () => {
    // Initialize any necessary setup before tests
    userTokenMintKey = anchor.web3.Keypair.generate();
    depositer = anchor.web3.Keypair.generate();
    vaultAuthority = anchor.web3.Keypair.generate();

    // Fund redeemer and vaultAuthority accounts
    await anchor.AnchorProvider.env().connection.requestAirdrop(depositer.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Initialize mint state account
    console.log("admin: ", adminKey);
    await program.methods.initializeMintState(new BN(1000), new BN(1000)).rpc()

    // Remove admin from whitelisted minters / redeemers (added by default)
    await program.methods.removeMinter(adminKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Removed minter: ", adminKey);

    await program.methods.removeRedeemer(adminKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Removed redeemer: ", adminKey);

    // Whitelist minter (add admin back as whitelisted minter / redeemer)
    await program.methods.whitelistMinter(adminKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Whitelisted minter: ", adminKey);

    await program.methods.whitelistRedeemer(adminKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Whitelisted redeemer: ", adminKey);
  });
 
  it("Initialize Mint and mint tokens", async () => {
    // Get anchor's wallet's public key
    const key = adminKey;

    // Create a new mint keypair
    mintKey = anchor.web3.Keypair.generate();

    // Get the amount of SOL needed to pay rent for our Token Mint
    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(
      MINT_SIZE
    );

    // Get the ATA for a token and the account that will own the ATA
    associatedTokenAccount = await getAssociatedTokenAddress(
      mintKey.publicKey,
      key
    );

    const mint_tx = new anchor.web3.Transaction().add(
      // Create an account from the mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: key,
        newAccountPubkey: mintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create mint account that is controlled by anchor wallet
      createInitializeMintInstruction(
        mintKey.publicKey, 0, key, key
      ),
      // Create the ATA account that is associated with mint on anchor wallet
      createAssociatedTokenAccountInstruction(
        key, associatedTokenAccount, key, mintKey.publicKey
      )
    );

    const res = await anchor.AnchorProvider.env().sendAndConfirm(mint_tx, [mintKey]);

    console.log(
      await program.provider.connection.getParsedAccountInfo(mintKey.publicKey)
    );

    console.log("Account: ", res);
    console.log("Mint key: ", mintKey.publicKey.toString());
    console.log("User: ", key.toString());

    // Executes code to mint token into specified ATA
    const tx = await program.methods.mintToken(new anchor.BN(10)).accounts({
      mint: mintKey.publicKey,
      recipient: associatedTokenAccount,
      authority: key,
      mintState: mintStatePDA,
    }).rpc();

    console.log("VaultToken minting signature: ", tx);

    // Get minted token amount on the ATA for anchor wallet
    const accountInfo = await program.provider.connection.getParsedAccountInfo(associatedTokenAccount);
    if (accountInfo.value && accountInfo.value.data.parsed) {
      const minted = accountInfo.value.data.parsed.info.tokenAmount.amount;
      assert.equal(minted, 10);
    } else {
      throw new Error("Failed to retrieve parsed account data");
    }

    // Create mint for UserToken
    const userTokenMintTx = new anchor.web3.Transaction().add(
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: userTokenMintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      createInitializeMintInstruction(userTokenMintKey.publicKey, 0, adminKey, adminKey)
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTokenMintTx, [userTokenMintKey]);

    mintCollatVault = await getAssociatedTokenAddress(userTokenMintKey.publicKey, vaultAuthority.publicKey);
    mintTokenVault = await getAssociatedTokenAddress(mintKey.publicKey, vaultAuthority.publicKey);
    callerCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, depositer.publicKey);
    callerToken = await getAssociatedTokenAddress(mintKey.publicKey, depositer.publicKey);

    const createVaultsTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, mintCollatVault, vaultAuthority.publicKey, userTokenMintKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, mintTokenVault, vaultAuthority.publicKey, mintKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, callerCollat, depositer.publicKey, userTokenMintKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, callerToken, depositer.publicKey, mintKey.publicKey)
    );

    await anchor.AnchorProvider.env().sendAndConfirm(createVaultsTx, []);

    // Mint UserToken to depositer
    
    const mintUserTokenTx = await program.methods.mintToken(new anchor.BN(100)).accounts({
      mint: userTokenMintKey.publicKey,
      recipient: callerCollat,
      authority: key,
      mintState: mintStatePDA,
    }).rpc();

    console.log("UserToken minting signature: ", mintUserTokenTx);
    
  });

  it("Add asset, deposit, redeem", async () => {
    const assetKey = TOKEN_PROGRAM_ID;
    const depositRate = new anchor.BN(1);
    const redeemRate = new anchor.BN(1);

    // Add asset
    const addAssetTx = await program.methods.addAsset(assetKey, depositRate, redeemRate).accounts({
      mintState: mintStatePDA,
      authority: adminKey,
    }).rpc();

    console.log("Add asset signature: ", addAssetTx);
    console.log("Asset added: ", assetKey);

    // Whitelist depositer
    await program.methods.whitelistMinter(depositer.publicKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Whitelisted minter: ", adminKey);

    await program.methods.whitelistRedeemer(depositer.publicKey).accounts({
      mintState: mintStatePDA,
    }).rpc()
    console.log("Whitelisted redeemer: ", adminKey);

    const amt = new anchor.BN(1);

    // Deposit
    const depositTx = await program.methods.deposit(amt).accounts({
      mintCollatVault: mintCollatVault,
      mintTokenVault: mintTokenVault,
      callerCollat: callerCollat,
      callerToken: callerToken,
      depositer: depositer.publicKey,
      mintState: mintStatePDA,
      vaultAuthority: adminKey,
      mint: mintKey.publicKey,
    }).signers([depositer]).rpc();

    console.log("Deposit signature: ", depositTx);

    // Redeem
    const redeemTx = await program.methods.redeem(amt).accounts({
      mintCollatVault: mintCollatVault,
      mintTokenVault: mintTokenVault,
      callerCollat: callerCollat,
      callerToken: callerToken,
      redeemer: depositer.publicKey,
      mintState: mintStatePDA,
      vaultAuthority: vaultAuthority.publicKey,
      mint: mintKey.publicKey,
    }).signers([depositer, vaultAuthority]).rpc();

    console.log("Redemption signature: ", redeemTx);
  });
 
  it("Withdraw deposited collateral", async () => {
    const amt = new anchor.BN(1);
    
    // Add manager
    await program.methods.addManager(callerCollat).accounts({
      mintState: mintStatePDA,
    }).rpc()

    // Deposit
    const depositTx = await program.methods.deposit(amt).accounts({
      mintCollatVault: mintCollatVault,
      mintTokenVault: mintTokenVault,
      callerCollat: callerCollat,
      callerToken: callerToken,
      depositer: depositer.publicKey,
      mintState: mintStatePDA,
      vaultAuthority: adminKey,
      mint: mintKey.publicKey,
    }).signers([depositer]).rpc();
    
    // Withdraw
    const withdrawTx = await program.methods.withdraw(amt).accounts({
      mintCollatVault: mintCollatVault,
      caller: callerCollat,
      mintState: mintStatePDA,
      vaultAuthority: vaultAuthority.publicKey,
    }).signers([vaultAuthority]).rpc();

    console.log("Withdraw signature: ", withdrawTx);
  });
 });
