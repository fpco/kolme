use std::collections::HashMap;

use anyhow::{anyhow, Result};
use kolme::{AccountId, AssetId, Wallet};
use rust_decimal::{dec, Decimal};

use crate::{Config, Odds, Transfer, OUTCOME_COUNT};

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct State {
    markets: HashMap<u64, Market>,
    config: Config,
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
        id: u64,
        asset_id: AssetId,
        name: &str,
    ) -> Result<Transfer> {
        let Config::Configured {
            sr_account,
            market_funds_account,
        } = self.config
        else {
            return Err(anyhow!("Application accounts are not configured yet"));
        };
        if self.markets.contains_key(&id) {
            return Err(anyhow!("Market with id {id} already exist"));
        }
        let provision_amount = HOUSE_FUNDS;
        let market = Market::new(id, asset_id, name, provision_amount);

        self.markets.insert(id, market);
        Ok(Transfer {
            asset_id,
            amount: provision_amount,
            from: sr_account,
            to: market_funds_account,
            withdraw_wallet: None,
        })
    }

    pub(crate) fn settle_market(&mut self, market_id: u64, outcome: u8) -> Result<Vec<Transfer>> {
        assert_outcome(outcome)?;
        let Config::Configured {
            sr_account,
            market_funds_account,
        } = self.config
        else {
            return Err(anyhow!("application accounts are not configured"));
        };
        let market = self.get_operational_market_mut(market_id)?;
        market.settle(market_funds_account, sr_account, outcome)
    }

    pub(crate) fn place_bet(
        &mut self,
        bettor: AccountId,
        market_id: u64,
        wallet: Wallet,
        amount: Decimal,
        outcome: u8,
        odds: Odds,
    ) -> std::result::Result<Transfer, anyhow::Error> {
        assert_outcome(outcome)?;
        let market_funds_account = self.configured_market_funds_account()?;
        let Some(market) = self.markets.get_mut(&market_id) else {
            return Err(anyhow!("Market with id {market_id} doesn't exist"));
        };
        market.place_bet(bettor, wallet, amount, outcome, odds)?;
        Ok(Transfer {
            asset_id: market.asset_id,
            amount,
            from: bettor,
            to: market_funds_account,
            withdraw_wallet: None,
        })
    }

    pub(crate) fn set_config(
        &mut self,
        sr_account: AccountId,
        market_funds_account: AccountId,
    ) -> std::result::Result<(), anyhow::Error> {
        match self.config {
            Config::Configured { market_funds_account: current,.. } if current != market_funds_account => {
                return Err(anyhow!("Can't switch market_funds_account, it was already set to {current} but {market_funds_account} was supplied"))
            }
            _ =>{}
        }
        self.config = Config::Configured {
            sr_account,
            market_funds_account,
        };
        Ok(())
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

    fn configured_market_funds_account(&self) -> Result<AccountId> {
        match self.config {
            Config::Configured {
                market_funds_account,
                ..
            } => Ok(market_funds_account),
            _ => Err(anyhow!("market funds account is not configured")),
        }
    }
}

fn assert_outcome(outcome: u8) -> Result<()> {
    if outcome >= 3 {
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
            return Err(anyhow!("House has not enough funds to cover this bet"));
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

    fn settle(
        &mut self,
        market_funds_account: AccountId,
        sr_account: AccountId,
        outcome: u8,
    ) -> Result<Vec<Transfer>> {
        let mut transfers = vec![];
        let mut payouts = Decimal::ZERO;

        for bet in &self.bets {
            if bet.outcome == outcome {
                payouts += bet.returns;
                transfers.push(Transfer {
                    asset_id: self.asset_id,
                    amount: bet.returns,
                    from: market_funds_account,
                    to: bet.bettor,
                    withdraw_wallet: Some(bet.wallet.clone()),
                });
            }
        }
        assert!(payouts == self.liabilities[usize::from(outcome)]);

        if self.total_funds > payouts {
            transfers.push(Transfer {
                asset_id: self.asset_id,
                amount: (self.total_funds - payouts),
                from: market_funds_account,
                to: sr_account,
                withdraw_wallet: None, // house withdrawals are expected to be done separately
            })
        }

        self.state = MarketState::Settled;
        Ok(transfers)
    }
}
