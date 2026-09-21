from __future__ import annotations

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
        self.skills = self.root / "skills"
        for name in ("kat", "kat-analyze", "kat-author", "kat-review", "kat-dev-sdk"):
            skill = self.skills / name
            skill.mkdir(parents=True)
            (skill / "SKILL.md").write_text(
                f"---\nname: {name}\ndescription: Test skill\n---\n# {name}\n",
                encoding="utf-8",
            )
        shared = self.skills / "kat"
        (shared / "agents").mkdir()
        (shared / "agents/openai.yaml").write_text(
            "interface:\n  display_name: KAT\n", encoding="utf-8"
        )
        (shared / "references").mkdir()
        (shared / "references/command-reference.md").write_text(
            "# Shared command reference\n", encoding="utf-8"
        )
        author = self.skills / "kat-author"
        reference_pack = author / "references/examples/dataprovider-pack"
        reference_pack.mkdir(parents=True)
        (reference_pack / "pack.toml").write_text(
            'name = "dataprovider-pack"\n', encoding="utf-8"
        )
        (author / "scripts").mkdir()
        (author / "scripts/init_pack.py").write_text(
            "# Opaque scaffold script\n", encoding="utf-8"
        )

        self.packs = self.root / "packs"
        (self.packs / "kat-example").mkdir(parents=True)
        (self.packs / "kat-example/pack.toml").write_text(
            'name = "kat-example"\n', encoding="utf-8"
        )

        # 假载荷只验证装配路径与字节保留，不作为 CLI/Host 运行证据。
        self.linux_payload = self.root / "linux-payload"
        (self.linux_payload / "python/bin").mkdir(parents=True)
        (self.linux_payload / "kat").write_bytes(b"opaque CLI")
        (self.linux_payload / "python/bin/python3").write_bytes(b"opaque Linux host")
        (self.linux_payload / "python/native.so").write_bytes(b"opaque native")

        self.windows_payload = self.root / "windows-payload"
        (self.windows_payload / "python").mkdir(parents=True)
        (self.windows_payload / "kat.exe").write_bytes(b"opaque Windows CLI")
        (self.windows_payload / "python/python.exe").write_bytes(b"opaque Windows host")
        (self.windows_payload / "python/native.pyd").write_bytes(b"opaque native")
        self.output = self.root / "dist/skills"

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def assemble(self, **overrides: Path) -> Path:
        arguments = {
            "skills_source": self.skills,
            "packs": self.packs,
            "linux_payload": self.linux_payload,
            "windows_payload": self.windows_payload,
            "output": self.output,
        }
        arguments.update(overrides)
        return assembly.assemble_skill(**arguments)

    @staticmethod
    def relative_files(root: Path, prefix: str = "") -> dict[str, bytes]:
        return {
            f"{prefix}{path.relative_to(root).as_posix()}": path.read_bytes()
            for path in root.rglob("*")
            if path.is_file()
        }

    def assert_no_partial_output(self) -> None:
        self.assertFalse(self.output.exists())
        self.assertEqual(
            list(self.output.parent.glob(f".{self.output.name}-assembly-*")), []
        )

    def test_assembly_maps_five_skills_and_each_shared_input_once(self) -> None:
        self.assertEqual(self.assemble(), self.output.resolve())
        self.assertEqual(
            {path.name for path in self.output.iterdir()},
            {"kat", "kat-analyze", "kat-author", "kat-review", "kat-dev-sdk"},
        )
        self.assertEqual(
            self.relative_files(self.output),
            self.relative_files(self.skills)
            | self.relative_files(self.packs, "kat/assets/packs/")
            | self.relative_files(
                self.linux_payload, "kat/scripts/targets/linux-x86_64/"
            )
            | self.relative_files(
                self.windows_payload, "kat/scripts/targets/windows-x86_64/"
            ),
        )
        self.assertFalse(
            (self.output / "kat/assets/packs/dataprovider-pack").exists()
        )

    def test_whole_collection_move_preserves_sibling_resource_paths(self) -> None:
        self.assemble()
        moved = self.root / "installed elsewhere/skills"
        moved.parent.mkdir()
        self.output.rename(moved)

        for task in ("kat-analyze", "kat-author", "kat-review", "kat-dev-sdk"):
            with self.subTest(task=task):
                shared = moved / task / "../kat"
                self.assertTrue((shared / "SKILL.md").is_file())
                self.assertTrue((shared / "references/command-reference.md").is_file())
                for target, cli_name, python_path in (
                    ("linux-x86_64", "kat", "python/bin/python3"),
                    ("windows-x86_64", "kat.exe", "python/python.exe"),
                ):
                    cli = shared / "scripts/targets" / target / cli_name
                    self.assertTrue(cli.is_file())
                    self.assertTrue((cli.parent / python_path).is_file())
                    self.assertTrue(
                        (cli.parents[3] / "assets/packs/kat-example/pack.toml").is_file()
                    )
        self.assertTrue((moved / "kat-author/scripts/init_pack.py").is_file())
        self.assertTrue(
            (moved / "kat-author/references/examples/dataprovider-pack/pack.toml").is_file()
        )

    def test_assembly_includes_user_manual_from_real_skills_source(self) -> None:
        skills_source = Path(__file__).resolve().parents[2] / "kat/skills"
        self.assemble(skills_source=skills_source)

        manual = self.output / "kat/user-manual.html"
        self.assertTrue(manual.is_file())
        self.assertEqual(
            manual.read_bytes(), (skills_source / "kat/user-manual.html").read_bytes()
        )

    def test_assembly_rejects_missing_or_invalid_skill_entries(self) -> None:
        for name in ("kat", "kat-analyze", "kat-author", "kat-review", "kat-dev-sdk"):
            skill = self.skills / name
            saved = self.root / f"saved-{name}"
            with self.subTest(skill=name, case="missing directory"):
                skill.rename(saved)
                with self.assertRaisesRegex(assembly.AssemblyError, f"Skill {name}.*missing"):
                    self.assemble()
                self.assert_no_partial_output()
                saved.rename(skill)
            entry = skill / "SKILL.md"
            original = entry.read_bytes()
            for content, message in ((b" \n", "empty"), (b"\xff", "UTF-8")):
                with self.subTest(skill=name, case=message):
                    entry.write_bytes(content)
                    with self.assertRaisesRegex(assembly.AssemblyError, message):
                        self.assemble()
                    self.assert_no_partial_output()
            entry.unlink()
            with self.subTest(skill=name, case="missing SKILL.md"):
                with self.assertRaisesRegex(assembly.AssemblyError, "SKILL.md.*missing"):
                    self.assemble()
                self.assert_no_partial_output()
            entry.mkdir()
            with self.subTest(skill=name, case="directory named SKILL.md"):
                with self.assertRaisesRegex(assembly.AssemblyError, "SKILL.md.*missing"):
                    self.assemble()
                self.assert_no_partial_output()
            entry.rmdir()
            entry.write_bytes(original)

    def test_assembly_rejects_extra_collection_root_entries(self) -> None:
        (self.skills / "unapproved-skill").mkdir()
        with self.assertRaisesRegex(assembly.AssemblyError, "unexpected entries"):
            self.assemble()
        self.assert_no_partial_output()

    def test_assembly_preserves_existing_output(self) -> None:
        self.output.mkdir(parents=True)
        marker = self.output / "existing.txt"
        marker.write_text("keep", encoding="utf-8")
        with self.assertRaisesRegex(assembly.AssemblyError, "output already exists"):
            self.assemble()
        self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
        self.assertEqual(list(self.output.parent.iterdir()), [self.output])

    def test_assembly_rejects_missing_inputs_and_overlapping_paths(self) -> None:
        for argument in ("skills_source", "packs", "linux_payload", "windows_payload"):
            with self.subTest(case=f"missing {argument}"):
                with self.assertRaisesRegex(assembly.AssemblyError, "missing"):
                    self.assemble(**{argument: self.root / f"missing-{argument}"})
                self.assert_no_partial_output()
        for source in (self.skills, self.packs, self.linux_payload, self.windows_payload):
            with self.subTest(case=f"output inside {source.name}"):
                with self.assertRaisesRegex(assembly.AssemblyError, "overlaps output"):
                    self.assemble(output=source / "dist/skills")
                self.assertFalse((source / "dist").exists())
        with self.subTest(case="inputs overlap"):
            with self.assertRaisesRegex(assembly.AssemblyError, "assembly inputs overlap"):
                self.assemble(packs=self.skills / "kat")
            self.assert_no_partial_output()

    def test_assembly_cleans_staging_after_copy_or_publish_failure(self) -> None:
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
        self.assert_no_partial_output()

        with self.subTest(case="publish failure"), mock.patch.object(
            Path, "rename", side_effect=OSError("injected publish failure")
        ), self.assertRaisesRegex(OSError, "injected publish failure"):
            self.assemble()
        self.assert_no_partial_output()

    def test_assembly_treats_pack_and_payload_contents_as_opaque(self) -> None:
        empty_packs = self.root / "empty-packs"
        empty_packs.mkdir()
        opaque_linux = self.root / "opaque-linux"
        opaque_linux.mkdir()
        (opaque_linux / "arbitrary-linux-content").write_bytes(b"linux")
        opaque_windows = self.root / "opaque-windows"
        opaque_windows.mkdir()
        (opaque_windows / "arbitrary-windows-content").write_bytes(b"windows")

        self.assemble(
            packs=empty_packs,
            linux_payload=opaque_linux,
            windows_payload=opaque_windows,
        )
        self.assertEqual(
            self.relative_files(self.output),
            self.relative_files(self.skills)
            | {
                "kat/scripts/targets/linux-x86_64/arbitrary-linux-content": b"linux",
                "kat/scripts/targets/windows-x86_64/arbitrary-windows-content": b"windows",
            },
        )
        self.assertTrue((self.output / "kat/assets/packs").is_dir())

    def test_assembly_rejects_absolute_symlink_in_an_input(self) -> None:
        external = self.root / "external.txt"
        external.write_text("external", encoding="utf-8")
        link = self.skills / "kat/external-link"
        try:
            link.symlink_to(external)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(assembly.AssemblyError, "absolute symbolic link"):
            self.assemble()
        self.assert_no_partial_output()

    def test_assembly_rejects_symlink_that_escapes_its_input(self) -> None:
        external = self.root / "external.txt"
        external.write_text("external", encoding="utf-8")
        link = self.packs / "external-link"
        try:
            link.symlink_to(Path("..") / external.name)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(assembly.AssemblyError, "escapes its input directory"):
            self.assemble()
        self.assert_no_partial_output()

    def test_assembly_rejects_dangling_symlink_in_an_input(self) -> None:
        link = self.linux_payload / "missing-link"
        try:
            link.symlink_to("missing-target")
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(assembly.AssemblyError, "dangling symbolic link"):
            self.assemble()
        self.assert_no_partial_output()

    def test_assembly_preserves_relative_symlink_within_its_input(self) -> None:
        target = self.skills / "kat/references/command-reference.md"
        link = self.skills / "kat-review/shared-reference.md"
        try:
            link.symlink_to(Path("..") / "kat" / "references" / "command-reference.md")
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        self.assemble()

        assembled_link = self.output / "kat-review/shared-reference.md"
        self.assertTrue(assembled_link.is_symlink())
        self.assertEqual(assembled_link.readlink(), link.readlink())
        self.assertEqual(assembled_link.read_bytes(), target.read_bytes())

    def test_assembly_rejects_dangling_output_symlink(self) -> None:
        self.output.parent.mkdir()
        try:
            self.output.symlink_to("missing-output", target_is_directory=True)
        except OSError as error:
            self.skipTest(f"symbolic links are unavailable: {error}")

        with self.assertRaisesRegex(assembly.AssemblyError, "output already exists"):
            self.assemble()
        self.assertTrue(self.output.is_symlink())
        self.assertFalse((self.output.parent / "missing-output").exists())


if __name__ == "__main__":
    unittest.main()
