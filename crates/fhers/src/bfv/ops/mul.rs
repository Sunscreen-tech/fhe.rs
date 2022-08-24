use std::sync::Arc;

use math::{
	rns::ScalingFactor,
	rq::{scaler::Scaler, Context, Representation},
	zq::nfl::generate_prime,
};

use crate::{
	bfv::{keys::RelinearizationKey, BfvParameters, Ciphertext},
	Error, Result,
};

/// Multiplicator that implements a strategy for multiplying. In particular, the
/// following information can be specified:
/// - Whether `lhs` must be scaled;
/// - Whether `rhs` must be scaled;
/// - The basis at which the multiplication will occur;
/// - The scaling factor after multiplication;
/// - Whether relinearization should be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multiplicator {
	par: Arc<BfvParameters>,
	pub(crate) extender_lhs: Scaler,
	pub(crate) extender_rhs: Scaler,
	pub(crate) down_scaler: Scaler,
	pub(crate) base_ctx: Arc<Context>,
	pub(crate) mul_ctx: Arc<Context>,
	rk: Option<RelinearizationKey>,
	mod_switch: bool,
	level: usize,
}

impl Multiplicator {
	/// Construct a multiplicator using custom scaling factors and extended
	/// basis.
	pub fn new(
		lhs_scaling_factor: ScalingFactor,
		rhs_scaling_factor: ScalingFactor,
		extended_basis: &[u64],
		post_mul_scaling_factor: ScalingFactor,
		par: &Arc<BfvParameters>,
	) -> Result<Self> {
		Self::new_leveled(
			lhs_scaling_factor,
			rhs_scaling_factor,
			extended_basis,
			post_mul_scaling_factor,
			0,
			par,
		)
	}

	/// Construct a multiplicator using custom scaling factors and extended
	/// basis at a given level.
	#[cfg(feature = "leveled_bfv")]
	pub fn new_leveled(
		lhs_scaling_factor: ScalingFactor,
		rhs_scaling_factor: ScalingFactor,
		extended_basis: &[u64],
		post_mul_scaling_factor: ScalingFactor,
		level: usize,
		par: &Arc<BfvParameters>,
	) -> Result<Self> {
		let base_ctx = par.ctx_at_level(level)?;
		let mul_ctx = Arc::new(Context::new(extended_basis, par.degree())?);
		let extender_lhs = Scaler::new(base_ctx, &mul_ctx, lhs_scaling_factor)?;
		let extender_rhs = Scaler::new(base_ctx, &mul_ctx, rhs_scaling_factor)?;
		let down_scaler = Scaler::new(&mul_ctx, base_ctx, post_mul_scaling_factor)?;
		Ok(Self {
			par: par.clone(),
			extender_lhs,
			extender_rhs,
			down_scaler,
			base_ctx: base_ctx.clone(),
			mul_ctx,
			rk: None,
			mod_switch: false,
			level,
		})
	}

	/// Default multiplication strategy using relinearization.
	pub fn default(rk: &RelinearizationKey) -> Result<Self> {
		Self::default_at_level(0, rk)
	}

	/// Default multiplication strategy using relinearization at a given level.
	#[cfg(feature = "leveled_bfv")]
	pub fn default_at_level(level: usize, rk: &RelinearizationKey) -> Result<Self> {
		use num_bigint::BigUint;

		let ctx = rk.ksk.par.ctx_at_level(level)?;

		let modulus_size = rk.ksk.par.moduli_sizes()[..ctx.moduli().len()]
			.iter()
			.sum::<usize>();
		let n_moduli = (modulus_size + 60).div_ceil(62);

		let mut extended_basis = Vec::with_capacity(ctx.moduli().len() + n_moduli);
		extended_basis.append(&mut ctx.moduli().to_vec());
		let mut upper_bound = 1 << 62;
		while extended_basis.len() != ctx.moduli().len() + n_moduli {
			upper_bound = generate_prime(62, 2 * rk.ksk.par.degree() as u64, upper_bound).unwrap();
			if !extended_basis.contains(&upper_bound) && !ctx.moduli().contains(&upper_bound) {
				extended_basis.push(upper_bound)
			}
		}

		let mut multiplicator = Multiplicator::new(
			ScalingFactor::one(),
			ScalingFactor::one(),
			&extended_basis,
			ScalingFactor::new(
				&BigUint::from(rk.ksk.par.plaintext.modulus()),
				ctx.modulus(),
			),
			&rk.ksk.par,
		)?;

		multiplicator.enable_relinearization(rk)?;
		Ok(multiplicator)
	}

	/// Enable relinearization after multiplication.
	pub fn enable_relinearization(&mut self, rk: &RelinearizationKey) -> Result<()> {
		let rk_ctx = self.par.ctx_at_level(rk.ksk.ksk_level)?;
		if rk_ctx != &self.base_ctx {
			return Err(Error::DefaultError(
				"Invalid relinearization key context".to_string(),
			));
		}
		self.rk = Some(rk.clone());
		Ok(())
	}

