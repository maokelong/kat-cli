from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[2] / "kat/skill/scripts/scaffold_pack.py"
SPEC = importlib.util.spec_from_file_location("scaffold_pack", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
scaffold_pack = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scaffold_pack)


class ScaffoldPackTests(unittest.TestCase):
    def test_creates_an_inspectable_authoring_skeleton(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            packs = Path(temporary) / "packs"

            result = scaffold_pack.scaffold_pack(
                packs_directory=packs,
                name="memory-analysis",
                title="内存分析",
                description="分析内存占用与变化趋势。",
                owner="性能团队",
            )

            pack = packs / "memory-analysis"
            self.assertEqual(Path(result["pack_directory"]), pack)
            self.assertEqual(
                (pack / "pack.toml").read_text(encoding="utf-8"),
                'name = "memory-analysis"\n'
                'title = "内存分析"\n'
                'description = "分析内存占用与变化趋势。"\n'
                'owner = "性能团队"\n',
            )
            self.assertTrue((pack / "README.md").is_file())
            for relative_path, _ in scaffold_pack._DIRECTORIES:
                self.assertTrue((pack / relative_path).is_dir())
            self.assertEqual(
                {entry["path"] for entry in result["entries"]},
                {
                    "pack.toml",
                    "README.md",
                    "workflows/",
                    "datasources/",
                    "helpers/",
                    "knowledge/",
                    "knowledge/workflows/",
                    "knowledge/providers/",
                    "tests/",
                },
            )

    def test_rejects_invalid_or_incomplete_metadata_without_creating_pack(self) -> None:
        invalid_cases = (
            {
                "name": "Memory",
                "title": "Title",
                "description": "Description",
                "owner": "Owner",
            },
            {
                "name": "con",
                "title": "Title",
                "description": "Description",
                "owner": "Owner",
            },
            {
                "name": "memory",
                "title": " ",
                "description": "Description",
                "owner": "Owner",
            },
            {
                "name": "memory",
                "title": "Title",
                "description": "",
                "owner": "Owner",
            },
            {
                "name": "memory",
                "title": "Title",
                "description": "Description",
                "owner": "\t",
            },
        )
        for metadata in invalid_cases:
            with self.subTest(metadata=metadata), tempfile.TemporaryDirectory() as temporary:
                packs = Path(temporary) / "packs"
                with self.assertRaises(scaffold_pack.ScaffoldError):
                    scaffold_pack.scaffold_pack(packs_directory=packs, **metadata)
                self.assertFalse((packs / metadata["name"]).exists())

    def test_refuses_to_overwrite_an_existing_target(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            packs = Path(temporary) / "packs"
            target = packs / "memory"
            target.mkdir(parents=True)
            marker = target / "owned.txt"
            marker.write_text("keep", encoding="utf-8")

            with self.assertRaisesRegex(scaffold_pack.ScaffoldError, "拒绝覆盖"):
                scaffold_pack.scaffold_pack(
                    packs_directory=packs,
                    name="memory",
                    title="Memory",
                    description="Memory analysis.",
                    owner="Performance Team",
                )

            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
            self.assertEqual(list(packs.glob(".memory-scaffold-*")), [])


if __name__ == "__main__":
    unittest.main()
