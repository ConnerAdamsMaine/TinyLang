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


import re


class ProgramContext(ctypes.Structure):
    """Mirror of the emitted program ABI invocation context (repr(C), 40 bytes)."""

    _fields_ = [
        ("depth", ctypes.c_uint64),
        ("trap", ctypes.c_uint32),
        ("pad", ctypes.c_uint32),
        ("fuel", ctypes.c_uint64),
        ("bank", ctypes.c_void_p),
        ("pc", ctypes.c_uint64),
    ]


@unittest.skipUnless(os.name == "nt" and platform.machine().lower() in {"amd64", "x86_64"},
                     "LLVM program execution currently certified on Windows x86-64 only")
class LLVMProgramExecutionTests(unittest.TestCase):
    """Executes the actual program-profile LLVM IR through llvmlite and checks
    parity with the authoritative VM: results, trap tags and the shared fuel
    budget across the whole call chain. The emitter is validated end to end;
    nothing here fabricates success."""

    @classmethod
    def setUpClass(cls) -> None:
        llvm.initialize_native_target()
        llvm.initialize_native_asmprinter()
        if not ENGINE.is_file() or not VM.is_file():
            raise FileNotFoundError("Build the release v2 CLI and debug core tinylang CLI first")
        machine = llvm.Target.from_default_triple().create_target_machine(opt=2)
        print(f"LLVM program validation: library={llvm.llvm_version_info} target={machine.triple}")

    def vm_reference(self, source: str, entry: str, args: list[int]) -> subprocess.CompletedProcess[str]:
        with TemporaryDirectory(prefix="tinylang-llvm-program-ref-") as directory:
            filename = Path(directory) / "program.to"
            filename.write_text(
                f"{source}\nprint {entry}({','.join(str(a) for a in args)})\n", encoding="utf-8",
            )
            return subprocess.run(
                [str(VM), "--vm", "--no-cache", "-O0", str(filename)],
                capture_output=True, text=True, timeout=15,
            )

    def execute_program(
        self, source: str, entry: str, args: list[int], fuel: int = 100000,
    ) -> tuple[int, int, int, int, subprocess.CompletedProcess[str]]:
        """Returns (result, trap_tag, failing_pc, remaining_fuel, vm_reference)."""
        with TemporaryDirectory(prefix="tinylang-llvm-program-") as directory:
            filename = Path(directory) / "program.to"
            filename.write_text(source, encoding="utf-8")
            ir = subprocess.run(
                [str(ENGINE), "--backend", "llvm", str(filename), entry],
                check=True, capture_output=True, text=True, timeout=15,
            ).stdout
            entry_match = re.search(r"^; entry: (tinylang_program_\d+)$", ir, re.MULTILINE)
            stride_match = re.search(r"^; stride: (\d+) i64 values", ir, re.MULTILINE)
            depth_match = re.search(r"^; depth_limit: (\d+)$", ir, re.MULTILINE)
            self.assertIsNotNone(entry_match, "emitted program header must document the entry symbol")
            self.assertIsNotNone(stride_match)
            self.assertIsNotNone(depth_match)
            entry_symbol = entry_match.group(1)
            stride = int(stride_match.group(1))
            depth_limit = int(depth_match.group(1))
            module = llvm.parse_assembly(ir)
            module.verify()
            machine = llvm.Target.from_default_triple().create_target_machine(opt=2)
            module.triple = machine.triple
            module.data_layout = str(machine.target_data)
            object_code = machine.emit_object(module)
            segments = depth_limit + 1
            bank = (ctypes.c_int64 * (segments * max(stride, 1)))()
            for index, value in enumerate(args):
                bank[index] = value
            context = ProgramContext()
            context.depth = 0
            context.trap = 0
            context.fuel = fuel
            context.bank = ctypes.cast(bank, ctypes.c_void_p)
            context.pc = 0
            loader = llvm.parse_assembly("")
            loader.triple = machine.triple
            loader.data_layout = str(machine.target_data)
            with llvm.create_mcjit_compiler(loader, machine, use_lmm=False) as compiler:
                compiler.add_object_file(llvm.ObjectFileRef.from_data(object_code))
                compiler.finalize_object()
                address = compiler.get_function_address(entry_symbol)
                self.assertNotEqual(address, 0)
                query = ctypes.WinDLL("kernel32", use_last_error=True).VirtualQuery
                query.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryInfo), ctypes.c_size_t]
                query.restype = ctypes.c_size_t
                memory = MemoryInfo()
                if query(address, ctypes.byref(memory), ctypes.sizeof(memory)) == 0:
                    raise ctypes.WinError(ctypes.get_last_error())
                self.assertEqual(memory.protect & 0xFF, 0x20, "LLVM program code must be RX, not WX")
                function = ctypes.CFUNCTYPE(ctypes.c_int64, ctypes.POINTER(ProgramContext))(address)
                result = function(ctypes.byref(context))
            return result, context.trap, context.pc, context.fuel, self.vm_reference(source, entry, args)

    def test_recursion_and_zero_argument_calls_match_the_actual_vm(self) -> None:
        for source, entry, args, expected in [
            ("fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }", "fact", [5], 120),
            ("fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }", "fact", [12], 479_001_600),
            ("fn answer() { return 7 * 6 }\nfn probe() { return answer() + answer() }", "probe", [], 84),
            ("fn pack(a, b, c) { return (a - b) * c }\nfn drive() { return pack(10, 3, 2) }", "drive", [], 14),
        ]:
            with self.subTest(entry=entry, args=args):
                result, trap, _, _, reference = self.execute_program(source, entry, args)
                self.assertEqual(reference.returncode, 0, reference.stderr)
                self.assertEqual(reference.stdout.strip(), str(expected))
                self.assertEqual(trap, 0)
                self.assertEqual(result, expected)

    def test_trap_tags_match_the_actual_vm_failures(self) -> None:
        for source, entry, args, expected_tag in [
            ("fn bump(n) { return n * 4294967296 * 4294967296 }\nfn drive(n) { return bump(n) }", "drive", [4294967296], 1),
            ("fn division(cond) { if cond == 1 { return 7 / 0 } return 0 }\nfn probe(cond) { return division(cond) }", "probe", [1], 2),
            ("fn spin(n) { return spin(n + 1) }", "spin", [0], 4),
        ]:
            with self.subTest(entry=entry, args=args):
                result, trap, _, _, reference = self.execute_program(source, entry, args)
                self.assertNotEqual(reference.returncode, 0)
                self.assertNotEqual(reference.stderr, "")
                self.assertEqual(trap, expected_tag)
                self.assertEqual(result, 0)

    def test_shared_fuel_budget_bounds_the_whole_call_chain(self) -> None:
        source = "fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }"
        minimum = None
        for fuel in range(0, 4096):
            _, trap, _, remaining, _ = self.execute_program(source, "fact", [5], fuel=fuel)
            if trap == 0:
                minimum = fuel
                break
            self.assertEqual(trap, 3, f"fuel {fuel}: expected fuel exhaustion, got tag {trap}")
            self.assertEqual(remaining, 0)
        self.assertIsNotNone(minimum, "fact(5) never completed within the bisect ceiling")
        assert minimum is not None
        # The debug VM CLI exposes no fuel flag, so the shared-budget minimum is
        # checked for internal consistency here; exact per-fuel equality with the
        # interpreter is covered by the Rust program-parity suites.
        _, trap, _, remaining, _ = self.execute_program(source, "fact", [5], fuel=minimum - 1)
        self.assertEqual(trap, 3)
        self.assertEqual(remaining, 0)
        result, trap, _, _, _ = self.execute_program(source, "fact", [5], fuel=minimum)
        self.assertEqual(trap, 0)
        self.assertEqual(result, 120)


if __name__ == "__main__":
    unittest.main()
