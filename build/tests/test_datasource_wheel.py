from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
MODULE_PATH = REPOSITORY / "build/build_datasource_wheel.py"


def load_builder():
    spec = importlib.util.spec_from_file_location("build_datasource_wheel", MODULE_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def write_wheel(path: Path, *, tag: str, extension: str) -> None:
    version = "0.1.1rc1"
    dist_info = f"kat_datasource-{version}.dist-info"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("kat_datasource/__init__.py", "")
        archive.writestr("kat_datasource/hitrace.py", "")
        archive.writestr(extension, b"native extension")
        archive.writestr(
            f"{dist_info}/METADATA",
            "Metadata-Version: 2.4\n"
            "Name: kat-datasource\n"
            f"Version: {version}\n",
        )
        archive.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\n"
            "Root-Is-Purelib: false\n"
            f"Tag: {tag}\n",
        )


class DatasourceWheelTests(unittest.TestCase):
    def test_maturin_is_locked_only_for_the_builder_environment(self) -> None:
        builder_lock = (REPOSITORY / "build/requirements-builder.lock.txt").read_text(
            encoding="utf-8"
        )
        self.assertIn("maturin==1.15.0", builder_lock)
        self.assertRegex(builder_lock, r"maturin==1\.15\.0.*--hash=sha256:")
        for platform in ("linux", "windows"):
            runtime_lock = (
                REPOSITORY / f"build/requirements-{platform}.lock.txt"
            ).read_text(encoding="utf-8")
            self.assertNotIn("maturin", runtime_lock.lower())

    def test_native_build_uses_maturin_and_validates_each_platform_tag(self) -> None:
        builder = load_builder()
        cases = (
            (
                "windows-x86_64",
                "x86_64-pc-windows-msvc",
                "cp314-cp314-win_amd64",
                "kat_datasource/_native.cp314-win_amd64.pyd",
                False,
            ),
            (
                "linux-x86_64",
                "x86_64-unknown-linux-gnu",
                "cp314-cp314-manylinux_2_28_x86_64",
                "kat_datasource/_native.cpython-314-x86_64-linux-gnu.so",
                True,
            ),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            python = root / "python"
            python.write_bytes(b"python")
            for platform, rust_target, tag, extension, manylinux in cases:
                with self.subTest(platform=platform):
                    output = root / platform

                    def fake_run(command: list[str], **_: object) -> mock.Mock:
                        built = Path(command[command.index("--out") + 1])
                        filename = f"kat_datasource-0.1.1rc1-{tag}.whl"
                        write_wheel(built / filename, tag=tag, extension=extension)
                        return mock.Mock()

                    with mock.patch.object(
                        builder.subprocess,
                        "run",
                        side_effect=fake_run,
                    ) as run:
                        wheel, checksum = builder.build_datasource_wheel(
                            REPOSITORY,
                            python,
                            output,
                            platform=platform,
                            expected_version="0.1.1rc1",
                            cargo_target_dir=root / "cargo" / platform,
                        )

                    command = run.call_args.args[0]
                    self.assertEqual(command[:3], [str(python), "-m", "maturin"])
                    self.assertIn("--locked", command)
                    self.assertEqual(
                        command[command.index("--target") + 1], rust_target
                    )
                    self.assertEqual(
                        "manylinux_2_28" in command,
                        manylinux,
                    )
                    self.assertEqual(
                        checksum.read_text(encoding="ascii"),
                        f"{builder.file_sha256(wheel)}  {wheel.name}\n",
                    )


if __name__ == "__main__":
    unittest.main()
