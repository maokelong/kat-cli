from __future__ import annotations

import hashlib
import importlib.util
import io
import stat
import struct
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
MODULE_PATH = REPOSITORY / "build/build_windows_payload.py"
SPEC = importlib.util.spec_from_file_location("build_windows_payload", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
build_windows_payload = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = build_windows_payload
SPEC.loader.exec_module(build_windows_payload)


def write_pe(path: Path, machine: int = build_windows_payload.PE_X86_64) -> None:
    content = bytearray(0x86)
    content[0:2] = b"MZ"
    content[0x3C:0x40] = struct.pack("<I", 0x80)
    content[0x80:0x84] = b"PE\0\0"
    content[0x84:0x86] = struct.pack("<H", machine)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


class WindowsPayloadBuilderTests(unittest.TestCase):
    def test_runtime_inputs_lock_standard_gil_python_uv_msvc_and_redist_source(
        self,
    ) -> None:
        inputs = build_windows_payload.load_inputs(REPOSITORY)

        self.assertEqual(inputs.python_version, "3.14.6")
        self.assertEqual(inputs.uv_version, "0.11.28")
        self.assertEqual(inputs.rust_target, "x86_64-pc-windows-msvc")
        self.assertEqual(inputs.minimum_windows, 10)
        self.assertEqual(inputs.vc_runtime.version, "14.44.35211")
        self.assertEqual(
            inputs.vc_runtime.content_root.as_posix(),
            "Contents/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT",
        )
        self.assertEqual(
            inputs.python_archive.sha256,
            "ec05b628ad749682d06d225780fbc02e7bbb5ce2146c9bd8e74a3659b14b693a",
        )
        self.assertEqual(
            inputs.python_archive.filename,
            "cpython-3.14.6+20260623-x86_64-pc-windows-msvc-install_only_stripped.tar.gz",
        )
        self.assertEqual(
            inputs.uv_archive.sha256,
            "0a23463216d09c6a72ff80ef5dc5a795f07dc1575cb84d24596c2f124a441b7b",
        )
        self.assertEqual(
            inputs.vc_runtime.archive.sha256,
            "4aaf54db0bfc9435f7c3660e1a00237a4b556042bfeea64bde44c2e0194e6ee5",
        )

    def test_requirements_lock_is_the_complete_windows_closure(self) -> None:
        locked = build_windows_payload.parse_requirements_lock(
            REPOSITORY / "build/requirements-windows.lock.txt"
        )

        self.assertEqual(
            {name: version for name, (version, _) in locked.items()},
            build_windows_payload.APPROVED_REQUIREMENTS,
        )
        self.assertEqual(
            locked["pyarrow"][1],
            "38be1808cdd068605b787e6ca9119b27eb275a0234e50212c3492331680c3b1e",
        )
        self.assertEqual(
            locked["datafusion"][1],
            "b934e097e1bdca7d5768a81ac1bc4a1812cb459269f8b1a5d892a5d930f18376",
        )
        self.assertIn("colorama", locked)

    def test_injected_locked_asset_still_requires_name_and_hash(self) -> None:
        content = b"locked input"
        asset = build_windows_payload.LockedAsset(
            "input.zip",
            "https://example.invalid/input.zip",
            hashlib.sha256(content).hexdigest(),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            supplied = root / asset.filename
            supplied.write_bytes(content)

            resolved = build_windows_payload.resolve_locked_asset(
                asset, supplied, root / "cache", offline=True
            )
            self.assertEqual(resolved, supplied.resolve())

            supplied.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                build_windows_payload.resolve_locked_asset(
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

            with self.assertRaises(tarfile.FilterError):
                build_windows_payload.safe_extract_tar(archive, root / "extract")

            self.assertFalse((root / "outside").exists())

    def test_safe_zip_extraction_rejects_parent_traversal_and_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            traversal = root / "traversal.zip"
            with zipfile.ZipFile(traversal, "w") as output:
                output.writestr("../outside", b"x")
            with self.assertRaisesRegex(ValueError, "unsafe zip member"):
                build_windows_payload.safe_extract_zip(traversal, root / "extract-1")

            symlink = root / "symlink.zip"
            link = zipfile.ZipInfo("link")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            with zipfile.ZipFile(symlink, "w") as output:
                output.writestr(link, "../outside")
            with self.assertRaisesRegex(ValueError, "unsafe zip member"):
                build_windows_payload.safe_extract_zip(symlink, root / "extract-2")

            self.assertFalse((root / "outside").exists())

    def test_workflow_wheel_is_hash_checked_and_installed_without_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheel = root / "kat_workflow-0.1.0-py3-none-any.whl"
            wheel.write_bytes(b"one private wheel")
            checksum = build_windows_payload.file_sha256(wheel)
            wheel.with_name(f"{wheel.name}.sha256").write_text(
                f"{checksum}  {wheel.name}\n", encoding="ascii"
            )
            self.assertEqual(build_windows_payload.validated_workflow_wheel(wheel), wheel)

            with mock.patch.object(build_windows_payload.subprocess, "run") as run:
                build_windows_payload.install_workflow_wheel(
                    root / "uv.exe", root / "python.exe", wheel, root / "cache"
                )
            command = run.call_args.args[0]
            self.assertIn("--no-deps", command)
            self.assertIn("--no-index", command)
            self.assertEqual(command[-1], str(wheel))

    def test_default_paths_use_target_build_root_without_cli_injection(self) -> None:
        options = build_windows_payload.parse_args(["--repository", str(REPOSITORY)])

        self.assertEqual(
            options.output,
            REPOSITORY / "target/kat/payloads/windows-x86_64",
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

    def test_cli_build_invokes_locked_msvc_target(self) -> None:
        inputs = build_windows_payload.load_inputs(REPOSITORY)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            options = build_windows_payload.BuildOptions(
                repository=REPOSITORY,
                output=root / "payload",
                download_cache=root / "downloads",
                python_archive=None,
                uv_archive=None,
                wheelhouse=None,
                vc_redist_archive=None,
                cargo="locked-cargo",
                readobj="llvm-readobj",
                offline=False,
            )

            def fake_run(command: list[str], **arguments: object) -> None:
                self.assertEqual(
                    command[0:4], ["locked-cargo", "build", "--locked", "--release"]
                )
                self.assertIn("x86_64-pc-windows-msvc", command)
                environment = arguments["env"]
                assert isinstance(environment, dict)
                self.assertEqual(
                    environment["RUSTFLAGS"], "-C target-feature=+crt-static"
                )
                write_pe(target / inputs.rust_target / "release/kat.exe")

            with mock.patch.object(
                build_windows_payload.subprocess, "run", side_effect=fake_run
            ):
                binary = build_windows_payload.build_cli_binary(options, inputs, target)

            self.assertEqual(binary, target / inputs.rust_target / "release/kat.exe")

    def test_readobj_parser_collects_regular_and_delay_imports(self) -> None:
        imports = build_windows_payload.parse_readobj_imports(
            """
Import {
  Name: KERNEL32.dll
  Symbol: ExitProcess (343)
}
DelayImport {
  Name: arbitrary_runtime.DLL
  Import {
    Symbol: DelayedFunction (0)
    Address: 0x140001000
  }
}
""",
            Path("example.exe"),
        )

        self.assertEqual(imports, {"kernel32.dll", "arbitrary_runtime.dll"})

    def test_actual_redist_closure_is_recursive_and_wins_over_installed_system_copy(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "payload/kat.exe"
            runtime = root / "Microsoft.VC143.CRT/arbitrary_runtime.dll"
            nested = root / "Microsoft.VC143.CRT/nested_runtime.dll"
            for path in (app, runtime, nested):
                write_pe(path)
            payload_index = build_windows_payload.index_pe_paths([app], "payload")
            redist_index = build_windows_payload.index_pe_paths(
                [runtime, nested], "redist"
            )
            imports = {
                app.resolve(): {"arbitrary_runtime.dll", "kernel32.dll"},
                runtime.resolve(): {"nested_runtime.dll"},
                nested.resolve(): {"api-ms-win-core-file-l1-1-0.dll"},
            }

            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                side_effect=lambda path, _: imports[path.resolve()],
            ):
                required = build_windows_payload.collect_vc_runtime_closure(
                    [app],
                    payload_index,
                    app.parent,
                    redist_index,
                    {"kernel32.dll", "arbitrary_runtime.dll"},
                    "llvm-readobj",
                    allow_redist=True,
                )

            self.assertEqual(required, {runtime.resolve(), nested.resolve()})

    def test_missing_non_system_dependency_fails_the_closure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "kat.exe"
            write_pe(app)
            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                return_value={"missing_runtime.dll"},
            ):
                with self.assertRaisesRegex(ValueError, "unresolved PE dependency"):
                    build_windows_payload.collect_vc_runtime_closure(
                        [app],
                        build_windows_payload.index_pe_paths([app], "payload"),
                        app.parent,
                        {},
                        set(),
                        "llvm-readobj",
                        allow_redist=True,
                    )

    def test_final_closure_rejects_redist_available_only_from_system32(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "kat.exe"
            runtime = root / "redist/arbitrary_runtime.dll"
            write_pe(app)
            write_pe(runtime)
            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                return_value={"arbitrary_runtime.dll"},
            ):
                with self.assertRaisesRegex(
                    ValueError, "app-local VC Runtime dependency"
                ):
                    build_windows_payload.collect_vc_runtime_closure(
                        [app],
                        build_windows_payload.index_pe_paths([app], "payload"),
                        app.parent,
                        build_windows_payload.index_pe_paths([runtime], "redist"),
                        {"arbitrary_runtime.dll"},
                        "llvm-readobj",
                        allow_redist=False,
                    )

    def test_payload_dependency_in_unsearched_sibling_directory_is_rejected(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            python = Path(directory) / "python"
            extension = python / "Lib/site-packages/example/extension.pyd"
            inaccessible = python / "Lib/site-packages/other/private.dll"
            for path in (extension, inaccessible):
                write_pe(path)
            payload_index = build_windows_payload.index_pe_paths(
                [extension, inaccessible], "payload"
            )

            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                side_effect=lambda path, _: (
                    {"private.dll"} if path.resolve() == extension.resolve() else set()
                ),
            ):
                with self.assertRaisesRegex(ValueError, "unresolved PE dependency"):
                    build_windows_payload.collect_vc_runtime_closure(
                        [extension],
                        payload_index,
                        python,
                        {},
                        set(),
                        "llvm-readobj",
                        allow_redist=True,
                    )

    def test_payload_dependency_in_package_owned_delvewheel_directory_is_allowed(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            python = Path(directory) / "python"
            extension = python / "Lib/site-packages/example/extension.pyd"
            private = python / "Lib/site-packages/example.libs/private.dll"
            for path in (extension, private):
                write_pe(path)
            payload_index = build_windows_payload.index_pe_paths(
                [extension, private], "payload"
            )

            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                side_effect=lambda path, _: (
                    {"private.dll"} if path.resolve() == extension.resolve() else set()
                ),
            ):
                required = build_windows_payload.collect_vc_runtime_closure(
                    [extension],
                    payload_index,
                    python,
                    {},
                    set(),
                    "llvm-readobj",
                    allow_redist=True,
                )

            self.assertEqual(required, set())

    def test_system32_file_must_identify_as_a_windows_os_component(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            system32 = Path(directory)
            runtime = system32 / "arbitrary_runtime.dll"
            runtime.write_bytes(b"installed runtime")
            names = build_windows_payload.WindowsSystemDllNames(system32)

            with mock.patch.object(
                build_windows_payload,
                "is_windows_system_component",
                return_value=False,
            ):
                self.assertNotIn("arbitrary_runtime.dll", names)

            names = build_windows_payload.WindowsSystemDllNames(system32)
            with mock.patch.object(
                build_windows_payload,
                "is_windows_system_component",
                return_value=True,
            ):
                self.assertIn("arbitrary_runtime.dll", names)

    def test_non_x86_64_pe_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "wrong.exe"
            write_pe(binary, machine=0x014C)

            with self.assertRaisesRegex(ValueError, "non-x86_64"):
                build_windows_payload.index_pe_paths([binary], "payload")

    def test_payload_shape_exposes_only_kat_exe_at_the_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            payload = Path(directory)
            (payload / "kat.exe").write_bytes(b"cli")
            python = payload / "python/python.exe"
            python.parent.mkdir()
            python.write_bytes(b"python")
            build_windows_payload.assert_payload_shape(payload)

            (payload / "runtime.dll").write_bytes(b"runtime")
            with self.assertRaisesRegex(ValueError, "only kat.exe"):
                build_windows_payload.assert_payload_shape(payload)

    def test_output_overlap_is_rejected_without_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "payload"
            options = build_windows_payload.BuildOptions(
                repository=REPOSITORY,
                output=output,
                download_cache=output / "downloads",
                python_archive=None,
                uv_archive=None,
                wheelhouse=None,
                vc_redist_archive=None,
                cargo="cargo",
                readobj="llvm-readobj",
                offline=False,
            )

            with self.assertRaisesRegex(ValueError, "overlaps download cache"):
                build_windows_payload.reject_output_input_overlap(options)

            self.assertFalse(output.exists())

    def test_output_overlap_with_locked_vc_runtime_vsix_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "payload"
            options = build_windows_payload.BuildOptions(
                repository=REPOSITORY,
                output=output,
                download_cache=root / "downloads",
                python_archive=None,
                uv_archive=None,
                wheelhouse=None,
                vc_redist_archive=output / "runtime.vsix",
                cargo="cargo",
                readobj="llvm-readobj",
                offline=False,
            )

            with self.assertRaisesRegex(ValueError, "overlaps VC Runtime VSIX"):
                build_windows_payload.reject_output_input_overlap(options)

            self.assertFalse(output.exists())

    def test_builder_has_no_fixed_redistributable_dll_name_list(self) -> None:
        source = MODULE_PATH.read_text(encoding="utf-8").casefold()

        self.assertNotIn("vcruntime140.dll", source)
        self.assertNotIn("msvcp140.dll", source)
        self.assertNotIn("redistributable_dlls", source)


if __name__ == "__main__":
    unittest.main()
