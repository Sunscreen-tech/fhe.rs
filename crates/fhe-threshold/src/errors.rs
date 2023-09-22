use thiserror::Error;

/// The Result type for this library.
pub type Result<T> = std::result::Result<T, Error>;

/// Enum encapsulating all the possible errors from this library.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// Indicates that an error from the underlying fhe library was encountered.
    #[error("{0}")]
    FheError(#[from] fhe::Error),

    /// Indicates that an error from the underlying fhe-math library was encountered.
    #[error("{0}")]
    MathError(#[from] fhe_math::Error),
}
