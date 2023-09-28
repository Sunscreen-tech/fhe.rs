//! Relin Key generation protocol.
//!
//! TODO:
//! 1. Implement CRS->CRP common random polynomial + protocols around it

use std::marker::PhantomData;
use std::sync::Arc;

use crate::bfv::{BfvParameters, KeySwitchingKey, RelinearizationKey, SecretKey};
use crate::errors::Result;
use crate::mbfv::Aggregate;
use crate::Error;
use fhe_math::rns::RnsContext;
use fhe_math::rq::{traits::TryConvertFrom, Poly, Representation};
use itertools::izip;
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

pub trait Round: sealed::Sealed {}

/// Marks the shares produced in round 1
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct R1;
/// Marks the aggregated shares from round 1
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct R1Aggregated;
/// Marks the shares from round 2
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct R2;

impl Round for R1 {}
impl Round for R1Aggregated {}
impl Round for R2 {}

/// The publicly disclosed shares of a Relin Key Gen protocol round.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RelinKeyShare<R: Round = R1> {
    pub(crate) par: Arc<BfvParameters>,
    pub(crate) h0: Box<[Poly]>,
    pub(crate) h1: Box<[Poly]>,
    // This is a hack to get this done quickly. The `Aggregate` pattern
    // doesn't really work for this protocol, it should be revised.
    last_round: Option<Box<RelinKeyShare<R1Aggregated>>>,
    _phantom_data: PhantomData<R>,
}

/// Each party uses the `RelinKeyGenerator` to generate their shares and participate in the
/// "Protocol 2: RelinKeyGen" protocol detailed in Multiparty BFV (p6).
pub struct RelinKeyGenerator<'a, 'b> {
    sk_share: &'a SecretKey,
    crp: &'b [Poly],
    u: Zeroizing<Poly>,
}

impl<'a, 'b> RelinKeyGenerator<'a, 'b> {
    /// Create a new relin key generator for a given party.
    pub fn new<R: RngCore + CryptoRng>(
        sk_share: &'a SecretKey,
        crp: &'b [Poly],
        rng: &mut R,
    ) -> Result<Self> {
        let par = sk_share.par.clone();
        let ctx = par.ctx_at_level(0)?;
        if crp.len() != ctx.moduli().len() {
            Err(Error::DefaultError(
                "The size of the CRP polynomial vector must equal the number of ciphertext moduli."
                    .to_string(),
            ))
        } else {
            let u = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
            Ok(Self { sk_share, crp, u })
        }
    }

    /// Generate shares for round 1
    pub fn round_1<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Result<RelinKeyShare<R1>> {
        <RelinKeyShare<R1>>::new(self.sk_share, self.crp, &self.u, rng)
    }

    /// Generate shares for round 2
    pub fn round_2<R: RngCore + CryptoRng>(
        &self,
        r1: &RelinKeyShare<R1Aggregated>,
        rng: &mut R,
    ) -> Result<RelinKeyShare<R2>> {
        <RelinKeyShare<R2>>::new(self.sk_share, &self.u, r1, rng)
    }
}

impl RelinKeyShare<R1> {
    fn new<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        crp: &[Poly],
        u: &Zeroizing<Poly>,
        rng: &mut R,
    ) -> Result<Self> {
        let par = sk_share.par.clone();

        if crp.len() != par.ctx_at_level(0)?.moduli().len() {
            Err(Error::DefaultError(
                "The size of the CRP polynomial vector must equal the number of ciphertext moduli."
                    .to_string(),
            ))
        } else {
            let h0 = Self::generate_h0(sk_share, crp, u, rng)?;
            let h1 = Self::generate_h1(sk_share, crp, rng)?;
            Ok(Self {
                par,
                h0,
                h1,
                last_round: None,
                _phantom_data: PhantomData,
            })
        }
    }

    fn generate_h0<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        crp: &[Poly],
        u: &Zeroizing<Poly>,
        rng: &mut R,
    ) -> Result<Box<[Poly]>> {
        let par = sk_share.par.clone();
        let ctx = par.ctx_at_level(0)?;

        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);
        let rns = RnsContext::new(&sk_share.par.moduli[..crp.len()])?;
        let h0 = crp
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let w = rns.get_garner(i).unwrap();
                let w_s = Zeroizing::new(w * s.as_ref());

                let e = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);

                let mut h = a.clone();
                h.disallow_variable_time_computations();
                h.change_representation(Representation::Ntt);
                h *= u.as_ref();

                h += w_s.as_ref();
                h += e.as_ref();
                Ok(h)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(h0.into_boxed_slice())
    }

    fn generate_h1<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        crp: &[Poly],
        rng: &mut R,
    ) -> Result<Box<[Poly]>> {
        let par = sk_share.par.clone();
        let ctx = par.ctx_at_level(0)?;
        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);

        let h1 = crp
            .iter()
            .map(|a| {
                let mut h = a.clone();
                h.disallow_variable_time_computations();
                h.change_representation(Representation::Ntt);
                let e = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
                h *= s.as_ref();
                h += e.as_ref();
                Ok(h)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(h1.into_boxed_slice())
    }
}

