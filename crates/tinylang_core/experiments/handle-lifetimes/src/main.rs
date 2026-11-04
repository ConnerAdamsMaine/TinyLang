mod registry;

use std::hint::black_box;
use std::time::Instant;

use registry::{Registry, Selected};

fn main() {
    let mut registry = Selected::new();
    let start = Instant::now();
    let first = registry.insert().unwrap();
    assert!(registry.remove(first));
    println!("initialization_ns,{}", start.elapsed().as_nanos());
    println!("registry_inline_bytes,{}", std::mem::size_of::<Selected>());
    println!("workload,sample,iterations,ns_per_cycle");
    for occupied in [0, 63] {
        let mut retained = Vec::new();
        for _ in 0..occupied {
            retained.push(registry.insert().unwrap());
        }
        for sample in 0..8 {
            let start = Instant::now();
            for _ in 0..10_000_000 {
                let handle = black_box(&mut registry).insert().unwrap();
                assert!(black_box(&registry).contains(black_box(handle)));
                assert!(black_box(&mut registry).remove(black_box(handle)));
                assert!(!black_box(&registry).contains(black_box(handle)));
            }
            let elapsed = start.elapsed().as_secs_f64();
            if sample != 0 {
                println!(
                    "occupied_{occupied},{sample},10000000,{:.3}",
                    elapsed * 1e9 / 10_000_000.0
                );
            }
        }
        for handle in retained {
            assert!(registry.remove(handle));
        }
    }
}
