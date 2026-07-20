import unittest
from pathlib import Path
from subprocess import CompletedProcess
from tempfile import TemporaryDirectory
from unittest.mock import Mock, patch

import native_hook_capture as capture


class NativeHookCaptureTest(unittest.TestCase):
    def test_accepts_successful_hypium_report(self) -> None:
        output = """
OHOS_REPORT_RESULT: stream=Tests run: 1, Failure: 0, Error: 0, Pass: 1, Ignore: 0
OHOS_REPORT_CODE: 0
"""
        self.assertTrue(capture.hypium_report_passed(output))

    def test_rejects_failed_hypium_report_even_when_command_finished(self) -> None:
        output = """
OHOS_REPORT_RESULT: stream=Tests run: 1, Failure: 1, Error: 0, Pass: 0, Ignore: 0
OHOS_REPORT_CODE: -1
TestFinished-ResultCode: 0
"""
        self.assertFalse(capture.hypium_report_passed(output))

    @patch.object(capture, "hdc_run")
    def test_hypium_nonzero_command_is_fatal_even_with_passing_report(
        self, hdc_run: Mock
    ) -> None:
        output = """OHOS_REPORT_RESULT: stream=Tests run: 1, Failure: 0, Error: 0, Pass: 1, Ignore: 0
OHOS_REPORT_CODE: 0
"""
        hdc_run.return_value = CompletedProcess([], 9, output, "")
        with TemporaryDirectory() as temporary:
            log = Path(temporary) / "hypium.log"

            with self.assertRaisesRegex(RuntimeError, "code 9"):
                capture.run_hypium("hdc", "target", log)

            self.assertEqual(log.read_text(encoding="utf-8"), output)

    def test_selects_only_connected_targets(self) -> None:
        output = """
usb-serial USB Connected localhost hdc
COM256 UART Ready unknown hdc
offline-serial USB Offline localhost hdc
"""
        self.assertEqual(capture.connected_targets(output), ["usb-serial"])

    @patch.object(capture.subprocess, "run")
    def test_rejects_ambiguous_connected_targets(self, run: Mock) -> None:
        run.return_value = CompletedProcess([], 0, "one USB Connected x hdc\ntwo USB Connected x hdc\n", "")

        with self.assertRaisesRegex(RuntimeError, "expected exactly one"):
            capture.select_target("hdc", None)

    @patch.object(capture.subprocess, "run")
    def test_rejects_requested_offline_target(self, run: Mock) -> None:
        run.return_value = CompletedProcess([], 0, "online USB Connected x hdc\n", "")

        with self.assertRaisesRegex(RuntimeError, "target is not connected"):
            capture.select_target("hdc", "offline")

    @patch.object(capture, "hdc_run")
    @patch.object(capture.time, "sleep")
    @patch.object(capture.time, "monotonic", side_effect=[0.0, 0.0, 1.0])
    def test_profiler_readiness_timeout_terminates_process(
        self, monotonic: Mock, sleep: Mock, hdc_run: Mock
    ) -> None:
        del monotonic, sleep
        hdc_run.return_value = CompletedProcess([], 1, "", "")
        profiler = Mock()
        profiler.poll.return_value = None

        with self.assertRaisesRegex(RuntimeError, "did not become ready"):
            capture.wait_for_profiler("hdc", "target", "/remote", profiler, timeout=1)

        profiler.terminate.assert_called_once_with()
        profiler.wait.assert_called_once_with()

    @patch.object(capture, "hdc_run")
    def test_profiler_is_ready_only_after_non_empty_remote_trace(self, hdc_run: Mock) -> None:
        hdc_run.return_value = CompletedProcess([], 0, "42\n", "")
        profiler = Mock()
        profiler.poll.return_value = None

        capture.wait_for_profiler("hdc", "target", "/remote", profiler)

        profiler.terminate.assert_not_called()

    @patch.object(capture.subprocess, "run")
    def test_trace_streamer_failure_is_fatal(self, run: Mock) -> None:
        run.return_value = CompletedProcess([], 7)
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            trace = root / "trace.htrace"
            trace.write_bytes(b"trace")

            with self.assertRaisesRegex(RuntimeError, "code 7"):
                capture.convert_trace(
                    "trace_streamer", trace, root / "trace.db", root / "streamer.log"
                )

    @patch.object(capture.subprocess, "run")
    def test_trace_streamer_requires_non_empty_database(self, run: Mock) -> None:
        run.return_value = CompletedProcess([], 0)
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            trace = root / "trace.htrace"
            trace.write_bytes(b"trace")
            database = root / "trace.db"
            database.write_bytes(b"")

            with self.assertRaisesRegex(RuntimeError, "code 0"):
                capture.convert_trace(
                    "trace_streamer", trace, database, root / "streamer.log"
                )


if __name__ == "__main__":
    unittest.main()
