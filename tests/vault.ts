import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Vault } from "../target/types/vault";
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
import { publicKey } from "@coral-xyz/anchor/dist/cjs/utils";

describe("vault", () => {
  // Metaplex Constants
  const metadata = {
    name: 'Solana Gold',
    symbol: 'GOLDSOL',
    uri: 'https://raw.githubusercontent.com/solana-developers/program-examples/new-examples/tokens/tokens/.assets/spl-token.json',
  };
  // Configure client to use local cluster.
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.Vault as Program<Vault>;
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
  let tokenOneMintKey: anchor.web3.Keypair;
  let depositer: anchor.web3.Keypair;
  let userCollat: anchor.web3.PublicKey;
  let userVaultToken: anchor.web3.PublicKey;
  let tokenTwoMintKey: anchor.web3.Keypair;


  before(async () => {
    console.log("Vault state: ", vaultStatePDA.toString());
    console.log("Vault token: ", vaultMint.toString());
    // Initialize keypairs
    tokenOneMintKey = anchor.web3.Keypair.generate();
    tokenTwoMintKey = anchor.web3.Keypair.generate();
    depositer = anchor.web3.Keypair.generate();

    // Fund redeemer and vaultAuthority accounts
    await anchor.AnchorProvider.env().connection.requestAirdrop(depositer.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Initialize vault_state and vault_token
    await program.methods.initializeVaultState(adminKey, metadata).accounts({
      signer: adminKey,
    }).rpc().catch(e => console.error(e));
    console.log("Admin: ", adminKey.toString());

    // Add admin as manager
    await program.methods.addRoleManager(adminKey).rpc();
    console.log("Added role manager: ", adminKey.toString());

    await program.methods.addAssetManager(adminKey).rpc();
    console.log("Added asset manager: ", adminKey.toString());

    // Add admin as whitelisted minter / redeemer
    await program.methods.whitelistMinter(adminKey).rpc();
    console.log("Whitelisted minter: ", adminKey.toString());

    await program.methods.whitelistRedeemer(adminKey).rpc();
    console.log("Whitelisted redeemer: ", adminKey.toString());
  });
  
  it("Initialize Mint and mint tokens", async () => {
    const key = adminKey;

    // Get the amount of SOL needed to pay rent for our Token Mint
    const lamports: number = await program.provider.connection.getMinimumBalanceForRentExemption(MINT_SIZE);

    const mintAmount = 1000000000000;

    userCollat = await getAssociatedTokenAddress(tokenOneMintKey.publicKey, depositer.publicKey);
    userVaultToken = await getAssociatedTokenAddress(vaultMint, depositer.publicKey);

    // Create mint for UserToken and user ATA
    const userTokenMintTx = new anchor.web3.Transaction().add(
      // Create an account from the depositer mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: tokenOneMintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create collat mint account that is controlled by anchor wallet
      createInitializeMintInstruction(tokenOneMintKey.publicKey, 9, key, key),
      // Create the ATA account that is associated with collat mint on anchor wallet
      createAssociatedTokenAccountInstruction(key, userCollat, depositer.publicKey, tokenOneMintKey.publicKey),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTokenMintTx, [tokenOneMintKey]);

    // Create mint for UserTwoToken
    const userTwoTokenMintTx = new anchor.web3.Transaction().add(
      // Create an account from the depositer mint key
      anchor.web3.SystemProgram.createAccount({
        fromPubkey: adminKey,
        newAccountPubkey: tokenTwoMintKey.publicKey,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
        lamports,
      }),
      // Create collat mint account that is controlled by anchor wallet
      createInitializeMintInstruction(tokenTwoMintKey.publicKey, 9, key, key),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(userTwoTokenMintTx, [tokenTwoMintKey]);

    console.log("Collat One Mint key: ", tokenOneMintKey.publicKey.toString());
    console.log("Collat Two Mint key: ", tokenTwoMintKey.publicKey.toString());
    
    const createVaultsTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, userVaultToken, depositer.publicKey, vaultMint)
    );

    await anchor.AnchorProvider.env().sendAndConfirm(createVaultsTx, []);

    // Mint UserTokens (collat) to depositer
    const collatMintTx = new anchor.web3.Transaction().add(
      createMintToInstruction(tokenOneMintKey.publicKey, userCollat, adminKey, mintAmount),
    );

    await anchor.AnchorProvider.env().sendAndConfirm(collatMintTx, []);

    await anchor.AnchorProvider.env().connection.requestAirdrop(userCollat, 2 * anchor.web3.LAMPORTS_PER_SOL);

    // Get minted token amount on the ATA for depositer
    const depositerInfo = await program.provider.connection.getParsedAccountInfo(userCollat);
    const collatMinted = depositerInfo.value.data.parsed.info.tokenAmount.amount;
    console.log("Collat minted: ", collatMinted.toString());
  });

  
  it("Add asset, deposit, redeem", async () => {
    const depositRate = new anchor.BN(1000000000);
    const redeemRate = new anchor.BN(1000000000);

    // Add asset one
    await program.methods.updateAsset(tokenOneMintKey.publicKey, depositRate, redeemRate).accounts({
      authority: adminKey,
      collateralTokenMint: tokenOneMintKey.publicKey,
    }).rpc();
    console.log("Asset added: ", tokenOneMintKey.publicKey.toString());

    // Add asset two
    await program.methods.updateAsset(tokenTwoMintKey.publicKey, depositRate, redeemRate).accounts({
      authority: adminKey,
      collateralTokenMint: tokenTwoMintKey.publicKey,
    }).rpc();
    console.log("Asset added: ", tokenTwoMintKey.publicKey.toString());

    // Whitelist depositer as minter and redeemer
    await program.methods.whitelistMinter(depositer.publicKey).rpc();
    console.log("Whitelisted minter: ", adminKey.toString());

    await program.methods.whitelistRedeemer(depositer.publicKey).rpc();
    console.log("Whitelisted redeemer: ", adminKey.toString());

    const deposit = new anchor.BN(100000000000);
    const redeem = new anchor.BN(50000000000);
    
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
      collateralTokenMint: tokenOneMintKey.publicKey,
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
      collateralTokenMint: tokenOneMintKey.publicKey,
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

    // Remove admin from whitelisted minters / redeemers
    await program.methods.removeMinter(adminKey).rpc();
    console.log("Removed minter: ", adminKey.toString());

    await program.methods.removeRedeemer(adminKey).rpc();
    console.log("Removed redeemer: ", adminKey.toString());
  });
  
  it("Add withdrawer and withdraw deposited collateral", async () => {
    const withdrawer = anchor.web3.Keypair.generate();

    const withdrawerCollat = await getAssociatedTokenAddress(tokenOneMintKey.publicKey, withdrawer.publicKey);

    // Create withdrawer's ATA
    const ataTx = new anchor.web3.Transaction().add(
      createAssociatedTokenAccountInstruction(adminKey, withdrawerCollat, withdrawer.publicKey, tokenOneMintKey.publicKey),
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
      collatMint: tokenOneMintKey.publicKey,
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
