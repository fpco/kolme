mod sha256hash;

pub use sha256hash::*;

#[cfg(feature = "realcryptography")]
mod real;

#[cfg(not(feature = "realcryptography"))]
mod not_real;

#[cfg(feature = "realcryptography")]
pub use real::*;

#[cfg(not(feature = "realcryptography"))]
pub use not_real::*;
