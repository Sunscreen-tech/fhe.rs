use std::sync::Arc;

use crate::bfv::BfvParameters;
use crate::Result;
use fhe_math::rq::Poly;
use rand::{CryptoRng, RngCore};

/// Generate a new random CRP.
// Note: A slim wrapper type would be cleaner here.
pub fn generate_crp<R: RngCore + CryptoRng>(par: &Arc<BfvParameters>, rng: &mut R) -> Result<Poly> {
    generate_crp_leveled(par, 0, rng)
}

/// Generate a new random CRP vector.
///
/// The size of the vector is equal to the number of ciphertext moduli, as required for the
/// relinearization key generation protocol.
pub fn generate_crp_vec<R: RngCore + CryptoRng>(
    par: &Arc<BfvParameters>,
    rng: &mut R,
) -> Result<Vec<Poly>> {
    (0..par.moduli().len())
        .map(|_| generate_crp_leveled(par, 0, rng))
        .collect()
}

/// Generate a new random leveled CRP.
pub fn generate_crp_leveled<R: RngCore + CryptoRng>(
    par: &Arc<BfvParameters>,
    level: usize,
    rng: &mut R,
) -> Result<Poly> {
    let ctx = par.ctx_at_level(level)?;
    Ok(Poly::random(ctx, fhe_math::rq::Representation::Ntt, rng))
}
