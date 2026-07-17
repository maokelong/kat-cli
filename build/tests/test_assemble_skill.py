from __future__ import annotations

import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import assemble_skill as assembly


class AssembleSkillTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        self.skill = self.root / "skill"
        (self.skill / "agents").mkdir(parents=True)
        (self.skill / "SKILL.md").write_text("---\nname: kat\n---\n", encoding="utf-8")
        (self.skill / "agents" / "openai.yaml").write_text(
            "interface:\n  display_name: KAT\n",
            encoding="utf-8",
        )
        self.packs = self.root / "packs"
        (self.packs / "kat-example").mkdir(parents=True)
        (self.packs / "kat-example" / "pack.toml").write_text(
            'name = "kat-example"\n', encoding="utf-8"
        )
        self.payload = self.root / "linux-payload"
        (self.payload / "python" / "bin").mkdir(parents=True)
        (self.payload / "kat").write_bytes(b"opaque CLI")
        (self.payload / "python" / "bin" / "python3").write_bytes(b"opaque host")
        (self.payload / "python" / "opaque-native-extension.so").write_bytes(
            b"opaque native"
        )
        self.windows_payload = self.root / "windows-payload"
        (self.windows_payload / "python").mkdir(parents=True)
        (self.windows_payload / "kat.exe").write_bytes(b"opaque Windows CLI")
        (self.windows_payload / "python" / "python.exe").write_bytes(
            b"opaque Windows host"
        )
        (self.windows_payload / "python" / "opaque-native-extension.pyd").write_bytes(
            b"opaque Windows native"
        )
        self.output = self.root / "dist" / "kat"

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def assemble(self) -> Path:
        return assembly.assemble_skill(
            skill_source=self.skill,
            packs=self.packs,
            linux_payload=self.payload,
            windows_payload=self.windows_payload,
            output=self.output,
        )

    def test_maps_each_source_once_without_interpreting_payload(self) -> None:
        self.assertEqual(self.assemble(), self.output.resolve())

        self.assertEqual(
            {
                path.relative_to(self.output).as_posix()
                for path in self.output.rglob("*")
                if path.is_file()
            },
            {
                "SKILL.md",
                "agents/openai.yaml",
                "assets/packs/kat-example/pack.toml",
                "scripts/targets/linux-x86_64/kat",
                "scripts/targets/linux-x86_64/python/bin/python3",
                "scripts/targets/linux-x86_64/python/opaque-native-extension.so",
                "scripts/targets/windows-x86_64/kat.exe",
                "scripts/targets/windows-x86_64/python/python.exe",
                "scripts/targets/windows-x86_64/python/opaque-native-extension.pyd",
            },
        )
        self.assertEqual(len(list(self.output.rglob("kat-example"))), 1)
        self.assertEqual(len(list(self.output.rglob("python3"))), 1)
        self.assertEqual(len(list(self.output.rglob("python.exe"))), 1)
        self.assertEqual(len(list(self.output.rglob("packs"))), 1)

    def test_missing_input_does_not_leave_partial_output(self) -> None:
        cases = {
            "skill_source": self.root / "missing-skill",
            "packs": self.root / "missing-packs",
            "linux_payload": self.root / "missing-payload",
            "windows_payload": self.root / "missing-windows-payload",
        }

        for argument, missing in cases.items():
            with self.subTest(argument=argument):
                values = {
                    "skill_source": self.skill,
                    "packs": self.packs,
                    "linux_payload": self.payload,
                    "windows_payload": self.windows_payload,
                    "output": self.output,
                }
                values[argument] = missing
                with self.assertRaisesRegex(assembly.AssemblyError, "missing"):
                    assembly.assemble_skill(**values)
                self.assertFalse(self.output.exists())

    def test_copy_failure_removes_staging_and_output(self) -> None:
        real_copytree = assembly.shutil.copytree

        def fail_for_packs(source: Path, destination: Path, **kwargs: object) -> Path:
            if Path(source) == self.packs.resolve():
                raise OSError("injected copy failure")
            return real_copytree(source, destination, **kwargs)

        with mock.patch.object(assembly.shutil, "copytree", side_effect=fail_for_packs):
            with self.assertRaisesRegex(OSError, "injected copy failure"):
                self.assemble()

        self.assertFalse(self.output.exists())
        self.assertEqual(list((self.root / "dist").iterdir()), [])

    def test_rejects_output_or_inputs_that_overlap_sources(self) -> None:
        with self.assertRaisesRegex(assembly.AssemblyError, "overlaps output"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=self.payload,
                windows_payload=self.windows_payload,
                output=self.skill / "dist" / "kat",
            )

        nested_payload = self.packs / "kat-example" / "payload"
        nested_payload.mkdir()
        with self.assertRaisesRegex(assembly.AssemblyError, "inputs overlap"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=nested_payload,
                windows_payload=self.windows_payload,
                output=self.output,
            )

        nested_windows_payload = self.payload / "windows"
        nested_windows_payload.mkdir()
        with self.assertRaisesRegex(assembly.AssemblyError, "inputs overlap"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=self.payload,
                windows_payload=nested_windows_payload,
                output=self.output,
            )

    def test_refuses_to_merge_with_existing_output(self) -> None:
        self.output.mkdir(parents=True)
        (self.output / "duplicate-python").mkdir()

        with self.assertRaisesRegex(assembly.AssemblyError, "already exists"):
            self.assemble()

        self.assertTrue((self.output / "duplicate-python").is_dir())

    def test_cli_rejects_repeated_black_box_inputs(self) -> None:
        arguments = [
            "--skill-source",
            str(self.skill),
            "--packs",
            str(self.packs),
            "--packs",
            str(self.packs),
            "--linux-payload",
            str(self.payload),
            "--windows-payload",
            str(self.windows_payload),
            "--output",
            str(self.output),
        ]

        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as raised:
            assembly._parser().parse_args(arguments)

        self.assertEqual(raised.exception.code, 2)

    def test_cli_requires_both_payloads(self) -> None:
        arguments = [
            "--skill-source",
            str(self.skill),
            "--packs",
            str(self.packs),
            "--linux-payload",
            str(self.payload),
            "--output",
            str(self.output),
        ]

        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as raised:
            assembly._parser().parse_args(arguments)

        self.assertEqual(raised.exception.code, 2)


class SkillPlatformContractTests(unittest.TestCase):
    def test_skill_selects_windows_payload_and_rejects_unsupported_hosts(self) -> None:
        skill = (
            Path(__file__).resolve().parents[2] / "kat" / "skill" / "SKILL.md"
        ).read_text(encoding="utf-8")

        self.assertIn("每次操作前选择平台载荷", skill)
        self.assertEqual(
            skill.count("<skill-root>/scripts/targets/windows-x86_64/kat.exe"), 1
        )
        self.assertIn("Windows 10/11 x86_64 客户端", skill)
        self.assertIn("拒绝 Windows Server", skill)
        self.assertIn("ProductType", skill)
        self.assertIn("拒绝其他系统、架构、libc 或版本", skill)
        self.assertIn("模型结论写回 Runtime、Run Manifest 或 Dataset", skill)
        self.assertNotIn("--pack-root", skill)
        self.assertNotIn("<kat> check", skill)
        self.assertNotIn("<skill-root>/scripts/kat", skill)


if __name__ == "__main__":
    unittest.main()
