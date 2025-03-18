mod execute;
mod kolme;
mod kolme_app;
mod state;
mod types;

pub use execute::*;
pub use kolme::*;
pub use kolme_app::*;
use sqlx::Sqlite;
pub use types::*;

use crate::*;
use state::*;

pub(super) async fn get_state_payload(
    pool: &sqlx::SqlitePool,
    hash: &Sha256Hash,
) -> Result<String> {
    sqlx::query_scalar!("SELECT content FROM hashes WHERE hash=$1", hash)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Returns the hash of the content
pub(super) async fn insert_state_payload(
    e: &mut sqlx::Transaction<'_, Sqlite>,
    payload: &str,
) -> Result<Sha256Hash> {
    let hash = Sha256Hash::hash(payload);
    let count = sqlx::query_scalar!("SELECT COUNT(*) FROM hashes WHERE hash=$1", hash)
        .fetch_one(&mut **e)
        .await?;
    match count {
        0 => {
            sqlx::query!(
                "INSERT INTO hashes(hash,content) VALUES($1, $2)",
                hash,
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
