from __future__ import annotations

import json
import multiprocessing
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import uuid

from _kat_runtime import __main__ as runtime_main
from _kat_runtime.pack import (
    PackInspectionError,
    ProductionPack,
    RUN_PROFILE,
    SOURCE_OPERATION_PROFILE,
    SOURCE_RESOLUTION_PROFILE,
    _source_entries,
    _workflow_entries,
    inspect_pack,
)


class RuntimeProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.runtime_cwd = self.root / "unrelated-runtime-cwd"
        self.runtime_cwd.mkdir()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(
        self, request: object
    ) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
        completed, response_path = self.run_runtime_process(request)
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def run_runtime_process(
        self, request: object
    ) -> tuple[subprocess.CompletedProcess[bytes], Path]:
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
                "-I",
                "-B",
                "-X",
                "utf8",
                "-u",
                "-m",
                "_kat_runtime",
                "--request",
                str(request_path),
                "--response",
                str(response_path),
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            cwd=self.runtime_cwd,
            env={
                **os.environ,
                "NO_COLOR": "1",
                "PYTHONPATH": str(self.root / "must-not-import"),
            },
        )
        return completed, response_path

    def write_pack(self) -> Path:
        pack = self.root / "checkout-with-unrelated-name"
        (pack / "workflows" / "nested").mkdir(parents=True)
        (pack / "helpers").mkdir()
        (pack / "tests").mkdir()
        (pack / "helpers" / "rules.py").write_text(
            "def title():\n    return 'Helper title'\n", encoding="utf-8"
        )
        (pack / "workflows" / "nested" / "cpu.py").write_text(
            """import math
from kat import Context, workflow
from kat.pack.helpers.rules import title

@workflow(name="cpu-time", title=title(), parameters={"limit": "Maximum rows"})
def analyze(ctx: Context, *, limit: int = 10):
    \"\"\"Analyze CPU time.\"\"\"
""",
            encoding="utf-8",
        )
        (pack / "tests" / "must_not_import.py").write_text(
            "raise RuntimeError('tests are not production')\n", encoding="utf-8"
        )
        return pack.resolve()

    def test_inspect_pack_returns_complete_workflows_without_writing_source(self) -> None:
        pack = self.write_pack()
        before = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))

        completed, response = self.run_runtime(
            {"operation": "inspect_pack", "pack_name": "stable-name", "pack_path": str(pack)}
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(completed.stdout, b"")
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "source_guide": None,
                    "sources": [],
                    "workflows": [
                        {
                            "name": "cpu-time",
                            "title": "Helper title",
                            "description": "Analyze CPU time.",
                            "parameters": [
                                {
                                    "name": "limit",
                                    "option": "--limit",
                                    "type": "int64",
                                    "required": False,
                                    "description": "Maximum rows",
                                    "default": "10",
                                }
                            ],
                        }
                    ]
                },
            },
        )
        after = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))
        self.assertEqual(after, before)

    def test_inspect_pack_returns_sources_and_guide_without_normalizing_text(self) -> None:
        pack = self.root / "source-pack"
        (pack / "sources" / "nested").mkdir(parents=True)
        (pack / "sources" / "z.py").write_text(
            """from kat import source
@source(name='z_data')
def provide() -> MissingReturn:
    raise AssertionError('inspection must not call Source Entries')
""",
            encoding="utf-8",
        )
        (pack / "sources" / "nested" / "a.py").write_text(
            """from pathlib import Path
from kat import source
@source(name='a_data')
def provide(files: tuple[Path, ...] = (), optional: Path | None = None):
    raise AssertionError('inspection must not call Source Entries')
""",
            encoding="utf-8",
        )
        guide = "# Sources\r\n\r\nKeep trailing space. \r\n"
        (pack / "SOURCES.md").write_bytes(guide.encode("utf-8"))

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "source-pack",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "source_guide": guide,
                    "sources": [
                        {
                            "name": "a_data",
                            "parameters": [
                                {
                                    "name": "files",
                                    "option": "--files",
                                    "type": "path",
                                    "required": False,
                                    "repeatable": True,
                                    "default": [],
                                },
                                {
                                    "name": "optional",
                                    "option": "--optional",
                                    "type": "path",
                                    "required": False,
                                    "default": None,
                                },
                            ],
                        },
                        {"name": "z_data", "parameters": []},
                    ],
                    "workflows": [],
                },
            },
        )

    def test_source_guide_gate_and_operation_profiles_are_explicit(self) -> None:
        pack = self.root / "guide-profiles"
        (pack / "sources").mkdir(parents=True)
        (pack / "workflows").mkdir()
        (pack / "sources" / "facts.py").write_text(
            """from kat import source
@source(name='facts')
def provide():
    return None
""",
            encoding="utf-8",
        )
        root = pack.resolve()

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "guide-profiles",
                "pack_path": str(root),
            }
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertIn("SOURCES.md", response["error"]["causes"][0])

        with self.assertRaises(PackInspectionError):
            ProductionPack.open(
                "guide-profiles",
                root,
                profile=SOURCE_OPERATION_PROFILE,
            )
        # Run scans both entry kinds but deliberately does not read SOURCES.md.
        run_pack = ProductionPack.open("guide-profiles", root, profile=RUN_PROFILE)
        self.assertEqual(
            [entry.interface["name"] for entry in run_pack.source_entries],
            ["facts"],
        )
        resolution_pack = ProductionPack.open(
            "guide-profiles",
            root,
            profile=SOURCE_RESOLUTION_PROFILE,
        )
        self.assertEqual(
            [entry.interface["name"] for entry in resolution_pack.source_entries],
            ["facts"],
        )
        self.assertEqual(resolution_pack.workflow_entries, ())

        (pack / "workflows" / "broken.py").write_text(
            "raise RuntimeError('unrelated Workflow is broken')\n",
            encoding="utf-8",
        )
        (pack / "SOURCES.md").write_text("Facts.\n", encoding="utf-8", newline="")
        source_only = ProductionPack.open(
            "guide-profiles",
            root,
            profile=SOURCE_OPERATION_PROFILE,
        )
        self.assertEqual(
            [entry.interface["name"] for entry in source_only.source_entries],
            ["facts"],
        )
        self.assertEqual(source_only.workflow_entries, ())

    def test_public_pack_import_in_source_helper_fails_before_source_operations(
        self,
    ) -> None:
        pack = self.root / "absolute-source-import"
        (pack / "sources").mkdir(parents=True)
        (pack / "helpers").mkdir()
        (pack / "SOURCES.md").write_text("Facts.\n", encoding="utf-8")
        marker = self.root / "source-provider-called"
        (pack / "sources" / "facts.py").write_text(
            f'''from pathlib import Path
from kat import source
from ..helpers.shared import VALUE

@source(name="facts")
def provide():
    Path({str(marker)!r}).write_text(str(VALUE), encoding="utf-8")
    return None
''',
            encoding="utf-8",
        )
        (pack / "helpers" / "shared.py").write_text(
            "from kat.pack.helpers.value import VALUE\n",
            encoding="utf-8",
        )
        (pack / "helpers" / "value.py").write_text(
            "VALUE = 1\n",
            encoding="utf-8",
        )
        root = pack.resolve()
        common = {
            "pack_name": "absolute-source-import",
            "pack_path": str(root),
            "source_name": "facts",
            "arguments": [],
            "argument_base": str(self.root.resolve()),
        }

        requests: list[dict[str, object]] = [
            {"operation": "bind_source", **common},
        ]
        export = self.root / "absolute-source-import-export"
        export.mkdir()
        requests.append(
            {
                "operation": "materialize_source",
                **common,
                "tables": [],
                "export_path": str(export.resolve()),
            }
        )
        for request in requests:
            with self.subTest(operation=request["operation"]):
                completed, response = self.run_runtime(request)
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "failure", response)
                self.assertEqual(response["error"]["message"], "PACK inspection failed")
                cause = response["error"]["causes"][0]
                self.assertIn("No module named 'kat.pack'", cause)
                self.assertFalse(marker.exists())

    def test_existing_source_guide_is_read_even_without_source_entries(self) -> None:
        pack = self.root / "guide-only"
        pack.mkdir()
        guide = "Guide without entries.\r\n"
        (pack / "SOURCES.md").write_bytes(guide.encode("utf-8"))

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "guide-only",
                "pack_path": str(pack.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "source_guide": guide,
                    "sources": [],
                    "workflows": [],
                },
            },
        )

        (pack / "SOURCES.md").write_bytes(b"\xff")
        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "guide-only",
                "pack_path": str(pack.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertIn("valid UTF-8", response["error"]["causes"][0])

    def test_materialize_enumerates_property_table_names_once(self) -> None:
        pack = self.root / "counted-source"
        (pack / "sources").mkdir(parents=True)
        (pack / "SOURCES.md").write_text("Counted source.\n", encoding="utf-8")
        marker = self.root / "table-names-calls.txt"
        (pack / "sources" / "counted.py").write_text(
            f'''from pathlib import Path
from datafusion.catalog import SchemaProvider, Table
import pyarrow as pa
import pyarrow.dataset as ds
from kat import source

MARKER = Path({str(marker)!r})

class CountedSchema(SchemaProvider):
    @property
    def table_names(self):
        calls = int(MARKER.read_text(encoding="utf-8")) if MARKER.exists() else 0
        MARKER.write_text(str(calls + 1), encoding="utf-8")
        return ("events",)

    def table_exist(self, name):
        return name == "events"

    def table(self, name):
        if name != "events":
            return None
        return Table(ds.dataset(pa.table({{"value": [1, 2]}})))

@source(name="counted")
def provide():
    return CountedSchema()
''',
            encoding="utf-8",
        )
        export = self.root / "materialized"
        export.mkdir()

        completed, response = self.run_runtime(
            {
                "operation": "materialize_source",
                "pack_name": "counted-source",
                "pack_path": str(pack.resolve()),
                "source_name": "counted",
                "arguments": [],
                "argument_base": str(self.root.resolve()),
                "tables": [],
                "export_path": str(export.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response, {"status": "success", "result": {"tables": ["events"]}})
        self.assertEqual(marker.read_text(encoding="utf-8"), "1")
        self.assertTrue((export / "events.parquet").is_file())

    def test_materialize_accepts_an_official_datafusion_schema(self) -> None:
        pack = self.root / "official-schema-source"
        (pack / "sources").mkdir(parents=True)
        (pack / "SOURCES.md").write_text("Official schema source.\n", encoding="utf-8")
        (pack / "sources" / "official.py").write_text(
            '''from datafusion.catalog import Schema, Table
import pyarrow as pa
import pyarrow.dataset as ds
from kat import source

@source(name="official")
def provide():
    schema = Schema.memory_schema()
    schema.register_table(
        "events",
        Table(ds.dataset(pa.table({"value": [2, 1]}))),
    )
    return schema
''',
            encoding="utf-8",
        )
        export = self.root / "official-schema-materialized"
        export.mkdir()

        completed, response = self.run_runtime(
            {
                "operation": "materialize_source",
                "pack_name": "official-schema-source",
                "pack_path": str(pack.resolve()),
                "source_name": "official",
                "arguments": [],
                "argument_base": str(self.root.resolve()),
                "tables": [],
                "export_path": str(export.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response, {"status": "success", "result": {"tables": ["events"]}})
        self.assertTrue((export / "events.parquet").is_file())

    def test_source_entry_tree_uses_the_same_deterministic_rules(self) -> None:
        pack = self.root / "source-tree"
        (pack / "sources" / "nested").mkdir(parents=True)
        (pack / "sources" / "z.py").write_text("# source\n", encoding="utf-8")
        (pack / "sources" / "a.py").write_text("# source\n", encoding="utf-8")
        self.assertEqual(
            [path.relative_to(pack).as_posix() for path, _ in _source_entries(pack.resolve())],
            ["sources/a.py", "sources/z.py"],
        )
        (pack / "sources" / "nested" / "__init__.py").write_text(
            "# forbidden\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "sources/nested/__init__.py"):
            _source_entries(pack.resolve())
        self.assertFalse(any(path.name == "__pycache__" for path in pack.rglob("*")))

    def test_source_entry_cannot_register_a_workflow_instead(self) -> None:
        pack = self.root / "cross-kind-entry"
        (pack / "sources").mkdir(parents=True)
        (pack / "sources" / "wrong.py").write_text(
            '''from kat import Context, workflow

@workflow(name="wrong-kind", title="Wrong kind")
def provide(ctx: Context):
    """This is not a Source Entry."""
''',
            encoding="utf-8",
        )
        (pack / "SOURCES.md").write_text("Wrong kind.\n", encoding="utf-8")

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "cross-kind-entry",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertIn(
            "must register exactly one Source defined by that module",
            response["error"]["causes"][0],
        )

    def test_workflow_directory_state_errors_are_not_treated_as_absence(self) -> None:
        missing = self.root / "missing-workflows"
        self.assertEqual(_workflow_entries(missing), [])

        with mock.patch.object(
            Path,
            "lstat",
            side_effect=PermissionError("workflow directory state is unavailable"),
        ):
            with self.assertRaisesRegex(OSError, "failed to scan PACK workflows directory"):
                _workflow_entries(self.root)

    def test_non_ordinary_workflow_paths_are_ignored(self) -> None:
        with (
            mock.patch.object(Path, "lstat", return_value=mock.Mock(st_mode=stat.S_IFLNK)),
            mock.patch.object(Path, "stat") as target_stat,
        ):
            self.assertEqual(_workflow_entries(self.root), [])
            target_stat.assert_not_called()

        file_root = self.root / "file-workflows"
        file_root.mkdir()
        (file_root / "workflows").write_text("not a directory", encoding="utf-8")
        self.assertEqual(_workflow_entries(file_root), [])

        dangling_root = self.root / "dangling-workflows"
        dangling_root.mkdir()
        try:
            (dangling_root / "workflows").symlink_to(
                dangling_root / "missing-target", target_is_directory=True
            )
        except OSError:
            return
        self.assertEqual(_workflow_entries(dangling_root), [])

        linked_entry_root = self.root / "linked-entry"
        workflow_directory = linked_entry_root / "workflows"
        workflow_directory.mkdir(parents=True)
        linked_source = linked_entry_root / "outside.py"
        linked_source.write_text("raise AssertionError('must not import')\n", encoding="utf-8")
        try:
            (workflow_directory / "linked.py").symlink_to(linked_source)
        except OSError:
            return
        self.assertEqual(_workflow_entries(linked_entry_root), [])

    def test_diagnostic_ignores_hostile_syntax_error_metadata(self) -> None:
        pack = self.root / "hostile-syntax-metadata"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "hostile.py").write_text(
            "class HostileSyntaxError(SyntaxError):\n"
            "    @property\n"
            "    def __cause__(self):\n"
            "        raise SystemExit('dynamic cause metadata')\n"
            "    @property\n"
            "    def __context__(self):\n"
            "        raise SystemExit('dynamic context metadata')\n"
            "    @property\n"
            "    def __suppress_context__(self):\n"
            "        raise SystemExit('dynamic suppression metadata')\n"
            "    def __getattribute__(self, attribute):\n"
            "        if attribute in {'filename', 'lineno', 'offset', 'end_lineno', 'end_offset'}:\n"
            "            raise SystemExit('dynamic syntax metadata')\n"
            "        return super().__getattribute__(attribute)\n"
            "raise HostileSyntaxError('author syntax failure')\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "hostile-syntax-metadata",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK inspection failed")
        self.assertNotIn("location", response["error"])

    def test_diagnostic_omits_pack_root_syntax_error_location(self) -> None:
        pack = self.root / "root-syntax-location"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "root_location.py").write_text(
            "from pathlib import Path\n"
            "root = str(Path(__file__).parent.parent)\n"
            "raise SyntaxError('root location', (root, 1, 1, 'x', 1, 2))\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "root-syntax-location",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK inspection failed")
        self.assertNotIn("location", response["error"])

    @unittest.skipUnless(sys.version_info >= (3, 14), "requires Python 3.14 annotations")
    def test_python_314_resolves_only_supported_input_annotations(self) -> None:
        pack = self.root / "python-314-annotations"
        (pack / "workflows").mkdir(parents=True)
        entry = pack / "workflows" / "annotation.py"
        entry.write_text(
            "from typing import Optional\n"
            "from kat import Context, workflow\n"
            "@workflow(name='annotation', title='Annotation', parameters={'value': 'Value'})\n"
            "def annotation(ctx: Context, value: Optional['str'] = None) -> MissingReturn:\n"
            "    \"\"\"Inspect annotations.\"\"\"\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "python-314-annotations",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success")
        parameter = response["result"]["workflows"][0]["parameters"][0]
        self.assertEqual(parameter["default"], None)

        entry.write_text(
            entry.read_text(encoding="utf-8").replace("MissingReturn", "1 / 0"),
            encoding="utf-8",
        )
        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "python-314-annotations",
                "pack_path": str(pack.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK inspection failed")

    def test_invalid_request_and_invalid_entry_return_strict_private_failure(self) -> None:
        for request in [
            {"operation": "inspect_pack", "pack_name": "alpha", "pack_path": "relative", "extra": True},
            {"operation": "inspect_pack", "pack_name": "", "pack_path": str(self.root.resolve())},
            {"operation": "inspect_pack", "pack_name": "alpha", "pack_path": "relative"},
            {
                "operation": "inspect_pack",
                "pack_name": "alpha",
                "pack_path": str((self.root / "private-missing-pack").resolve()),
            },
        ]:
            with self.subTest(request=request):
                completed, response = self.run_runtime(request)
                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure")
                self.assertEqual(response["error"]["message"], "Runtime Request is invalid")
                self.assertEqual(
                    response["error"]["help"],
                    "Use a compatible KAT CLI and Runtime deployment",
                )
                self.assertEqual(set(response), {"status", "error"})
                self.assertEqual(set(response["error"]), {"message", "causes", "help"})
                self.assertNotIn("private-missing-pack", json.dumps(response))

        pack = self.write_pack()
        (pack / "workflows" / "broken.py").write_text(
            "from kat import Context, workflow\n"
            "@workflow(name='broken', title='Broken')\n"
            "def broken(ctx: Context):\n    pass\n",
            encoding="utf-8",
        )
        completed, response = self.run_runtime(
            {"operation": "inspect_pack", "pack_name": "stable-name", "pack_path": str(pack)}
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertNotIn("result", response)
        self.assertTrue(response["error"]["causes"])

        syntax_pack = self.root / "syntax-error"
        (syntax_pack / "workflows").mkdir(parents=True)
        (syntax_pack / "workflows" / "broken.py").write_text(
            "def broken(:\n    pass\n", encoding="utf-8"
        )
        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "syntax-error",
                "pack_path": str(syntax_pack.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        location = response["error"]["location"]
        self.assertEqual(location["source"], "workflows/broken.py")
        self.assertGreater(location["start"]["line"], 0)
        self.assertGreater(location["start"]["column"], 0)
        self.assertGreaterEqual(
            (location["end"]["line"], location["end"]["column"]),
            (location["start"]["line"], location["start"]["column"]),
        )

    def test_declarative_entry_tree_rejects_initializers_conflicts_and_entry_imports(self) -> None:
        cases: list[tuple[str, dict[str, str]]] = [
            (
                "initializer",
                {
                    "workflows/__init__.py": "raise AssertionError('must not import')\n",
                },
            ),
            (
                "module-package-conflict",
                {
                    "workflows/cpu.py": "# entry\n",
                    "workflows/cpu/nested.py": "# entry\n",
                },
            ),
            (
                "entry-import",
                {
                    "workflows/a.py": """from kat import Context, workflow
@workflow(name='a', title='A')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.workflows.a import analyze
@workflow(name='b', title='B')
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-dynamic-discarded",
                {
                    "workflows/a.py": """from kat import Context, workflow
@workflow(name='a', title='A')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import importlib
from kat import Context, workflow
importlib.import_module('kat.pack.workflows.a')
@workflow(name='b', title='B')
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-cached-import-module",
                {
                    "helpers/cached.py": """import importlib
import_entry = importlib.import_module
""",
                    "workflows/a.py": """from kat import Context, workflow
from kat.pack.helpers import cached
SHARED = "not a helper"
@workflow(name='a', title='A')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.helpers import cached
title = cached.import_entry('kat.pack.workflows.a').SHARED
@workflow(name='b', title=title)
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-parent-namespace",
                {
                    "workflows/a.py": """from kat import Context, workflow
SHARED = "not a helper"
@workflow(name='a', title='A')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import kat
from kat import Context, workflow
title = kat.pack.workflows.a.SHARED
@workflow(name='b', title=title)
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
        ]
        for case, files in cases:
            with self.subTest(case=case):
                pack = self.root / case
                for relative, contents in files.items():
                    target = pack / relative
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(contents, encoding="utf-8")
                completed, response = self.run_runtime(
                    {"operation": "inspect_pack", "pack_name": case, "pack_path": str(pack.resolve())}
                )
                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure")
                self.assertNotIn("result", response)

    def test_entry_worker_preserves_standard_module_cache(self) -> None:
        pack = self.root / "standard-module-cache"
        (pack / "helpers").mkdir(parents=True)
        (pack / "workflows").mkdir()
        (pack / "helpers" / "identity.py").write_text(
            "value = object()\n", encoding="utf-8"
        )
        (pack / "workflows" / "cached.py").write_text(
            """import importlib
from kat import Context, workflow

first = importlib.import_module("kat.pack.helpers.identity")
second = importlib.import_module("kat.pack.helpers.identity")
if first is not second or first.value is not second.value:
    raise RuntimeError("standard module cache was not preserved")

@workflow(name="cached", title="Cached")
def analyze(ctx: Context):
    \"\"\"Inspect the standard module cache.\"\"\"
""",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "standard-module-cache",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success")

    def test_inspection_reaps_its_spawn_worker_before_returning(self) -> None:
        pack = self.root / "reaped-worker"
        (pack / "workflows").mkdir(parents=True)
        worker_pid = self.root / "inspection-worker.pid"
        (pack / "workflows" / "worker.py").write_text(
            f"""import os
from pathlib import Path
from kat import Context, workflow

Path({str(worker_pid)!r}).write_text(str(os.getpid()), encoding="utf-8")

@workflow(name="worker", title="Worker")
def analyze(ctx: Context):
    \"\"\"Record the real inspection worker.\"\"\"
""",
            encoding="utf-8",
        )
        before = {child.pid for child in multiprocessing.active_children()}

        result = inspect_pack("reaped-worker", pack.resolve())

        pid = int(worker_pid.read_text(encoding="utf-8"))
        after = {child.pid for child in multiprocessing.active_children()}
        self.assertEqual([workflow["name"] for workflow in result.workflows], ["worker"])
        self.assertNotIn(pid, after)
        self.assertEqual(after, before)

    def test_declarative_entry_errors_follow_portable_relative_path_order(self) -> None:
        pack = self.root / "ordered-errors"
        for directory in ("z", "a"):
            initializer = pack / "workflows" / directory / "__init__.py"
            initializer.parent.mkdir(parents=True)
            initializer.write_text("raise AssertionError('must not import')\n", encoding="utf-8")

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "ordered-errors",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertIn("workflows/a/__init__.py", response["error"]["causes"][0])

    def test_empty_exception_chain_does_not_invent_a_cause(self) -> None:
        pack = self.root / "empty-exception"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "broken.py").write_text(
            "raise RuntimeError()\n", encoding="utf-8"
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "empty-exception",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(set(response["error"]), {"message", "help"})

    def test_pack_exception_chain_is_safe_bounded_and_respects_suppression(self) -> None:
        cases = {
            "unsafe-string": (
                """class UnsafeStringError(Exception):
    def __str__(self):
        raise RuntimeError('string rendering failed')
raise UnsafeStringError()
""",
                [],
            ),
            "cyclic-chain": (
                """first = RuntimeError('first')
second = ValueError('second')
first.__cause__ = second
second.__cause__ = first
raise first
""",
                ["first", "second"],
            ),
            "suppressed-context": (
                """try:
    raise ValueError('hidden context')
except ValueError:
    raise RuntimeError('visible cause') from None
""",
                ["visible cause"],
            ),
        }
        for name, (source, expected_causes) in cases.items():
            with self.subTest(name=name):
                pack = self.root / name
                (pack / "workflows").mkdir(parents=True)
                (pack / "workflows" / "broken.py").write_text(source, encoding="utf-8")

                completed, response = self.run_runtime(
                    {
                        "operation": "inspect_pack",
                        "pack_name": name,
                        "pack_path": str(pack.resolve()),
                    }
                )

                self.assertEqual(
                    completed.returncode, 0, completed.stderr.decode(errors="replace")
                )
                self.assertEqual(response["status"], "failure")
                self.assertEqual(response["error"]["message"], "PACK inspection failed")
                self.assertEqual(response["error"].get("causes", []), expected_causes)

    def test_pack_system_exit_is_a_pack_failure(self) -> None:
        pack = self.root / "system-exit"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "broken.py").write_text(
            "raise SystemExit('PACK requested exit')\n", encoding="utf-8"
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_pack",
                "pack_name": "system-exit",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK inspection failed")
        self.assertIn("PACK requested exit", response["error"]["causes"])

    def test_author_controlled_inspection_errors_are_pack_failures(self) -> None:
        cases = {
            "annotation-system-exit": """from __future__ import annotations
from kat import Context, workflow
@workflow(name='annotation-exit', title='Annotation exit', parameters={'value': 'Value'})
def analyze(ctx: Context, value: __import__('sys').exit('annotation requested exit')):
    \"\"\"Inspect an annotation.\"\"\"
""",
            "callable-default-runtime-error": """from kat import Context, workflow
def invalid_default():
    raise RuntimeError('callable default failed')
@workflow(name='default-error', title='Default error', parameters={'value': 'Value'})
def analyze(ctx: Context, value: str = invalid_default):
    \"\"\"Inspect a callable default.\"\"\"
""",
        }
        for name, source in cases.items():
            with self.subTest(name=name):
                pack = self.root / name
                (pack / "workflows").mkdir(parents=True)
                (pack / "workflows" / "broken.py").write_text(source, encoding="utf-8")

                completed, response = self.run_runtime(
                    {
                        "operation": "inspect_pack",
                        "pack_name": name,
                        "pack_path": str(pack.resolve()),
                    }
                )

                self.assertEqual(
                    completed.returncode, 0, completed.stderr.decode(errors="replace")
                )
                self.assertEqual(response["status"], "failure")
                self.assertEqual(response["error"]["message"], "PACK inspection failed")

    def test_unexpected_runtime_error_exits_without_a_pack_failure_response(self) -> None:
        request_path = self.root / "request.json"
        response_path = self.root / "response.json"
        request_path.write_text(
            json.dumps(
                {
                    "operation": "inspect_pack",
                    "pack_name": "stable-name",
                    "pack_path": str(self.root.resolve()),
                }
            ),
            encoding="utf-8",
        )
        arguments = [
            "_kat_runtime",
            "--request",
            str(request_path),
            "--response",
            str(response_path),
        ]

        with (
            mock.patch.object(sys, "argv", arguments),
            mock.patch.object(
                runtime_main,
                "inspect_pack",
                side_effect=AttributeError("injected Runtime implementation failure"),
            ),
            self.assertRaisesRegex(AttributeError, "Runtime implementation failure"),
        ):
            runtime_main.main()

        self.assertFalse(response_path.exists())

    def test_entry_worker_crash_exits_without_a_pack_failure_response(self) -> None:
        pack = self.root / "worker-crash"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "crash.py").write_text(
            "import os\nos._exit(17)\n", encoding="utf-8"
        )

        completed, response_path = self.run_runtime_process(
            {
                "operation": "inspect_pack",
                "pack_name": "worker-crash",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(response_path.exists())
        self.assertIn(
            "Workflow inspection worker exited without a result",
            completed.stderr.decode(errors="replace"),
        )

    def test_run_worker_crash_exits_without_a_workflow_failure_response(self) -> None:
        pack = self.root / "run-worker-crash"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "crash.py").write_text(
            "import os\nos._exit(17)\n", encoding="utf-8"
        )
        candidate_id = str(uuid.uuid7())
        candidate = self.root / candidate_id
        candidate.mkdir()

        completed, response_path = self.run_runtime_process(
            {
                "operation": "run_workflow",
                "pack_name": "run-worker-crash",
                "pack_path": str(pack.resolve()),
                "pack_paths": {"run-worker-crash": str(pack.resolve())},
                "workflow_name": "analyze",
                "arguments": [],
                "candidate_id": candidate_id,
                "candidate_path": str(candidate.resolve()),
            }
        )

        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(response_path.exists())
        self.assertIn(
            "Workflow inspection worker exited without a result",
            completed.stderr.decode(errors="replace"),
        )


if __name__ == "__main__":
    unittest.main()
