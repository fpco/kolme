use crate::*;

impl<T> Clone for Locked<T> {
    fn clone(&self) -> Self {
        Locked(self.0.clone())
    }
}

impl<T: Clone> Locked<T> {
    pub(crate) fn into_inner(self) -> T {
        match Arc::try_unwrap(self.0) {
            Ok(x) => x.inner,
            Err(x) => T::clone(&x.inner),
        }
    }
}
