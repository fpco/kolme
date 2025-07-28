use crate::*;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountsError {
    #[error("Insufficient balance for account {account_id}, asset {asset_id}. Requested: {requested}. Available: {available}.")]
    InsufficientBalance {
        account_id: AccountId,
        asset_id: AssetId,
        requested: Decimal,
        available: Decimal,
    },
    #[error("Must provide positive values for minting. Tried to mint {to_mint} from account {account_id}, asset {asset_id}.")]
    MustMintPositive {
        account_id: AccountId,
        asset_id: AssetId,
        to_mint: Decimal,
    },
    #[error("Must provide positive values for burning. Tried to burn {to_burn} from account {account_id}, asset {asset_id}.")]
    MustBurnPositive {
        account_id: AccountId,
        asset_id: AssetId,
        to_burn: Decimal,
    },
}

/// Track all information on accounts.
///
/// This includes wallets, public keys, and balances.
///
/// Storage is handled via [MerkleMap], together with a non-serialized
/// set of reverse lookup maps for efficient operations.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Accounts {
    accounts: MerkleMap<AccountId, Account>,
    /// Not serialized! Uses MerkleMap for cheap cloning and modification.
    wallets: MerkleMap<Wallet, AccountId>,
    /// Not serialized! Uses MerkleMap for cheap cloning and modification.
    pubkeys: MerkleMap<PublicKey, AccountId>,
}

/// Information on a specific account
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Account {
    /// Using a [BTreeMap] on the assumption that this data will be relatively small.
    assets: BTreeMap<AssetId, Decimal>,
    wallets: BTreeSet<Wallet>,
    pubkeys: BTreeSet<PublicKey>,
    next_nonce: AccountNonce,
}
impl Account {
    pub(crate) fn get_next_nonce(&self) -> AccountNonce {
        self.next_nonce
    }
}

impl Accounts {
    pub(in crate::core) fn get_assets(
        &self,
        account_id: &AccountId,
    ) -> Option<&BTreeMap<AssetId, Decimal>> {
        self.accounts.get(account_id).map(|x| &x.assets)
    }

    pub(in crate::core) fn mint(
        &mut self,
        account_id: AccountId,
        asset_id: AssetId,
        to_mint: Decimal,
    ) -> Result<(), AccountsError> {
        if to_mint.is_zero() || to_mint.is_sign_negative() {
            return Err(AccountsError::MustMintPositive {
                account_id,
                asset_id,
                to_mint,
            });
        }
        *self
            .accounts
            .get_or_default(account_id)
            .assets
            .entry(asset_id)
            .or_default() += to_mint;
        Ok(())
    }

    pub(in crate::core) fn burn(
        &mut self,
        account_id: AccountId,
        asset_id: AssetId,
        to_burn: Decimal,
    ) -> Result<(), AccountsError> {
        if to_burn.is_zero() || to_burn.is_sign_negative() {
            return Err(AccountsError::MustBurnPositive {
                account_id,
                asset_id,
                to_burn,
            });
        }
        (|| {
            let account = self.accounts.get_mut(&account_id).ok_or(Decimal::ZERO)?;
            let asset = account.assets.get_mut(&asset_id).ok_or(Decimal::ZERO)?;
            match (*asset).cmp(&to_burn) {
                std::cmp::Ordering::Less => Err(*asset),
                std::cmp::Ordering::Equal => {
                    account.assets.remove(&asset_id);
                    Ok(())
                }
                std::cmp::Ordering::Greater => {
                    *asset -= to_burn;
                    Ok(())
                }
            }
        })()
        .map_err(|available| AccountsError::InsufficientBalance {
            account_id,
            asset_id,
            requested: to_burn,
            available,
        })
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&AccountId, &BTreeMap<AssetId, Decimal>)> {
        self.accounts
            .iter()
            .map(|(id, account)| (id, &account.assets))
    }

    /// Adds an empty entry for the given account ID if not already present.
    ///
    /// Mostly useful for writing test cases.
    pub fn add_account(&mut self, id: AccountId) {
        self.accounts.get_or_default(id);
    }

