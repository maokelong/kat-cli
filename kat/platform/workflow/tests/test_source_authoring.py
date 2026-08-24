from __future__ import annotations

import inspect
import os
from pathlib import Path
import sys
import tempfile
import unittest
from typing import Literal

import kat
from kat._identifiers import valid_source_name
from _kat_runtime.inspection import (
    compile_declared_source,
    inspect_declared_source,
)
from _kat_runtime.pack import (
    ProductionPack,
    SOURCE_OPERATION_PROFILE,
    private_source_module_root,
)


@kat.source(name="raw_logs")
def raw_logs(
    primary: Path,
    files: tuple[Path, ...],
    label: str = "relative-looking.log",
    optional: Path | None = None,
    fallback: Path = Path("fallback.log"),
    count: int = 7,
    ratio: float = 1,
    enabled: bool = False,
    window: kat.Duration = "5ms",
    at: kat.WallClockTimestamp = "2026-07-14T08:30:00Z",
    mode: Literal["strict", "lenient"] = "strict",
) -> UndefinedReturn:
    raise AssertionError("inspection must not call the Source Entry")


@kat.source(name="optional_files")
def optional_files(files: tuple[Path, ...] = ()):
    raise AssertionError("inspection must not call the Source Entry")


@kat.source(name="dataset")
def reserved_source_name():
    return None


@kat.source(name="Bad-Name")
def malformed_source_name():
    return None


@kat.source(name="information_schema")
def datafusion_reserved_source_name():
    return None


@kat.source(name="async_source")
async def asynchronous_source(value: str):
    return value


@kat.source(name="context")
def source_with_context(ctx: kat.Context):
    return ctx


@kat.source(name="list_path")
def source_with_path_list(files: list[Path]):
    return files


@kat.source(name="positional_only")
def source_with_positional_only(value: str, /):
    return value


@kat.source(name="optional_repeated")
def source_with_optional_repeated(files: tuple[Path, ...] | None = None):
    return files


class SourceAuthoringTest(unittest.TestCase):
    def test_source_names_follow_the_portable_namespace_contract(self) -> None:
        windows_devices = {"con", "prn", "aux", "nul"} | {
            f"{prefix}{number}"
            for prefix in ("com", "lpt")
            for number in range(1, 10)
        }
        for name in windows_devices | {"dataset", "information_schema"}:
            with self.subTest(name=name):
                self.assertFalse(valid_source_name(name))

        for name in ("facts", "public", "com0", "com10", "lpt0", "lpt10"):
            with self.subTest(name=name):
                self.assertTrue(valid_source_name(name))

    def test_public_source_contract_is_identity_only(self) -> None:
        signature = inspect.signature(kat.source)
        self.assertEqual(list(signature.parameters), ["name"])
        self.assertEqual(signature.parameters["name"].kind, inspect.Parameter.KEYWORD_ONLY)
        documentation = " ".join((inspect.getdoc(kat.source) or "").split())
        for boundary in (
            "module-top-level synchronous",
            "Windows device name",
            "tables belong to the returned provider",
            "pathlib.Path",
            "does not receive ``kat.Context``",
            "never calls the function",
        ):
            with self.subTest(boundary=boundary):
                self.assertIn(boundary, documentation)

    def test_source_interface_and_path_compilation_are_exact(self) -> None:
        self.assertEqual(
            inspect_declared_source(raw_logs),
            {
                "name": "raw_logs",
                "parameters": [
                    {"name": "primary", "option": "--primary", "type": "path", "required": True},
                    {"name": "files", "option": "--files", "type": "path", "required": True, "repeatable": True},
                    {"name": "label", "option": "--label", "type": "string", "required": False, "default": "relative-looking.log"},
                    {"name": "optional", "option": "--optional", "type": "path", "required": False, "default": None},
                    {"name": "fallback", "option": "--fallback", "type": "path", "required": False, "default": "fallback.log"},
                    {"name": "count", "option": "--count", "type": "int64", "required": False, "default": "7"},
                    {"name": "ratio", "option": "--ratio", "type": "float64", "required": False, "default": 1.0},
                    {"name": "enabled", "option": "--enabled", "negative_option": "--no-enabled", "type": "boolean", "required": False, "default": False},
                    {"name": "window", "option": "--window", "type": "duration", "required": False, "default": "5ms"},
                    {"name": "at", "option": "--at", "type": "wall_clock_timestamp", "required": False, "default": "2026-07-14T08:30:00Z"},
                    {"name": "mode", "option": "--mode", "type": "string", "required": False, "choices": ["lenient", "strict"], "default": "strict"},
                ],
            },
        )

        base = Path.cwd() / "argument-base"
        compiled = compile_declared_source(raw_logs)
        effective = compiled.parse_arguments(
            [
                "--primary",
                "missing/../capture.log",
                "--files",
                "first.log",
                "--files",
                "nested/second.log",
                "--label",
                "still/a/string.log",
            ],
            argument_base=base,
        )
        self.assertEqual(
            effective["primary"],
            Path(os.path.normpath(base / "capture.log")),
        )
        self.assertEqual(
            effective["files"],
            (
                Path(os.path.normpath(base / "first.log")),
                Path(os.path.normpath(base / "nested/second.log")),
            ),
        )
        self.assertEqual(effective["label"], "still/a/string.log")
        self.assertIsNone(effective["optional"])
        self.assertEqual(
            effective["fallback"],
            Path(os.path.normpath(base / "fallback.log")),
        )
        with self.assertRaises(ValueError):
            compiled.parse_arguments(
                ["--primary", "capture.log"],
                argument_base=base,
            )

    def test_repeated_path_default_is_an_empty_tuple(self) -> None:
        compiled = compile_declared_source(optional_files)
        self.assertEqual(
            compiled.interface["parameters"],
            [
                {
                    "name": "files",
                    "option": "--files",
                    "type": "path",
                    "required": False,
                    "repeatable": True,
                    "default": [],
                }
            ],
        )
        self.assertEqual(
            compiled.parse_arguments([], argument_base=Path.cwd())["files"],
            (),
        )

    def test_information_schema_source_name_is_rejected_as_engine_reserved(self) -> None:
        with self.assertRaisesRegex(
            ValueError,
            "DataFusion 保留的系统 schema",
        ):
            inspect_declared_source(datafusion_reserved_source_name)

    def test_invalid_source_shapes_fail_during_inspection(self) -> None:
        invalid = [
            (reserved_source_name, "invalid Source name"),
            (malformed_source_name, "invalid Source name"),
            (asynchronous_source, "plain synchronous function"),
            (source_with_context, "unsupported annotation"),
            (source_with_path_list, "unsupported annotation"),
            (source_with_positional_only, "unsupported kind"),
            (source_with_optional_repeated, "cannot be a repeated Path"),
        ]
        for function, message in invalid:
            with self.subTest(function=function), self.assertRaisesRegex(
                ValueError,
                message,
            ):
                inspect_declared_source(function)

        with self.assertRaises(TypeError):
            kat.source(name=1)  # type: ignore[arg-type]


class PrivateSourcePackTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.module_roots: set[str] = set()

    def tearDown(self) -> None:
        for name in tuple(sys.modules):
            if any(
                name == root or name.startswith(f"{root}.")
                for root in self.module_roots
            ):
                sys.modules.pop(name, None)
        self.temporary.cleanup()

    def test_external_source_packs_keep_same_named_helpers_isolated(self) -> None:
        packs: list[Path] = []
        for name, value in (("alpha", "first"), ("beta", "second")):
            pack = self.root / name
            (pack / "sources").mkdir(parents=True)
            (pack / "helpers").mkdir()
            (pack / "SOURCES.md").write_text("Facts.\n", encoding="utf-8")
            (pack / "helpers" / "shared.py").write_text(
                f"VALUE = {value!r}\n",
                encoding="utf-8",
            )
            (pack / "sources" / "facts.py").write_text(
                "from kat import source\n"
                "from ..helpers.shared import VALUE\n\n"
                "@source(name='facts')\n"
                "def provide():\n"
                "    return VALUE\n",
                encoding="utf-8",
            )
            packs.append(pack.resolve())

        module_roots = [private_source_module_root(pack) for pack in packs]
        self.module_roots.update(module_roots)
        self.assertNotEqual(module_roots[0], module_roots[1])
        self.assertEqual(
            private_source_module_root(packs[0] / "helpers" / ".."),
            module_roots[0],
        )

        loaded = [
            ProductionPack.open(
                name,
                pack,
                profile=SOURCE_OPERATION_PROFILE,
            ).load_source("facts")
            for name, pack, module_root in zip(
                ("alpha", "beta"),
                packs,
                module_roots,
                strict=True,
            )
        ]

        self.assertEqual([source.function() for source in loaded], ["first", "second"])
        self.assertEqual(
            [source.function.__module__ for source in loaded],
            [f"{root}.sources.facts" for root in module_roots],
        )
        self.assertIsNot(
            sys.modules[f"{module_roots[0]}.helpers.shared"],
            sys.modules[f"{module_roots[1]}.helpers.shared"],
        )

    def test_source_inspection_does_not_analyze_unexecuted_imports(self) -> None:
        pack = self.root / "ordinary-import-semantics"
        (pack / "sources").mkdir(parents=True)
        (pack / "SOURCES.md").write_text("Facts.\n", encoding="utf-8")
        (pack / "sources" / "facts.py").write_text(
            "from typing import TYPE_CHECKING\n"
            "from kat import source\n\n"
            "if TYPE_CHECKING:\n"
            "    from kat.pack.helpers import only_for_types\n\n"
            "@source(name='facts')\n"
            "def provide():\n"
            "    return None\n",
            encoding="utf-8",
        )
        root = pack.resolve()
        module_root = private_source_module_root(root)
        self.module_roots.add(module_root)

        opened = ProductionPack.open(
            "ordinary-import-semantics",
            root,
            profile=SOURCE_OPERATION_PROFILE,
        )

        self.assertEqual([entry.interface["name"] for entry in opened.source_entries], ["facts"])


if __name__ == "__main__":
    unittest.main()
