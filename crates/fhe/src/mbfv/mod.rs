//! The Multiparty BFV scheme, as described by Christian Mouchet et. al.
mod aggregator;
pub use aggregator::{Aggregate, AggregateIter};
pub mod protocols;