impl Aggregate for RelinKeyShare<R1> {
    type Output = RelinKeyShare<R1Aggregated>;

    fn aggregate<I>(shares: I) -> Result<Self::Output>
    where
        I: IntoIterator<Item = Self>,
    {
        let mut shares = shares.into_iter();
        let share = shares.next().ok_or(Error::TooFewValues(0, 1))?;
        let mut h0 = share.h0;
        let mut h1 = share.h1;
        for sh in shares {
            izip!(h0.iter_mut(), sh.h0.iter()).for_each(|(h0i, sh_h0i)| *h0i += sh_h0i);
            izip!(h1.iter_mut(), sh.h1.iter()).for_each(|(h1i, sh_h1i)| *h1i += sh_h1i);
        }

        Ok(RelinKeyShare {
            par: share.par,
            h0,
            h1,
            last_round: None,
            _phantom_data: PhantomData,
        })
    }
}

impl RelinKeyShare<R2> {
    fn new<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        u: &Zeroizing<Poly>,
        r1: &RelinKeyShare<R1Aggregated>,
        rng: &mut R,
    ) -> Result<Self> {
        let par = sk_share.par.clone();
        let h0 = Self::generate_h0(sk_share, &r1.h0, rng)?;
        let h1 = Self::generate_h1(sk_share, u, &r1.h1, rng)?;
        Ok(Self {
            par,
            h0,
            h1,
            last_round: Some(Box::new(r1.clone())),
            _phantom_data: PhantomData,
        })
    }

    fn generate_h0<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        r1_h0: &[Poly],
        rng: &mut R,
    ) -> Result<Box<[Poly]>> {
        let par = sk_share.par.clone();
        let ctx = par.ctx_at_level(0)?;

        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);
        let h0 = r1_h0
            .iter()
            .map(|h| {
                let e = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);

                let mut h_prime = h.clone();
                h_prime.disallow_variable_time_computations();
                h_prime.change_representation(Representation::Ntt);
                h_prime *= s.as_ref();

                h_prime += e.as_ref();
                Ok(h_prime)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(h0.into_boxed_slice())
    }

    fn generate_h1<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        u: &Zeroizing<Poly>,
        r1_h1: &[Poly],
        rng: &mut R,
    ) -> Result<Box<[Poly]>> {
        let par = sk_share.par.clone();
        let ctx = par.ctx_at_level(0)?;
        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);

        let u_s = Zeroizing::new(u.as_ref() - s.as_ref());

        let h1 = r1_h1
            .iter()
            .map(|h| {
                let mut h_prime = h.clone();
                h_prime.disallow_variable_time_computations();
                h_prime.change_representation(Representation::Ntt);
                let e = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
                h_prime *= u_s.as_ref();
                h_prime += e.as_ref();
                Ok(h_prime)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(h1.into_boxed_slice())
    }
}

impl Aggregate for RelinKeyShare<R2> {
    type Output = RelinearizationKey;

    fn aggregate<I>(shares: I) -> Result<Self::Output>
    where
        I: IntoIterator<Item = Self>,
    {
        let mut shares = shares.into_iter();
        let share = shares.next().ok_or(Error::TooFewValues(0, 1))?;
        let par = share.par.clone();
        let ctx = par.ctx_at_level(0)?.clone();
        let r1 = share.last_round.ok_or(Error::DefaultError(
            "Shares from round 2 should include a copy for the round 1 aggregation.".to_string(),
        ))?;

        // TODO this is correct right? write it out to be sure...
        let mut h = share.h0;
        izip!(h.iter_mut(), share.h1.iter()).for_each(|(hi, h1)| *hi += h1);
        for sh in shares {
            izip!(h.iter_mut(), sh.h0.iter(), sh.h1.iter()).for_each(|(hi, h0i, h1i)| {
                *hi += h0i;
                *hi += h1i;
            });
        }

        let ksk = KeySwitchingKey {
            par,
            seed: None,
            c0: h,
            c1: r1.h1,
            ciphertext_level: 0,
            ctx_ciphertext: ctx.clone(),
            ksk_level: 0,
            ctx_ksk: ctx.clone(),
        };
        Ok(RelinearizationKey { ksk })
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::R1 {}
    impl Sealed for super::R1Aggregated {}
    impl Sealed for super::R2 {}
}

#[cfg(test)]
mod tests {
    // use super::*;
    // use fhe_math::rq::{Poly, Representation};
    // use fhe_traits::{FheEncoder, FheEncrypter};
    // use rand::thread_rng;
    //
    // use crate::bfv::{BfvParameters, Encoding, Plaintext, SecretKey};
    //
    // const NUM_PARTIES: usize = 11;
}
