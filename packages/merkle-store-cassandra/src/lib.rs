use std::sync::Arc;

use futures::StreamExt;
use merkle_map::{MerkleLayerContents, MerkleSerialError, MerkleStore, Sha256Hash};
use scylla::{
    client::{session::Session, session_builder::SessionBuilder},
    statement::prepared::PreparedStatement,
};

pub use scylla;

mod migrations;

#[derive(Clone)]
pub struct MerkleCassandraStore(std::sync::Arc<MerkleCassandraStoreInner>);

struct MerkleCassandraStoreInner {
    pub session: Session,
    pub save_statement: PreparedStatement,
}

impl MerkleCassandraStore {
    pub async fn new_with_known_nodes<NodeAddr: AsRef<str>>(
        known_nodes: impl IntoIterator<Item = NodeAddr>,
    ) -> Result<Self, MerkleSerialError> {
        let mut session_builder = SessionBuilder::new();

        for node_addr in known_nodes.into_iter() {
            session_builder = session_builder.known_node(node_addr.as_ref());
        }

        Self::new_with_builder(session_builder).await
    }
    pub async fn new_with_builder(
        session_builder: SessionBuilder,
    ) -> Result<Self, MerkleSerialError> {
        let mut session = session_builder
            .build()
            .await
            .map_err(MerkleSerialError::custom)?;

        Self::ensure_migrations(&mut session).await?;

        let save_statement = session
            .prepare("INSERT INTO kolme.merkle_contents(hash, payload, children) VALUES (?, ?, ?)")
            .await
            .map_err(MerkleSerialError::custom)?;

        Ok(MerkleCassandraStore(std::sync::Arc::new(
            MerkleCassandraStoreInner {
                session,
                save_statement,
            },
        )))
    }

    pub async fn ensure_migrations(session: &mut Session) -> Result<(), MerkleSerialError> {
        // Unrolled first and second iteration to create keyspace and migrations table.
        // If there's no keyspace or kolme.migrations table, then the SELECT ... FROM kolme.migrations
        // query cannot happen
        for migration in migrations::MIGRATIONS.iter().take(2) {
            let statement = session
                .prepare(migration.content)
                .await
                .map_err(MerkleSerialError::custom)?;
            session
                .execute_iter(statement, ())
                .await
                .map_err(MerkleSerialError::custom)?;
        }
        let migration_insertion_stmt = session
            .prepare("INSERT INTO kolme.migrations(version, content, hash) VALUES (?, ?, ?)")
            .await
            .map_err(MerkleSerialError::custom)?;

        // We save the execution of the first two migrations in this loop
        // as we have to make sure that
        for migration in migrations::MIGRATIONS.iter().take(2) {
            session
                .execute_iter(
                    migration_insertion_stmt.clone(),
                    (migration.version, migration.content, migration.hash),
                )
                .await
                .map_err(MerkleSerialError::custom)?;
        }
        for migration in migrations::MIGRATIONS.iter().skip(2) {
            let mut stream = session
                .query_iter(
                    "SELECT hash FROM kolme.migrations WHERE version = ?;",
                    (migration.version,),
                )
                .await
                .map_err(MerkleSerialError::custom)?
                .rows_stream::<(Vec<u8>,)>()
                .map_err(MerkleSerialError::custom)?;

            match stream.next().await {
                // If found and the contents match, do nothing
                Some(Ok((hash,))) if hash == migration.hash => {}
                // If found and the contents missmatch, abort
                // this is a constraint error as migrations previously
                // applied should not be modified
                Some(Ok((hash,))) => {
                    return Err(MerkleSerialError::Other(format!(
                        "Migration {version}'s content hash changed. Applied hash ({ehash}) != Current hash ({chash})",
                        version = migration.version,
                        ehash = hex::encode(hash),
                        chash = hex::encode(migration.hash)
                    )));
                }
                // If not found, append statements to batch and push migration params
                // We execute these statements on batch to guarantee that
                // if and only if there are no errors preprocessing the migrations
                // then the new ones can be executed
                None => {
                    let migration_stmt = session
                        .prepare(migration.content)
                        .await
                        .map_err(MerkleSerialError::custom)?;

                    session
                        .execute_iter(migration_stmt, ())
                        .await
                        .map_err(MerkleSerialError::custom)?;
                    session
                        .execute_iter(
                            migration_insertion_stmt.clone(),
                            (migration.version, migration.content, migration.hash),
                        )
                        .await
                        .map_err(MerkleSerialError::custom)?;
                }
                Some(Err(err)) => return Err(MerkleSerialError::custom(err)),
            }
        }

        Ok(())
    }
}

impl MerkleStore for MerkleCassandraStore {
    async fn load_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> Result<Option<merkle_map::MerkleLayerContents>, MerkleSerialError> {
        let mut stream = self
            .0
            .session
            .query_iter(
                "SELECT payload, children FROM kolme.merkle_contents WHERE hash = ?",
                (hash.as_array(),),
            )
            .await
            .map_err(MerkleSerialError::custom)?
            .rows_stream::<(Vec<u8>, Vec<Vec<u8>>)>()
            .map_err(MerkleSerialError::custom)?;

        let Some(query_result) = stream.next().await else {
            return Ok(None);
        };
        let (payload, children) = query_result.map_err(MerkleSerialError::custom)?;

        Ok(Some(MerkleLayerContents {
            payload: Arc::from(payload),
            children: children
                .into_iter()
                .map(|hash| Sha256Hash::from_hash(&hash).unwrap())
                .collect(),
        }))
    }

    async fn save_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
        layer: &merkle_map::MerkleLayerContents,
    ) -> Result<(), MerkleSerialError> {
        let payload = layer.payload.to_vec();
        let children = layer
            .children
            .iter()
            .map(|hash| *hash.as_array())
            .collect::<Vec<_>>();

        self.0
            .session
            .execute_iter(
                self.0.save_statement.clone(),
                (hash.as_array(), payload, children),
            )
            .await
            .map_err(MerkleSerialError::custom)?;

        Ok(())
    }

    async fn contains_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> Result<bool, MerkleSerialError> {
        Ok(self
            .0
            .session
            .query_iter(
                "SELECT (bigint)1 FROM kolme.merkle_contents WHERE hash = ?",
                (hash.as_array(),),
            )
            .await
            .map_err(MerkleSerialError::custom)?
            .rows_stream::<(i64,)>()
            .map_err(MerkleSerialError::custom)?
            .next()
            .await
            .transpose()
            .map_err(MerkleSerialError::custom)?
            .is_some())
    }
}
