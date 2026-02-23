use crate::*;
use alloy::primitives::{Address, B256, U256};

pub(crate) const FUNDS_RECEIVED_EVENT_SIGNATURE: &str = "FundsReceived(address,uint256)";
const FUNDS_RECEIVED_TOPICS_LEN: usize = 2;
const U256_WORD_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundsReceivedEvent {
    pub sender: Address,
    pub amount: U256,
}

pub(crate) fn funds_received_event_topic0() -> B256 {
    alloy::primitives::keccak256(FUNDS_RECEIVED_EVENT_SIGNATURE.as_bytes())
}

pub(crate) fn decode_funds_received_log(topics: &[B256], data: &[u8]) -> Result<FundsReceivedEvent> {
    anyhow::ensure!(
        topics.len() == FUNDS_RECEIVED_TOPICS_LEN,
        "Expected {FUNDS_RECEIVED_TOPICS_LEN} topics for {FUNDS_RECEIVED_EVENT_SIGNATURE}, got {}",
        topics.len()
    );
    anyhow::ensure!(
        topics[0] == funds_received_event_topic0(),
        "Unexpected topic0 for {FUNDS_RECEIVED_EVENT_SIGNATURE}"
    );
    anyhow::ensure!(
        data.len() == U256_WORD_BYTES,
        "Expected {U256_WORD_BYTES} data bytes for {FUNDS_RECEIVED_EVENT_SIGNATURE}, got {}",
        data.len()
    );

    let sender = Address::from_slice(&topics[1].as_slice()[12..]);
    let amount = U256::from_be_slice(data);

    Ok(FundsReceivedEvent { sender, amount })
}

pub async fn listen<App: KolmeApp>(
    _kolme: Kolme<App>,
    _secret: SecretKey,
    chain: EthereumChain,
    contract: String,
) -> Result<()> {
    tokio::task::yield_now().await;
    tracing::warn!(
        "Ethereum listener scaffold is present but not implemented yet (chain={chain:?}, contract={contract})"
    );
    Ok(())
}

pub async fn sanity_check_contract(
    _rpc_url: &str,
    _contract: &str,
    _info: &GenesisInfo,
) -> Result<()> {
    tokio::task::yield_now().await;
    anyhow::bail!("Ethereum listener contract checks are not implemented yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funds_received_topic_hash_matches_constant() {
        const FUNDS_RECEIVED_EVENT_TOPIC0_HEX: &str =
            "0x8e47b87b0ef542cdfa1659c551d88bad38aa7f452d2bbb349ab7530dfec8be8f";
        let expected = FUNDS_RECEIVED_EVENT_TOPIC0_HEX.parse::<B256>().unwrap();
        assert_eq!(funds_received_event_topic0(), expected);
    }

    #[test]
    fn decode_funds_received_log_extracts_sender_and_amount() {
        let sender = "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap();

        let mut sender_topic = [0u8; 32];
        sender_topic[12..].copy_from_slice(sender.as_slice());

        let amount = U256::from(42u64);
        let mut data = [0u8; 32];
        amount.to_be_bytes::<32>().clone_into(&mut data);

        let decoded = decode_funds_received_log(
            &[funds_received_event_topic0(), B256::from(sender_topic)],
            &data,
        )
        .unwrap();

        assert_eq!(decoded.sender, sender);
        assert_eq!(decoded.amount, amount);
    }
}
