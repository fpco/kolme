use merkle_map::{MerkleDeserialize, MerkleSerialError, MerkleSerialize, Sha256Hash};
use std::sync::Arc;

/// Contents of a block to be stored in a database.
#[derive(Debug)]
pub struct StorableBlock<Block, FrameworkState, AppState> {
    pub height: u64,
    pub blockhash: Sha256Hash,
    pub txhash: Sha256Hash,
    pub block: Arc<Block>,
    pub framework_state: Arc<FrameworkState>,
    pub app_state: Arc<AppState>,
    pub logs: Arc<[Vec<String>]>,
}

impl<Block, FrameworkState, AppState> Clone for StorableBlock<Block, FrameworkState, AppState> {
    fn clone(&self) -> Self {
        Self {
            height: self.height,
            blockhash: self.blockhash,
            txhash: self.txhash,
            block: self.block.clone(),
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            logs: self.logs.clone(),
        }
    }
}

impl<Block: MerkleSerialize, FrameworkState: MerkleSerialize, AppState: MerkleSerialize>
    MerkleSerialize for StorableBlock<Block, FrameworkState, AppState>
{
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Self {
            height,
            blockhash,
            txhash,
            block,
            framework_state,
            app_state,
            logs,
        } = self;
        serializer.store(height)?;
        serializer.store(blockhash)?;
        serializer.store(txhash)?;
        serializer.store(block)?;
        serializer.store_by_hash(framework_state)?;
        serializer.store_by_hash(app_state)?;
        serializer.store_by_hash(logs)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        1
    }
}