    pub(in crate::core) fn add_pubkey_to_account_error_overlap(
        &mut self,
        account_id: AccountId,
        key: PublicKey,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.pubkeys.contains_key(&key),
            "Pubkey {key} already in use"
        );
        let account = self
            .accounts
            .get_mut(&account_id)
            .with_context(|| format!("Account ID {account_id} not found"))?;
        self.pubkeys.insert(key, account_id);
        account.pubkeys.insert(key);
        Ok(())
    }

    /// Note: panics if the given account ID isn't present.
    pub(in crate::core) fn add_pubkey_to_account_ignore_overlap(
        &mut self,
        account_id: AccountId,
        key: PublicKey,
    ) {
        if self.pubkeys.contains_key(&key) {
            return;
        }
        let account = self.accounts.get_mut(&account_id).unwrap();
        self.pubkeys.insert(key, account_id);
        account.pubkeys.insert(key);
    }

    fn get_next_account_id(&self) -> AccountId {
        self.accounts
            .iter()
            .next_back()
            .map_or(AccountId(0), |(curr_highest, _)| (*curr_highest).next())
    }

    pub(in crate::core) fn get_or_add_account_for_key(&mut self, key: &PublicKey) -> AccountId {
        if let Some(account_id) = self.pubkeys.get(key) {
            return *account_id;
        }
        let account_id = self.get_next_account_id();
        let account = self.accounts.get_or_default(account_id);
        self.pubkeys.insert(*key, account_id);
        account.pubkeys.insert(*key);
        account_id
    }

    pub(in crate::core) fn get_or_add_account_for_wallet(
        &mut self,
        wallet: &Wallet,
    ) -> (AccountId, &mut Account) {
        match self.wallets.get(wallet).cloned() {
            Some(id) => (id, self.accounts.get_mut(&id).unwrap()),
            None => {
                let id = self.get_next_account_id();
                self.wallets.insert(wallet.clone(), id);
                let account = self.accounts.get_or_default(id);
                account.wallets.insert(wallet.clone());
                (id, account)
            }
        }
    }

    pub(in crate::core) fn get_wallets_for(&self, id: AccountId) -> Option<&BTreeSet<Wallet>> {
        self.accounts.get(&id).map(|account| &account.wallets)
    }

    pub(in crate::core) fn get_account_for_key(
        &self,
        public_key: PublicKey,
    ) -> Option<(AccountId, &Account)> {
        let account_id = self.pubkeys.get(&public_key)?;
        Some((*account_id, self.accounts.get(account_id).unwrap()))
    }

    pub(in crate::core) fn get_account_for_wallet(
        &self,
        wallet: &Wallet,
    ) -> Option<(AccountId, &Account)> {
        let account_id = self.wallets.get(wallet)?;
        Some((*account_id, self.accounts.get(account_id).unwrap()))
    }

    pub(in crate::core) fn remove_pubkey_from_account(
        &mut self,
        id: AccountId,
        key: PublicKey,
    ) -> Result<()> {
        let (_, actual_id) = self
            .pubkeys
            .remove(&key)
            .with_context(|| format!("Cannot remove unknown pubkey {key}"))?;
        anyhow::ensure!(
            id == actual_id,
            "Cannot remove pubkey {key} from account {id}, it's actually connected to {actual_id}"
        );
        let was_present = self.accounts.get_mut(&id).unwrap().pubkeys.remove(&key);
        assert!(was_present);
        Ok(())
    }

    pub(in crate::core) fn add_wallet_to_account(
        &mut self,
        account_id: AccountId,
        wallet: &Wallet,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.wallets.contains_key(wallet),
            "Wallet {wallet} already in use"
        );
        let account = self
            .accounts
            .get_mut(&account_id)
            .with_context(|| format!("Account ID {account_id} not found"))?;
        self.wallets.insert(wallet.clone(), account_id);
        account.wallets.insert(wallet.clone());
        Ok(())
    }

    pub(crate) fn remove_wallet_from_account(
        &mut self,
        id: AccountId,
        wallet: &Wallet,
    ) -> Result<()> {
        let (_, actual_id) = self
            .wallets
            .remove(wallet)
            .with_context(|| format!("Cannot remove unknown wallet {wallet}"))?;
        anyhow::ensure!(id == actual_id, "Cannot remove wallet {wallet} from account {id}, it's actually connected to {actual_id}");
        let was_present = self.accounts.get_mut(&id).unwrap().wallets.remove(wallet);
        assert!(was_present);
        Ok(())
    }

    /// Use the nonce for the given public key
    ///
    /// Returns an error if the wrong nonce is provided. Initiates the account if necessary.
    pub(crate) fn use_nonce(
        &mut self,
        pubkey: PublicKey,
        nonce: AccountNonce,
    ) -> Result<AccountId> {
        match self.pubkeys.get(&pubkey) {
            Some(account_id) => {
                let account = self.accounts.get_mut(account_id).unwrap();
                if account.next_nonce != nonce {
                    return Err(KolmeError::InvalidNonce {
                        pubkey: Box::new(pubkey),
                        account_id: *account_id,
                        expected: account.next_nonce,
                        actual: nonce,
                    }
                    .into());
                }
                account.next_nonce = account.next_nonce.next();
                Ok(*account_id)
            }
            None => {
                let account_id = self.get_next_account_id();
                let account = self.accounts.get_or_default(account_id);
                self.pubkeys.insert(pubkey, account_id);
                account.pubkeys.insert(pubkey);
                anyhow::ensure!(nonce == account.next_nonce, "New account for pubkey {pubkey} expects an initial nonce of {}, received {nonce}", account.next_nonce);
                account.next_nonce = account.next_nonce.next();
                Ok(account_id)
            }
        }
    }
}

