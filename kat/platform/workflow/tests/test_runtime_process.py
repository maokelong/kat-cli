from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid


class RuntimeProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(self, request: object) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
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
            env={**os.environ, "NO_COLOR": "1"},
        )
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

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

    def test_invalid_request_and_invalid_entry_return_only_diagnostic(self) -> None:
        completed, response = self.run_runtime(
            {"operation": "inspect_pack", "pack_name": "alpha", "pack_path": "relative", "extra": True}
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
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
                "entry-import-constant",
                {
                    "workflows/a.py": """from kat import Context, workflow
SHARED = "not a helper"
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
from kat.pack.workflows.a import SHARED
@workflow(name='b', title=SHARED, required_tables=[])
def other(ctx: Context):
    \"\"\"B.\"\"\"
""",
                },
            ),
            (
                "entry-import-root-binding",
                {
                    "workflows/a.py": """from kat import Context, workflow
@workflow(name='a', title='A', required_tables=[])
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
""",
                    "workflows/b.py": """from kat import Context, workflow
import kat.pack.workflows.a
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


if __name__ == "__main__":
    unittest.main()
