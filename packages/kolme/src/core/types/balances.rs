use crate::*;

#[derive(snafu::Snafu, Debug)]
pub enum BalancesError {
    #[snafu(display("Insufficient balance for account {account_id}, asset {asset_id}. Requested: {requested}. Available: {available}."))]
    InsufficientBalance {
        account_id: AccountId,
        asset_id: AssetId,
        requested: Decimal,
        available: Decimal,
    },
    #[snafu(display("Must provide positive values for minting. Tried to mint {to_mint} from account {account_id}, asset {asset_id}."))]
    MustMintPositive {
        account_id: AccountId,
        asset_id: AssetId,
        to_mint: Decimal,
    },
    #[snafu(display("Must provide positive values for burning. Tried to burn {to_burn} from account {account_id}, asset {asset_id}."))]
    MustBurnPositive {
        account_id: AccountId,
        asset_id: AssetId,
        to_burn: Decimal,
    },
}

/// Track balances of all accounts for all assets.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Balances(MerkleMap<AccountId, BTreeMap<AssetId, Decimal>>);

impl Balances {
    pub(in crate::core) fn get(
        &self,
        account_id: &AccountId,
    ) -> Option<&BTreeMap<AssetId, Decimal>> {
        self.0.get(account_id)
    }

    pub(in crate::core) fn mint(
        &mut self,
        account_id: AccountId,
        asset_id: AssetId,
        to_mint: Decimal,
    ) -> Result<(), BalancesError> {
        if to_mint.is_zero() || to_mint.is_sign_negative() {
            return Err(BalancesError::MustMintPositive {
                account_id,
                asset_id,
                to_mint,
            });
        }
        *self
            .0
            .get_or_insert(account_id, BTreeMap::new)
            .entry(asset_id)
            .or_default() += to_mint;
        Ok(())
    }

    pub(in crate::core) fn burn(
        &mut self,
        account_id: AccountId,
        asset_id: AssetId,
        to_burn: Decimal,
    ) -> Result<(), BalancesError> {
        if to_burn.is_zero() || to_burn.is_sign_negative() {
            return Err(BalancesError::MustBurnPositive {
                account_id,
                asset_id,
                to_burn,
            });
        }
        (|| {
            let account = self.0.get_mut(&account_id).ok_or(Decimal::ZERO)?;
            let asset = account.get_mut(&asset_id).ok_or(Decimal::ZERO)?;
            match (*asset).cmp(&to_burn) {
                std::cmp::Ordering::Less => Err(Decimal::ZERO),
                std::cmp::Ordering::Equal => {
                    account.remove(&asset_id);
                    if account.is_empty() {
                        self.0.remove(&account_id);
                    }
                    Ok(())
                }
                std::cmp::Ordering::Greater => {
                    *asset -= to_burn;
                    Ok(())
                }
            }
        })()
        .map_err(|available| BalancesError::InsufficientBalance {
            account_id,
            asset_id,
            requested: to_burn,
            available,
        })
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&AccountId, &BTreeMap<AssetId, Decimal>)> {
        self.0.iter()
    }
}

impl MerkleSerialize for Balances {
    fn merkle_serialize(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        self.0.merkle_serialize(serializer)
    }
}

impl MerkleDeserialize for Balances {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        MerkleDeserialize::merkle_deserialize(deserializer).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn test_add_remove() {
        let mut balances = Balances::default();
        balances
            .mint(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap();
        balances
            .burn(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap();
        assert_eq!(balances, Balances::default());
    }

    #[test]
    fn test_multi_adds() {
        let mut balances = Balances::default();
        balances
            .mint(AccountId(101), AssetId(201), dec!(50))
            .unwrap();
        balances
            .mint(AccountId(101), AssetId(201), dec!(2.1))
            .unwrap();
        balances
            .burn(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap();
        assert_eq!(balances, Balances::default());
    }

    #[test]
    fn test_multi_removes() {
        let mut balances = Balances::default();
        balances
            .mint(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap();
        balances
            .burn(AccountId(101), AssetId(201), dec!(50))
            .unwrap();
        balances
            .burn(AccountId(101), AssetId(201), dec!(2.1))
            .unwrap();
        assert_eq!(balances, Balances::default());
    }

    #[test]
    fn test_cannot_over_remove() {
        let mut balances = Balances::default();
        balances
            .burn(AccountId(101), AssetId(201), dec!(52.1))
            .unwrap_err();
    }

    #[test]
    fn test_add_negatives() {
        let mut balances = Balances::default();
        balances
            .mint(AccountId(101), AssetId(201), dec!(-5))
            .unwrap_err();
        balances
            .mint(AccountId(101), AssetId(201), dec!(0))
            .unwrap_err();
        balances
            .mint(AccountId(101), AssetId(201), dec!(-0))
            .unwrap_err();
    }

    #[test]
    fn test_remove_negatives() {
        let mut balances = Balances::default();
        balances
            .mint(AccountId(101), AssetId(201), dec!(5000000))
            .unwrap();

        balances
            .burn(AccountId(101), AssetId(201), dec!(-5))
            .unwrap_err();
        balances
            .burn(AccountId(101), AssetId(201), dec!(0))
            .unwrap_err();
        balances
            .burn(AccountId(101), AssetId(201), dec!(-0))
            .unwrap_err();
        balances
            .burn(AccountId(101), AssetId(201), dec!(5))
            .unwrap();
    }
}
