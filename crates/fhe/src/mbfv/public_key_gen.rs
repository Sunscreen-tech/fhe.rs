//! Public Key generation protocol.
//!
//! TODO:
//! 1. Implement CRS->CRP common random polynomial + protocols around it

use std::sync::Arc;

use crate::bfv::{BfvParameters, SecretKey};
use crate::errors::Result;
use fhe_math::rq::{traits::TryConvertFrom, Poly, Representation};
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

/// Each party uses the `PublicKeyShare` to generate their share of the public key and participate
/// in the "Protocol 1: EncKeyGen" protocol detailed in Multiparty BFV (p6).
struct PublicKeyShare {
    pub(crate) par: Arc<BfvParameters>,
    pub(crate) p0_share: Poly,
}

impl PublicKeyShare {
    /// 1. *Private input*: BFV secret key share
    /// 2. *Public input*: common random polynomial
    pub fn new<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        crp: &Poly,
        rng: &mut R,
    ) -> Result<Self> {
        let par = sk_share.par.clone();
        // TODO Assuming level zero is the only thing that makes sense here?
        let ctx = par.ctx_at_level(0)?;

        // Sample error
        let e = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
        // Convert secret key to usable polynomial
        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);

        // Create p0_i share
        let mut p0_share = -(crp * s.as_ref());
        p0_share += e.as_ref();

        Ok(Self { par, p0_share })
    }
}
