use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Context, Result};
use kolme::{
    AccountId, AssetId, MerkleDeserialize, MerkleMap, MerkleSerialError, MerkleSerialize,
    PublicKey, Wallet,
};
use rust_decimal::{dec, Decimal};

use crate::{AppState, BalanceChange, Odds, OUTCOME_COUNT};

#[derive(Clone)]
pub struct State {
    admin_keys: BTreeSet<PublicKey>,
    markets: MerkleMap<u64, Market>,
    state: AppState,
    strategic_reserve: BTreeMap<AssetId, Decimal>,
}

impl MerkleSerialize for State {
    fn merkle_serialize(
        &self,
        serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let State {
            admin_keys,
            markets,
            state,
            strategic_reserve,
        } = self;
        serializer.store(admin_keys)?;
        serializer.store(markets)?;
        serializer.store(state)?;
        serializer.store(strategic_reserve)?;
        Ok(())
    }
}

impl MerkleDeserialize for State {
    fn merkle_deserialize(
        deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            admin_keys: deserializer.load()?,
            markets: deserializer.load()?,
            state: deserializer.load()?,
            strategic_reserve: deserializer.load()?,
        })
    }
}

impl State {
    pub fn new(admin_keys: impl IntoIterator<Item = PublicKey>) -> Self {
        Self {
            admin_keys: BTreeSet::from_iter(admin_keys),
            markets: MerkleMap::new(),
            state: AppState::Uninitialized,
            strategic_reserve: BTreeMap::new(),
        }
    }
}

// funds provided to fund every market
const HOUSE_FUNDS: Decimal = dec!(1000); // 1000 coins with 6 decimals

#[derive(
    PartialEq, serde::Serialize, serde::Deserialize, Clone, strum::AsRefStr, strum::EnumString,
)]
enum MarketState {
    Operational,
    Settled,
}

impl MerkleSerialize for MarketState {
    fn merkle_serialize(
        &self,
        serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(self.as_ref())
    }
}

