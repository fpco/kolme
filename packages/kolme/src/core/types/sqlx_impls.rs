use crate::core::types::BlockHeight;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::sqlite::{SqliteTypeInfo, SqliteValueRef};
use sqlx::{Database, Decode, Encode, Sqlite, Type};
use std::result::Result;

impl Type<Sqlite> for BlockHeight {
    fn type_info() -> SqliteTypeInfo {
        <i64 as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <i64 as Type<Sqlite>>::compatible(ty)
    }
}

impl<'r> Decode<'r, Sqlite> for BlockHeight {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let as_int = <i64 as Decode<Sqlite>>::decode(value)?;
        Ok(Self(as_int.try_into()?))
    }
}

impl<'q> Encode<'q, Sqlite> for BlockHeight {
    fn encode_by_ref(
        &self,
        buf: &mut <Sqlite as Database>::ArgumentBuffer<'q>,
    ) -> Result<IsNull, BoxDynError> {
        let as_i64: i64 = self.0.try_into()?;
        <i64 as Encode<Sqlite>>::encode_by_ref(&as_i64, buf)
    }
}
