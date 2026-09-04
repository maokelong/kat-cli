from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from _test_control_peer import run_runtime_with_test_control


class SkillReferencePackProcessTest(unittest.TestCase):
    def test_runtime_executes_the_only_public_reference_pack_source(self) -> None:
        repository = Path(__file__).resolve().parents[4]
        pack = (
            repository
            / "kat"
            / "skill"
            / "references"
            / "examples"
            / "dataprovider-pack"
        )
        self.assertTrue(pack.is_dir(), f"missing public reference PACK: {pack}")

        with tempfile.TemporaryDirectory() as temporary:
            private = Path(temporary)
            request_path = private / "request.json"
            response_path = private / "response.json"
            report_path = private / "report.xml"
            request_path.write_text(
                json.dumps(
                    {
                        "operation": "test_pack",
                        "pack_name": "dataprovider-pack",
                        "pack_path": str(pack),
                        "tests": [],
                    }
                ),
                encoding="utf-8",
            )

            completed = run_runtime_with_test_control(
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
                cwd=pack,
                environment={**os.environ, "NO_COLOR": "1"},
                data_home=private / "host",
            )

            terminal = completed.stderr.decode(errors="replace")
            self.assertEqual(completed.returncode, 0, terminal)
            response = json.loads(response_path.read_text(encoding="utf-8"))
            self.assertEqual(response["status"], "success")
            summary = response["result"]["summary"]
            self.assertGreater(summary.get("passed", 0), 0)
            self.assertNotIn("failed", summary)
            self.assertTrue(report_path.is_file())
            self.assertIn("testsuite", report_path.read_text(encoding="utf-8"))

            provider_request_path = private / "provider-request.json"
            provider_response_path = private / "provider-response.json"
            provider_request_path.write_text(
                json.dumps(
                    {
                        "operation": "inspect_provider",
                        "pack_name": "dataprovider-pack",
                        "pack_path": str(pack),
                        "provider_name": None,
                    }
                ),
                encoding="utf-8",
            )
            inspected = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    "-X",
                    "utf8",
                    "-u",
                    "-m",
                    "_kat_runtime",
                    "--request",
                    str(provider_request_path),
                    "--response",
                    str(provider_response_path),
                ],
                cwd=pack,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                env={**os.environ, "NO_COLOR": "1"},
            )
            terminal = inspected.stderr.decode(errors="replace")
            self.assertEqual(inspected.returncode, 0, terminal)
            provider_response = json.loads(
                provider_response_path.read_text(encoding="utf-8")
            )
            self.assertEqual(provider_response["status"], "success")
            providers = provider_response["result"]["providers"]
            self.assertEqual(
                [provider["name"] for provider in providers],
                ["ftrace-text", "postgresql", "trace-streamer-sqlite"],
            )
            self.assertTrue(
                all(set(provider) == {"name", "description"} for provider in providers)
            )
            self.assertFalse(any(path.name == "__pycache__" for path in pack.rglob("*")))


if __name__ == "__main__":
    unittest.main()
