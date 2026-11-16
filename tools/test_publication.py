import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PublicationTests(unittest.TestCase):
    def cargo(self, *args: str) -> str:
        result = subprocess.run(
            ["cargo", *args],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_workspace_exposes_publishable_crates_with_registry_dependencies(self) -> None:
        metadata = json.loads(
            self.cargo("metadata", "--no-deps", "--offline", "--format-version", "1")
        )
        packages = {
            package["name"]: package
            for package in metadata["packages"]
            if package["publish"] != []
        }
        self.assertEqual(
            set(packages), {"tynra-v2", "tynra_ralloc", "tynra_test_support"}
        )
        core = packages["tynra-v2"]
        libraries = [
            target["name"] for target in core["targets"] if "rlib" in target["kind"]
        ]
        self.assertEqual(libraries, ["tynra"])
        self.assertIn(
            "tynra-sandbox-worker",
            [target["name"] for target in core["targets"] if "bin" in target["kind"]],
        )
        for package in packages.values():
            with self.subTest(package=package["name"]):
                self.assertTrue(package["description"])
                self.assertTrue(package["repository"])
                directory = Path(package["manifest_path"]).parent
                self.assertTrue((directory / package["readme"]).is_file())
                self.assertTrue((directory / package["license_file"]).is_file())
                for dependency in package["dependencies"]:
                    if dependency.get("path"):
                        self.assertIn(dependency["name"], packages)
                        self.assertNotEqual(dependency["req"], "*")

    def test_core_package_contains_ffi_assets_and_worker(self) -> None:
        files = {
            Path(line).as_posix()
            for line in self.cargo(
                "package", "--package", "tynra-v2", "--list", "--allow-dirty", "--offline"
            ).splitlines()
        }
        required = {
            "include/tynra.h",
            "tynra-response-schema.json",
            "src/main.rs",
            "src/bin/tynra-sandbox-worker.rs",
            "tests/abi_api_soundness.rs",
            "README.md",
            "LICENSE",
        }
        self.assertEqual(required - files, set(), "missing files in crate archive")


if __name__ == "__main__":
    unittest.main()
