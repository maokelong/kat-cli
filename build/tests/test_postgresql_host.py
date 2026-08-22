from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
MODULE_PATH = REPOSITORY / "build/payload_builder.py"
SPEC = importlib.util.spec_from_file_location(
    "payload_builder_postgresql_test", MODULE_PATH
)
assert SPEC is not None and SPEC.loader is not None
payload_builder = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = payload_builder
SPEC.loader.exec_module(payload_builder)


class PostgreSqlHostTests(unittest.TestCase):
    def test_bundled_python_imports_public_postgresql_capability_in_isolation(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            python = Path(directory) / "payload/python/python.exe"

            with mock.patch.object(payload_builder.subprocess, "run") as run:
                payload_builder.check_postgresql_host(python)

            command = run.call_args.args[0]
            self.assertEqual(
                command[:6], [str(python), "-I", "-B", "-X", "utf8", "-c"]
            )
            self.assertTrue(run.call_args.kwargs["check"])
            self.assertEqual(
                run.call_args.kwargs["env"],
                payload_builder.isolated_environment(),
            )
            script = command[6]
            for contract in (
                "psycopg.__version__",
                'pq.__impl__ != "binary"',
                "postgresql.execute_sql_file",
                "postgresql.execute_sql_text",
            ):
                self.assertIn(contract, script)


if __name__ == "__main__":
    unittest.main()
