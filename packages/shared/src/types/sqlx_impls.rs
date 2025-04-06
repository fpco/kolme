use std::borrow::Cow;

use sqlx::Sqlite;

use crate::cryptography::PublicKey;
use crate::types::Sha256Hash;

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

impl<'de> serde::Deserialize<'de> for Sha256Hash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(D::Error::custom)?;
        Sha256Hash::from_hash(&bytes).map_err(D::Error::custom)
    }
}

impl sqlx::Type<Sqlite> for Sha256Hash {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <&[u8] as sqlx::Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <&[u8] as sqlx::Type<Sqlite>>::compatible(ty)
    }
}

impl sqlx::Encode<'_, Sqlite> for Sha256Hash {
    fn encode_by_ref(
        &self,
        args: &mut Vec<sqlx::sqlite::SqliteArgumentValue>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        args.push(sqlx::sqlite::SqliteArgumentValue::Blob(Cow::Owned(
            self.0.to_vec(),
        )));

        Ok(sqlx::encode::IsNull::No)
    }
}

impl sqlx::Decode<'_, Sqlite> for Sha256Hash {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
        let vec: Vec<u8> = sqlx::Decode::<Sqlite>::decode(value)?;
        Sha256Hash::from_hash(&vec).map_err(Into::into)
    }
}
