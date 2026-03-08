use crate::core::types::{
    AccountId, AccountNonce, AssetConfig, AssetId, AssetName, BridgeContract, ChainConfig,
    ExternalChain, Wallet,
};
use quickcheck::{Arbitrary, Gen};
use std::collections::BTreeMap;

macro_rules! arbitrary_for_wrapper_type {
    ($wrapper_type: ty, $wrapped_type: ty) => {
        impl Arbitrary for $wrapper_type {
            fn arbitrary(g: &mut Gen) -> Self {
                Self(<$wrapped_type>::arbitrary(g))
            }
            fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
                Box::new(self.0.shrink().map(Self))
            }
        }
    };
}

arbitrary_for_wrapper_type!(AssetId, u64);
arbitrary_for_wrapper_type!(AccountId, u64);
arbitrary_for_wrapper_type!(AccountNonce, u64);
arbitrary_for_wrapper_type!(AssetName, String);
arbitrary_for_wrapper_type!(Wallet, String);

impl Arbitrary for BridgeContract {
    fn arbitrary(g: &mut Gen) -> Self {
        let values = [
            BridgeContract::NeededCosmosBridge {
                code_id: <u64>::arbitrary(g),
            },
            BridgeContract::NeededSolanaBridge {
                program_id: <String>::arbitrary(g),
            },
            BridgeContract::Deployed(<String>::arbitrary(g)),
        ];
        g.choose(&values).unwrap().clone()
    }
}
impl Arbitrary for AssetConfig {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            decimals: <u8>::arbitrary(g),
            asset_id: <AssetId>::arbitrary(g),
        }
    }
}

impl Arbitrary for ChainConfig {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            assets: <BTreeMap<AssetName, AssetConfig>>::arbitrary(g),
            bridge: <BridgeContract>::arbitrary(g),
        }
    }
}

impl Arbitrary for ExternalChain {
    fn arbitrary(g: &mut Gen) -> Self {
        let values = [
            ExternalChain::OsmosisTestnet,
            ExternalChain::NeutronTestnet,
            ExternalChain::OsmosisLocal,
            ExternalChain::SolanaMainnet,
            ExternalChain::SolanaTestnet,
            ExternalChain::SolanaDevnet,
            ExternalChain::SolanaLocal,
            ExternalChain::EthereumMainnet,
            ExternalChain::EthereumSepolia,
            ExternalChain::EthereumLocal,
            #[cfg(feature = "pass_through")]
            ExternalChain::PassThrough,
        ];
        *g.choose(&values).unwrap()
    }
}
