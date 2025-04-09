use crate::*;

impl<T: MerkleSerialize> MerkleSerializeComplete for T {
    fn serialize_complete(
        &self,
        manager: &mut MerkleSerializeManager,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        let mut serializer = MerkleSerializer::new();
        self.serialize(&mut serializer)?;
        let (hash, payload) = serializer.finish();
        manager.add_contents(hash, payload, vec![]);
        Ok(hash)
    }
}

impl<K: ToMerkleKey + 'static, V: MerkleSerialize + 'static> MerkleSerializeComplete
    for MerkleMap<K, V>
{
    fn serialize_complete(
        &self,
        manager: &mut MerkleSerializeManager,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        self.0.serialize_complete(manager)
    }
}

impl<K: ToMerkleKey + 'static, V: MerkleSerialize + 'static> MerkleSerializeComplete
    for Node<K, V>
{
    fn serialize_complete(
        &self,
        manager: &mut MerkleSerializeManager,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        match &self {
            Node::Leaf(leaf) => {
                let (hash, payload) = leaf.lock()?;
                manager.add_contents(hash, payload, vec![]);
                Ok(hash)
            }
            Node::Tree(tree) => {
                let (hash, payload) = tree.lock()?;
                let mut children: Vec<Box<dyn MerkleSerializeComplete>> = vec![];
                for branch in &tree.as_ref().branches {
                    children.push(Box::new(branch.clone()));
                }
                manager.add_contents(hash, payload, children);
                Ok(hash)
            }
        }
    }
}
