use crate::invocation::Failure;
use std::{hint::black_box, time::Instant};
use tinyone::{RuntimeValue, VM, VerifiedProgram, VmExecutionLimits, VmHeapValue};

pub fn fixture() -> Vec<VmHeapValue> {
    vec![
        VmHeapValue::String("snowman ☃\0tail".into()),
        VmHeapValue::Bytes(vec![0, 255, 128, 42]),
        VmHeapValue::Array(vec![RuntimeValue::I64(i64::MIN), RuntimeValue::I64(42)]),
    ]
}

struct HeapInvocation {
    vm: VM,
    #[cfg(feature = "c")]
    stack: Vec<RuntimeValue>,
}

impl HeapInvocation {
    fn new() -> Self {
        let compiled = tinyone::compile_source("fn identity(value) { return value }").unwrap();
        let verified =
            VerifiedProgram::from_binary_artifact(&compiled.to_binary_artifact().unwrap()).unwrap();
        Self {
            // Only three live objects fit. Repeated calls prove successful
            // fixture cleanup, rather than relying on a large heap budget.
            vm: VM::from_verified_with_limits(
                &verified,
                vec![],
                VmExecutionLimits::new(4096, 3, 4).unwrap(),
            )
            .unwrap(),
            #[cfg(feature = "c")]
            stack: Vec::new(),
        }
    }

    #[cfg(feature = "c")]
    #[inline(never)]
    fn push(&mut self, value: VmHeapValue) -> Result<(), Failure> {
        let value = self
            .vm
            .import_heap_value(value)
            .map_err(Failure::Execution)?;
        self.stack.push(value);
        Ok(())
    }

    #[inline(never)]
    fn invoke(&mut self, input: &[VmHeapValue]) -> Result<Vec<VmHeapValue>, Failure> {
        #[cfg(any(feature = "a", feature = "b"))]
        let input = crate::heap_wire::decode(&crate::heap_wire::encode(input)?)?;
        #[cfg(any(feature = "baseline", feature = "c"))]
        let input = input.to_vec();

        let mut args = Vec::new();
        #[cfg(feature = "c")]
        {
            for value in input {
                if let Err(error) = self.push(value) {
                    for value in self.stack.drain(..) {
                        self.vm.release_heap_value(&value).unwrap();
                    }
                    return Err(error);
                }
            }
            args.append(&mut self.stack);
        }
        #[cfg(not(feature = "c"))]
        for value in input {
            match self.vm.import_heap_value(value) {
                Ok(value) => args.push(value),
                Err(error) => {
                    for value in args {
                        self.vm.release_heap_value(&value).unwrap();
                    }
                    return Err(Failure::Execution(error));
                }
            }
        }

        let result = (|| {
            let mut output = Vec::new();
            for value in &args {
                let result = self
                    .vm
                    .invoke_function_with_fuel(0, vec![value.clone()], &mut Vec::new(), 10)
                    .map_err(Failure::Execution)?;
                // Identity returns an alias, so only the original is freed.
                output.push(
                    self.vm
                        .copy_heap_value(&result)
                        .map_err(Failure::Execution)?,
                );
            }
            #[cfg(any(feature = "a", feature = "b"))]
            {
                crate::heap_wire::decode(&crate::heap_wire::encode(&output)?)
            }
            #[cfg(any(feature = "baseline", feature = "c"))]
            {
                Ok(output)
            }
        })();
        for value in args {
            self.vm.release_heap_value(&value).unwrap();
        }
        result
    }
}

pub fn benchmark() {
    let mut invocation = HeapInvocation::new();
    let input = fixture();
    println!("sample,iterations,ns_per_roundtrip");
    for sample in 0..8 {
        let start = Instant::now();
        for _ in 0..10_000 {
            assert_eq!(
                black_box(invocation.invoke(black_box(&input)).unwrap()),
                input
            );
        }
        if sample != 0 {
            println!(
                "{sample},10000,{:.3}",
                start.elapsed().as_secs_f64() * 1e9 / 10_000.0
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_heap_roundtrips_preserve_contents_and_release_all_three_objects() {
        let mut invocation = HeapInvocation::new();
        let input = fixture();
        for _ in 0..100 {
            assert_eq!(invocation.invoke(&input).unwrap(), input);
        }
        let empty = vec![
            VmHeapValue::String(String::new()),
            VmHeapValue::Bytes(vec![]),
            VmHeapValue::Array(vec![]),
        ];
        assert_eq!(invocation.invoke(&empty).unwrap(), empty);
    }
    #[test]
    fn allocation_failure_cleans_earlier_arguments_and_allows_retry() {
        let mut invocation = HeapInvocation::new();
        let mut input = fixture();
        input[1] = VmHeapValue::Bytes(vec![0; 8192]);
        assert!(invocation.invoke(&input).is_err());
        assert_eq!(invocation.invoke(&fixture()).unwrap(), fixture());
    }
}
