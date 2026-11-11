use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{ObjectKind, ObjectPayload, REGISTRY, find_object};
use crate::interoperability::ffi::v2::session::VmSession;
use crate::interoperability::ffi::v2::status::{TINY_STATUS_BUSY, TINY_STATUS_INTERNAL, TINY_STATUS_PANIC, TinyStatus};
use crate::interoperability::ffi::v2::types::TinyRuntimeHandle;

// Integration of the selected direct-ownership session strategy. No VM code
// or user callback runs under the registry mutex. The live Running slot pins
// both identities until execution has returned, including during unwinding.
pub(in crate::interoperability::ffi::v2) fn with_vm<R>(
    runtime: TinyRuntimeHandle,
    handle: u64,
    execute: impl FnOnce(&mut VmSession) -> Result<R, TinyStatus>,
) -> Result<R, TinyStatus> {
    let (index, mut session) = {
        let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
        registry.validate_runtime(runtime)?;
        let index = find_object(&registry, runtime, handle, Some(ObjectKind::Vm))?;
        let payload = &mut registry.objects[index].payload;
        match payload.as_ref() {
            Some(ObjectPayload::Vm(_)) => {}
            Some(ObjectPayload::VmRunning) => return Err(TINY_STATUS_BUSY),
            _ => return Err(TINY_STATUS_INTERNAL),
        }
        let Some(ObjectPayload::Vm(session)) = payload.replace(ObjectPayload::VmRunning) else {
            return Err(TINY_STATUS_INTERNAL);
        };
        (index, session)
    };

    // VM invariants are not promised after panic. Only successful unwinding
    // to this boundary permits quarantine; aborting panics cannot be caught.
    let outcome = catch_unwind(AssertUnwindSafe(|| execute(&mut session)));
    let payload = if outcome.is_ok() {
        ObjectPayload::Vm(session)
    } else {
        drop(session);
        ObjectPayload::VmFailed
    };
    {
        let mut registry = REGISTRY.lock().map_err(|_| TINY_STATUS_INTERNAL)?;
        // Release rejects Running and runtime destruction rejects live objects,
        // so no caller can recycle this slot while the session is checked out.
        registry.objects[index].payload = Some(payload);
    }
    outcome.map_err(|_| TINY_STATUS_PANIC)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interoperability::ffi::v2::handles::{
        create_runtime, destroy_runtime, insert_vm, release_object, validate_runtime,
    };
    use crate::interoperability::ffi::v2::schema::VmCreate;
    use crate::interoperability::ffi::v2::status::{
        TINY_STATUS_INVALID_HANDLE, TINY_STATUS_OK, TINY_STATUS_STALE_HANDLE, TINY_STATUS_VM_TRAP,
    };
    use crate::{Instr, Op, Program, RuntimeValue, VerifiedProgram};

    fn session(runtime: TinyRuntimeHandle) -> u64 {
        let program = Program::new(
            vec![
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::Store, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            1,
        );
        let session = VmSession::new(
            VerifiedProgram::verify(program).unwrap(),
            VmCreate {
                program: 0,
                max_heap_bytes: 1024,
                max_heap_objects: 16,
                max_call_depth: 4,
                vm_flags: 0,
            },
        )
        .unwrap();
        insert_vm(runtime, session).unwrap()
    }

    #[test]
    fn reentrant_access_is_busy_and_traps_restore_completed_state() {
        let runtime = create_runtime().unwrap();
        let vm = session(runtime);
        let other = session(runtime);
        let result: Result<(), _> = with_vm(runtime, vm, |session| {
            assert_eq!(with_vm(runtime, vm, |_| Ok(())), Err(TINY_STATUS_BUSY));
            assert_eq!(release_object(runtime, vm), TINY_STATUS_BUSY);
            assert_eq!(destroy_runtime(runtime), TINY_STATUS_BUSY);
            assert_eq!(with_vm(runtime, other, |_| Ok(())), Ok(()));
            session
                .execution
                .execute_main_with_fuel(&mut Vec::new(), 2)
                .map_err(|_| TINY_STATUS_VM_TRAP)
        });
        assert_eq!(result, Err(TINY_STATUS_VM_TRAP));
        with_vm(runtime, vm, |session| {
            assert_eq!(session.execution.memory().load(0).unwrap(), RuntimeValue::I64(42));
            Ok(())
        })
        .unwrap();
        assert_eq!(release_object(runtime, vm), TINY_STATUS_OK);
        assert_eq!(with_vm(runtime, vm, |_| Ok(())), Err(TINY_STATUS_STALE_HANDLE));
        assert_eq!(release_object(runtime, other), TINY_STATUS_OK);
        assert_eq!(destroy_runtime(runtime), TINY_STATUS_OK);
    }

    #[test]
    fn concurrent_release_and_invoke_cannot_recycle_an_executing_session() {
        let runtime = create_runtime().unwrap();
        let vm = session(runtime);
        let foreign = create_runtime().unwrap();
        assert_eq!(with_vm(foreign, vm, |_| Ok(())), Err(TINY_STATUS_INVALID_HANDLE));
        let (entered, waiting) = std::sync::mpsc::channel();
        let (resume, resumed) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            with_vm(runtime, vm, |_| {
                entered.send(()).unwrap();
                resumed.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                Ok(())
            })
        });
        waiting.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(release_object(runtime, vm), TINY_STATUS_BUSY);
        assert_eq!(with_vm(runtime, vm, |_| Ok(())), Err(TINY_STATUS_BUSY));
        assert_eq!(validate_runtime(foreign), TINY_STATUS_OK);
        resume.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert_eq!(with_vm(runtime, vm, |_| Ok(())), Ok(()));
        assert_eq!(release_object(runtime, vm), TINY_STATUS_OK);
        assert_eq!(destroy_runtime(runtime), TINY_STATUS_OK);
        assert_eq!(destroy_runtime(foreign), TINY_STATUS_OK);
    }

    #[test]
    fn panic_quarantines_only_the_executing_vm_and_allows_cleanup() {
        let runtime = create_runtime().unwrap();
        let vm = session(runtime);
        let other = session(runtime);
        let result: Result<(), _> = with_vm(runtime, vm, |_| panic!("injected VM failure"));
        assert_eq!(result, Err(TINY_STATUS_PANIC));
        assert_eq!(with_vm(runtime, vm, |_| Ok(())), Err(TINY_STATUS_INTERNAL));
        assert_eq!(with_vm(runtime, other, |_| Ok(())), Ok(()));
        assert_eq!(release_object(runtime, vm), TINY_STATUS_OK);
        assert_eq!(release_object(runtime, other), TINY_STATUS_OK);
        assert_eq!(destroy_runtime(runtime), TINY_STATUS_OK);
    }

    #[test]
    #[ignore = "manual release benchmark; includes registry entry/exit and execution"]
    fn measure_checked_execution() {
        let runtime = create_runtime().unwrap();
        let vm = session(runtime);
        println!("sample,iterations,ns_per_invocation");
        for sample in 0..8 {
            let start = std::time::Instant::now();
            for _ in 0..100_000 {
                with_vm(runtime, vm, |session| {
                    session
                        .execution
                        .execute_main_with_fuel(&mut Vec::new(), std::hint::black_box(3))
                        .map_err(|_| TINY_STATUS_VM_TRAP)
                })
                .unwrap();
            }
            if sample != 0 {
                println!("{sample},100000,{:.3}", start.elapsed().as_secs_f64() * 1e9 / 100_000.0);
            }
        }
        assert_eq!(release_object(runtime, vm), TINY_STATUS_OK);
        assert_eq!(destroy_runtime(runtime), TINY_STATUS_OK);
    }
}
