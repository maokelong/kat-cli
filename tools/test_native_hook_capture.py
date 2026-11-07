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

    def test_profiler_config_uses_selected_scenario_bundle(self) -> None:
        config = capture.profiler_config("example.bundle")

        self.assertIn('process_name: "example.bundle"', config)
        self.assertNotIn(capture.CALCULATOR_BUNDLE, config)

    @patch.object(capture, "prepare_calculator")
    def test_calculator_scenario_delegates_prepare(self, prepare: Mock) -> None:
        scenario = capture.CalculatorScenario()

        scenario.prepare("hdc", "target", Path("log"))

        prepare.assert_called_once_with("hdc", "target", Path("log"))

    @patch.object(capture, "exercise_calculator")
    def test_calculator_scenario_delegates_exercise(self, exercise: Mock) -> None:
        scenario = capture.CalculatorScenario()
        profiler = Mock()

        scenario.exercise("hdc", "target", Path("log"), profiler)

        exercise.assert_called_once_with("hdc", "target", Path("log"), profiler)

    def test_scenario_registry_includes_calculator_and_note(self) -> None:
        self.assertEqual(set(capture.SCENARIOS), {"calculator", "note"})
        self.assertEqual(capture.SCENARIOS["note"].bundle, "com.ohos.note")

    @patch.object(capture, "exercise_note")
    @patch.object(capture, "prepare_note")
    def test_note_scenario_reuses_actions_discovered_during_prepare(
        self, prepare: Mock, exercise: Mock
    ) -> None:
        centers = {"search:center": (20, 30)}
        prepare.return_value = centers
        scenario = capture.NoteScenario()
        profiler = Mock()

        scenario.prepare("hdc", "target", Path("log"))
        scenario.exercise("hdc", "target", Path("log"), profiler)

        exercise.assert_called_once_with(
            "hdc", "target", Path("log"), profiler, centers
        )

    def test_discovers_only_clickable_calculator_buttons(self) -> None:
        layout = {
            "children": [
                self.component("1", type="Button"),
                self.component("+", type="Button", bounds="[30,40][50,80]"),
                self.component("label", type="Text"),
                self.component("disabled", type="Button", enabled="false"),
                self.component("", type="Button"),
            ]
        }

        self.assertEqual(
            capture.calculator_button_centers(layout),
            {"+": (40, 60), "1": (20, 30)},
        )

    def test_rejects_duplicate_clickable_button_ids(self) -> None:
        layout = {
            "children": [
                self.component("1", type="Button"),
                self.component("1", type="Button"),
            ]
        }

        with self.assertRaisesRegex(RuntimeError, "duplicate clickable"):
            capture.calculator_button_centers(layout)

    def test_discovers_only_safe_note_actions(self) -> None:
        layout = {
            "children": [
                self.component(
                    "searchInput",
                    type="TextInput",
                    longClickable="true",
                    bounds="[100,200][500,240]",
                ),
                self.component("", type="Image", accessibilityId="48"),
            ]
        }

        self.assertEqual(
            capture.note_action_centers(layout),
            {
                "search:left": (200, 220),
                "search:center": (300, 220),
                "search:right": (400, 220),
            },
        )

    @patch.object(capture.time, "sleep")
    @patch.object(capture, "logged_shell")
    def test_note_exercise_clicks_cached_safe_action(
        self,
        logged_shell: Mock,
        sleep: Mock,
    ) -> None:
        del sleep
        profiler = Mock()
        profiler.poll.side_effect = [None, 0]

        with TemporaryDirectory() as temporary:
            count = capture.exercise_note(
                "hdc",
                "target",
                Path(temporary) / "uitest.log",
                profiler,
                {"search:center": (20, 30)},
                seed=1234,
            )

        self.assertEqual(count, 1)
        self.assertEqual(
            [call.args[2] for call in logged_shell.call_args_list],
            ["uitest uiInput click 20 30"],
        )

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
    @patch.object(capture, "logged_shell")
    @patch.object(capture, "dump_layout")
    def test_random_clicks_continue_until_profiler_exits(
        self, dump_layout: Mock, logged_shell: Mock, hdc_run: Mock
    ) -> None:
        del hdc_run
        dump_layout.return_value = {
            "children": [
                self.component("1", type="Button"),
                self.component("+", type="Button", bounds="[30,40][50,80]"),
            ]
        }
        profiler = Mock()
        profiler.poll.side_effect = [None, None, 0]

        with TemporaryDirectory() as temporary:
            log_path = Path(temporary) / "uitest.log"
            count = capture.exercise_calculator(
                "hdc", "target", log_path, profiler, seed=1234
            )
            log_text = log_path.read_text(encoding="utf-8")

        self.assertEqual(count, 2)
        self.assertEqual(len(logged_shell.call_args_list), 2)
        self.assertTrue(
            all(
                call.args[2] in {
                    "uitest uiInput click 20 30",
                    "uitest uiInput click 40 60",
                }
                for call in logged_shell.call_args_list
            )
        )
        self.assertIn("random seed: 1234", log_text)
        self.assertIn("random buttons: +, 1", log_text)
        self.assertIn("random click count: 2", log_text)

    @patch.object(capture, "hdc_run")
    @patch.object(capture, "logged_shell", side_effect=RuntimeError("click failed"))
    @patch.object(capture, "dump_layout")
    def test_random_click_failure_is_ignored_after_profiler_exits(
        self, dump_layout: Mock, logged_shell: Mock, hdc_run: Mock
    ) -> None:
        del logged_shell, hdc_run
        dump_layout.return_value = {
            "children": [self.component("1", type="Button")]
        }
        profiler = Mock()
        profiler.poll.side_effect = [None, 0]

        with TemporaryDirectory() as temporary:
            log_path = Path(temporary) / "uitest.log"
            count = capture.exercise_calculator(
                "hdc", "target", log_path, profiler, seed=1234
            )
            log_text = log_path.read_text(encoding="utf-8")

        self.assertEqual(count, 0)
        self.assertIn("random click count: 0", log_text)

    @patch.object(capture, "hdc_run")
    def test_dump_layout_rejects_invalid_json(self, hdc_run: Mock) -> None:
        hdc_run.side_effect = [
            CompletedProcess([], 0, "saved", ""),
            CompletedProcess([], 0, "not json", ""),
        ]
        with TemporaryDirectory() as temporary:
            with (Path(temporary) / "uitest.log").open("w", encoding="utf-8") as log:
                with self.assertRaisesRegex(RuntimeError, "invalid layout JSON"):
                    capture.dump_layout(
                        "hdc", "target", "example.bundle", "/remote", log
                    )

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

    @patch.object(capture, "capture_failure")
    def test_scenario_interaction_failure_is_only_a_warning(
        self, capture_failure: Mock
    ) -> None:
        scenario = Mock()
        scenario.name = "example"
        scenario.exercise.side_effect = RuntimeError("click failed")
        profiler = Mock()

        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            log_path = root / "uitest.log"
            log_path.write_text("", encoding="utf-8")
            warning = capture.exercise_scenario(
                scenario,
                "hdc",
                "target",
                log_path,
                root / "failure.png",
                profiler,
            )
            log_text = log_path.read_text(encoding="utf-8")

        self.assertEqual(warning, "example interaction warning: click failed")
        self.assertIn(warning, log_text)
        capture_failure.assert_called_once()


if __name__ == "__main__":
    unittest.main()
