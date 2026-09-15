from __future__ import annotations

import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
from verify_skill_collection import SKILLS, verify_skill_collection


class SkillCollectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        self.skills = self.work / "skills"
        shutil.copytree(REPOSITORY / "kat/skills", self.skills)
        # 这些占位文件只验证部署路径，绝不作为可执行载荷使用。
        for relative in (
            "kat/scripts/targets/linux-x86_64/kat",
            "kat/scripts/targets/linux-x86_64/python/bin/python3",
            "kat/scripts/targets/windows-x86_64/kat.exe",
            "kat/scripts/targets/windows-x86_64/python/python.exe",
        ):
            path = self.skills / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("path fixture; not executable\n", encoding="utf-8")
        (self.skills / "kat/assets/packs").mkdir(parents=True)

    def _archive(self, names: tuple[str, ...]) -> Path:
        archive = self.work / "kat-skill-test.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            for name in names:
                output.add(self.skills / name, arcname=name)
        extracted = self.work / "extracted"
        with tarfile.open(archive) as source:
            source.extractall(extracted, filter="data")
        relocated = self.work / "relocated collection"
        extracted.rename(relocated)
        return relocated

    def test_archive_relocation_preserves_source_links_and_shared_paths(self) -> None:
        relocated = self._archive(SKILLS)
        before = {
            path.relative_to(relocated): path.read_bytes()
            for path in relocated.rglob("*")
            if path.is_file()
        }
        result = subprocess.run(
            [
                sys.executable,
                "-O",
                "-I",
                "-B",
                str(REPOSITORY / "build/verify_skill_collection.py"),
                str(relocated),
            ],
            cwd=self.work,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("runtime execution is checked by platform smoke", result.stdout)
        self.assertEqual(
            before,
            {
                path.relative_to(relocated): path.read_bytes()
                for path in relocated.rglob("*")
                if path.is_file()
            },
        )

    def test_old_single_skill_archive_is_rejected(self) -> None:
        relocated = self._archive(("kat",))
        with self.assertRaisesRegex(ValueError, "exactly these top-level directories"):
            verify_skill_collection(relocated)

    def test_extra_top_level_file_is_rejected(self) -> None:
        (self.skills / "unexpected.txt").write_text("extra", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "exactly these top-level directories"):
            verify_skill_collection(self.skills)

    def test_each_entrypoint_is_required(self) -> None:
        for name in SKILLS:
            with self.subTest(skill=name):
                path = self.skills / name / "SKILL.md"
                original = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(ValueError, "missing"):
                    verify_skill_collection(self.skills)
                path.write_bytes(original)

    def test_each_platform_requires_adjacent_cli_and_host(self) -> None:
        for relative in (
            "linux-x86_64/kat",
            "linux-x86_64/python/bin/python3",
            "windows-x86_64/kat.exe",
            "windows-x86_64/python/python.exe",
        ):
            with self.subTest(path=relative):
                path = self.skills / "kat/scripts/targets" / relative
                original = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(
                    ValueError, "Required collection file is missing"
                ):
                    verify_skill_collection(self.skills)
                path.write_bytes(original)

    def test_empty_bundled_pack_directory_is_valid(self) -> None:
        self.assertEqual(list((self.skills / "kat/assets/packs").iterdir()), [])
        verify_skill_collection(self.skills)

    def test_bundled_pack_directory_is_required(self) -> None:
        (self.skills / "kat/assets/packs").rmdir()
        with self.assertRaisesRegex(ValueError, "missing its Bundled PACK directory"):
            verify_skill_collection(self.skills)

    def test_missing_and_external_local_references_are_rejected(self) -> None:
        entrypoint = self.skills / "kat-review/SKILL.md"
        original = entrypoint.read_text(encoding="utf-8")
        (self.work / "outside.md").write_text("exists", encoding="utf-8")
        for href, error in (
            ("missing.md", "reference is missing"),
            ("../../outside.md", "escapes collection"),
        ):
            with self.subTest(href=href):
                entrypoint.write_text(f"{original}\n[broken]({href})\n", encoding="utf-8")
                with self.assertRaisesRegex(ValueError, error):
                    verify_skill_collection(self.skills)

    def test_task_skill_must_use_the_adjacent_shared_contract(self) -> None:
        entrypoint = self.skills / "kat-review/SKILL.md"
        content = entrypoint.read_text(encoding="utf-8")
        entrypoint.write_text(
            content.replace(
                "](../kat/references/command-reference.md)",
                "](https://example.org/commands.md)",
            ),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "adjacent shared command contract"):
            verify_skill_collection(self.skills)

    def test_task_skill_cannot_duplicate_shared_payload(self) -> None:
        (self.skills / "kat-author/scripts/targets").mkdir()
        with self.assertRaisesRegex(ValueError, "must use the shared kat payload"):
            verify_skill_collection(self.skills)


if __name__ == "__main__":
    unittest.main()
