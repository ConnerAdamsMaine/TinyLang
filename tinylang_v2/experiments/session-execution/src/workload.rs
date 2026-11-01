use crate::session::{Outcome, Selected, Session};
use std::hint::black_box;
use std::time::Instant;

pub fn benchmark() {
    println!("fuel,sample,iterations,ns_per_invocation");
    for fuel in [4, 5] {
        let mut session = Selected::new();
        let mut total = 0;
        for sample in 0..8 {
            let start = Instant::now();
            for _ in 0..100_000 {
                total += 1;
                let value = if cfg!(feature = "baseline") { 1 } else { total };
                let expected = if fuel == 4 {
                    Outcome::Trapped(value)
                } else {
                    Outcome::Completed(value)
                };
                assert_eq!(black_box(session.invoke(black_box(fuel))), expected);
            }
            if sample != 0 {
                println!(
                    "{fuel},{sample},100000,{:.3}",
                    start.elapsed().as_secs_f64() * 1e9 / 100_000.0
                );
            }
        }
    }
}
