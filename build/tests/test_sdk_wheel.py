from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
import zipfile
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import payload_builder
from test_wheel_artifacts import write_sdk_wheel

class SdkBoundaryTests(unittest.TestCase):
    def test_runtime_lock_inputs_cover_sdk_requirements(self):
        root = Path(__file__).resolve().parents[2]
        with (root / "kat/sdk/pyproject.toml").open("rb") as stream:
            required = set(tomllib.load(stream)["project"]["dependencies"])
        actual = {line.strip() for line in (root / "build/runtime-requirements.in").read_text().splitlines()
                  if line.strip() and not line.startswith("#")}
        self.assertEqual(actual, required)

    def test_sdk_rejects_foreign_files_dependencies_and_entrypoints(self):
        for extra, requirement in [("_kat_cli/launcher.py", ""), ("kat_sdk-0.1.1rc1.dist-info/entry_points.txt", ""),
                                    ("", "kat-workflow==1.0"), ("", "kat-datasource==1.0"), ("", "kat-cli==1.0")]:
            with self.subTest(extra=extra, requirement=requirement), tempfile.TemporaryDirectory() as tmp:
                wheel = Path(tmp) / "kat_sdk-0.1.1rc1-cp314-cp314-win_amd64.whl"
                write_sdk_wheel(wheel, requires=requirement)
                if extra:
                    with zipfile.ZipFile(wheel, "a") as archive:
                        archive.writestr(extra, "")
                with self.assertRaises(ValueError):
                    payload_builder.validate_sdk_wheel_archive(wheel, expected_version="0.1.1rc1", platform="windows-x86_64")
