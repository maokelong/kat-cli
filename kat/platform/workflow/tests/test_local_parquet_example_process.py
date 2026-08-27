from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid


class LocalParquetExampleProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.pack = (
            Path(__file__).resolve().parents[4]
            / "examples"
            / "packs"
            / "local-parquet-fusion"
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_real_external_pack_passes_its_workflow_tests(self) -> None:
        self.assertTrue(self.pack.is_dir(), f"missing example PACK: {self.pack}")
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        report_path = self.root / f"report-{token}.xml"
        request_path.write_text(
            json.dumps(
                {
                    "operation": "test_pack",
                    "pack_name": "local-parquet-fusion",
                    "pack_path": str(self.pack.resolve(strict=True)),
                    "datasets": {},
                    "tests": [],
                }
            ),
            encoding="utf-8",
        )

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
                "--test-report",
                str(report_path),
            ],
            cwd=self.pack,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env={**os.environ, "NO_COLOR": "1"},
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(
            response_path.is_file(),
            completed.stderr.decode(errors="replace"),
        )
        response = json.loads(response_path.read_text(encoding="utf-8"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["summary"], {"passed": 6})
        self.assertTrue(report_path.is_file())


if __name__ == "__main__":
    unittest.main()
