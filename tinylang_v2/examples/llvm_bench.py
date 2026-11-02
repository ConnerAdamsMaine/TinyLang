"""LLVM object preparation and ctypes invocation with reused host frames."""

from collections.abc import Callable, Iterator
from contextlib import contextmanager
import ctypes
from pathlib import Path
import subprocess
import time

import llvmlite.binding as llvm


@contextmanager
def prepared(ir: str) -> Iterator[llvm.ExecutionEngine]:
    machine = llvm.Target.from_default_triple().create_target_machine(opt=2)
    with llvm.parse_assembly(ir) as module:
        module.verify()
        module.get_function("tinylang_scalar")
        module.triple = machine.triple
        module.data_layout = str(machine.target_data)
        object_code = machine.emit_object(module)
    loader = llvm.parse_assembly("")
    loader.triple = machine.triple
    loader.data_layout = str(machine.target_data)
    with llvm.create_mcjit_compiler(loader, machine, use_lmm=False) as engine:
        engine.add_object_file(llvm.ObjectFileRef.from_data(object_code))
        engine.finalize_object()
        yield engine


def measure(name: str, batch: int, operation: Callable[[], None]) -> None:
    for _ in range(8):
        operation()
    samples: list[int] = []
    for _ in range(101):
        start = time.perf_counter_ns()
        for _ in range(batch):
            operation()
        samples.append((time.perf_counter_ns() - start) // batch)
    samples.sort()
    print(f"{name},{samples[50]},{samples[95]},{samples[99]},101,{batch}")


def run(ir: str) -> None:
    def prepare_once() -> None:
        with prepared(ir) as engine:
            if engine.get_function_address("tinylang_scalar") == 0:
                raise RuntimeError("LLVM missing generated function")

    print("metric,median_ns,p95_ns,p99_ns,samples,batch")
    measure("llvm20_parse_object_prepare_drop", 1, prepare_once)
    with prepared(ir) as engine:
        function = ctypes.CFUNCTYPE(
            ctypes.c_int32,
            ctypes.POINTER(ctypes.c_int64),
            ctypes.POINTER(ctypes.c_int64),
            ctypes.POINTER(ctypes.c_int64),
            ctypes.POINTER(ctypes.c_uint64),
            ctypes.POINTER(ctypes.c_uint64),
        )(engine.get_function_address("tinylang_scalar"))
        slots = (ctypes.c_int64 * 4096)()
        stack = (ctypes.c_int64 * 4096)()
        output = ctypes.c_int64()
        fuel = ctypes.c_uint64()
        pc = ctypes.c_uint64()
        slots[0] = 100

        def invoke() -> None:
            fuel.value = 10000
            status = function(
                slots, stack, ctypes.byref(output), ctypes.byref(fuel), ctypes.byref(pc),
            )
            if status != 0 or output.value != 4950:
                raise RuntimeError(
                    f"Incorrect LLVM result: status={status}, value={output.value}"
                )

        measure("llvm20_ctypes_reused_frame_sum100", 256, invoke)


def main() -> None:
    llvm.initialize_native_target()
    llvm.initialize_native_asmprinter()
    root = Path(__file__).resolve().parents[1]
    ir = subprocess.run(
        [
            str(root / "target" / "release" / "tinylang-v2.exe"), "--backend", "llvm",
            str(root / "examples" / "scalar.tinylang"), "sum",
        ],
        check=True, capture_output=True, text=True, timeout=15,
    ).stdout
    run(ir)


if __name__ == "__main__":
    main()