impl MerkleSerialize for Accounts {
    fn merkle_serialize(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        serializer.store(&self.accounts)
    }
}

impl MerkleDeserialize for Accounts {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let accounts: MerkleMap<AccountId, Account> = deserializer.load()?;
        let mut wallets = MerkleMap::new();
        let mut pubkeys = MerkleMap::new();
        for (id, account) in &accounts {
            for wallet in &account.wallets {
                let x = wallets.insert(wallet.clone(), *id);
                if let Some((_, id2)) = x {
                    return Err(MerkleSerialError::Other(format!(
                        "Wallet {wallet} used in both account {id} and {id2}"
                    )));
                }
            }
            for pubkey in &account.pubkeys {
                let x = pubkeys.insert(*pubkey, *id);
                if let Some((_, id2)) = x {
                    return Err(MerkleSerialError::Other(format!(
                        "Pubkey {pubkey} used in both account {id} and {id2}"
                    )));
                }
            }
        }
        Ok(Self {
            accounts,
            wallets,
            pubkeys,
        })
    }
}

impl MerkleSerialize for Account {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Account {
            assets,
            wallets,
            pubkeys,
            next_nonce,
        } = self;
        serializer.store(assets)?;
        serializer.store(wallets)?;
        serializer.store(pubkeys)?;
        serializer.store(next_nonce)?;
        Ok(())
    }
}

impl MerkleDeserialize for Account {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Account {
            assets: deserializer.load()?,
            wallets: deserializer.load()?,
            pubkeys: deserializer.load()?,
            next_nonce: deserializer.load()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn test_add_remove() {
        let mut balances = Accounts::default();
        let id = AccountId(101);
        balances.mint(id, AssetId(201), dec!(52.1)).unwrap();
        balances.burn(id, AssetId(201), dec!(52.1)).unwrap();
        let mut expected = Accounts::default();
        expected.add_account(id);
        assert_eq!(balances, expected);
    }

    #[test]
    fn test_multi_adds() {
        let mut balances = Accounts::default();
        let id = AccountId(101);
        balances.mint(id, AssetId(201), dec!(50)).unwrap();
        balances.mint(id, AssetId(201), dec!(2.1)).unwrap();
        balances.burn(id, AssetId(201), dec!(52.1)).unwrap();
        let mut expected = Accounts::default();
        expected.add_account(id);
        assert_eq!(balances, expected);
    }

    #[test]
    fn test_multi_removes() {
        let mut accounts = Accounts::default();
        let id = AccountId(101);
        accounts.mint(id, AssetId(201), dec!(52.1)).unwrap();
        accounts.burn(id, AssetId(201), dec!(50)).unwrap();
        accounts.burn(id, AssetId(201), dec!(2.1)).unwrap();
        let mut expected = Accounts::default();
        expected.add_account(id);
        assert_eq!(accounts, expected);
    }

    #[test]
    fn test_cannot_over_remove() {
        let mut accounts = Accounts::default();
        accounts
            .burn(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap_err();
    }

    #[test]
    fn test_add_negatives() {
        let mut accounts = Accounts::default();
        accounts
            .mint(AccountId(101), AssetId(201), dec!(-5))
            .unwrap_err();
        accounts
            .mint(AccountId(101), AssetId(201), dec!(0))
            .unwrap_err();
        accounts
            .mint(AccountId(101), AssetId(201), dec!(-0))
            .unwrap_err();
    }

    #[test]
    fn test_remove_negatives() {
        let mut accounts = Accounts::default();
        accounts
            .mint(AccountId(101), AssetId(201), dec!(5000000))
            .unwrap();

        accounts
            .burn(AccountId(101), AssetId(201), dec!(-5))
            .unwrap_err();
        accounts
            .burn(AccountId(101), AssetId(201), dec!(0))
            .unwrap_err();
        accounts
            .burn(AccountId(101), AssetId(201), dec!(-0))
            .unwrap_err();
        accounts
            .burn(AccountId(101), AssetId(201), dec!(5))
            .unwrap();
    }
}
