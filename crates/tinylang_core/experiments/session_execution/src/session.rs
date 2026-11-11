use std::sync::Arc;
use tinylang::{Instr, Op, Program, RuntimeValue, TinyMemory, VM};

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Completed(i64),
    Trapped(i64),
}

pub trait Session {
    fn new() -> Self;
    fn invoke(&mut self, fuel: u64) -> Outcome;
}

fn make_vm() -> VM {
    let program = Program::new(
        vec![
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ],
        1,
    );
    let mut memory = TinyMemory::try_new(1).unwrap();
    memory.store(0, RuntimeValue::I64(0)).unwrap();
    VM::new(Arc::new(program), memory, vec![]).unwrap()
}

fn execute(vm: &mut VM, fuel: u64) -> Outcome {
    let result = vm.execute_main_with_fuel(&mut Vec::new(), fuel);
    let RuntimeValue::I64(value) = vm.memory().load(0).unwrap() else {
        panic!("fixture invariant")
    };
    match result {
        Ok(()) => Outcome::Completed(value),
        Err(_) => Outcome::Trapped(value),
    }
}

#[cfg(feature = "baseline")]
pub struct Selected;
#[cfg(feature = "baseline")]
impl Session for Selected {
    fn new() -> Self {
        Self
    }
    fn invoke(&mut self, fuel: u64) -> Outcome {
        execute(&mut make_vm(), fuel)
    }
}

#[cfg(feature = "a")]
pub struct Selected(VM);
#[cfg(feature = "a")]
impl Session for Selected {
    fn new() -> Self {
        Self(make_vm())
    }
    fn invoke(&mut self, fuel: u64) -> Outcome {
        execute(&mut self.0, fuel)
    }
}

#[cfg(feature = "b")]
pub struct Selected(std::sync::Mutex<VM>);
#[cfg(feature = "b")]
impl Session for Selected {
    fn new() -> Self {
        Self(std::sync::Mutex::new(make_vm()))
    }
    fn invoke(&mut self, fuel: u64) -> Outcome {
        execute(&mut self.0.lock().unwrap(), fuel)
    }
}

#[cfg(feature = "c")]
enum Command {
    Invoke(u64),
    Stop,
}
#[cfg(feature = "c")]
pub struct Selected {
    sender: std::sync::mpsc::SyncSender<Command>,
    receiver: std::sync::mpsc::Receiver<Outcome>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[cfg(feature = "c")]
impl Session for Selected {
    fn new() -> Self {
        let (sender, commands) = std::sync::mpsc::sync_channel(1);
        let (results, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let mut vm = make_vm();
            while let Ok(Command::Invoke(fuel)) = commands.recv() {
                if results.send(execute(&mut vm, fuel)).is_err() {
                    break;
                }
            }
        });
        Self {
            sender,
            receiver,
            worker: Some(worker),
        }
    }
    fn invoke(&mut self, fuel: u64) -> Outcome {
        self.sender.send(Command::Invoke(fuel)).unwrap();
        self.receiver.recv().unwrap()
    }
}
#[cfg(feature = "c")]
impl Drop for Selected {
    fn drop(&mut self) {
        if self.sender.send(Command::Stop).is_err() {
            eprintln!("session worker had already stopped");
        }
        if let Some(worker) = self.worker.take() {
            worker.join().expect("session worker panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_state_after_success_and_trap() {
        let mut session = Selected::new();
        assert_eq!(session.invoke(0), Outcome::Trapped(0));
        assert_eq!(session.invoke(5), Outcome::Completed(1));
        if cfg!(feature = "baseline") {
            assert_eq!(session.invoke(4), Outcome::Trapped(1));
            assert_eq!(session.invoke(5), Outcome::Completed(1));
        } else {
            assert_eq!(session.invoke(4), Outcome::Trapped(2));
            assert_eq!(session.invoke(5), Outcome::Completed(3));
        }
    }
}
