from __future__ import annotations

import hashlib
import importlib.util
import io
import os
import stat
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
MODULE_PATH = REPOSITORY / "build/build_linux_payload.py"
SPEC = importlib.util.spec_from_file_location("build_linux_payload", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
build_linux_payload = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = build_linux_payload
SPEC.loader.exec_module(build_linux_payload)


class LinuxPayloadBuilderTests(unittest.TestCase):
    def test_runtime_inputs_lock_standard_gil_python_uv_and_glibc(self) -> None:
        inputs = build_linux_payload.load_inputs(REPOSITORY)

        self.assertEqual(inputs.python_version, "3.14.6")
        self.assertEqual(inputs.uv_version, "0.11.28")
        self.assertEqual(inputs.rust_target, "x86_64-unknown-linux-gnu")
        self.assertEqual(inputs.minimum_glibc, (2, 28))
        self.assertEqual(
            inputs.python_archive.sha256,
            "c172314f4a8ec137a8f605289010c3d19c8b56867d968f0095074cc68efa1d29",
        )

    def test_requirements_lock_is_the_complete_approved_closure(self) -> None:
        locked = build_linux_payload.parse_requirements_lock(
            REPOSITORY / "build/requirements-linux.lock.txt"
        )

        self.assertEqual(
            {name: version for name, (version, _) in locked.items()},
            build_linux_payload.APPROVED_REQUIREMENTS,
        )
        self.assertEqual(
            locked["pyarrow"][1],
            "ae8a1145af31d903fa9bb166824d7abe9b4681a000b0159c9fb99c11bc11ad26",
        )
        self.assertEqual(
            locked["datafusion"][1],
            "3bcd4d213fa74710e75e6e182cc468c2bdbc5ffc74a08c8155d414fbbfa1b3f6",
        )

    def test_injected_locked_asset_still_requires_name_and_hash(self) -> None:
        content = b"locked input"
        asset = build_linux_payload.LockedAsset(
            "input.tar.gz",
            "https://example.invalid/input.tar.gz",
            hashlib.sha256(content).hexdigest(),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            supplied = root / asset.filename
            supplied.write_bytes(content)

            resolved = build_linux_payload.resolve_locked_asset(
                asset, supplied, root / "cache", offline=True
            )
            self.assertEqual(resolved, supplied.resolve())

            supplied.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                build_linux_payload.resolve_locked_asset(
                    asset, supplied, root / "cache", offline=True
                )

    def test_safe_tar_extraction_rejects_parent_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "unsafe.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                member = tarfile.TarInfo("../outside")
                member.size = 1
                output.addfile(member, io.BytesIO(b"x"))

            with self.assertRaisesRegex(tarfile.FilterError, "outside"):
                build_linux_payload.safe_extract_tar(archive, root / "extract")

            self.assertFalse((root / "outside").exists())

    def test_safe_tar_extraction_rejects_runtime_symlink_chain_escape(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            probe = root / "symlink-probe"
            try:
                os.symlink(".", probe)
            except OSError as error:
                self.skipTest(f"host cannot create symlinks: {error}")
            probe.unlink()
            archive = root / "symlink-chain.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                pivot = tarfile.TarInfo("pivot")
                pivot.type = tarfile.SYMTYPE
                pivot.linkname = "."
                pivot.mode = 0o777
                output.addfile(pivot)
                nested = tarfile.TarInfo("pivot/a")
                nested.type = tarfile.DIRTYPE
                nested.mode = 0o755
                output.addfile(nested)
                escape = tarfile.TarInfo("pivot/a/link")
                escape.type = tarfile.SYMTYPE
                escape.linkname = "../../outside"
                escape.mode = 0o777
                output.addfile(escape)
                owned = tarfile.TarInfo("pivot/a/link/owned")
                owned.size = 5
                output.addfile(owned, io.BytesIO(b"owned"))
            outside = root / "outside"
            outside.mkdir()

            with self.assertRaises(tarfile.FilterError):
                build_linux_payload.safe_extract_tar(archive, root / "extract")

            self.assertFalse((outside / "owned").exists())

    def test_workflow_wheel_is_hash_checked_and_installed_without_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheel = root / "kat_workflow-0.1.0-py3-none-any.whl"
            wheel.write_bytes(b"one private wheel")
            checksum = build_linux_payload.file_sha256(wheel)
            wheel.with_name(f"{wheel.name}.sha256").write_text(
                f"{checksum}  {wheel.name}\n", encoding="ascii"
            )
            self.assertEqual(build_linux_payload.validated_workflow_wheel(wheel), wheel)

            with mock.patch.object(build_linux_payload.subprocess, "run") as run:
                build_linux_payload.install_workflow_wheel(
                    root / "uv", root / "python", wheel, root / "cache"
                )
            command = run.call_args.args[0]
            self.assertIn("--no-deps", command)
            self.assertIn("--no-index", command)
            self.assertEqual(command[-1], str(wheel))

    def test_private_host_prunes_platform_specific_terminfo(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            python_root = Path(directory)
            terminfo = python_root / "share/terminfo"
            terminfo.mkdir(parents=True)
            (terminfo / "xterm").write_bytes(b"terminal data")
            keep = python_root / "share/runtime-data"
            keep.mkdir()
            (keep / "required").write_bytes(b"runtime data")

            build_linux_payload.prune_private_host(python_root)

            self.assertFalse(terminfo.exists())
            self.assertTrue((keep / "required").is_file())

    def test_payload_shape_exposes_only_kat_at_the_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            payload = Path(directory)
            cli = payload / "kat"
            cli.write_bytes(b"cli")
            cli.chmod(cli.stat().st_mode | stat.S_IXUSR)
            python = payload / "python/bin/python3"
            python.parent.mkdir(parents=True)
            python.write_bytes(b"python")

            build_linux_payload.assert_payload_shape(payload)

            terminfo = payload / "python/share/terminfo"
            terminfo.mkdir(parents=True)
            entry = terminfo / "xterm"
            entry.write_bytes(b"terminal data")
            with self.assertRaisesRegex(ValueError, "terminfo"):
                build_linux_payload.assert_payload_shape(payload)
            entry.unlink()
            terminfo.rmdir()

            (payload / "pack.toml").write_text("", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "root must contain only"):
                build_linux_payload.assert_payload_shape(payload)

    def test_glibc_versions_are_compared_numerically(self) -> None:
        self.assertEqual(
            build_linux_payload.parse_glibc_versions(
                "Name: GLIBC_2.17\nName: GLIBC_2.28\nName: GLIBC_2.9"
            ),
            {(2, 17), (2, 28), (2, 9)},
        )

    def test_default_generated_paths_stay_under_target_build_root(self) -> None:
        options = build_linux_payload.parse_args(["--repository", str(REPOSITORY)])

        self.assertEqual(
            options.output,
            REPOSITORY / "target/kat/payloads/linux-x86_64",
        )
        self.assertEqual(
            options.download_cache,
            REPOSITORY / "target/kat/downloads",
        )
        self.assertFalse(hasattr(options, "kat_binary"))
        self.assertEqual(
            options.workflow_wheel,
            REPOSITORY
            / "target/kat/workflow-wheel/kat_workflow-0.1.0-py3-none-any.whl",
        )

    def test_output_containing_download_cache_is_rejected_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "payload"
            options = build_linux_payload.BuildOptions(
                repository=REPOSITORY,
                output=output,
                download_cache=output / "downloads",
                python_archive=None,
                uv_archive=None,
                wheelhouse=None,
                cargo="cargo",
                readelf="readelf",
                offline=False,
            )

            with self.assertRaisesRegex(ValueError, "overlaps download cache"):
                build_linux_payload.build_payload(options)

            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
