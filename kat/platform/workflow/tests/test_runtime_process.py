from __future__ import annotations

import json
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
from _kat_runtime.pack import _workflow_entries


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

@workflow(name="cpu-time", title=title(), required_tables=["thread", "sched_slice"], parameters={"limit": "Maximum rows"})
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
                    "workflows": [
                        {
                            "name": "cpu-time",
                            "title": "Helper title",
                            "description": "Analyze CPU time.",
                            "required_tables": ["sched_slice", "thread"],
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
        self.assertFalse(any(path.name == "__pycache__" for path in pack.rglob("*")))

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

        with (
            mock.patch.object(Path, "lstat", return_value=mock.Mock(st_mode=stat.S_IFLNK)),
            mock.patch.object(Path, "stat", side_effect=FileNotFoundError("dangling target")),
        ):
            with self.assertRaisesRegex(OSError, "failed to scan PACK workflows directory"):
                _workflow_entries(self.root)

        dangling_root = self.root / "dangling-workflows"
        dangling_root.mkdir()
        try:
            (dangling_root / "workflows").symlink_to(
                dangling_root / "missing-target", target_is_directory=True
            )
        except OSError:
            return
        with self.assertRaisesRegex(OSError, "failed to scan PACK workflows directory"):
            _workflow_entries(dangling_root)

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
            "@workflow(name='annotation', title='Annotation', required_tables=[], parameters={'value': 'Value'})\n"
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

        pack = self.write_pack()
        (pack / "workflows" / "broken.py").write_text(
            "from kat import Context, workflow\n"
            "@workflow(name='broken', title='Broken', required_tables=[])\n"
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
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.workflows.a import analyze
@workflow(name='b', title='B', required_tables=[])
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-dynamic-discarded",
                {
                    "workflows/a.py": """from kat import Context, workflow
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import importlib
from kat import Context, workflow
importlib.import_module('kat.pack.workflows.a')
@workflow(name='b', title='B', required_tables=[])
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
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.helpers import cached
title = cached.import_entry('kat.pack.workflows.a').SHARED
@workflow(name='b', title=title, required_tables=[])
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
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """import kat
from kat import Context, workflow
title = kat.pack.workflows.a.SHARED
@workflow(name='b', title=title, required_tables=[])
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

@workflow(name="cached", title="Cached", required_tables=[])
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
@workflow(name='annotation-exit', title='Annotation exit', required_tables=[], parameters={'value': 'Value'})
def analyze(ctx: Context, value: __import__('sys').exit('annotation requested exit')):
    \"\"\"Inspect an annotation.\"\"\"
""",
            "callable-default-runtime-error": """from kat import Context, workflow
def invalid_default():
    raise RuntimeError('callable default failed')
@workflow(name='default-error', title='Default error', required_tables=[], parameters={'value': 'Value'})
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


if __name__ == "__main__":
    unittest.main()
