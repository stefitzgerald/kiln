//! TC-ECS-08: query throughput baseline. Run with `cargo bench -p kiln_ecs`.
#![allow(missing_docs, clippy::unwrap_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use kiln_ecs::{Component, World};
use std::hint::black_box;

struct Position([f32; 3]);
impl Component for Position {}
struct Velocity([f32; 3]);
impl Component for Velocity {}
struct Health(#[allow(dead_code)] f32);
impl Component for Health {}

fn world(n: usize) -> World {
    let mut w = World::new();
    for i in 0..n {
        let e = w.spawn((Position([0.0; 3]), Velocity([1.0, 0.5, 0.25])));
        if i % 3 == 0 {
            w.insert(e, Health(100.0)).unwrap();
        }
    }
    w
}

fn bench(c: &mut Criterion) {
    let mut w = world(100_000);
    c.bench_function("query_100k_pos_vel", |b| {
        b.iter(|| {
            for (p, v) in w.query::<(&mut Position, &Velocity)>() {
                p.0[0] += v.0[0];
                p.0[1] += v.0[1];
                p.0[2] += v.0[2];
            }
        })
    });
    c.bench_function("query_100k_sparse_health", |b| {
        b.iter(|| black_box(w.query::<(&Position, &Health)>().count()))
    });
    c.bench_function("spawn_despawn_10k", |b| {
        b.iter(|| {
            let mut w = World::new();
            let es: Vec<_> = (0..10_000).map(|_| w.spawn(Position([0.0; 3]))).collect();
            for e in es {
                w.despawn(e);
            }
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
