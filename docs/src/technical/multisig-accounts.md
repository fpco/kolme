# Multisig accounts

Kolme supports two different account types: regular accounts controlled by [external wallets and public keys](./wallets-keys.md), and multisig accounts. (**NOTE** at time of writing, multisig accounts have not yet been implemented.) Multisig accounts allow for a quorum of users to control actions from an account, a common desire for more secure management.
The basic workflow of multisig accounts is:

* Create the account. This is done by using a normal account to send a "create multisig account" transaction. Any account can perform this action, and the new account will be created with the given set of keys and quorum rules, with no connection to the original account.
* Any member of the multisig set can propose a list of messages to be run by the multisig account. These messages will be assigned a multisig proposal ID and await voting.
* Other members can vote yes or no. If a quorum of yes can no longer be made, the proposal is removed from the pending proposals. If a quorum for yes is achieved, the proposal is removed and the messages are executed in the same block that the final yes vote occurred.
    * Note that we need to consider how we associate log messages with each individual transaction, we may end up needing some kind of "hierarchical messages", TBD. We also need to handle the possibility of these messages failing, probably by putting an explicit "this proposal failed" message in the logs.
    * Question: any reason to consider separating voting from execution?
* A special message can be used to change the voting set needed for a multisig. This must be voted on by the existing quorum.
    * Question: do we want to generalize this to a "convert account" message, and allow converting multisigs to regular accounts and vice-versa?
