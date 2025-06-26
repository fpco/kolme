use merkle_map::{
    MerkleDeserialize, MerkleManager, MerkleMap, MerkleMemoryStore, MerkleSerialError,
    MerkleSerialize, MerkleSerializer,
};
use quickcheck::Arbitrary;

#[derive(Clone, PartialEq, Eq, Debug)]
struct Person0 {
    name: String,
    age: u16,
}

impl MerkleSerialize for Person0 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { name, age } = self;
        serializer.store(name)?;
        serializer.store(age)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        0
    }
}

impl MerkleDeserialize for Person0 {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        assert!(version == 0);
        Ok(Self {
            name: deserializer.load()?,
            age: deserializer.load()?,
        })
    }
}

impl Arbitrary for Person0 {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            name: Arbitrary::arbitrary(g),
            age: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Person1 {
    name: String,
    age: u16,
    street: String,
}

const DEFAULT_STREET: &str = "Default street";

impl From<Person0> for Person1 {
    fn from(Person0 { name, age }: Person0) -> Self {
        Self {
            name,
            age,
            street: DEFAULT_STREET.to_owned(),
        }
    }
}

impl MerkleSerialize for Person1 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { name, age, street } = self;
        serializer.store(name)?;
        serializer.store(age)?;
        serializer.store(street)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        1
    }
}

impl MerkleDeserialize for Person1 {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        assert!(version <= 1);
        Ok(Self {
            name: deserializer.load()?,
            age: deserializer.load()?,
            street: if version == 0 {
                DEFAULT_STREET.to_owned()
            } else {
                deserializer.load()?
            },
        })
    }
}

impl Arbitrary for Person1 {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            name: Arbitrary::arbitrary(g),
            age: Arbitrary::arbitrary(g),
            street: Arbitrary::arbitrary(g),
        }
    }
}

const DEFAULT_ZIP: u32 = 99999;

#[derive(Clone, PartialEq, Eq, Debug)]
struct Person2 {
    name: String,
    age: u16,
    street: String,
    zip: u32,
}

impl From<Person0> for Person2 {
    fn from(Person0 { name, age }: Person0) -> Self {
        Self {
            name,
            age,
            street: DEFAULT_STREET.to_owned(),
            zip: DEFAULT_ZIP,
        }
    }
}

impl From<Person1> for Person2 {
    fn from(Person1 { name, age, street }: Person1) -> Self {
        Self {
            name,
            age,
            street,
            zip: DEFAULT_ZIP,
        }
    }
}

impl MerkleSerialize for Person2 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            name,
            age,
            street,
            zip,
        } = self;
        serializer.store(name)?;
        serializer.store(age)?;
        serializer.store(street)?;
        serializer.store(zip)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        2
    }
}

impl MerkleDeserialize for Person2 {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        assert!(version <= 2);
        Ok(Self {
            name: deserializer.load()?,
            age: deserializer.load()?,
            street: if version == 0 {
                DEFAULT_STREET.to_owned()
            } else {
                deserializer.load()?
            },
            zip: if version < 2 {
                DEFAULT_ZIP
            } else {
                deserializer.load()?
            },
        })
    }
}

impl Arbitrary for Person2 {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            name: Arbitrary::arbitrary(g),
            age: Arbitrary::arbitrary(g),
            street: Arbitrary::arbitrary(g),
            zip: Arbitrary::arbitrary(g),
        }
    }
}

#[tokio::main]
async fn load_from_zero_helper(people: Vec<Person0>, to_modify: usize, new_street: String) -> bool {
    let mut m0 = MerkleMap::new();
    let mut m1 = MerkleMap::new();
    let mut m2 = MerkleMap::new();
    for (idx, person) in people.into_iter().enumerate() {
        let idx = idx as u32;
        m0.insert(idx, person.clone());
        m1.insert(idx, Person1::from(person.clone()));
        m2.insert(idx, Person2::from(person));
    }

    let manager = MerkleManager::default();
    let mut store = MerkleMemoryStore::default();
    let m0_contents = manager.save(&mut store, &m0).await.unwrap();

    let parsed1 = manager.load(&mut store, m0_contents.hash).await.unwrap();
    assert_eq!(m1, parsed1);

    // Serializing m1 directly should generate a different hash because it will use
    // the new serialized format.
    //
    // Only check this if we actually have values in the map, otherwise there was nothing
    // to reserialize.
    let m1_contents = manager.save(&mut store, &m1).await.unwrap();
    if !m0.is_empty() {
        assert_ne!(m0_contents.hash, m1_contents.hash);
    }

    // Reserializing without any changes should produce the same hash, since it's already cached
    let parsed1_contents = manager.save(&mut store, &parsed1).await.unwrap();
    assert_eq!(m0_contents.hash, parsed1_contents.hash);

    // Should also work to load directly into Person2
    let parsed2 = manager.load(&mut store, m0_contents.hash).await.unwrap();
    assert_eq!(m2, parsed2);
    let parsed2 = manager.load(&mut store, m1_contents.hash).await.unwrap();
    assert_eq!(m2, parsed2);

    // Now try modifying some random part of the Map and ensure we can get everything to match.
    // Requires some values in the map.
    if m0.is_empty() {
        return true;
    }

    let to_modify = (to_modify % m0.len()) as u32;

    let mut person = m1.get(&to_modify).unwrap().clone();
    person.street = new_street;
    assert!(m1.insert(to_modify, person.clone()).is_some());
    assert!(m2.insert(to_modify, person.into()).is_some());

    let m1_contents = manager.save(&mut store, &m1).await.unwrap();
    let parsed2 = manager.load(&mut store, m1_contents.hash).await.unwrap();
    assert_eq!(m2, parsed2);

    true
}

quickcheck::quickcheck! {
    fn load_from_zero(people:Vec<Person0>, to_modify:usize, new_street: String) -> bool {
        load_from_zero_helper(people, to_modify, new_street)
    }
}
