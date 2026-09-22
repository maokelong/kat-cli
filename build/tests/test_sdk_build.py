from __future__ import annotations

import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest

REPOSITORY = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    "sdk_build", REPOSITORY / "kat/sdk/sdk_build.py"
)
sdk_build = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sdk_build)


class SDKKnowledgeTests(unittest.TestCase):
    def package(self, root: Path) -> Path:
        package = root / "kat_sdk"
        shutil.copytree(
            REPOSITORY / "kat/sdk",
            package,
            ignore=shutil.ignore_patterns(
                "build", "*.egg-info", "__pycache__", "tests"
            ),
        )
        return package

    def snapshot(self, package: Path) -> dict[Path, bytes]:
        return {
            path.relative_to(package): path.read_bytes()
            for path in package.rglob("*")
            if path.is_file()
        }

    def test_validation_is_static_and_does_not_modify_the_sdk_tree(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            workflows = package / "workflows/domain"
            workflows.mkdir(parents=True)
            (workflows / "probe.py").write_text(
                'raise RuntimeError("must not import")\n', encoding="utf-8"
            )
            before = self.snapshot(package)

            sdk_build.validate_knowledge(package)

            self.assertEqual(self.snapshot(package), before)
            self.assertFalse(any(package.rglob("*.api.md")))

    def test_missing_provider_guide_fails_before_wheel_publication(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = self.package(Path(temporary))
            (package / "knowledge/providers/ftrace.md").unlink()
            with self.assertRaisesRegex(ValueError, "Missing or invalid Guide"):
                sdk_build.validate_knowledge(package)

    def test_broken_and_escaping_guide_links_are_rejected(self):
        for link in ("missing.md", "../../../pyproject.toml"):
            with self.subTest(link=link), tempfile.TemporaryDirectory() as temporary:
                package = self.package(Path(temporary))
                guide = package / "knowledge/workflows/demo/greeting.md"
                guide.write_text(f"[link]({link})", encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "Broken knowledge link"):
                    sdk_build.validate_knowledge(package)
