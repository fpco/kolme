use sqlx::Sqlite;

use crate::cryptography::PublicKey;

impl sqlx::Type<sqlx::Sqlite> for PublicKey {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <&[u8] as sqlx::Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <&[u8] as sqlx::Type<Sqlite>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, sqlx::Sqlite> for PublicKey {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Sqlite as sqlx::Database>::ArgumentBuffer<'_>,
    ) -> std::result::Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        self.as_bytes().encode_by_ref(buf)
    }
}
