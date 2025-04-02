use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use kolme::{AccountId, AssetId, PublicKey, Wallet};
use rust_decimal::{dec, Decimal};

use crate::{AppState, BalanceChange, Odds, OUTCOME_COUNT};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct State {
    admin_keys: HashSet<PublicKey>,
    markets: HashMap<u64, Market>,
    state: AppState,
    strategic_reserve: BTreeMap<AssetId, Decimal>,
}

impl State {
    pub fn new(admin_keys: impl IntoIterator<Item = PublicKey>) -> Self {
        Self {
            admin_keys: HashSet::from_iter(admin_keys),
            markets: HashMap::new(),
            state: AppState::Uninitialized,
            strategic_reserve: BTreeMap::new(),
        }
    }
}

// funds provided to fund every market
const HOUSE_FUNDS: Decimal = dec!(1000); // 1000 coins with 6 decimals

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Clone)]
enum MarketState {
    Operational,
    Settled,
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Bet {
    bettor: AccountId,
    wallet: Wallet,
    amount: Decimal,
    outcome: u8,
    returns: Decimal,
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

    pub(crate) fn place_bet(
        &mut self,
        bettor: AccountId,
        market_id: u64,
        wallet: Wallet,
        amount: Decimal,
        outcome: u8,
        odds: Odds,
    ) -> std::result::Result<BalanceChange, anyhow::Error> {
        assert_outcome(outcome)?;
        self.assert_operational()?;
        let Some(market) = self.markets.get_mut(&market_id) else {
            return Err(anyhow!("Market with id {market_id} doesn't exist"));
        };
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