impl MerkleDeserialize for MarketState {
    fn merkle_deserialize(
        deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer
            .load_str()?
            .parse()
            .map_err(MerkleSerialError::custom)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Market {
    state: MarketState,
    id: u64,
    asset_id: AssetId,
    name: String,
    bets: Vec<Bet>,
    total_funds: Decimal,
    available_liquidity: Decimal,
    max_allowed_liability: Decimal,
    liabilities: [Decimal; OUTCOME_COUNT as usize],
}

impl MerkleSerialize for Market {
    fn merkle_serialize(
        &self,
        serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Market {
            state,
            id,
            asset_id,
            name,
            bets,
            total_funds,
            available_liquidity,
            max_allowed_liability,
            liabilities,
        } = self;
        serializer.store(state)?;
        serializer.store(id)?;
        serializer.store(asset_id)?;
        serializer.store(name)?;
        serializer.store(bets)?;
        serializer.store(total_funds)?;
        serializer.store(available_liquidity)?;
        serializer.store(max_allowed_liability)?;
        serializer.store(liabilities)?;
        todo!()
    }
}

impl MerkleDeserialize for Market {
    fn merkle_deserialize(
        deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            state: deserializer.load()?,
            id: deserializer.load()?,
            asset_id: deserializer.load()?,
            name: deserializer.load()?,
            bets: deserializer.load()?,
            total_funds: deserializer.load()?,
            available_liquidity: deserializer.load()?,
            max_allowed_liability: deserializer.load()?,
            liabilities: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Bet {
    bettor: AccountId,
    wallet: Wallet,
    amount: Decimal,
    outcome: u8,
    returns: Decimal,
}

impl MerkleSerialize for Bet {
    fn merkle_serialize(
        &self,
        serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Bet {
            bettor,
            wallet,
            amount,
            outcome,
            returns,
        } = self;
        serializer.store(bettor)?;
        serializer.store(wallet)?;
        serializer.store(amount)?;
        serializer.store(outcome)?;
        serializer.store(returns)?;
        Ok(())
    }
}

impl MerkleDeserialize for Bet {
    fn merkle_deserialize(
        deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            bettor: deserializer.load()?,
            wallet: deserializer.load()?,
            amount: deserializer.load()?,
            outcome: deserializer.load()?,
            returns: deserializer.load()?,
        })
    }
}

impl State {
    pub(crate) fn add_market(
        &mut self,
        signing_key: PublicKey,
        id: u64,
        asset_id: AssetId,
        name: &str,
    ) -> Result<()> {
        self.assert_admin(signing_key)?;
        self.assert_operational()?;

        if self.markets.contains_key(&id) {
            anyhow::bail!("Market with id {id} already exist");
        }
        let provision_amount = HOUSE_FUNDS;
        let mut sr_funds = self.strategic_reserve.get_mut(&asset_id).with_context(|| {
            format!("Application doesn't have asset {asset_id} to fund the market")
        })?;
        if *sr_funds < HOUSE_FUNDS {
            anyhow::bail!("Application doesn't have enough funds to add the market")
        }
        sr_funds -= HOUSE_FUNDS;
        let market = Market::new(id, asset_id, name, provision_amount);

        self.markets.insert(id, market);
        Ok(())
    }

    pub(crate) fn settle_market(
        &mut self,
        signing_key: PublicKey,
        market_id: u64,
        outcome: u8,
    ) -> Result<Vec<BalanceChange>> {
        self.assert_admin(signing_key)?;
        assert_outcome(outcome)?;
        self.assert_operational()?;
        let market = self.get_operational_market_mut(market_id)?;
        let (changes, house_payout) = market.settle(outcome)?;
        let asset_id = market.asset_id;
        let asset_balance = self.strategic_reserve.entry(asset_id).or_default();
        *asset_balance += house_payout;
        Ok(changes)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_bet(
        &mut self,
        bettor: AccountId,
        balances: BTreeMap<AssetId, Decimal>,
        market_id: u64,
        wallet: Wallet,
        amount: Decimal,
        outcome: u8,
        odds: Odds,
    ) -> std::result::Result<BalanceChange, anyhow::Error> {
        assert_outcome(outcome)?;
        self.assert_operational()?;
        let Some(market) = self.markets.get_mut(&market_id) else {
            anyhow::bail!("Market with id {market_id} doesn't exist");
        };
        if !balances
            .get(&market.asset_id)
            .is_some_and(|balance| balance >= &amount)
        {
            anyhow::bail!("Account doesn't have enough funds to place this bet")
        }
        market.place_bet(bettor, wallet, amount, outcome, odds)?;
        Ok(BalanceChange::Burn {
            asset_id: market.asset_id,
            amount,
            account: bettor,
        })
    }

    pub fn send_funds(&mut self, asset_id: AssetId, amount: Decimal) -> Result<()> {
        let entry = self.strategic_reserve.entry(asset_id).or_default();
        *entry += amount;
        Ok(())
    }

    pub(crate) fn initialize(
        &mut self,
        signing_key: PublicKey,
    ) -> std::result::Result<(), anyhow::Error> {
        self.assert_admin(signing_key)?;
        if self.state == AppState::Operational {
            Err(anyhow!("Can't initialize application in operational state"))
        } else {
            self.state = AppState::Operational;
            Ok(())
        }
    }

    fn get_operational_market_mut(&mut self, market_id: u64) -> Result<&'_ mut Market> {
        let Some(market) = self.markets.get_mut(&market_id) else {
            return Err(anyhow!("Market with id {market_id} doesn't exist"));
        };
        if market.state != MarketState::Operational {
            return Err(anyhow!("Market is not operational"));
        }
        Ok(market)
    }

    fn assert_operational(&self) -> Result<()> {
        if self.state != AppState::Operational {
            Err(anyhow!("Application is not operational yet"))
        } else {
            Ok(())
        }
    }

    fn assert_admin(&self, signing_key: PublicKey) -> Result<()> {
        if !self.admin_keys.contains(&signing_key) {
            Err(anyhow!("Admin permissions are required for this action"))
        } else {
            Ok(())
        }
    }
}

fn assert_outcome(outcome: u8) -> Result<()> {
    if outcome >= OUTCOME_COUNT {
        Err(anyhow!(
            "Market supports only 0, 1 or 2 as outcomes but {outcome} was supplied"
        ))
    } else {
        Ok(())
    }
}
impl Market {
    pub fn new(id: u64, asset_id: AssetId, name: impl Into<String>, funds: Decimal) -> Self {
        Self {
            state: MarketState::Operational,
            id,
            asset_id,
            name: name.into(),
            total_funds: funds,
            available_liquidity: funds,
            liabilities: [Decimal::ZERO; OUTCOME_COUNT as usize],
            max_allowed_liability: funds,
            bets: vec![],
        }
    }

    fn place_bet(
        &mut self,
        bettor: AccountId,
        wallet: Wallet,
        amount: Decimal,
        outcome: u8,
        odds: Odds,
    ) -> Result<()> {
        let outcome_pos = usize::from(outcome);
        let odds = odds[outcome_pos];
        let returns = amount * odds;
        if self.liabilities[outcome_pos] + returns > self.max_allowed_liability {
            return Err(anyhow!(
                "House does not have enough funds to cover this bet"
            ));
        }
        self.liabilities[outcome_pos] += returns;
        self.total_funds += amount;

        self.bets.push(Bet {
            bettor,
            wallet,
            amount,
            outcome,
            returns,
        });
        Ok(())
    }

    fn settle(&mut self, outcome: u8) -> Result<(Vec<BalanceChange>, Decimal)> {
        let mut transfers = vec![];
        let mut payouts = Decimal::ZERO;

        for bet in &self.bets {
            if bet.outcome == outcome {
                payouts += bet.returns;
                transfers.push(BalanceChange::Mint {
                    asset_id: self.asset_id,
                    amount: bet.returns,
                    account: bet.bettor,
                    withdraw_wallet: bet.wallet.clone(),
                });
            }
        }
        assert!(payouts == self.liabilities[usize::from(outcome)]);

        let house_payout = self.total_funds - payouts;
        self.state = MarketState::Settled;
        Ok((transfers, house_payout))
    }
}
