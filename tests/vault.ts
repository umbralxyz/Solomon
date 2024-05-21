import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Vault } from "../target/types/vault";
import {
  TOKEN_PROGRAM_ID,
  MINT_SIZE,
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddress,
  createInitializeMintInstruction,
} from "@solana/spl-token"; 
import { assert } from "chai";
import { BN } from "bn.js";

describe("vault", () => {
   // Configure client to use local cluster.
   anchor.setProvider(anchor.AnchorProvider.env());
   const program = anchor.workspace.Vault as Program<Vault>;
   // Generate a random keypair to represent token
   const mintKey: anchor.web3.Keypair = anchor.web3.Keypair.generate();
   let associatedTokenAccount = undefined;
   const mintStateAccount: anchor.web3.Keypair = anchor.web3.Keypair.generate();
   const adminKey = anchor.AnchorProvider.env().wallet.publicKey;

   before(async () => {
    // Initialize any necessary setup before tests

    // Initialize mint state account
    console.log("admin: ", adminKey);
    await program.methods.initializeMintState(new BN(1000), new BN(1000), new BN(1)).signers([mintStateAccount]).accounts({
      mintState: mintStateAccount.publicKey,
      admin: adminKey,
      }).rpc()

    // Whitelist minter
    await program.methods.whitelistMinter(adminKey).accounts({
      mintState: mintStateAccount.publicKey,
      admin: adminKey,
    }).rpc()
    console.log("Whitelisted minter: ", adminKey);

   });
 
   it("Mint a token", async () => {
     // Get anchor's wallet's public key
     const key = anchor.AnchorProvider.env().wallet.publicKey;
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
     const tx = await program.methods.mintToken(new BN(10)).accounts({
       mint: mintKey.publicKey,
       tokenAccount: associatedTokenAccount,
       authority: key,
       mintState: mintStateAccount.publicKey,
     }).rpc();
     console.log("Transaction signature: ", tx);
 
     // Get minted token amount on the ATA for anchor wallet
     const accountInfo = await program.provider.connection.getParsedAccountInfo(associatedTokenAccount);
     if (accountInfo.value && isParsedAccountData(accountInfo.value.data)) {
       const minted = accountInfo.value.data.parsed.info.tokenAmount.amount;
       assert.equal(minted, 10);
     } else {
       throw new Error("Failed to retrieve parsed account data");
     }
   });
 
   it("Transfer token", async () => {
     // Get anchor's wallet's public key
     const myWallet = anchor.AnchorProvider.env().wallet.publicKey;
     // Wallet that will receive the token 
     const toWallet: anchor.web3.Keypair = anchor.web3.Keypair.generate();
     // The ATA for a token on the to wallet
     const toATA = await getAssociatedTokenAddress(
       mintKey.publicKey,
       toWallet.publicKey
     );
 
     const mint_tx = new anchor.web3.Transaction().add(
       // Create the ATA account that is associated with To wallet
       createAssociatedTokenAccountInstruction(
         myWallet, toATA, toWallet.publicKey, mintKey.publicKey
       )
     );

     await anchor.AnchorProvider.env().sendAndConfirm(mint_tx, []);
 
     // Executes transfer 
     await program.methods.transferToken(new BN(5)).accounts({
       from: associatedTokenAccount,
       fromAuthority: myWallet,
       to: toATA,
       mintState: mintStateAccount.publicKey,
     }).rpc();
  
     // Get minted token amount on the ATA for anchor wallet
     const accountInfo = await program.provider.connection.getParsedAccountInfo(associatedTokenAccount);
     if (accountInfo.value && isParsedAccountData(accountInfo.value.data)) {
      const minted = accountInfo.value.data.parsed.info.tokenAmount.amount;
      assert.equal(minted, 5);
    } else {
      throw new Error("Failed to retrieve parsed account data");
    }
   });
 });

 function isParsedAccountData(data: Buffer | anchor.web3.ParsedAccountData): data is anchor.web3.ParsedAccountData {
  return (data as anchor.web3.ParsedAccountData).parsed !== undefined;
}