"""Optional real LLVM verification, object emission and VM differential execution."""

import ctypes
import os
from pathlib import Path
import platform
import subprocess
from tempfile import TemporaryDirectory
import unittest

import llvmlite.binding as llvm


ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT.parents[1]
ENGINE = WORKSPACE / "target" / "release" / "tinylang-v2.exe"
VM = WORKSPACE / "target" / "debug" / "tinylang.exe"
MIN_I64 = -(1 << 63)
MAX_I64 = (1 << 63) - 1


class MemoryInfo(ctypes.Structure):
    _fields_ = [
        ("base", ctypes.c_void_p),
        ("allocation_base", ctypes.c_void_p),
        ("allocation_protect", ctypes.c_uint32),
        ("partition", ctypes.c_uint16),
        ("region_bytes", ctypes.c_size_t),
        ("state", ctypes.c_uint32),
        ("protect", ctypes.c_uint32),
        ("kind", ctypes.c_uint32),
    ]


@unittest.skipUnless(os.name == "nt" and platform.machine().lower() in {"amd64", "x86_64"},
                     "LLVM frame execution currently certified on Windows x86-64 only")
class LLVMExecutionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        llvm.initialize_native_target()
        llvm.initialize_native_asmprinter()
        if not ENGINE.is_file() or not VM.is_file():
            raise FileNotFoundError("Build the release v2 CLI and debug core tinylang CLI first")
        machine = llvm.Target.from_default_triple().create_target_machine(opt=2)
        print(f"LLVM validation: library={llvm.llvm_version_info} target={machine.triple}")

    def execute(
        self, source: str, name: str, args: list[int], fuel: int = 10000,
    ) -> tuple[int, int, int, subprocess.CompletedProcess[str]]:
        with TemporaryDirectory(prefix="tinylang-llvm-") as directory:
            filename = Path(directory) / "scalar.to"
            filename.write_text(source, encoding="utf-8")
            ir = subprocess.run(
                [str(ENGINE), "--backend", "llvm", str(filename), name],
                check=True, capture_output=True, text=True, timeout=15,
            ).stdout
            module = llvm.parse_assembly(ir)
            module.verify()
            module.get_function("tinylang_scalar")
            machine = llvm.Target.from_default_triple().create_target_machine(opt=2)
            module.triple = machine.triple
            module.data_layout = str(machine.target_data)
            object_code = machine.emit_object(module)
            # This llvmlite Windows binding explicitly selects an ELF JIT target.
            self.assertEqual(object_code[:6], b"\x7fELF\x02\x01")
            self.assertEqual(object_code[18:20], b"\x3e\x00", "expected real AMD64 object")
            slots = (ctypes.c_int64 * 4096)()
            stack = (ctypes.c_int64 * 4096)()
            for index, value in enumerate(args):
                slots[index] = value
            output = ctypes.c_int64(-999)
            remaining = ctypes.c_uint64(fuel)
            error_pc = ctypes.c_uint64(MAX_I64)
            loader = llvm.parse_assembly("")
            loader.triple = machine.triple
            loader.data_layout = str(machine.target_data)
            with llvm.create_mcjit_compiler(loader, machine, use_lmm=False) as compiler:
                compiler.add_object_file(llvm.ObjectFileRef.from_data(object_code))
                compiler.finalize_object()
                address = compiler.get_function_address("tinylang_scalar")
                self.assertNotEqual(address, 0)
                query = ctypes.WinDLL("kernel32", use_last_error=True).VirtualQuery
                query.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryInfo), ctypes.c_size_t]
                query.restype = ctypes.c_size_t
                memory = MemoryInfo()
                if query(address, ctypes.byref(memory), ctypes.sizeof(memory)) == 0:
                    raise ctypes.WinError(ctypes.get_last_error())
                self.assertEqual(memory.protect & 0xFF, 0x20, "LLVM code must be RX, not WX")
                function = ctypes.CFUNCTYPE(
                    ctypes.c_int32,
                    ctypes.POINTER(ctypes.c_int64),
                    ctypes.POINTER(ctypes.c_int64),
                    ctypes.POINTER(ctypes.c_int64),
                    ctypes.POINTER(ctypes.c_uint64),
                    ctypes.POINTER(ctypes.c_uint64),
                )(address)
                status = function(
                    slots, stack, ctypes.byref(output),
                    ctypes.byref(remaining), ctypes.byref(error_pc),
                )
            expressions = [
                "(-9223372036854775807 - 1)" if value == MIN_I64 else str(value)
                for value in args
            ]
            filename.write_text(
                f"{source}\nprint {name}({','.join(expressions)})\n", encoding="utf-8",
            )
            reference = subprocess.run(
                [str(VM), "--vm", "--no-cache", "-O0", str(filename)],
                capture_output=True, text=True, timeout=15,
            )
            return status, output.value, error_pc.value, reference

    def assert_value(self, source: str, args: list[int], expected: int) -> None:
        status, value, _, reference = self.execute(source, "run", args)
        self.assertEqual(reference.returncode, 0, reference.stderr)
        self.assertEqual(reference.stdout.strip(), str(expected))
        self.assertEqual(status, 0)
        self.assertEqual(value, expected)

    def test_compiled_add_and_floor_division_match_the_actual_vm(self) -> None:
        self.assert_value("fn run(a,b) { return a+b }", [19, 23], 42)
        for args, expected in [
            ([-7, 3], -3), ([7, -3], -3), ([-7, -3], 2),
            ([MIN_I64, 3], -3074457345618258603),
        ]:
            with self.subTest(args=args):
                self.assert_value("fn run(a,b) { return a/b }", args, expected)

    def test_generated_branches_and_loop_match_the_actual_vm(self) -> None:
        self.assert_value(
            "fn run(n) { let t=0 let i=0 while i<n { t=t+i i=i+1 } return t }",
            [100], 4950,
        )
        for seed in range(16):
            factor = seed % 5 + 1
            a, b = seed - 8, seed % 7 - 3
            source = (
                f"fn run(a,b) {{ let x=a*{factor} "
                "if x<b { return b-x } else { return x-b } }"
            )
            with self.subTest(seed=seed):
                self.assert_value(source, [a, b], abs(a * factor - b))

    def test_overflow_and_divide_by_zero_trap_at_the_original_instruction(self) -> None:
        for source, args, expected_status in [
            ("fn run(a,b) { return a+b }", [MAX_I64, 1], 1),
            ("fn run(a,b) { return a/b }", [MIN_I64, -1], 1),
            ("fn run(a,b) { return a/b }", [1, 0], 2),
        ]:
            with self.subTest(source=source, args=args):
                status, output, pc, reference = self.execute(source, "run", args)
                self.assertNotEqual(reference.returncode, 0)
                self.assertNotEqual(reference.stderr, "")
                self.assertEqual(status, expected_status)
                self.assertEqual(pc, 2)
                self.assertEqual(output, -999)

    def test_fuel_is_consumed_before_return_and_exact_budget_succeeds(self) -> None:
        source = "fn run(a,b) { return a+b }"
        status, output, pc, _ = self.execute(source, "run", [19, 23], fuel=3)
        self.assertEqual(status, 3)
        self.assertEqual(pc, 3)
        self.assertEqual(output, -999)
        status, output, _, _ = self.execute(source, "run", [19, 23], fuel=4)
        self.assertEqual(status, 0)
        self.assertEqual(output, 42)


if __name__ == "__main__":
    unittest.main()
