# Wallets and keys

Kolme keeps an internal concept of accounts. Accounts are able to receive and send funds and perform other actions. Each account is either a [multisig account](./multisig-accounts.md) or a regular account, equivalent to Externally Owned Accounts (EOAs) from other chains.

Each regular account has 0 or more *wallets* and *public keys* associated with it, and must at all times hve at least 1 wallet or 1 public key. Public keys are the only authentication mechanism supported within Kolme, meaning every transaction you send to the chain must be signed with a public/private keypair.

Wallets, on the other hand, represent a wallet on an external blockchain. Since many blockchains uses the same wallet addresses (e.g., all EVM chains use identical representations), we internally track wallet addresses as simple strings, not tied to a specific chain.

Wallet addresses can only be used for controlling an account through the bridge contract.  It's easiest to understand the workflow by following how a user will normally initiate and use an account.

1. Most applications begin with a fund transfer to bridge funds into the Kolme app. Let's say that the user has a wallet address `0xDEADBEEF`. They first initiate a transfer of 100 USDC into the bridge contract on an external chain. The message includes a public key, `PUBKEY1`.
2. Listeners see this bridge event and submit it to the chain as a new deposit. When a quorum of listeners signs off, the processor accepts the event and executes it. During execution, it does the following:
    * Look up the `0xDEADBEEF` wallet address. If it has an existing account ID associated with it, we use that ID. Otherwise, we add a new account entry with the next available account ID and associate `0xDEADBEEF` with it.
    * Look up the `PUBKEY1` public key. If it is currently unused, we associate that public key with our account.
    * Increase the balance for the account by 100 USDC.
3. The user uses `PUBKEY1`'s secret key to sign a message to interact with Kolme. The public key is looked up, we find the appropriate account, and all actions are taken on behalf of this account.
4. A few days later, the user is on a new machine and no longer has the secret key for `PUBKEY1`. They only have the `0xDEADBEEF` wallet. They need to add a new key to their account, so they send a new bridge contract message. This one includes no USDC, but specifies the public key `PUBKEY2`.
5. The same listener/processor dance occurs as in step (2), and unless `PUBKEY2` is already used by another account, we add it to our existing account.
6. The user can now perform Kolme transactions on their new device. They can also choose to remove `PUBKEY1` from their account, or even disassociate the `0xDEADBEEF` wallet to allow it to be used with a new account.
