use cosmwasm_std::{testing::MockApi, Api};
use sha2::{Digest, Sha256};

#[test]
fn test_signature() {
    const MSG: &str = "This is a test message";
    const SIGNATURE: &str = "D236913E08C9DE2BF776321FB8B32ABE1E6E92685E5D3BF9511F4F14FBC5C96C7A25427CA4150554AC4430C18DBD268DE9BB66E43892B9D37867970A9886B64F";
    const RECOVERY: u8 = 0;
    const PUBLIC: &str = "0264eb26609d15e709227b9ddc46c11a738b210bb237949aa86d7d490a35ae0f0a";
    // Secret key
    // 658c3528422eb527b4c108b8f6d1e5f629543c304ea49cf608c67794424291c4

    let message_hash = Sha256::digest(MSG);

    let x = MockApi::default()
        .secp256k1_recover_pubkey(&message_hash, &hex::decode(SIGNATURE).unwrap(), RECOVERY)
        .unwrap();
    assert_eq!(&x, &hex::decode(PUBLIC).unwrap());
}
