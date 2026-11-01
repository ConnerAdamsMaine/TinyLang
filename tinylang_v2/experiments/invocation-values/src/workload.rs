use crate::invocation::{Failure, Invocation};
use std::{hint::black_box, time::Instant};
use tinyone::RuntimeValue;

pub fn benchmark() {
    let mut invocation = Invocation::new();
    println!("sample,iterations,ns_per_invocation");
    for sample in 0..8 {
        let start = Instant::now();
        for _ in 0..100_000 {
            match invocation.invoke(black_box(&[50, 8])) {
                Ok(value) => assert_eq!(black_box(value), RuntimeValue::I64(42)),
                Err(Failure::Execution(error)) => panic!("execution failed: {error}"),
                Err(Failure::Arguments) => panic!("fixture arguments rejected"),
            }
        }
        if sample != 0 {
            println!(
                "{sample},100000,{:.3}",
                start.elapsed().as_secs_f64() * 1e9 / 100_000.0
            );
        }
    }
}
