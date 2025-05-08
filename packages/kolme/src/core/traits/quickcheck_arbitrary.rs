use crate::core::types::{
    AccountId, AccountNonce, AssetConfig, AssetId, AssetName, BridgeContract, Wallet,
};
use quickcheck::{Arbitrary, Gen};

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
