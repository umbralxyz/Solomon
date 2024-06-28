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
  let userCollat: anchor.web3.PublicKey;
  let userVaultToken: anchor.web3.PublicKey;
  let userTwoTokenMintKey: anchor.web3.Keypair;


  before(async () => {
    // Initialize keypairs
    userTokenMintKey = anchor.web3.Keypair.generate();
    userTwoTokenMintKey = anchor.web3.Keypair.generate();
    depositer = anchor.web3.Keypair.generate();
    vaultAuthority = anchor.web3.Keypair.generate();

    // Fund redeemer and vaultAuthority accounts
    await anchor.AnchorProvider.env().connection.requestAirdrop(depositer.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await anchor.AnchorProvider.env().connection.requestAirdrop(vaultAuthority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Initialize vault_state and vault_token
    await program.methods.initializeVaultState(adminKey).accounts({
      signer: adminKey,
    }).rpc();
    console.log("Admin: ", adminKey.toString());

    // Add admin as manager
    await program.methods.addRoleManager(adminKey).rpc();
    console.log("Added role manager: ", adminKey.toString());

    await program.methods.addAssetManager(adminKey).rpc();
    console.log("Added asset manager: ", adminKey.toString());

    // Remove admin from whitelisted minters / redeemers (added by default)
    await program.methods.removeMinter(adminKey).rpc();
    console.log("Removed minter: ", adminKey.toString());

    await program.methods.removeRedeemer(adminKey).rpc();
    console.log("Removed redeemer: ", adminKey.toString());

    // Whitelist minter (add admin back as whitelisted minter / redeemer)
    await program.methods.whitelistMinter(adminKey).rpc();
    console.log("Whitelisted minter: ", adminKey.toString());

    await program.methods.whitelistRedeemer(adminKey).rpc();
    console.log("Whitelisted redeemer: ", adminKey.toString());
  });
  
  it("Initialize Mint and mint tokens", async () => {
    const key = adminKey;

    // Get the amount of SOL needed to pay rent for our Token Mint
    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(MINT_SIZE);

    // Get the address for admin's VaultToken ATA and create
    associatedTokenAccount = await getAssociatedTokenAddress(vaultMint, key);

    console.log("Vault mint address: ", vaultMint.toString());
    const ataTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(key, associatedTokenAccount, key, vaultMint),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(ataTx, []);

    const mintAmount = 10000000000;

    vaultCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, vaultAuthority.publicKey);
    userCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, depositer.publicKey);
    userVaultToken = await getAssociatedTokenAddress(vaultMint, depositer.publicKey);

    // Create mint for UserToken and user ATA
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
      createInitializeMintInstruction(userTokenMintKey.publicKey, 9, key, key),
      // Create the ATA account that is associated with collat mint on anchor wallet
      createAssociatedTokenAccountInstruction(key, userCollat, depositer.publicKey, userTokenMintKey.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTokenMintTx, [userTokenMintKey]);

    // Create mint for UserTwoToken
    const userTwoTokenMintTx = new anchor.web3.Transaction().add(
      // Create an account from the depositer mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: userTwoTokenMintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create collat mint account that is controlled by anchor wallet
      createInitializeMintInstruction(userTwoTokenMintKey.publicKey, 9, key, key),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTwoTokenMintTx, [userTwoTokenMintKey]);

    console.log("Collat Mint key: ", userTokenMintKey.publicKey.toString());
    console.log("Collat Two Mint key: ", userTwoTokenMintKey.publicKey.toString());
    
    const createVaultsTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, vaultCollat, vaultAuthority.publicKey, userTokenMintKey.publicKey),
      createAssociatedTokenAccountInstruction(adminKey, userVaultToken, depositer.publicKey, vaultMint)
    );

    await anchor.AnchorProvider.env().sendAndConfirm(createVaultsTx, []);

    // Mint UserTokens (collat) to depositer
    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(userTokenMintKey.publicKey, userCollat, adminKey, mintAmount),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    await anchor.AnchorProvider.env().connection.requestAirdrop(userCollat, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Get minted token amount on the ATA for depositer
    const depositerInfo = await program.provider.connection.getParsedAccountInfo(userCollat);
    const collatMinted = depositerInfo.value.data.parsed.info.tokenAmount.amount;
    console.log("Collat minted: ", collatMinted.toString());
  });

  
  it("Add asset, deposit, redeem", async () => {
    const assetKey = TOKEN_PROGRAM_ID;
    const depositRate = new anchor.BN(1000000000);
    const redeemRate = new anchor.BN(1000000000);

    // Add asset one
    const addAssetTx = await program.methods.updateAsset(userTokenMintKey.publicKey, depositRate, redeemRate).accounts({
      authority: adminKey,
      collateralTokenMint: userTokenMintKey.publicKey,
    }).rpc();
    console.log("Asset added: ", userTokenMintKey.publicKey.toString());

    // Add asset two
    const addAssetTwoTx = await program.methods.updateAsset(userTwoTokenMintKey.publicKey, depositRate, redeemRate).accounts({
      authority: adminKey,
      collateralTokenMint: userTwoTokenMintKey.publicKey,
    }).rpc();
    console.log("Asset added: ", userTwoTokenMintKey.publicKey.toString());

    // Whitelist depositer as minter and redeemer
    await program.methods.whitelistMinter(depositer.publicKey).rpc();
    console.log("Whitelisted minter: ", adminKey.toString());

    await program.methods.whitelistRedeemer(depositer.publicKey).rpc();
    console.log("Whitelisted redeemer: ", adminKey.toString());

    const deposit = new anchor.BN(1000000000);
    const redeem = new anchor.BN(500000000);
    
    console.log("Caller collat mint: ", userCollat.toString())
    // Get user balances before deposit
    let callerInfo = await program.provider.connection.getParsedAccountInfo(userVaultToken);
    const vaultTokensBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userCollat);
    const collatTokensBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    
    // Deposit as user
    const depositTx = await program.methods.deposit(deposit).accounts({
      callerCollateral: userCollat,
      callerVaultToken: userVaultToken,
      minter: depositer.publicKey,
      collateralTokenMint: userTokenMintKey.publicKey,
    }).signers([depositer]).rpc().catch(e => console.error(e));
    

    // Get user balances after deposit
    callerInfo = await program.provider.connection.getParsedAccountInfo(userVaultToken);
    const vaultTokensAfterDeposit = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userCollat);
    const collatTokensAfterDeposit = callerInfo.value.data.parsed.info.tokenAmount.amount;

    // Redeem as user
    const redeemTx = await program.methods.redeem(redeem).accounts({
      callerCollateral: userCollat,
      callerVaultToken: userVaultToken,
      redeemer: depositer.publicKey,
      collateralTokenMint: userTokenMintKey.publicKey,
    }).signers([depositer]).rpc().catch(e => console.error(e));

    // Get user balances after redemption
    callerInfo = await program.provider.connection.getParsedAccountInfo(userVaultToken);
    const vaultTokensAfterRedemption = callerInfo.value.data.parsed.info.tokenAmount.amount;
    callerInfo = await program.provider.connection.getParsedAccountInfo(userCollat);
    const collatTokensAfterRedemption = callerInfo.value.data.parsed.info.tokenAmount.amount;
    
    console.log("User vault tokens before deposit: ", vaultTokensBefore);
    console.log("User collat tokens before deposit: ", collatTokensBefore);
    console.log("User vault tokens after deposit: ", vaultTokensAfterDeposit);
    console.log("User collat tokens after deposit: ", collatTokensAfterDeposit);
    console.log("User vault tokens after redemption: ", vaultTokensAfterRedemption);
    console.log("User collat tokens after redemption: ", collatTokensAfterRedemption);
  });
  
  it("Add withdrawer and withdraw deposited collateral", async () => {
    const withdrawer = anchor.web3.Keypair.generate();

    const withdrawerCollat = await getAssociatedTokenAddress(userTokenMintKey.publicKey, withdrawer.publicKey);

    // Create withdrawer's ATA
    const ataTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, withdrawerCollat, withdrawer.publicKey, userTokenMintKey.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(ataTx, []);

    const amt = new anchor.BN(40000);
    
    // Add withdraw address
    await program.methods.addWithdrawAddress(withdrawerCollat).rpc();

    console.log("Added withdraw address: ", withdrawerCollat.toString());

    let callerInfo = await program.provider.connection.getParsedAccountInfo(withdrawerCollat);
    const withdrawerBefore = callerInfo.value.data.parsed.info.tokenAmount.amount;
    
    // Withdraw
    const withdrawTx = await program.methods.withdraw(amt).accounts({
      destination: withdrawerCollat,
      collatMint: userTokenMintKey.publicKey,
    }).signers([]).rpc();
    
    callerInfo = await program.provider.connection.getParsedAccountInfo(withdrawerCollat);
    const withdrawerAfter = callerInfo.value.data.parsed.info.tokenAmount.amount;

    console.log("Withdrawer collat tokens before withdraw: ", withdrawerBefore);
    console.log("Withdrawer collat tokens after withdraw: ", withdrawerAfter);

    // Remove withdraw address
    await program.methods.removeWithdrawAddress(withdrawerCollat).rpc();
    console.log("Removed withdraw address: ", withdrawerCollat.toString());

    // Remove admin from managers
    await program.methods.removeRoleManager(adminKey).rpc();
    console.log("Removed role manager: ", adminKey.toString());

    await program.methods.removeAssetManager(adminKey).rpc();
    console.log("Removed asset manager: ", adminKey.toString());
  });

  it("Transfer admin back and forth", async () => {
    await program.methods.transferAdmin(depositer.publicKey).rpc()
    console.log("Transfered admin to: ", depositer.publicKey.toString());

    await program.methods.transferAdmin(adminKey).accounts({
      caller: depositer.publicKey,
    }).signers([depositer]).rpc();
    console.log("Transfered admin back to: ", adminKey.toString());
  });
 });
