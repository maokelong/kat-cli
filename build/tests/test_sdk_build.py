from __future__ import annotations

import importlib.util
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location("sdk_build", REPOSITORY / "kat/sdk/sdk_build.py")
sdk_build = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sdk_build)


class SDKKnowledgeTests(unittest.TestCase):
    def package(self, root: Path) -> Path:
        package = root / "kat_sdk"
        shutil.copytree(REPOSITORY / "kat/sdk", package,
                        ignore=shutil.ignore_patterns("build", "*.egg-info", "__pycache__", "tests"))
        return package

    def test_static_api_generation_handles_unimportable_workflows_and_chinese(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            workflows = package / "workflows/domain"
            workflows.mkdir(parents=True)
            (workflows / "probe.py").write_text('''raise RuntimeError("must not import")

def probe(value: int) -> int:
    """保留中文语义。"""
    return value
''', encoding="utf-8")
            sdk_build.generate_knowledge(package)
            generated = (package / "knowledge/workflows/domain/probe.api.md").read_text(encoding="utf-8")
            self.assertIn("probe(value: int) -> int", generated)
            self.assertIn("保留中文语义", generated)
            self.assertFalse((workflows / "__init__.py").exists())

    def test_library_code_remains_without_sdk_library_knowledge(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            library = package / "helpers/demo/greeting.py"
            original = library.read_bytes()
            sdk_build.generate_knowledge(package)
            self.assertEqual(library.read_bytes(), original)
            self.assertFalse((package / "knowledge/helpers").exists())
            self.assertTrue((package / "knowledge/providers/ftrace.api.md").is_file())
            self.assertTrue((package / "knowledge/workflows/demo/greeting.api.md").is_file())

    def test_missing_provider_guide_fails_before_wheel_publication(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            (package / "knowledge/providers/ftrace.md").unlink()
            with self.assertRaisesRegex(ValueError, "Missing or invalid Guide"):
                sdk_build.generate_knowledge(package)

    def test_generated_api_cannot_overwrite_handwritten_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            target = package / "knowledge/providers/ftrace.api.md"
            target.write_text("Handwritten content", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "overwrite"):
                sdk_build.generate_knowledge(package)
            self.assertEqual(target.read_text(encoding="utf-8"), "Handwritten content")

    def test_broken_and_escaping_knowledge_links_are_rejected(self):
        for link in ("missing.md", "../../pyproject.toml"):
            with self.subTest(link=link), tempfile.TemporaryDirectory() as temporary:
                package = self.package(Path(temporary))
                (package / "knowledge/index.md").write_text(f"[link]({link})", encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "Broken knowledge link"):
                    sdk_build.generate_knowledge(package)
