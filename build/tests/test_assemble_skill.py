from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
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
        (self.skill / "agents/openai.yaml").write_text(
            "interface:\n  display_name: KAT\n", encoding="utf-8"
        )
        (self.skill / "references").mkdir()
        for name in (
            "analysis-flow.md",
            "command-reference.md",
            "pack-authoring-flow.md",
            "result-contract.md",
            "unlisted-reference.md",
        ):
            (self.skill / "references" / name).write_text(
                "# Reference\n", encoding="utf-8"
            )

        self.packs = self.root / "packs"
        (self.packs / "kat-example").mkdir(parents=True)
        (self.packs / "kat-example/pack.toml").write_text(
            'name = "kat-example"\n', encoding="utf-8"
        )

        self.linux_payload = self.root / "linux-payload"
        (self.linux_payload / "python/bin").mkdir(parents=True)
        (self.linux_payload / "kat").write_bytes(b"opaque CLI")
        (self.linux_payload / "python/bin/python3").write_bytes(b"opaque host")
        (self.linux_payload / "python/native.so").write_bytes(b"opaque native")

        self.windows_payload = self.root / "windows-payload"
        (self.windows_payload / "python").mkdir(parents=True)
        (self.windows_payload / "kat.exe").write_bytes(b"opaque Windows CLI")
        (self.windows_payload / "python/python.exe").write_bytes(b"opaque host")
        (self.windows_payload / "python/native.pyd").write_bytes(b"opaque native")
        self.output = self.root / "dist/kat"

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def assemble(self, **overrides: Path) -> Path:
        arguments = {
            "skill_source": self.skill,
            "packs": self.packs,
            "linux_payload": self.linux_payload,
            "windows_payload": self.windows_payload,
            "output": self.output,
        }
        arguments.update(overrides)
        return assembly.assemble_skill(**arguments)

    @staticmethod
    def relative_files(root: Path, prefix: str = "") -> set[str]:
        return {
            f"{prefix}{path.relative_to(root).as_posix()}"
            for path in root.rglob("*")
            if path.is_file()
        }

    def test_assembly_maps_every_source_once_and_never_leaves_partial_output(
        self,
    ) -> None:
        self.assertEqual(self.assemble(), self.output.resolve())
        self.assertEqual(
            {
                path.relative_to(self.output).as_posix()
                for path in self.output.rglob("*")
                if path.is_file()
            },
            self.relative_files(self.skill)
            | self.relative_files(self.packs, "assets/packs/")
            | self.relative_files(
                self.linux_payload, "scripts/targets/linux-x86_64/"
            )
            | self.relative_files(
                self.windows_payload, "scripts/targets/windows-x86_64/"
            ),
        )
        shutil.rmtree(self.output)

        with self.subTest(case="existing output"):
            self.output.mkdir(parents=True)
            marker = self.output / "existing.txt"
            marker.write_text("keep", encoding="utf-8")
            with self.assertRaisesRegex(assembly.AssemblyError, "output already exists"):
                self.assemble()
            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
            self.assertEqual(
                list(self.output.parent.glob(f".{self.output.name}-assembly-*")), []
            )
            shutil.rmtree(self.output)

        missing_cases = {
            "skill_source": self.root / "missing-skill",
            "packs": self.root / "missing-packs",
            "linux_payload": self.root / "missing-linux",
            "windows_payload": self.root / "missing-windows",
        }
        for argument, missing in missing_cases.items():
            with self.subTest(case=f"missing {argument}"):
                with self.assertRaisesRegex(assembly.AssemblyError, "missing"):
                    self.assemble(**{argument: missing})
                self.assertFalse(self.output.exists())

        with self.subTest(case="overlapping output"):
            with self.assertRaisesRegex(assembly.AssemblyError, "overlaps output"):
                self.assemble(output=self.skill / "dist/kat")
            self.assertFalse((self.skill / "dist/kat").exists())

        real_copytree = assembly.shutil.copytree

        def fail_for_packs(
            source: Path, destination: Path, *args: object, **kwargs: object
        ) -> Path:
            if Path(source) == self.packs.resolve():
                raise OSError("injected copy failure")
            return real_copytree(source, destination, *args, **kwargs)

        with self.subTest(case="copy failure"), mock.patch.object(
            assembly.shutil, "copytree", side_effect=fail_for_packs
        ), self.assertRaisesRegex(OSError, "injected copy failure"):
            self.assemble()
        self.assertFalse(self.output.exists())
        self.assertEqual(list((self.root / "dist").iterdir()), [])

    def test_assembly_treats_input_contents_as_opaque(self) -> None:
        opaque_skill = self.root / "opaque-skill"
        opaque_skill.mkdir()
        (opaque_skill / "arbitrary-skill-content").write_text(
            "skill", encoding="utf-8"
        )

        empty_packs = self.root / "empty-packs"
        empty_packs.mkdir()

        opaque_linux = self.root / "opaque-linux"
        opaque_linux.mkdir()
        (opaque_linux / "arbitrary-linux-content").write_text(
            "linux", encoding="utf-8"
        )

        opaque_windows = self.root / "opaque-windows"
        opaque_windows.mkdir()
        (opaque_windows / "arbitrary-windows-content").write_text(
            "windows", encoding="utf-8"
        )

        output = self.root / "opaque-dist/kat"
        self.assertEqual(
            self.assemble(
                skill_source=opaque_skill,
                packs=empty_packs,
                linux_payload=opaque_linux,
                windows_payload=opaque_windows,
                output=output,
            ),
            output.resolve(),
        )
        self.assertEqual(
            self.relative_files(output),
            {
                "arbitrary-skill-content",
                "scripts/targets/linux-x86_64/arbitrary-linux-content",
                "scripts/targets/windows-x86_64/arbitrary-windows-content",
            },
        )
        self.assertTrue((output / "assets/packs").is_dir())

    def test_assembly_rejects_absolute_symlink_in_an_input(self) -> None:
        external = self.root / "external.txt"
        external.write_text("external", encoding="utf-8")
        link = self.skill / "external-link"
        try:
            link.symlink_to(external)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(
            assembly.AssemblyError, "absolute symbolic link"
        ):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_assembly_rejects_symlink_that_escapes_its_input(self) -> None:
        external = self.root / "external.txt"
        external.write_text("external", encoding="utf-8")
        link = self.skill / "external-link"
        try:
            link.symlink_to(Path("..") / external.name)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(
            assembly.AssemblyError, "escapes its input directory"
        ):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_assembly_rejects_dangling_symlink_in_an_input(self) -> None:
        link = self.skill / "missing-link"
        try:
            link.symlink_to("missing-target")
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(assembly.AssemblyError, "dangling symbolic link"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_assembly_preserves_relative_symlink_within_its_input(self) -> None:
        target = self.skill / "linked-target.txt"
        target.write_text("linked", encoding="utf-8")
        link = self.skill / "linked.txt"
        try:
            link.symlink_to(target.name)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        self.assemble()

        assembled_link = self.output / link.name
        self.assertTrue(assembled_link.is_symlink())
        self.assertEqual(assembled_link.readlink(), Path(target.name))
        self.assertEqual(assembled_link.read_text(encoding="utf-8"), "linked")


if __name__ == "__main__":
    unittest.main()
