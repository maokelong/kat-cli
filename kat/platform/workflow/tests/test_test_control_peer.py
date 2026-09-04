from __future__ import annotations

import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

from _test_control_peer import run_runtime_with_test_control


class TestControlPeerTest(unittest.TestCase):
    def test_hanging_runtime_is_killed_after_bounded_stderr_drain(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            started = time.monotonic()

            with self.assertRaisesRegex(
                RuntimeError,
                "did not exit within 1 seconds and was killed",
            ) as raised:
                run_runtime_with_test_control(
                    [
                        sys.executable,
                        "-B",
                        "-c",
                        (
                            "import sys, time; "
                            "sys.stderr.write('timeout sentinel\\n'); "
                            "sys.stderr.flush(); "
                            "time.sleep(60)"
                        ),
                    ],
                    cwd=root,
                    environment=dict(os.environ),
                    data_home=root / "data-home",
                    process_timeout_seconds=1,
                )

            self.assertLess(time.monotonic() - started, 10)
            self.assertIn("timeout sentinel", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
