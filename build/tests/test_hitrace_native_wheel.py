from __future__ import annotations

import os
import shutil
import sys
import unittest
import uuid
import zipfile
from contextlib import contextmanager
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import payload_builder


VERSION = "0.1.1rc1"


@contextmanager
def temporary_directory():
    base = REPOSITORY / "target/kat/builder-tests"
    base.mkdir(parents=True, exist_ok=True)
    root = base / f"kat-hitrace-wheel-test-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        yield root
    finally:
        shutil.rmtree(root)


def platform_spec(*, windows: bool = False) -> payload_builder.PlatformSpec:
    return payload_builder.PlatformSpec(
        key="windows-x86_64" if windows else "linux-x86_64",
        label="Windows" if windows else "Linux",
        managed_python_fields=("platform", "os", "libc"),
        managed_python_launcher_glob="python",
        managed_python_root_parents=1,
        private_python_parts=("python",),
        copy_uv_links=windows,
        site_packages_globs=(),
        prune_paths=(),
        private_bin_parts=None,
        private_bin_keep_prefix=None,
        cli_filename="kat.exe" if windows else "kat",
        cargo_environment=(("LINK", "/Brepro"),) if windows else (),
        native_wheel_platform_tag="win_amd64"
        if windows
        else "manylinux_2_28_x86_64",
        native_extension_suffix=".pyd" if windows else ".so",
        native_wheel_compatibility=None if windows else "manylinux_2_28",
    )


def write_native_wheel(
    directory: Path,
    spec: payload_builder.PlatformSpec,
    *,
    version: str = VERSION,
    python_tag: str = "cp314",
    abi_tag: str = "cp314",
    platform_tag: str | None = None,
    distribution: str = "kat-hitrace-native",
    purelib: str = "false",
) -> Path:
    platform_tag = platform_tag or spec.native_wheel_platform_tag
    name = (
        f"kat_hitrace_native-{version}-{python_tag}-{abi_tag}-{platform_tag}.whl"
    )
    path = directory / name
    dist_info = f"kat_hitrace_native-{version}.dist-info"
    module = f"_kat_hitrace.cp314-{platform_tag}{spec.native_extension_suffix}"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(module, b"native")
        archive.writestr(
            f"{dist_info}/METADATA",
            "Metadata-Version: 2.4\n"
            f"Name: {distribution}\n"
            f"Version: {version}\n",
        )
        archive.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\n"
            f"Root-Is-Purelib: {purelib}\n"
            f"Tag: {python_tag}-{abi_tag}-{platform_tag}\n",
        )
    return path


