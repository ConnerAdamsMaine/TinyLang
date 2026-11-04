use std::hint::black_box;
#[cfg(feature = "c")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub executed: u64,
    pub checksum: u64,
}

fn effect(checksum: &mut u64, instruction: u64) {
    *checksum = black_box(*checksum).wrapping_add(black_box(instruction));
}

// The trace comprises straight-line blocks with known sizes. No speculative
// side effects occur: a short budget executes only the affordable prefix.
pub fn run(blocks: &[u64], fuel: u64) -> Outcome {
    let mut outcome = Outcome {
        executed: 0,
        checksum: 0,
    };
    #[cfg(any(feature = "a", feature = "b"))]
    let mut remaining = fuel;
    #[cfg(feature = "c")]
    let remaining = AtomicU64::new(fuel);
    #[cfg(feature = "baseline")]
    let _ = fuel;
    for &length in blocks {
        #[cfg(feature = "b")]
        {
            let admitted = length.min(remaining);
            remaining -= admitted;
            for _ in 0..admitted {
                effect(&mut outcome.checksum, outcome.executed);
                outcome.executed += 1;
            }
            if admitted != length {
                return outcome;
            }
        }
        #[cfg(not(feature = "b"))]
        for _ in 0..length {
            #[cfg(feature = "a")]
            {
                let Some(next) = remaining.checked_sub(1) else {
                    return outcome;
                };
                remaining = next;
            }
            #[cfg(feature = "c")]
            if remaining
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_sub(1)
                })
                .is_err()
            {
                return outcome;
            }
            effect(&mut outcome.checksum, outcome.executed);
            outcome.executed += 1;
        }
    }
    outcome
}

pub fn benchmark() {
    println!("workload,sample,instructions,ns_per_instruction");
    for block_size in [1u64, 32] {
        let blocks = vec![block_size; 1024];
        let count = block_size * blocks.len() as u64;
        let iterations = 100_000 / block_size;
        for sample in 0..8 {
            let start = Instant::now();
            for _ in 0..iterations {
                let result = run(black_box(&blocks), black_box(count));
                assert_eq!(result.executed, count);
                assert_eq!(result.checksum, count * (count - 1) / 2);
                black_box(result);
            }
            if sample != 0 {
                println!(
                    "block_{block_size},{sample},{},{:.3}",
                    count * iterations,
                    start.elapsed().as_secs_f64() * 1e9 / (count * iterations) as f64
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_effect_prefix_at_every_budget_boundary() {
        let blocks = [0, 1, 7, 0, 32, 2];
        for budget in 0..=45 {
            let expected = if cfg!(feature = "baseline") {
                42
            } else {
                budget.min(42)
            };
            assert_eq!(
                run(&blocks, budget),
                Outcome {
                    executed: expected,
                    checksum: expected * expected.saturating_sub(1) / 2
                }
            );
        }
    }

    #[test]
    fn maximum_budget_does_not_overflow() {
        assert_eq!(
            run(&[3, 4], u64::MAX),
            Outcome {
                executed: 7,
                checksum: 21
            }
        );
        assert_eq!(
            run(&[], 0),
            Outcome {
                executed: 0,
                checksum: 0
            }
        );
    }
}
