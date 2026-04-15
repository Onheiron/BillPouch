//! Criterion benchmarks for the core coding and storage subsystems.
//!
//! Run with:
//!   cargo bench -p bp-core
//!
//! To compare against a saved baseline:
//!   cargo bench -p bp-core -- --baseline main

use bp_core::{
    coding::{
        gf256,
        params::{compute_coding_params, compute_network_storage_factor},
        rlnc::{decode, encode, recode},
    },
    storage::encryption::ChunkCipher,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::RngCore;

// ── GF(2⁸) arithmetic ─────────────────────────────────────────────────────────

fn bench_gf256(c: &mut Criterion) {
    let mut group = c.benchmark_group("gf256");

    group.bench_function("mul_random", |b| {
        let mut rng = rand::thread_rng();
        b.iter(|| {
            let a = (rng.next_u32() & 0xFF) as u8;
            let x = (rng.next_u32() & 0xFF) as u8;
            black_box(gf256::mul(a, x))
        })
    });

    group.bench_function("inv_random", |b| {
        let mut rng = rand::thread_rng();
        b.iter(|| {
            // Avoid inv(0) which is undefined/0 by convention
            let a = ((rng.next_u32() & 0xFE) as u8) | 1;
            black_box(gf256::inv(a))
        })
    });

    group.finish();
}

// ── RLNC encode ──────────────────────────────────────────────────────────────

fn bench_rlnc_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("rlnc_encode");

    for (k, n, chunk_kb) in [(4, 8, 64), (8, 16, 256), (16, 32, 512)] {
        let chunk_size = chunk_kb * 1024;
        group.throughput(Throughput::Bytes((chunk_size * k) as u64));
        group.bench_with_input(
            BenchmarkId::new(format!("k{k}_n{n}"), format!("{chunk_kb}kB")),
            &(k, n, chunk_size),
            |b, &(k, n, sz)| {
                let mut data = vec![0u8; sz * k];
                rand::thread_rng().fill_bytes(&mut data);
                b.iter(|| {
                    black_box(encode(black_box(&data), k, n).expect("encode failed"))
                })
            },
        );
    }

    group.finish();
}

// ── RLNC recode ──────────────────────────────────────────────────────────────

fn bench_rlnc_recode(c: &mut Criterion) {
    let mut group = c.benchmark_group("rlnc_recode");

    let k = 8usize;
    let n = 16usize;
    let chunk_size = 64 * 1024usize;

    let mut data = vec![0u8; chunk_size * k];
    rand::thread_rng().fill_bytes(&mut data);
    let encoded = encode(&data, k, n).expect("encode failed");

    group.throughput(Throughput::Bytes((chunk_size * k) as u64));
    group.bench_function("k8_n16_64kB", |b| {
        b.iter(|| {
            black_box(recode(black_box(&encoded), k).expect("recode failed"))
        })
    });

    group.finish();
}

// ── RLNC decode (varying drop rate) ──────────────────────────────────────────

fn bench_rlnc_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("rlnc_decode");

    let k = 8usize;
    let n = 16usize;
    let chunk_size = 64 * 1024usize;

    let mut data = vec![0u8; chunk_size * k];
    rand::thread_rng().fill_bytes(&mut data);
    let encoded = encode(&data, k, n).expect("encode failed");

    // Decode with exactly k fragments (no drops beyond redundancy)
    let min_set: Vec<_> = encoded[..k].to_vec();

    group.throughput(Throughput::Bytes((chunk_size * k) as u64));
    group.bench_function("k8_exact_k_frags", |b| {
        b.iter(|| {
            black_box(decode(black_box(&min_set), k, chunk_size).expect("decode failed"))
        })
    });

    // Decode with all n fragments (extra work but tests the general path)
    group.bench_function("k8_all_n_frags", |b| {
        b.iter(|| {
            black_box(decode(black_box(&encoded), k, chunk_size).expect("decode failed"))
        })
    });

    group.finish();
}

// ── CEK encryption / decryption ───────────────────────────────────────────────

fn bench_cek(c: &mut Criterion) {
    let mut group = c.benchmark_group("cek");

    for chunk_kb in [64usize, 256, 1024] {
        let chunk_size = chunk_kb * 1024;
        group.throughput(Throughput::Bytes(chunk_size as u64));

        let cek = ChunkCipher::generate();
        let mut plaintext = vec![0u8; chunk_size];
        rand::thread_rng().fill_bytes(&mut plaintext);

        group.bench_with_input(
            BenchmarkId::new("encrypt", format!("{chunk_kb}kB")),
            &chunk_size,
            |b, _| {
                b.iter(|| {
                    black_box(cek.encrypt(black_box(&plaintext)).expect("encrypt failed"))
                })
            },
        );

        let ciphertext = cek.encrypt(&plaintext).expect("encrypt failed");
        group.bench_with_input(
            BenchmarkId::new("decrypt", format!("{chunk_kb}kB")),
            &chunk_size,
            |b, _| {
                b.iter(|| {
                    black_box(cek.decrypt(black_box(&ciphertext)).expect("decrypt failed"))
                })
            },
        );
    }

    group.finish();
}

// ── Coding params (Poisson-Binomial k-computation) ───────────────────────────

fn bench_coding_params(c: &mut Criterion) {
    let mut group = c.benchmark_group("coding_params");

    for n_peers in [5usize, 20, 100] {
        let stabilities: Vec<f64> = (0..n_peers)
            .map(|i| 0.5 + 0.4 * (i as f64 / n_peers as f64))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("compute_coding_params", format!("N={n_peers}")),
            &stabilities,
            |b, stabs| {
                b.iter(|| {
                    black_box(
                        compute_coding_params(black_box(stabs), 0.999, 1.0)
                            .expect("params failed"),
                    )
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("storage_factor", format!("N={n_peers}")),
            &stabilities,
            |b, stabs| {
                b.iter(|| {
                    black_box(compute_network_storage_factor(black_box(stabs), 0.90))
                })
            },
        );
    }

    group.finish();
}

// ── Register benchmarks ───────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_gf256,
    bench_rlnc_encode,
    bench_rlnc_recode,
    bench_rlnc_decode,
    bench_cek,
    bench_coding_params,
);
criterion_main!(benches);