impl<
        Block: MerkleSerialize + MerkleDeserialize,
        FrameworkState: MerkleSerialize + MerkleDeserialize,
        AppState: MerkleSerialize + MerkleDeserialize,
    > MerkleDeserialize for StorableBlock<Block, FrameworkState, AppState>
{
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let height = deserializer.load()?;
        let blockhash = deserializer.load()?;
        let txhash = deserializer.load()?;
        let block = deserializer.load()?;


        let (framework_state, app_state, logs) = match version {
            0 => {
                let framework_state = deserializer.load()?;
                let app_state = deserializer.load()?;
                let logs = deserializer
                    .load()
                    .map(|x: Vec<Vec<String>>| x.into())?;

                (framework_state, app_state, logs)
            }
            1 => {
                let framework_state = deserializer.load_by_hash()?;
                let app_state = deserializer.load_by_hash()?;
                let logs = deserializer
                    .load_by_hash()
                    .map(|x: Vec<Vec<String>>| x.into())?;

                (framework_state, app_state, logs)
            },
            _ => unreachable!("Validation of version is carried out at the trait level on MerkleDeserializeRaw")
        };

        Ok(Self {
            height,
            blockhash,
            txhash,
            block,
            framework_state,
            app_state,
            logs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::StorableBlock;
    use merkle_map::{
        MerkleContents, MerkleDeserialize, MerkleDeserializer, MerkleManager, MerkleMemoryStore,
        MerkleSerialError, MerkleSerialize, MerkleSerializeRaw, MerkleSerializer, Sha256Hash,
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

    fn serialize<T: MerkleSerializeRaw>(item: &T) -> Arc<MerkleContents> {
        let manager = MerkleManager::new(10);
        let result = manager.serialize(item);

        result.expect("Unable to serialize item")
    }

    #[tokio::test]
    async fn it_serializes_framework_state_app_state_and_logs_as_independent_layers() {
        // Arrange
        let framework_state = Arc::new(DummyBytes(b"Dummy FrameworkState".to_vec()));
        let framework_state_merkle = serialize(&framework_state);
        let app_state = Arc::new(DummyBytes(b"Dummy AppState".to_vec()));
        let app_state_merkle = serialize(&app_state);
        let logs = Arc::from(vec![vec!["Dummy".to_owned()], vec!["Logs".to_owned()]]);
        let logs_merkle = serialize(&logs);
        let manager = MerkleManager::new(10);
        let mut store = MerkleMemoryStore::default();
        let block = StorableBlock {
            height: 0,
            blockhash: Sha256Hash::from_array(Default::default()),
            txhash: Sha256Hash::from_array(Default::default()),
            framework_state,
            logs,
            app_state,
            block: Arc::new(DummyBytes(b"Dummy block".to_vec())),
        };

        // Act
        let storable_block_merkle = manager
            .save(&mut store, &block)
            .await
            .expect("Unable to save storable block");
        let snapshot = store.get_map_snapshot();

        // Assert
        assert_eq!(
            snapshot.len(),
            4,
            "Storable block's contents were not stored in their own layers correctly"
        );
        let stored_framework_state_contents = snapshot.get(&framework_state_merkle.hash).unwrap();
        assert_eq!(
            stored_framework_state_contents.payload, framework_state_merkle.payload,
            "Framework state merkle contents were not generated correctly"
        );
        assert!(
            stored_framework_state_contents.children.is_empty(),
            "Framework state merkle children were not generated correctly"
        );
        let stored_app_state_contents = snapshot.get(&app_state_merkle.hash).unwrap();
        assert_eq!(
            stored_app_state_contents.payload, app_state_merkle.payload,
            "App state merkle contents were not generated correctly"
        );
        assert!(
            stored_app_state_contents.children.is_empty(),
            "App state merkle children were not generated correctly"
        );
        let stored_logs_contents = snapshot.get(&logs_merkle.hash).unwrap();
        assert_eq!(
            stored_logs_contents.payload, logs_merkle.payload,
            "Logs merkle contents were not generated correctly"
        );
        assert!(
            stored_logs_contents.children.is_empty(),
            "Logs merkle children were not generated correctly"
        );
        assert_eq!(
            storable_block_merkle.children.len(),
            3,
            "Storable block's children were not stored correctly"
        );
        assert_eq!(
            storable_block_merkle
                .children
                .iter()
                .map(|c| c.hash)
                .collect::<Vec<_>>(),
            vec![
                framework_state_merkle.hash,
                app_state_merkle.hash,
                logs_merkle.hash
            ],
            "Storable block's children hashes were not stored correctly"
        );
    }

    #[tokio::test]
    async fn it_deserializes_framework_state_app_state_and_logs_from_independent_layers() {
        // Arrange
        let framework_state = Arc::new(DummyBytes(b"Dummy FrameworkState".to_vec()));
        let app_state = Arc::new(DummyBytes(b"Dummy AppState".to_vec()));
        let logs: Arc<[Vec<String>]> =
            Arc::from(vec![vec!["Dummy".to_owned()], vec!["Logs".to_owned()]]);
        let manager = MerkleManager::new(10);
        let mut store = MerkleMemoryStore::default();
        let block = StorableBlock {
            height: 0,
            blockhash: Sha256Hash::from_array(Default::default()),
            txhash: Sha256Hash::from_array(Default::default()),
            framework_state: framework_state.clone(),
            logs: logs.clone(),
            app_state: app_state.clone(),
            block: Arc::new(DummyBytes(b"Dummy block".to_vec())),
        };
        let storable_block_merkle = manager
            .save(&mut store, &block)
            .await
            .expect("Unable to save storable block");

        // Act
        let storable_block = manager
            .deserialize::<StorableBlock<DummyBytes, DummyBytes, DummyBytes>>(
                storable_block_merkle.hash,
                storable_block_merkle.payload.clone(),
            )
            .expect("Unable to deserialize storable_block");

        // Assert
        assert_eq!(
            storable_block.framework_state, framework_state,
            "Framework state was not deserialized correctly"
        );
        assert_eq!(
            storable_block.app_state, app_state,
            "App state was not deserialized correctly"
        );
        assert_eq!(
            storable_block.logs,
            vec![vec!["Dummy".to_owned()], vec!["Logs".to_owned()]].into(),
            "Logs were not deserialized correctly"
        );
    }
}
