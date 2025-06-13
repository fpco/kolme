//! Cosmos-specific helpers.

use cosmos::proto::{cosmos::base::abci::v1beta1::TxResponse, tendermint::abci::EventAttribute};
use shared::types::BridgeEventId;
use smallvec::SmallVec;

/// Parse any bridge event IDs from the given Cosmos transaction response.
pub fn parse_bridge_event_ids(tx_response: &TxResponse) -> SmallVec<[BridgeEventId; 4]> {
    let mut res = SmallVec::new();
    for event in &tx_response.events {
        if event.r#type != "wasm-bridge-event" {
            continue;
        }
        for EventAttribute {
            key,
            value,
            index: _,
        } in &event.attributes
        {
            if key != "id" {
                continue;
            }
            if let Ok(id) = value.parse() {
                res.push(BridgeEventId(id));
            }
        }
    }
    res
}