class HitraceNativeWheelTests(unittest.TestCase):
    def test_archive_requires_the_release_cp314_and_native_platform_tag(self) -> None:
        for spec in (platform_spec(), platform_spec(windows=True)):
            with self.subTest(platform=spec.key), temporary_directory() as root:
                wheel = write_native_wheel(root, spec)
                self.assertEqual(
                    payload_builder.validate_hitrace_wheel_archive(
                        wheel,
                        expected_version=VERSION,
                        spec=spec,
                    ),
                    VERSION,
                )

        cases = (
            ("wrong release", {"version": "9.9.9"}, "version"),
            ("stable ABI", {"abi_tag": "abi3"}, "CPython 3.14 ABI"),
            ("wrong platform", {"platform_tag": "linux_x86_64"}, "must use"),
            (
                "wrong distribution",
                {"distribution": "other"},
                "unexpected distribution",
            ),
            ("pure wheel", {"purelib": "true"}, "platform-specific"),
        )
        for name, changes, message in cases:
            with self.subTest(case=name), temporary_directory() as root:
                spec = platform_spec()
                wheel = write_native_wheel(root, spec, **changes)
                with self.assertRaisesRegex(ValueError, message):
                    payload_builder.validate_hitrace_wheel_archive(
                        wheel,
                        expected_version=VERSION,
                        spec=spec,
                    )

    def test_build_uses_locked_maturin_and_the_platform_cargo_cache(self) -> None:
        for spec in (platform_spec(), platform_spec(windows=True)):
            with self.subTest(platform=spec.key), temporary_directory() as root_path:
                repository = root_path / "repository"
                repository.mkdir()
                output = root_path / "wheels"
                target_dir = repository / "target/kat/cargo" / spec.key
                inputs = mock.Mock(rust_target=f"{spec.key}-target")

                def run(command: list[str], **_: object) -> None:
                    write_native_wheel(output, spec)

                inherited = {
                    "CARGO_TARGET_DIR": str(root_path / "caller-target"),
                    "CARGO_BUILD_TARGET_DIR": str(root_path / "caller-build-target"),
                    "KAT_TEST_ENV": "preserved",
                }
                with (
                    mock.patch.dict(os.environ, inherited, clear=True),
                    mock.patch.object(
                        payload_builder.subprocess,
                        "run",
                        side_effect=run,
                    ) as maturin,
                ):
                    wheel = payload_builder.build_hitrace_wheel(
                        builder_python=Path("builder-python"),
                        target_python=Path("private-python"),
                        repository=repository,
                        target_dir=target_dir,
                        output=output,
                        cargo="locked-cargo",
                        inputs=inputs,
                        spec=spec,
                        expected_version=VERSION,
                        offline=True,
                    )

                self.assertEqual(wheel.parent, output)
                command = maturin.call_args.args[0]
                self.assertEqual(
                    command[:5],
                    ["builder-python", "-m", "maturin", "build", "--release"],
                )
                for option in (
                    "--locked",
                    "--offline",
                    "--interpreter",
                    "--target-dir",
                    "--target",
                    "--manifest-path",
                    "--out",
                ):
                    self.assertIn(option, command)
                self.assertEqual(
                    command[command.index("--target-dir") + 1], str(target_dir)
                )
                if spec.native_wheel_compatibility is None:
                    self.assertNotIn("--compatibility", command)
                else:
                    self.assertEqual(
                        command[command.index("--compatibility") + 1],
                        spec.native_wheel_compatibility,
                    )
                environment = maturin.call_args.kwargs["env"]
                self.assertEqual(environment["CARGO"], "locked-cargo")
                self.assertEqual(environment["KAT_TEST_ENV"], "preserved")
                self.assertNotIn("CARGO_TARGET_DIR", environment)
                self.assertNotIn("CARGO_BUILD_TARGET_DIR", environment)

    def test_install_and_real_ffi_smoke_commands_are_isolated(self) -> None:
        with temporary_directory() as root_path:
            wheel = root_path / "native.whl"
            wheel.write_bytes(b"wheel")
            observed_trace: bytes | None = None

            def run(command: list[str], **_: object) -> None:
                nonlocal observed_trace
                if "hitrace-ffi-smoke.htrace" in command[-1]:
                    observed_trace = Path(command[-1]).read_bytes()

            with mock.patch.object(
                payload_builder.subprocess,
                "run",
                side_effect=run,
            ) as subprocess_run:
                payload_builder.install_private_wheel(
                    root_path / "uv",
                    root_path / "python",
                    wheel,
                    root_path / "uv-cache",
                    copy_links=False,
                )
                payload_builder.smoke_test_hitrace_source(
                    root_path / "python",
                    root_path,
                )

            install = subprocess_run.call_args_list[0].args[0]
            for option in ("--no-deps", "--no-index", "--break-system-packages"):
                self.assertIn(option, install)
            smoke = subprocess_run.call_args_list[1].args[0]
            self.assertEqual(smoke[:4], [str(root_path / "python"), "-I", "-B", "-c"])
            self.assertIn("__datafusion_schema_provider__", smoke[4])
            self.assertIn("SELECT COUNT(*)", smoke[4])
            self.assertIsNotNone(observed_trace)
            assert observed_trace is not None
            self.assertEqual(len(observed_trace), 1024)
            self.assertEqual(
                observed_trace[:8],
                (0x464F_5250_534F_484F).to_bytes(8, "little"),
            )
            self.assertFalse((root_path / "hitrace-ffi-smoke.htrace").exists())


if __name__ == "__main__":
    unittest.main()