	/// Enable modulus switching after multiplication (and relinearization, if
	/// applicable).
	#[cfg(feature = "leveled_bfv")]
	pub fn enable_mod_switching(&mut self) -> Result<()> {
		if self.par.ctx_at_level(self.par.max_level())? == &self.base_ctx {
			Err(Error::DefaultError(
				"Cannot modulo switch as this is already the last level".to_string(),
			))
		} else {
			self.mod_switch = true;
			Ok(())
		}
	}

	/// Multiply two ciphertexts using the defined multiplication strategy.
	pub fn multiply(&self, lhs: &Ciphertext, rhs: &Ciphertext) -> Result<Ciphertext> {
		if lhs.par != self.par || rhs.par != self.par {
			return Err(Error::DefaultError(
				"Ciphertexts do not have the same parameters".to_string(),
			));
		}
		if lhs.level != self.level || rhs.level != self.level {
			return Err(Error::DefaultError(
				"Ciphertexts are not at expected level".to_string(),
			));
		}
		if lhs.c.len() != 2 || rhs.c.len() != 2 {
			return Err(Error::DefaultError(
				"Multiplication can only be performed on ciphertexts of size 2".to_string(),
			));
		}

		// Extend
		// let mut now = std::time::SystemTime::now();
		let c00 = lhs.c[0].scale(&self.extender_lhs)?;
		let c01 = lhs.c[1].scale(&self.extender_lhs)?;
		let c10 = rhs.c[0].scale(&self.extender_rhs)?;
		let c11 = rhs.c[1].scale(&self.extender_rhs)?;
		// println!("Extend: {:?}", now.elapsed().unwrap());

		// Multiply
		// now = std::time::SystemTime::now();
		let mut c0 = &c00 * &c10;
		let mut c1 = &c00 * &c11;
		c1 += &(&c01 * &c10);
		let mut c2 = &c01 * &c11;
		c0.change_representation(Representation::PowerBasis);
		c1.change_representation(Representation::PowerBasis);
		c2.change_representation(Representation::PowerBasis);
		// println!("Multiply: {:?}", now.elapsed().unwrap());

		// Scale
		// now = std::time::SystemTime::now();
		let c0 = c0.scale(&self.down_scaler)?;
		let c1 = c1.scale(&self.down_scaler)?;
		let c2 = c2.scale(&self.down_scaler)?;
		// println!("Scale: {:?}", now.elapsed().unwrap());

		let mut c = vec![c0, c1, c2];

		if let Some(rk) = self.rk.as_ref() {
			c[0].change_representation(Representation::Ntt);
			c[1].change_representation(Representation::Ntt);
			let (c0r, c1r) = rk.relinearizes_with_poly(&c[2])?;
			c[0] += &c0r;
			c[1] += &c1r;
			c.truncate(2);
		}

		// We construct a ciphertext, but it may not have the right representation for
		// the polynomials yet.
		let mut c = Ciphertext {
			par: self.par.clone(),
			seed: None,
			c,
			level: self.level,
		};

		if self.mod_switch {
			c.mod_switch_to_next_level();
		} else if self.rk.is_none() {
			// We need to fix the polynomials representation in case we did not relinearize.
			c.c.iter_mut()
				.for_each(|p| p.change_representation(Representation::Ntt))
		}

		Ok(c)
	}
}

#[cfg(test)]
mod tests {
	use crate::bfv::{BfvParameters, Encoding, Plaintext, RelinearizationKey, SecretKey};
	use fhers_traits::{FheDecoder, FheDecrypter, FheEncoder, FheEncrypter};
	use std::{error::Error, sync::Arc};

	use super::Multiplicator;

	#[test]
	fn test_mul() -> Result<(), Box<dyn Error>> {
		let par = Arc::new(BfvParameters::default(2));
		for _ in 0..30 {
			// We will encode `values` in an Simd format, and check that the product is
			// computed correctly.
			let values = par.plaintext.random_vec(par.degree());
			let mut expected = values.clone();
			par.plaintext.mul_vec(&mut expected, &values);

			let sk = SecretKey::random(&par);
			let rk = RelinearizationKey::new(&sk)?;
			let pt = Plaintext::try_encode(&values as &[u64], Encoding::simd(), &par)?;
			let ct1 = sk.try_encrypt(&pt)?;
			let ct2 = sk.try_encrypt(&pt)?;

			let mut multiplicator = Multiplicator::default(&rk)?;
			let ct3 = multiplicator.multiply(&ct1, &ct2)?;
			println!("Noise: {}", unsafe { sk.measure_noise(&ct3)? });
			let pt = sk.try_decrypt(&ct3)?;
			assert_eq!(Vec::<u64>::try_decode(&pt, Encoding::simd())?, expected);

			multiplicator.enable_mod_switching()?;
			let ct3 = multiplicator.multiply(&ct1, &ct2)?;
			assert_eq!(ct3.level, 1);
			println!("Noise: {}", unsafe { sk.measure_noise(&ct3)? });
			let pt = sk.try_decrypt(&ct3)?;
			assert_eq!(Vec::<u64>::try_decode(&pt, Encoding::simd())?, expected);
		}
		Ok(())
	}
}