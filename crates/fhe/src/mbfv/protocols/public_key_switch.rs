use std::sync::Arc;

use fhe_math::rq::traits::TryConvertFrom;
use fhe_math::rq::{Poly, Representation};

use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

use crate::bfv::{BfvParameters, Ciphertext, PublicKey, SecretKey};
use crate::mbfv::Aggregate;
use crate::{Error, Result};

/// Each party uses the `PublicKeySwitchShare` to generate their share of the new ciphertext and
/// participate in the "Protocol 4: PubKeySwitch" protocol detailed in Multiparty BFV (p7).
pub struct PublicKeySwitchShare {
    pub(crate) par: Arc<BfvParameters>,
    /// The first component of the input ciphertext
    pub(crate) c0: Poly,
    pub(crate) h0_share: Poly,
    pub(crate) h1_share: Poly,
}

impl PublicKeySwitchShare {
    /// Participate in a new PubKeySwitch protocol
    ///
    /// 1. *Private input*: BFV secret key share
    /// 2. *Public input*: BFV output public key
    /// 3. *Public input*: Ciphertext
    /// 4. *Public input*: TODO: variance of the ciphertext noise
    pub fn new<R: RngCore + CryptoRng>(
        sk_share: &SecretKey,
        public_key: &PublicKey,
        ct: &Ciphertext,
        rng: &mut R,
    ) -> Result<Self> {
        if sk_share.par != public_key.par || public_key.par != ct.par {
            return Err(Error::DefaultError(
                "Incompatible BFV parameters".to_string(),
            ));
        }
        let par = sk_share.par.clone();

        // Get appropriate context / level for the following computations
        let mut pk_ct = public_key.c.clone();
        while pk_ct.level != ct.level {
            pk_ct.mod_switch_to_next_level()?;
        }
        let ctx = par.ctx_at_level(ct.level)?;

        let mut s = Zeroizing::new(Poly::try_convert_from(
            sk_share.coeffs.as_ref(),
            ctx,
            false,
            Representation::PowerBasis,
        )?);
        s.change_representation(Representation::Ntt);
        s.disallow_variable_time_computations();

        let u = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
        // TODO this should be exponential in ciphertext noise!
        let e0 = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);
        let e1 = Zeroizing::new(Poly::small(ctx, Representation::Ntt, par.variance, rng)?);

        let mut h0 = pk_ct.c[0].clone();
        h0.disallow_variable_time_computations();
        h0 *= u.as_ref();
        *s.as_mut() *= &ct.c[1];
        h0 += s.as_ref();
        h0 += e0.as_ref();

        let mut h1 = pk_ct.c[1].clone();
        h1.disallow_variable_time_computations();
        h1 *= u.as_ref();
        h1 += e1.as_ref();

        Ok(Self {
            par,
            c0: ct.c[0].clone(),
            h0_share: h0,
            h1_share: h1,
        })
    }
}

impl Aggregate for PublicKeySwitchShare {
    type Output = Ciphertext;

    fn aggregate<I>(shares: I) -> Result<Self::Output>
    where
        I: IntoIterator<Item = Self>,
    {
        let mut shares = shares.into_iter();
        let share = shares.next().ok_or(Error::TooFewValues(0, 1))?;
        let mut h0 = share.h0_share;
        let mut h1 = share.h1_share;
        for sh in shares {
            h0 += &sh.h0_share;
            h1 += &sh.h1_share;
        }

        let c0 = &share.c0 + &h0;

        Ciphertext::new(vec![c0, h1], &share.par)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fhe_math::rq::{Poly, Representation};
    use fhe_traits::{FheDecrypter, FheEncoder, FheEncrypter};
    use rand::thread_rng;

    use crate::{
        bfv::{BfvParameters, Encoding, Plaintext, SecretKey},
        mbfv::{protocols::PublicKeyShare, Aggregate},
    };

    use super::*;

    const NUM_PARTIES: usize = 11;

    struct Party {
        sk_share: SecretKey,
        pk_share: PublicKeyShare,
    }

    #[test]
    fn encrypt_keyswitch_decrypt() {
        let mut rng = thread_rng();
        for par in [
            BfvParameters::default_arc(1, 8),
            BfvParameters::default_arc(6, 8),
        ] {
            for level in 0..=par.max_level() {
                for _ in 0..20 {
                    let crp =
                        Poly::random(par.ctx_at_level(0).unwrap(), Representation::Ntt, &mut rng);

                    // Parties collectively generate public key
                    let mut parties: Vec<Party> = vec![];
                    for _ in 0..NUM_PARTIES {
                        let sk_share = SecretKey::random(&par, &mut rng);
                        let pk_share =
                            PublicKeyShare::new(&sk_share, crp.clone(), &mut rng).unwrap();
                        parties.push(Party { sk_share, pk_share })
                    }

                    let public_key =
                        PublicKeyShare::aggregate(parties.iter().map(|p| p.pk_share.clone()))
                            .unwrap();

                    // Use it to encrypt a random polynomial ct1
                    let pt1 = Plaintext::try_encode(
                        &par.plaintext.random_vec(par.degree(), &mut rng),
                        Encoding::poly_at_level(level),
                        &par,
                    )
                    .unwrap();
                    let ct1 = Arc::new(public_key.try_encrypt(&pt1, &mut rng).unwrap());

                    // Key switch ct1 to a new keypair
                    let sk_out = SecretKey::random(&par, &mut rng);
                    let pk_out = PublicKey::new(&sk_out, &mut rng);
                    let pkss = parties.iter().map(|p| {
                        PublicKeySwitchShare::new(&p.sk_share, &pk_out, &ct1, &mut rng).unwrap()
                    });
                    let ct2 = PublicKeySwitchShare::aggregate(pkss).unwrap();

                    let pt2 = sk_out.try_decrypt(&ct2).unwrap();
                    assert_eq!(pt1, pt2);
                }
            }
        }
    }
}
