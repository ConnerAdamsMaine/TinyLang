use tinyone::{RuntimeValue, VM, VerifiedProgram, VmExecutionLimits};

#[derive(Debug)]
pub enum Failure {
    Arguments,
    Execution(tinyone::TinyOneError),
}

pub struct Invocation {
    vm: VM,
    function: usize,
    #[cfg(feature = "c")]
    stack: Vec<RuntimeValue>,
}

impl Invocation {
    pub fn new() -> Self {
        let compiled = tinyone::compile_source("fn difference(a, b) { return a - b }").unwrap();
        // Exercise the same untrusted compiler-artifact loader as the v2 adapter.
        let program =
            VerifiedProgram::from_binary_artifact(&compiled.to_binary_artifact().unwrap()).unwrap();
        let function = program
            .program()
            .functions()
            .iter()
            .position(|f| f.name() == "difference")
            .unwrap();
        Self {
            vm: VM::from_verified_with_limits(&program, vec![], VmExecutionLimits::default())
                .unwrap(),
            function,
            #[cfg(feature = "c")]
            stack: Vec::new(),
        }
    }

    fn execute(&mut self, arguments: Vec<RuntimeValue>) -> Result<RuntimeValue, Failure> {
        self.vm
            .invoke_function_with_fuel(self.function, arguments, &mut Vec::new(), 100)
            .map_err(Failure::Execution)
    }

    // Host-side input encoding is included for all candidates. This is a Rust
    // experiment; none of these functions is an exported foreign entry point.
    #[inline(never)]
    pub fn invoke(&mut self, input: &[i64]) -> Result<RuntimeValue, Failure> {
        #[cfg(feature = "baseline")]
        {
            if input.len() != 2 {
                return Err(Failure::Arguments);
            }
            self.execute(input.iter().copied().map(RuntimeValue::I64).collect())
        }
        #[cfg(feature = "a")]
        {
            let mut bytes = Vec::with_capacity(input.len() * 24);
            for value in input {
                bytes.extend_from_slice(&9u16.to_le_bytes()); // Canonical I64 tag.
                bytes.extend_from_slice(&0u16.to_le_bytes());
                bytes.extend_from_slice(&8u32.to_le_bytes());
                bytes.extend_from_slice(&0u64.to_le_bytes());
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            self.tagged(&bytes)
        }
        #[cfg(feature = "b")]
        {
            let mut bytes = Vec::with_capacity(input.len() * 8);
            for value in input {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            self.packed(&bytes)
        }
        #[cfg(feature = "c")]
        {
            for value in input {
                if let Err(error) = self.push(*value) {
                    self.stack.clear();
                    return Err(error);
                }
            }
            self.call_stack()
        }
    }

    #[cfg(feature = "a")]
    pub fn tagged(&mut self, bytes: &[u8]) -> Result<RuntimeValue, Failure> {
        if bytes.len() != 48 {
            return Err(Failure::Arguments);
        }
        let mut args = Vec::with_capacity(2);
        for record in bytes.as_chunks::<24>().0 {
            if record[..16] != [9, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
                return Err(Failure::Arguments);
            }
            args.push(RuntimeValue::I64(i64::from_le_bytes(
                record[16..24].try_into().unwrap(),
            )));
        }
        self.execute(args)
    }

    #[cfg(feature = "b")]
    pub fn packed(&mut self, bytes: &[u8]) -> Result<RuntimeValue, Failure> {
        // A prepared signature says exactly two little-endian i64 values.
        if bytes.len() != 16 {
            return Err(Failure::Arguments);
        }
        let args = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|value| RuntimeValue::I64(i64::from_le_bytes(*value)))
            .collect();
        self.execute(args)
    }

    #[cfg(feature = "c")]
    #[inline(never)]
    fn push(&mut self, value: i64) -> Result<(), Failure> {
        if self.stack.len() == 2 {
            return Err(Failure::Arguments);
        }
        self.stack.push(RuntimeValue::I64(value));
        Ok(())
    }

    #[cfg(feature = "c")]
    #[inline(never)]
    fn call_stack(&mut self) -> Result<RuntimeValue, Failure> {
        if self.stack.len() != 2 {
            self.stack.clear();
            return Err(Failure::Arguments);
        }
        let args = std::mem::take(&mut self.stack);
        self.execute(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_function_preserves_argument_order_width_and_recovers_after_rejection() {
        let mut invocation = Invocation::new();
        for values in [&[][..], &[1][..], &[1, 2, 3][..]] {
            assert!(matches!(invocation.invoke(values), Err(Failure::Arguments)));
            assert_eq!(invocation.invoke(&[50, 8]).unwrap(), RuntimeValue::I64(42));
        }
        for (a, b, result) in [
            (i64::MAX, 0, i64::MAX),
            (i64::MIN, 0, i64::MIN),
            (-20, 22, -42),
        ] {
            assert_eq!(
                invocation.invoke(&[a, b]).unwrap(),
                RuntimeValue::I64(result)
            );
        }
    }

    #[cfg(feature = "a")]
    #[test]
    fn tagged_input_rejects_wrong_type_flags_length_and_auxiliary() {
        let mut invocation = Invocation::new();
        let record = [
            9, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,
        ];
        let bytes = [record, record].concat();
        for index in 0..16 {
            let mut invalid = bytes.clone();
            invalid[index] ^= 1;
            assert!(matches!(
                invocation.tagged(&invalid),
                Err(Failure::Arguments)
            ));
        }
        assert!(matches!(
            invocation.tagged(&bytes[..47]),
            Err(Failure::Arguments)
        ));
    }

    #[cfg(feature = "b")]
    #[test]
    fn prepared_signature_rejects_truncation_and_extra_values() {
        let mut invocation = Invocation::new();
        for length in [0, 8, 15, 17, 24] {
            assert!(matches!(
                invocation.packed(&vec![0; length]),
                Err(Failure::Arguments)
            ));
        }
    }
}
