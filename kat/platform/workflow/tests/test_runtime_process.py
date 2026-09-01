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
from _kat_runtime.pack import _workflow_entries, inspect_workflow


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
        (pack / "datasources").mkdir()
        (pack / "knowledge" / "workflows").mkdir(parents=True)
        (pack / "tests").mkdir()
        (pack / "helpers" / "rules.py").write_text(
            "def title():\n    return 'Helper title'\n", encoding="utf-8"
        )
        (pack / "datasources" / "titles.py").write_text(
            "def decorate(value):\n    return f'Datasource {value}'\n",
            encoding="utf-8",
        )
        (pack / "datasources" / "must_not_import.py").write_text(
            "raise RuntimeError('unreferenced Datasources must not be scanned')\n",
            encoding="utf-8",
        )
        (pack / "workflows" / "nested" / "cpu.py").write_text(
            """import math
from kat import Context, workflow
from kat.pack.datasources.titles import decorate
from kat.pack.helpers.rules import title

@workflow(name="cpu-time", description=decorate(title()), parameters={"limit": "Maximum rows"}, guide="workflows/cpu-time.md")
def analyze(ctx: Context, *, limit: int = 10):
    \"\"\"Analyze CPU time.\"\"\"
""",
            encoding="utf-8",
        )
        (pack / "knowledge" / "workflows" / "cpu-time.md").write_text(
            "# CPU time\r\n\r\nInspect the largest rows first.\r\n",
            encoding="utf-8",
            newline="",
        )
        (pack / "tests" / "must_not_import.py").write_text(
            "raise RuntimeError('tests are not production')\n", encoding="utf-8"
        )
        return pack.resolve()

    def test_inspect_workflow_lists_then_returns_one_detail_without_writing_source(self) -> None:
        pack = self.write_pack()
        before = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))

        listed, list_response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "pack_name": "stable-name",
                "pack_path": str(pack),
                "workflow_name": None,
            }
        )
        selected, detail_response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "pack_name": "stable-name",
                "pack_path": str(pack),
                "workflow_name": "cpu-time",
            }
        )

        self.assertEqual(listed.returncode, 0, listed.stderr.decode(errors="replace"))
        self.assertEqual(selected.returncode, 0, selected.stderr.decode(errors="replace"))
        self.assertEqual(listed.stdout, b"")
        self.assertEqual(selected.stdout, b"")
        self.assertEqual(
            list_response,
            {
                "status": "success",
                "result": {
                    "workflows": [
                        {
                            "name": "cpu-time",
                            "description": "Datasource Helper title",
                        }
                    ]
                },
            },
        )
        self.assertEqual(
            detail_response,
            {
                "status": "success",
                "result": {
                    "workflow": {
                        "name": "cpu-time",
                        "description": "Datasource Helper title",
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
                        "guide": "# CPU time\r\n\r\nInspect the largest rows first.\r\n",
                    }
                },
            },
        )
        after = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))
        self.assertEqual(after, before)
        self.assertFalse(any(path.name == "__pycache__" for path in pack.rglob("*")))

    def test_workflow_guide_is_nullable_and_the_complete_tree_is_atomic(self) -> None:
        pack = self.root / "workflow-guides"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "plain.py").write_text(
            "from kat import Context, workflow\n"
            "@workflow(name='plain', description='No guide is required.')\n"
            "def analyze(ctx: Context):\n    pass\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "pack_name": "workflow-guides",
                "pack_path": str(pack.resolve()),
                "workflow_name": "plain",
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success")
        self.assertIsNone(response["result"]["workflow"]["guide"])

        (pack / "workflows" / "broken.py").write_text(
            "from kat import Context, workflow\n"
            "@workflow(name='broken', description='Broken guide.', guide='workflows/missing.md')\n"
            "def analyze(ctx: Context):\n    pass\n",
            encoding="utf-8",
        )
        for workflow_name in (None, "plain"):
            with self.subTest(workflow_name=workflow_name):
                completed, response = self.run_runtime(
                    {
                        "operation": "inspect_workflow",
                        "pack_name": "workflow-guides",
                        "pack_path": str(pack.resolve()),
                        "workflow_name": workflow_name,
                    }
                )
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "failure")
                self.assertNotIn("result", response)

    def test_inspect_provider_lists_and_selects_recursive_declarations(self) -> None:
        pack = self.root / "provider-pack"
        (pack / "datasources" / "nested").mkdir(parents=True)
        (pack / "knowledge" / "providers").mkdir(parents=True)
        (pack / "datasources" / "__init__.py").write_text(
            "# Standard package initializer.\n", encoding="utf-8"
        )
        (pack / "datasources" / "nested" / "__init__.py").write_text(
            "# Nested package initializer.\n", encoding="utf-8"
        )
        (pack / "datasources" / "helpers.py").write_text(
            "DESCRIPTION = 'Parse ftrace text.'\n", encoding="utf-8"
        )
        (pack / "datasources" / "postgresql.py").write_text(
            "from kat import provider\n"
            "@provider(name='postgresql', description='Query PostgreSQL.', guide='providers/postgresql.md')\n"
            "class PostgreSQLProvider:\n"
            "    def __init__(self):\n"
            "        raise AssertionError('inspection must not instantiate Providers')\n",
            encoding="utf-8",
        )
        (pack / "datasources" / "nested" / "ftrace.py").write_text(
            "from kat import provider\n"
            "from kat.pack.datasources.helpers import DESCRIPTION\n"
            "from kat.pack.datasources.postgresql import PostgreSQLProvider\n"
            "@provider(name='ftrace', description=DESCRIPTION, guide='providers/ftrace.md')\n"
            "class FtraceProvider:\n"
            "    pass\n"
            "@provider(name='ftrace-memory', description='Read memory data.', guide='providers/ftrace.md')\n"
            "class FtraceMemoryProvider:\n"
            "    pass\n"
            "class UndecoratedSubclass(PostgreSQLProvider):\n"
            "    pass\n",
            encoding="utf-8",
        )
        postgresql_guide = "# PostgreSQL\r\n\r\nUse remote SQL.\r\n"
        (pack / "knowledge" / "providers" / "postgresql.md").write_text(
            postgresql_guide, encoding="utf-8", newline=""
        )
        ftrace_guide = "# Ftrace\n\nDecode a local text file.\n"
        (pack / "knowledge" / "providers" / "ftrace.md").write_text(
            ftrace_guide, encoding="utf-8", newline=""
        )
        before = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "provider-pack",
                "pack_path": str(pack.resolve()),
                "provider_name": None,
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "providers": [
                        {"name": "ftrace", "description": "Parse ftrace text."},
                        {
                            "name": "ftrace-memory",
                            "description": "Read memory data.",
                        },
                        {"name": "postgresql", "description": "Query PostgreSQL."},
                    ]
                },
            },
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "provider-pack",
                "pack_path": str(pack.resolve()),
                "provider_name": "postgresql",
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "provider": {
                        "name": "postgresql",
                        "description": "Query PostgreSQL.",
                        "module": "kat.pack.datasources.postgresql",
                        "qualname": "PostgreSQLProvider",
                        "guide": postgresql_guide,
                    }
                },
            },
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "provider-pack",
                "pack_path": str(pack.resolve()),
                "provider_name": "missing",
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertNotIn("result", response)
        after = sorted(path.relative_to(pack).as_posix() for path in pack.rglob("*"))
        self.assertEqual(after, before)
        self.assertFalse(any(path.name == "__pycache__" for path in pack.rglob("*")))

    def test_inspect_provider_validates_the_complete_tree_atomically(self) -> None:
        cases = {
            "import-error": (
                "raise RuntimeError('provider import failed')\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "duplicate-name": (
                "from kat import provider\n"
                "@provider(name='valid', description='Duplicate', guide='providers/valid.md')\n"
                "class Duplicate:\n    pass\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "invalid-declaration": (
                "class InvalidProvider:\n"
                "    __kat_provider__ = object()\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "invalid-name": (
                "from kat import provider\n"
                "@provider(name=' invalid ', description='Invalid', guide='providers/valid.md')\n"
                "class InvalidName:\n    pass\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "local-class": (
                "from kat import provider\n"
                "def make_provider():\n"
                "    @provider(name='local', description='Local', guide='providers/valid.md')\n"
                "    class LocalProvider:\n"
                "        pass\n"
                "    return LocalProvider\n"
                "LocalProvider = make_provider()\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "deleted-class-name": (
                "from kat import provider\n"
                "@provider(name='aliased', description='Aliased', guide='providers/valid.md')\n"
                "class OriginalProvider:\n"
                "    pass\n"
                "ProviderAlias = OriginalProvider\n"
                "del OriginalProvider\n",
                "providers/valid.md",
                b"# Valid\n",
            ),
            "absolute-guide": ("# helper\n", str((self.root / "outside.md").resolve()), b"# Outside\n"),
            "traversal-guide": ("# helper\n", "../outside.md", b"# Outside\n"),
            "wrong-suffix": ("# helper\n", "providers/valid.txt", b"# Valid\n"),
            "missing-guide": ("# helper\n", "providers/missing.md", None),
            "directory-guide": ("# helper\n", "providers/valid.md", b"# Valid\n"),
            "wrong-category": ("# helper\n", "workflows/valid.md", b"# Valid\n"),
            "empty-guide": ("# helper\n", "providers/valid.md", b""),
            "invalid-utf8": ("# helper\n", "providers/valid.md", b"\xff"),
        }
        for name, (second_source, guide_ref, guide_contents) in cases.items():
            with self.subTest(name=name):
                pack = self.root / name
                (pack / "datasources").mkdir(parents=True)
                (pack / "knowledge" / "providers").mkdir(parents=True)
                (pack / "datasources" / "valid.py").write_text(
                    "from kat import provider\n"
                    f"@provider(name='valid', description='Valid', guide={guide_ref!r})\n"
                    "class ValidProvider:\n    pass\n",
                    encoding="utf-8",
                )
                (pack / "datasources" / "second.py").write_text(
                    second_source, encoding="utf-8"
                )
                if guide_contents is not None:
                    guide_target = pack / "knowledge" / "providers" / "valid.md"
                    if name == "wrong-suffix":
                        guide_target = guide_target.with_suffix(".txt")
                    elif name == "wrong-category":
                        guide_target = pack / "knowledge" / "workflows" / "valid.md"
                        guide_target.parent.mkdir(parents=True)
                    if name == "directory-guide":
                        guide_target.mkdir()
                    else:
                        guide_target.write_bytes(guide_contents)
                if name in {"absolute-guide", "traversal-guide"}:
                    (self.root / "outside.md").write_text("# Outside\n", encoding="utf-8")

                completed, response = self.run_runtime(
                    {
                        "operation": "inspect_provider",
                        "pack_name": name,
                        "pack_path": str(pack.resolve()),
                        "provider_name": None,
                    }
                )

                self.assertEqual(
                    completed.returncode, 0, completed.stderr.decode(errors="replace")
                )
                self.assertEqual(response["status"], "failure")
                self.assertNotIn("result", response)
                self.assertEqual(response["error"]["message"], "Provider inspection failed")

        outside = self.root / "outside-knowledge"
        (outside / "providers").mkdir(parents=True)
        (outside / "providers" / "valid.md").write_text(
            "# Outside\n", encoding="utf-8"
        )
        for name, link_root in (
            ("linked-guide-outside", False),
            ("linked-knowledge-outside", True),
        ):
            with self.subTest(name=name):
                pack = self.root / name
                (pack / "datasources").mkdir(parents=True)
                (pack / "datasources" / "provider.py").write_text(
                    "from kat import provider\n"
                    "@provider(name='valid', description='Valid', guide='providers/valid.md')\n"
                    "class ValidProvider:\n    pass\n",
                    encoding="utf-8",
                )
                try:
                    if link_root:
                        (pack / "knowledge").symlink_to(outside, target_is_directory=True)
                    else:
                        (pack / "knowledge" / "providers").mkdir(parents=True)
                        (pack / "knowledge" / "providers" / "valid.md").symlink_to(
                            outside / "providers" / "valid.md"
                        )
                except OSError:
                    continue

                completed, response = self.run_runtime(
                    {
                        "operation": "inspect_provider",
                        "pack_name": name,
                        "pack_path": str(pack.resolve()),
                        "provider_name": None,
                    }
                )

                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "failure")
                self.assertNotIn("result", response)

        pack = self.root / "unselected-invalid-guide"
        (pack / "datasources").mkdir(parents=True)
        (pack / "knowledge" / "providers").mkdir(parents=True)
        (pack / "knowledge" / "providers" / "valid.md").write_text(
            "# Valid\n", encoding="utf-8"
        )
        (pack / "datasources" / "providers.py").write_text(
            "from kat import provider\n"
            "@provider(name='valid', description='Valid', guide='providers/valid.md')\n"
            "class ValidProvider:\n    pass\n"
            "@provider(name='broken', description='Broken', guide='providers/missing.md')\n"
            "class BrokenProvider:\n    pass\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "unselected-invalid-guide",
                "pack_path": str(pack.resolve()),
                "provider_name": "valid",
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertNotIn("result", response)

    def test_inspect_provider_does_not_resolve_qualname_through_user_code(self) -> None:
        pack = self.root / "dynamic-qualname"
        marker = self.root / "dynamic-lookup-ran"
        (pack / "datasources").mkdir(parents=True)
        (pack / "knowledge" / "providers").mkdir(parents=True)
        (pack / "knowledge" / "providers" / "dynamic.md").write_text(
            "# Dynamic\n", encoding="utf-8"
        )
        (pack / "datasources" / "dynamic.py").write_text(
            "from pathlib import Path\n"
            "from kat import provider\n"
            "class DynamicMeta(type):\n"
            "    def __getattribute__(cls, name):\n"
            "        if name == 'DynamicProvider':\n"
            f"            Path({str(marker)!r}).write_text('ran', encoding='utf-8')\n"
            "            return Provider\n"
            "        return super().__getattribute__(name)\n"
            "class Container(metaclass=DynamicMeta):\n"
            "    pass\n"
            "@provider(name='dynamic', description='Dynamic', guide='providers/dynamic.md')\n"
            "class Provider:\n"
            "    pass\n"
            "Provider.__qualname__ = 'Container.DynamicProvider'\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "dynamic-qualname",
                "pack_path": str(pack.resolve()),
                "provider_name": None,
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertFalse(marker.exists())

    def test_inspect_provider_request_is_strict(self) -> None:
        for request in (
            {
                "operation": "inspect_provider",
                "pack_name": "alpha",
                "pack_path": str(self.root.resolve()),
                "extra": True,
            },
            {
                "operation": "inspect_provider",
                "pack_name": "alpha",
                "pack_path": str(self.root.resolve()),
            },
            {
                "operation": "inspect_provider",
                "pack_name": "alpha",
                "pack_path": str(self.root.resolve()),
                "provider_name": "",
            },
            {
                "operation": "inspect_provider",
                "pack_name": "alpha",
                "pack_path": str(self.root.resolve()),
                "provider_name": 1,
            },
        ):
            with self.subTest(request=request):
                completed, response = self.run_runtime(request)
                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure")
                self.assertEqual(response["error"]["message"], "Runtime Request is invalid")

        completed, response = self.run_runtime(
            {
                "operation": "inspect_provider",
                "pack_name": "empty",
                "pack_path": str(self.root.resolve()),
                "provider_name": None,
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(
            response,
            {"status": "success", "result": {"providers": []}},
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
            "@workflow(name='annotation', description='Inspect annotations.', parameters={'value': 'Value'})\n"
            "def annotation(ctx: Context, value: Optional['str'] = None) -> MissingReturn:\n"
            "    \"\"\"Inspect annotations.\"\"\"\n",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "workflow_name": "annotation",
                "pack_name": "python-314-annotations",
                "pack_path": str(pack.resolve()),
            }
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success")
        parameter = response["result"]["workflow"]["parameters"][0]
        self.assertEqual(parameter["default"], None)

        entry.write_text(
            entry.read_text(encoding="utf-8").replace("MissingReturn", "1 / 0"),
            encoding="utf-8",
        )
        completed, response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "workflow_name": "annotation",
                "pack_name": "python-314-annotations",
                "pack_path": str(pack.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK inspection failed")

    def test_invalid_request_and_invalid_entry_return_strict_private_failure(self) -> None:
        for request in [
            {"operation": "inspect_workflow", "pack_name": "alpha", "pack_path": "relative", "workflow_name": None, "extra": True},
            {"operation": "inspect_workflow", "pack_name": "", "pack_path": str(self.root.resolve()), "workflow_name": None},
            {"operation": "inspect_workflow", "pack_name": "alpha", "pack_path": "relative", "workflow_name": None},
            {
                "operation": "inspect_workflow",
                "workflow_name": None,
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
            "@workflow(name='broken', description='Broken Workflow.')\n"
            "def broken(ctx):\n    pass\n",
            encoding="utf-8",
        )
        completed, response = self.run_runtime(
            {"operation": "inspect_workflow", "pack_name": "stable-name", "pack_path": str(pack), "workflow_name": None}
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
@workflow(name='a', description='A.')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.workflows.a import analyze
@workflow(name='b', description='B.')
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-dynamic-discarded",
                {
                    "workflows/a.py": """from kat import Context, workflow
@workflow(name='a', description='A.')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import importlib
from kat import Context, workflow
importlib.import_module('kat.pack.workflows.a')
@workflow(name='b', description='B.')
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
@workflow(name='a', description='A.')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.helpers import cached
description = cached.import_entry('kat.pack.workflows.a').SHARED
@workflow(name='b', description=description)
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
@workflow(name='a', description='A.')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import kat
from kat import Context, workflow
description = kat.pack.workflows.a.SHARED
@workflow(name='b', description=description)
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
                    {"operation": "inspect_workflow", "pack_name": case, "pack_path": str(pack.resolve()), "workflow_name": None}
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

@workflow(name="cached", description="Inspect the standard module cache.")
def analyze(ctx: Context):
    \"\"\"Inspect the standard module cache.\"\"\"
""",
            encoding="utf-8",
        )

        completed, response = self.run_runtime(
            {
                "operation": "inspect_workflow",
                "workflow_name": None,
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

@workflow(name="worker", description="Record the real inspection worker.")
def analyze(ctx: Context):
    \"\"\"Record the real inspection worker.\"\"\"
""",
            encoding="utf-8",
        )
        before = {child.pid for child in multiprocessing.active_children()}

        result = inspect_workflow("reaped-worker", pack.resolve())

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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
                        "operation": "inspect_workflow",
                        "workflow_name": None,
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
@workflow(name='annotation-exit', description='Inspect an annotation.', parameters={'value': 'Value'})
def analyze(ctx: Context, value: __import__('sys').exit('annotation requested exit')):
    \"\"\"Inspect an annotation.\"\"\"
""",
            "callable-default-runtime-error": """from kat import Context, workflow
def invalid_default():
    raise RuntimeError('callable default failed')
@workflow(name='default-error', description='Inspect a callable default.', parameters={'value': 'Value'})
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
                        "operation": "inspect_workflow",
                        "workflow_name": None,
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
                    "operation": "inspect_workflow",
                    "workflow_name": None,
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
                "inspect_workflow",
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
                "operation": "inspect_workflow",
                "workflow_name": None,
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
        candidate_id = f"019f6e00-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        candidate = self.root / "runs" / candidate_id
        candidate.mkdir(parents=True)

        completed, response_path = self.run_runtime_process(
            {
                "operation": "run_workflow",
                "pack_name": "run-worker-crash",
                "pack_path": str(pack.resolve()),
                "workflow_name": "analyze",
                "arguments": [],
                "candidate_id": candidate_id,
                "candidate_path": str(candidate.resolve()),
                "datasource_root": str(
                    (
                        self.root / "datasources" / "run-worker-crash"
                    ).resolve(strict=False)
                ),
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
