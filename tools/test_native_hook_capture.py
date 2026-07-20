import unittest
from pathlib import Path
from subprocess import CompletedProcess
from tempfile import TemporaryDirectory
from unittest.mock import Mock, patch

import native_hook_capture as capture


class NativeHookCaptureTest(unittest.TestCase):
    @staticmethod
    def component(component_id: str, **attributes: str) -> dict:
        return {
            "attributes": {
                "id": component_id,
                "bounds": "[10,20][30,40]",
                "visible": "true",
                "enabled": "true",
                "clickable": "true",
                **attributes,
            },
            "children": [],
        }

    def test_finds_nested_component_and_calculates_center(self) -> None:
        layout = {"children": [{"children": [self.component("1")]}]}

        self.assertEqual(capture.component_center(layout, "1"), (20, 30))

    def test_rejects_duplicate_component_ids(self) -> None:
        layout = {"children": [self.component("1"), self.component("1")]}

        with self.assertRaisesRegex(RuntimeError, "found 2"):
            capture.component_center(layout, "1")

    def test_rejects_non_clickable_component(self) -> None:
        layout = {"children": [self.component("1", clickable="false")]}

        with self.assertRaisesRegex(RuntimeError, "not clickable"):
            capture.component_center(layout, "1")

    def test_rejects_invalid_component_bounds(self) -> None:
        layout = {"children": [self.component("1", bounds="invalid")]}

        with self.assertRaisesRegex(RuntimeError, "invalid bounds"):
            capture.component_center(layout, "1")

    def test_reads_calculator_result(self) -> None:
        layout = {
            "children": [self.component(capture.RESULT_COMPONENT, text="10000")]
        }

        self.assertEqual(capture.calculator_result(layout), "10000")

    @patch.object(capture, "logged_shell")
    def test_clicks_components_in_calculation_order(self, logged_shell: Mock) -> None:
        centers = {
            component_id: (index, index + 1)
            for index, component_id in enumerate(
                dict.fromkeys(capture.CALCULATION_COMPONENTS)
            )
        }

        capture.click_calculation("hdc", "target", centers, "log")

        self.assertEqual(
            [item.args[2] for item in logged_shell.call_args_list],
            [
                f"uitest uiInput click {centers[item][0]} {centers[item][1]}"
                for item in capture.CALCULATION_COMPONENTS
            ],
        )

    @patch.object(capture, "hdc_run")
    def test_dump_layout_rejects_invalid_json(self, hdc_run: Mock) -> None:
        hdc_run.side_effect = [
            CompletedProcess([], 0, "saved", ""),
            CompletedProcess([], 0, "not json", ""),
        ]
        with TemporaryDirectory() as temporary:
            with (Path(temporary) / "uitest.log").open("w", encoding="utf-8") as log:
                with self.assertRaisesRegex(RuntimeError, "invalid layout JSON"):
                    capture.dump_layout("hdc", "target", "/remote", log)

    def test_selects_only_connected_targets(self) -> None:
        output = """
usb-serial USB Connected localhost hdc
COM256 UART Ready unknown hdc
offline-serial USB Offline localhost hdc
"""
        self.assertEqual(capture.connected_targets(output), ["usb-serial"])

    @patch.object(capture.subprocess, "run")
    def test_hdc_output_is_decoded_as_utf8(self, run: Mock) -> None:
        run.return_value = CompletedProcess([], 0, "", "")

        capture.hdc_run("hdc", "target", "shell", "echo", capture_output=True)

        self.assertEqual(run.call_args.kwargs["encoding"], "utf-8")

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
