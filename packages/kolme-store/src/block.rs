use merkle_map::{
    MerkleDeserialize, MerkleDeserializeRaw, MerkleSerialError, MerkleSerialize,
    MerkleSerializeRaw, Sha256Hash,
};
use std::sync::Arc;

/// Contents of a block to be stored in a database.
#[derive(Debug)]
pub struct StorableBlock<Block> {
    pub height: u64,
    pub blockhash: Sha256Hash,
    pub txhash: Sha256Hash,
    pub block: Arc<Block>,
}

impl<Block> Clone for StorableBlock<Block> {
    fn clone(&self) -> Self {
        Self {
            height: self.height,
            blockhash: self.blockhash,
            txhash: self.txhash,
            block: self.block.clone(),
        }
    }
}

impl<Block: MerkleSerializeRaw> MerkleSerialize for StorableBlock<Block> {
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Self {
            height,
            blockhash,
            txhash,
            block,
        } = self;
        serializer.store(height)?;
        serializer.store(blockhash)?;
        serializer.store(txhash)?;
        serializer.store(block)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        2
    }
}

impl<Block: MerkleSerializeRaw + MerkleDeserializeRaw> MerkleDeserialize for StorableBlock<Block> {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let height = deserializer.load()?;
        let blockhash = deserializer.load()?;
        let txhash = deserializer.load()?;
        let block = deserializer.load()?;

        match version {
            0 | 1 => {
                deserializer.ignore_remaining();
            }
            2 => (),
            _ => unreachable!(
                "Validation of version is carried out at the trait level on MerkleDeserializeRaw"
            ),
        };

        Ok(Self {
            height,
            blockhash,
            txhash,
            block,
        })
    }
}

/// The Merkle hashes associated with a block.
pub struct BlockHashes {
    pub framework_state: Sha256Hash,
    pub app_state: Sha256Hash,
    pub logs: Sha256Hash,
}

#[cfg(test)]
mod tests {
    use super::StorableBlock;
    use merkle_map::{
        CachedBytes, MerkleContents, MerkleDeserialize, MerkleDeserializer, MerkleSerialError,
        MerkleSerialize, MerkleSerializer, Sha256Hash,
    };
    use std::sync::Arc;

    #[derive(Debug, PartialEq, Eq)]
    struct DummyBytes(Vec<u8>);

    impl MerkleSerialize for DummyBytes {
        fn merkle_serialize(
            &self,
            serializer: &mut MerkleSerializer,
        ) -> Result<(), MerkleSerialError> {
            serializer.store(&self.0.len())?;
            serializer.store_raw_bytes(&self.0);

            Ok(())
        }
    }

    impl MerkleDeserialize for DummyBytes {
        fn merkle_deserialize(
            deserializer: &mut MerkleDeserializer,
            version: usize,
        ) -> Result<DummyBytes, MerkleSerialError> {
            assert_eq!(version, 0, "Invalid serialization of DummyBytes");
            let len = deserializer.load_usize()?;
            let bytes = deserializer.load_raw_bytes(len)?;

            Ok(DummyBytes(bytes.to_vec()))
        }
    }

    #[tokio::test]
    async fn it_deserializes_from_payload_with_previous_version() {
        // Arrange
        let payload: Arc<[u8]> = vec![
            0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 68, 117, 109, 109, 121, 32, 98,
            108, 111, 99, 107, 0, 20, 68, 117, 109, 109, 121, 32, 70, 114, 97, 109, 101, 119, 111,
            114, 107, 83, 116, 97, 116, 101, 0, 14, 68, 117, 109, 109, 121, 32, 65, 112, 112, 83,
            116, 97, 116, 101, 2, 1, 5, 68, 117, 109, 109, 121, 1, 4, 76, 111, 103, 115,
        ]
        .into();
        let contents = MerkleContents {
            payload: CachedBytes::new_bytes(payload),
            children: vec![].into(),
        };

        // Act
        let storable_block =
            merkle_map::api::deserialize::<StorableBlock<DummyBytes>>(Arc::new(contents))
                .expect("Unable to deserialize block with version 0");

        // Assert
        assert_eq!(
            storable_block.blockhash,
            Sha256Hash::from_array(Default::default()),
            "Blockhash was not deserialized correctly"
        );
        assert_eq!(
            storable_block.txhash,
            Sha256Hash::from_array(Default::default()),
            "TxHash was not deserialized correctly"
        );
        assert_eq!(
            storable_block.height, 1,
            "Height was not deserialized correctly"
        );
    }
}
