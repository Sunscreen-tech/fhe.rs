#![feature(int_log)]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use fhers::bfv::{
	dot_product_scalar, BfvParameters, BfvParametersBuilder, Encoding, Plaintext, SecretKey,
};
use fhers_traits::{FheEncoder, FheEncrypter};
use itertools::{izip, Itertools};
use std::time::Duration;
use std::{error::Error, sync::Arc};

fn params() -> Result<Vec<Arc<BfvParameters>>, Box<dyn Error>> {
	let par_small = BfvParametersBuilder::new()
		.set_degree(4096)
		.set_plaintext_modulus(1153)
		.set_ciphertext_moduli_sizes(&[36, 37, 37])
		.build()?;
	let par_large = BfvParametersBuilder::new()
		.set_degree(16384)
		.set_plaintext_modulus(1153)
		.set_ciphertext_moduli_sizes(&[62; 7])
		.build()
		.unwrap();
	Ok(vec![Arc::new(par_small), Arc::new(par_large)])
}

pub fn bfv_benchmark(c: &mut Criterion) {
	let mut group = c.benchmark_group("bfv_optimized_ops");
	group.sample_size(10);
	group.warm_up_time(Duration::from_secs(1));
	group.measurement_time(Duration::from_secs(1));

	for par in params().unwrap() {
		let sk = SecretKey::random(&par);
		let pt1 =
			Plaintext::try_encode(&(1..16u64).collect_vec() as &[u64], Encoding::poly(), &par)
				.unwrap();
		let pt2 =
			Plaintext::try_encode(&(3..39u64).collect_vec() as &[u64], Encoding::poly(), &par)
				.unwrap();
		let mut c1 = sk.try_encrypt(&pt1).unwrap();
		let c2 = sk.try_encrypt(&pt2).unwrap();

		let ct_vec = (0..128)
			.map(|i| {
				let pt = Plaintext::try_encode(
					&(i..16u64).collect_vec() as &[u64],
					Encoding::poly(),
					&par,
				)
				.unwrap();
				sk.try_encrypt(&pt).unwrap()
			})
			.collect_vec();
		let pt_vec = (0..128)
			.map(|i| {
				Plaintext::try_encode(&(i..39u64).collect_vec() as &[u64], Encoding::poly(), &par)
					.unwrap()
			})
			.collect_vec();

		group.bench_function(
			BenchmarkId::new(
				"dot_product/128/naive",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| izip!(&ct_vec, &pt_vec).for_each(|(cti, pti)| c1 += cti * pti));
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"dot_product/128/opt",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| dot_product_scalar(ct_vec.iter(), pt_vec.iter()));
			},
		);
	}

	group.finish();
}

criterion_group!(bfv, bfv_benchmark);
criterion_main!(bfv);