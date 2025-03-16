mod execute;
mod kolme;
mod kolme_app;
mod state;
mod types;

pub use execute::*;
pub use kolme::*;
pub use kolme_app::*;
use sqlx::{Sqlite, Transaction};
pub use types::*;

use crate::*;
use state::*;

pub(super) async fn get_state_payload(
    pool: &sqlx::SqlitePool,
    hash: &Sha256Hash,
) -> Result<String> {
    sqlx::query_scalar!("SELECT payload FROM state_payload WHERE hash=$1", hash)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Returns the hash of the content
pub(super) async fn insert_state_payload(
    e: &mut Transaction<'_, Sqlite>,
    payload: &str,
) -> Result<Sha256Hash> {
    let hash = Sha256Hash::hash(payload);
    let hash_bin = hash.0.as_slice();
    let count = sqlx::query_scalar!("SELECT COUNT(*) FROM state_payload WHERE hash=$1", hash_bin)
        .fetch_one(&mut **e)
        .await?;
    match count {
        0 => {
            sqlx::query!(
                "INSERT INTO state_payload(hash,payload) VALUES($1, $2)",
                hash_bin,
                payload
            )
            .execute(&mut **e)
            .await?;
        }
        1 => (),
        _ => anyhow::bail!("insert_state_payload: impossible result of {count} entries"),
    }
    Ok(hash)
}
