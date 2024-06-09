import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Vault } from "../target/types/vault";
import {
  TOKEN_PROGRAM_ID,
  MINT_SIZE,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddress,
  createInitializeMintInstruction,
  mintTo,
  createMintToInstruction,
} from "@solana/spl-token"; 
import { assert } from "chai";
import { BN } from "bn.js";
import { publicKey } from "@coral-xyz/anchor/dist/cjs/utils";

describe("vault", () => {
  // Configure client to use local cluster.
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.Vault as Program<Vault>;
  // Generate a random keypair to represent token
  let associatedTokenAccount = undefined;
  const [mintStatePDA, mintStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("mint-state")],
    program.programId
  );
  const [vaultStatePDA, vaultStateBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("vault-state")],
    program.programId
  );
  const [vaultMint, vaultTokenBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("mint")],
    program.programId
  );
  const adminWallet = anchor.AnchorProvider.env().wallet;
  const adminKey = adminWallet.publicKey;
  let userTokenMintKey: anchor.web3.Keypair;
  let depositer: anchor.web3.Keypair;
  let vaultAuthority: anchor.web3.Keypair;
  let vaultCollat: anchor.web3.PublicKey;
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

    // Initialize vault_state and vault_token
    await program.methods.initializeVaultState(adminKey).accounts({
      signer: adminKey,
    }).rpc();
    console.log("admin: ", adminKey);

    // Remove admin from whitelisted minters / redeemers (added by default)
    await program.methods.removeMinter(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Removed minter: ", adminKey);

    await program.methods.removeRedeemer(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Removed redeemer: ", adminKey);

    // Whitelist minter (add admin back as whitelisted minter / redeemer)
    await program.methods.whitelistMinter(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Whitelisted minter: ", adminKey);

    await program.methods.whitelistRedeemer(adminKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Whitelisted redeemer: ", adminKey);
  });
  
  it("Initialize Mint and mint tokens", async () => {
    // Get anchor's wallet's public key
    const key = adminKey;

    // Get the amount of SOL needed to pay rent for our Token Mint
    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(MINT_SIZE);

    // Get the ATA for a token and the account that will own the ATA
    associatedTokenAccount = await getAssociatedTokenAddress(vaultMint, key);

    console.log("Vault mint address: ", vaultMint);
    const ataTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(key, associatedTokenAccount, key, vaultMint),
    );

    const res = await anchor.AnchorProvider.env().sendAndConfirm(ataTx, []);

    console.log(
      await program.provider.connection.getParsedAccountInfo(vaultMint)
    );

    const mintAmount = 1000000000000000;

    vaultCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, vaultAuthority.publicKey);
    callerCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, depositer.publicKey);
    callerToken = await getAssociatedTokenAddress(vaultMint, depositer.publicKey);

    // Create mint for UserToken
    const userTokenMintTx = new anchor.web3.Transaction().add(
      // Create an account from the depositer mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: userTokenMintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create collat mint account that is controlled by anchor wallet
      createInitializeMintInstruction(userTokenMintKey.publicKey, 0, key, key),
      // Create the ATA account that is associated with collat mint on anchor wallet
      createAssociatedTokenAccountInstruction(key, callerCollat, depositer.publicKey, userTokenMintKey.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTokenMintTx, [userTokenMintKey]);

    //console.log("Account: ", userTokenMintTx);
    console.log("Collat Mint key: ", userTokenMintKey.publicKey);
    
    const createVaultsTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, vaultCollat, vaultAuthority.publicKey, userTokenMintKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, callerToken, depositer.publicKey, vaultMint)
    );

    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(userTokenMintKey.publicKey, callerCollat, adminKey, mintAmount),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    await anchor.AnchorProvider.env().connection.requestAirdrop(callerCollat, 2 * anchor.web3.LAMPORTS_PER_SOL);

    await anchor.AnchorProvider.env().sendAndConfirm(createVaultsTx, []);

    // Get minted token amount on the ATA for depositer
    const depositerInfo = await program.provider.connection.getParsedAccountInfo(callerCollat);
    const collatMinted = depositerInfo.value.data.parsed.info.tokenAmount.amount;
    console.log("Collat minted: ", collatMinted);
  });

  
  it("Add asset, deposit, redeem", async () => {
    const assetKey = TOKEN_PROGRAM_ID;
    const depositRate = new anchor.BN(1);
    const redeemRate = new anchor.BN(1);

    // Add asset
    const addAssetTx = await program.methods.addAsset(userTokenMintKey.publicKey, depositRate, redeemRate).accounts({
      authority: adminKey,
      collateralTokenMint: userTokenMintKey.publicKey,
      vaultState: vaultStatePDA,
    }).rpc();

    console.log("Add asset signature: ", addAssetTx);
    console.log("Asset added: ", assetKey);

    // Whitelist depositer
    await program.methods.whitelistMinter(depositer.publicKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Whitelisted minter: ", adminKey);

    await program.methods.whitelistRedeemer(depositer.publicKey).accounts({
      vaultState: vaultStatePDA,
    }).rpc()
    console.log("Whitelisted redeemer: ", adminKey);

    const deposit = new anchor.BN(100000000000000);
    const redeem = new anchor.BN(50);
    
    console.log("Caller collat mint: ", callerCollat)
    let callerInfo = await program.provider.connection.getParsedAccountInfo(callerToken);
    const vaultTokensBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(callerCollat);
    const collatTokensBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    
    // Deposit
    const depositTx = await program.methods.deposit(deposit).accounts({
      callerCollateral: callerCollat,
      callerVaultToken: callerToken,
      minter: depositer.publicKey,
      collateralTokenMint: userTokenMintKey.publicKey,
      vaultState: vaultStatePDA,
    }).signers([depositer]).rpc().catch(e => console.error(e));

    console.log("Deposit signature: ", depositTx);

    callerInfo = await program.provider.connection.getParsedAccountInfo(callerToken);
    const vaultTokensAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(callerCollat);
    const collatTokensAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;
    

    console.log("Caller vault tokens before deposit: ", vaultTokensBefore);
    console.log("Caller collat tokens before deposit: ", collatTokensBefore);
    console.log("Caller vault tokens after deposit: ", vaultTokensAfter);
    console.log("Caller collat tokens after deposit: ", collatTokensAfter);

    // Redeem
    const redeemTx = await program.methods.redeem(redeem).accounts({
      callerCollateral: callerCollat,
      callerVaultToken: callerToken,
      redeemer: depositer.publicKey,
      collateralTokenMint: userTokenMintKey.publicKey,
    }).signers([depositer]).rpc().catch(e => console.error(e));

    console.log("Redemption signature: ", redeemTx);

  });
  
  it("Add manager and withdraw deposited collateral", async () => {
    const withdrawer = anchor.web3.Keypair.generate();

    const withdrawerCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, withdrawer.publicKey);

    const ataTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, withdrawerCollat, withdrawer.publicKey, userTokenMintKey.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(ataTx, []);

    const amt = new anchor.BN(9);
    
    // Add manager
    await program.methods.addManager(withdrawerCollat).accounts({
      vaultState: vaultStatePDA,
    }).rpc()

    let callerInfo = await program.provider.connection.getParsedAccountInfo(withdrawerCollat);
    const withdrawerBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    
    // Withdraw
    const withdrawTx = await program.methods.withdraw(amt).accounts({
      caller: withdrawerCollat,
      collatMint: userTokenMintKey.publicKey,
      vaultState: vaultStatePDA,
    }).signers([]).rpc();

    console.log("Withdraw signature: ", withdrawTx);

    
    callerInfo = await program.provider.connection.getParsedAccountInfo(withdrawerCollat);
    const withdrawerAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("Withdrawer collat tokens before withdraw: ", withdrawerBefore);
    console.log("Withdrawer collat tokens after withdraw: ", withdrawerAfter);
  });
  
 });
