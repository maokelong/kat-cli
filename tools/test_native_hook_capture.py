import unittest
from pathlib import Path
from subprocess import CompletedProcess
from tempfile import TemporaryDirectory
from unittest.mock import Mock, call, patch

import native_hook_capture as capture


class NativeHookCaptureTest(unittest.TestCase):
    def test_hypium_controls_preinstalled_calculator_and_checks_result(self) -> None:
        by = Mock()
        by.id.side_effect = lambda value: f"selector:{value}"
        driver = Mock()
        components = [Mock() for _ in capture.CALCULATION_COMPONENTS]
        result = Mock()
        result.getText.return_value = capture.EXPECTED_RESULT
        driver.wait_for_component.side_effect = [*components, result]
        with TemporaryDirectory() as temporary:
            log = Path(temporary) / "hypium.log"
            capture.run_hypium(driver, by, log)

        driver.stop_app.assert_called_once_with(capture.BUNDLE)
        driver.start_app.assert_called_once_with(capture.BUNDLE, "MainAbility", wait_time=1)
        self.assertEqual(
            driver.wait_for_component.call_args_list,
            [
                *[call(f"selector:{item}", timeout=3) for item in capture.CALCULATION_COMPONENTS],
                call(f"selector:{capture.RESULT_COMPONENT}", timeout=3),
            ],
        )
        for component in components:
            component.click.assert_called_once_with()

    def test_hypium_connection_explicitly_selects_target(self) -> None:
        connector = Mock(return_value="driver")
        by = Mock()
        with TemporaryDirectory() as temporary:
            log = Path(temporary) / "hypium.log"
            actual = capture.connect_hypium("target", log, connector=connector, by=by)

        self.assertEqual(actual, ("driver", by))
        connector.assert_called_once_with(device_sn="target", report_path=str(log.parent))

    def test_hypium_rejects_unexpected_result_and_closes_driver(self) -> None:
        by = Mock()
        by.id.side_effect = lambda value: value
        driver = Mock()
        result = Mock()
        result.getText.return_value = "9999"
        driver.wait_for_component.side_effect = [
            *[Mock() for _ in capture.CALCULATION_COMPONENTS],
            result,
        ]

        with TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(RuntimeError, "unexpected calculator result"):
                capture.run_hypium(driver, by, Path(temporary) / "hypium.log")

    def test_close_hypium_closes_driver(self) -> None:
        driver = Mock()
        with TemporaryDirectory() as temporary:
            capture.close_hypium(driver, Path(temporary) / "hypium.log")
        driver.close.assert_called_once_with()

    @patch.dict("sys.modules", {"hypium": None})
    def test_missing_hypium_has_install_hint(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "requirements-native-hook-capture"):
            capture.load_hypium()

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
