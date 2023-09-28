use crate::errors::Result;

/// Aggregate shares in an MPC protocol
// Hmm. We could just impl FromIterator and then get shares.collect() for free.
pub trait Aggregate {
    /// The result of the aggregation
    type Output;

    /// Aggregate shares in an MPC protocol.
    fn aggregate<I>(shares: I) -> Result<Self::Output>
    where
        I: IntoIterator<Item = Self>;
}