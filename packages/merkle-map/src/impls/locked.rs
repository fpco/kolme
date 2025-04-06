use shared::types::Sha256Hash;

use crate::*;

impl<T> Clone for Locked<T> {
    fn clone(&self) -> Self {
        Locked {
            hash: self.hash,
            payload: self.payload.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<T: Clone> Locked<T> {
    pub(crate) fn into_inner(self) -> T {
        match Arc::try_unwrap(self.inner) {
            Ok(x) => x,
            Err(x) => T::clone(&x),
        }
    }
}

impl<T> Locked<T> {
    pub(crate) fn new(buff: impl Into<Arc<[u8]>>, inner: T) -> Self {
        let payload = buff.into();
        let hash = Sha256Hash::hash(&payload);
        Locked {
            hash,
            payload,
            inner: Arc::new(inner),
        }
    }
}
