use lazy_static::lazy_static;

pub struct Migration {
    pub content: &'static str,
    pub version: i64,
    pub hash: [u8; 32],
}

impl Migration {
    pub fn new(content: &'static str, version: i64) -> Self {
        let hash = {
            use sha2::{Digest, Sha256};

            let mut hash_builder = Sha256::new();

            hash_builder.update(content.as_bytes());
            hash_builder.update(version.to_ne_bytes());

            hash_builder.finalize()
        };

        Migration {
            content,
            version,
            hash: hash.into(),
        }
    }
}

lazy_static! {
    pub static ref MIGRATIONS: [Migration; 3] = [
        Migration::new(
            r#"
CREATE KEYSPACE IF NOT EXISTS kolme
WITH REPLICATION = {
    'class': 'SimpleStrategy', 'replication_factor': 1 };
            "#,
            0
        ),
        Migration::new(
            r#"
CREATE TABLE IF NOT EXISTS kolme.migrations (
    version bigint PRIMARY KEY,
    content text,
    hash    blob);
            "#,
            1
        ),
        Migration::new(
            r#"
CREATE TABLE IF NOT EXISTS kolme.merkle_contents (
    hash blob PRIMARY KEY,
    payload blob,
    children set<blob> );
            "#,
            2,
        ),
    ];
}
