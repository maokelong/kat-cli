from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import build_windows_payload


def write_binary(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(path.name.encode())


class WindowsPayloadBuilderTests(unittest.TestCase):
    @mock.patch.object(
        build_windows_payload,
        "pe_machine",
        return_value=build_windows_payload.PE_X86_64,
    )
    def test_native_closure_includes_regular_delay_and_recursive_redist_imports(
        self, _pe_machine: mock.Mock
    ) -> None:
        image = mock.Mock()
        image.DIRECTORY_ENTRY_IMPORT = [SimpleNamespace(dll=b"KERNEL32.dll")]
        image.DIRECTORY_ENTRY_DELAY_IMPORT = [
            SimpleNamespace(dll=b"arbitrary_runtime.DLL")
        ]
        with mock.patch.object(build_windows_payload.pefile, "PE", return_value=image):
            self.assertEqual(
                build_windows_payload.pe_imports(Path("example.exe")),
                {"kernel32.dll", "arbitrary_runtime.dll"},
            )

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "payload/kat.exe"
            runtime = root / "redist/arbitrary_runtime.dll"
            nested = root / "redist/nested_runtime.dll"
            for path in (app, runtime, nested):
                write_binary(path)
            imports = {
                app.resolve(): {"arbitrary_runtime.dll", "kernel32.dll"},
                runtime.resolve(): {"nested_runtime.dll"},
                nested.resolve(): {"api-ms-win-core-file-l1-1-0.dll"},
            }
            with mock.patch.object(
                build_windows_payload,
                "pe_imports",
                side_effect=lambda path: imports[path.resolve()],
            ):
                required = build_windows_payload.collect_vc_runtime_closure(
                    [app],
                    build_windows_payload.index_pe_paths([app], "payload"),
                    app.parent,
                    build_windows_payload.index_pe_paths([runtime, nested], "redist"),
                    {"kernel32.dll", "arbitrary_runtime.dll"},
                    allow_redist=True,
                )

            self.assertEqual(required, {runtime.resolve(), nested.resolve()})

    @mock.patch.object(
        build_windows_payload,
        "pe_machine",
        return_value=build_windows_payload.PE_X86_64,
    )
    def test_native_closure_fails_closed_for_unsafe_or_unresolved_dependencies(
        self, pe_machine: mock.Mock
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            with self.subTest(case="missing dependency"):
                app = root / "missing/kat.exe"
                write_binary(app)
                with mock.patch.object(
                    build_windows_payload,
                    "pe_imports",
                    return_value={"missing_runtime.dll"},
                ), self.assertRaisesRegex(ValueError, "unresolved PE dependency"):
                    build_windows_payload.collect_vc_runtime_closure(
                        [app],
                        build_windows_payload.index_pe_paths([app], "payload"),
                        app.parent,
                        {},
                        set(),
                        allow_redist=True,
                    )

            with self.subTest(case="redist only available outside final payload"):
                app = root / "system-copy/kat.exe"
                runtime = root / "system-copy/redist/arbitrary_runtime.dll"
                write_binary(app)
                write_binary(runtime)
                with mock.patch.object(
                    build_windows_payload,
                    "pe_imports",
                    return_value={"arbitrary_runtime.dll"},
                ), self.assertRaisesRegex(ValueError, "app-local VC Runtime dependency"):
                    build_windows_payload.collect_vc_runtime_closure(
                        [app],
                        build_windows_payload.index_pe_paths([app], "payload"),
                        app.parent,
                        build_windows_payload.index_pe_paths([runtime], "redist"),
                        {"arbitrary_runtime.dll"},
                        allow_redist=False,
                    )

            with self.subTest(case="unsearched sibling directory"):
                python = root / "sibling/python"
                extension = python / "Lib/site-packages/example/extension.pyd"
                inaccessible = python / "Lib/site-packages/other/private.dll"
                for path in (extension, inaccessible):
                    write_binary(path)
                with mock.patch.object(
                    build_windows_payload,
                    "pe_imports",
                    side_effect=lambda path: (
                        {"private.dll"}
                        if path.resolve() == extension.resolve()
                        else set()
                    ),
                ), self.assertRaisesRegex(ValueError, "unresolved PE dependency"):
                    build_windows_payload.collect_vc_runtime_closure(
                        [extension],
                        build_windows_payload.index_pe_paths(
                            [extension, inaccessible], "payload"
                        ),
                        python,
                        {},
                        set(),
                        allow_redist=True,
                    )

            with self.subTest(case="non x86-64 image"):
                binary = root / "wrong-machine/wrong.exe"
                write_binary(binary)
                pe_machine.return_value = 0x014C
                with self.assertRaisesRegex(ValueError, "non-x86_64"):
                    build_windows_payload.index_pe_paths([binary], "payload")
                pe_machine.return_value = build_windows_payload.PE_X86_64

            with self.subTest(case="non-system file found in System32"):
                system32 = root / "System32"
                runtime = system32 / "arbitrary_runtime.dll"
                runtime.parent.mkdir()
                runtime.write_bytes(b"installed runtime")
                names = build_windows_payload.BuildHostWindowsSystemDllNames(system32)
                with mock.patch.object(
                    build_windows_payload,
                    "is_windows_system_component",
                    return_value=False,
                ):
                    self.assertNotIn("arbitrary_runtime.dll", names)


if __name__ == "__main__":
    unittest.main()
