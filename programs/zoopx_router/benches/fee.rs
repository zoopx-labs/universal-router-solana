// Run:
//   cargo test -p zoopx_router
//   cargo bench -p zoopx_router

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use rand::{rngs::StdRng, Rng, SeedableRng};
use zoopx_router::validate_fee_cap;

fn bench_fee(c: &mut Criterion) {
    c.bench_function("validate_fee_cap 10k samples", |b| {
        let mut rng = StdRng::seed_from_u64(42);
        let pairs: Vec<(u64, u64)> = (0..10_000)
            .map(|_| {
                let amount: u64 = rng.gen_range(1_000u64..1_000_000u64);
                let max_fee: u64 = amount.saturating_mul(5) / 10_000;
                let fee: u64 = if max_fee == 0 {
                    0
                } else {
                    rng.gen_range(0..=max_fee)
                };
                (amount, fee)
            })
            .collect();

        b.iter_batched(
            || pairs.clone(),
            |ps| {
                for (a, f) in ps {
                    let _ = validate_fee_cap(a, f).unwrap();
                }
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_fee);
criterion_main!(benches);
