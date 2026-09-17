from __future__ import annotations

import pathlib
import tempfile
import threading
import time
import unittest


_HEADER = """# tracer: nop
#
# entries-in-buffer/entries-written: %lu/%lu   #P:%d
#
# _-----=> irqs-off/BH-disabled
# / _----=> need-resched
# | / _---=> hardirq/softirq
# || / _--=> preempt-depth
# ||| / _-=> migrate-disable
# |||| /     delay
# TASK-PID       TGID    CPU#  |||||  TIMESTAMP  FUNCTION
# | |            |       |   |||||     |         |
"""


class TextFtraceApiContractTests(unittest.TestCase):
    def test_public_surface_is_namespaced(self) -> None:
        import kat_datasource
        from kat_datasource import text_ftrace

        self.assertEqual(kat_datasource.__all__, ("hitrace", "text_ftrace"))
        self.assertEqual(
            text_ftrace.__all__,
            (
                "decode",
                "DecodeReport",
                "DecodeError",
                "HEADER_RELATION",
                "OCCURRENCE_RELATION",
                "EVENT_RELATION",
                "UNSUPPORTED_EVENT_RELATION",
                "MATERIALIZATION_VERSION_METADATA_KEY",
                "MATERIALIZATION_VERSION",
            ),
        )
        self.assertEqual(text_ftrace.HEADER_RELATION, "text_ftrace_header")
        self.assertEqual(
            text_ftrace.OCCURRENCE_RELATION,
            "text_ftrace_event_occurrence",
        )
        self.assertEqual(text_ftrace.EVENT_RELATION, "text_ftrace_event")
        self.assertEqual(
            text_ftrace.UNSUPPORTED_EVENT_RELATION,
            "text_ftrace_unsupported_event",
        )
        self.assertEqual(
            text_ftrace.MATERIALIZATION_VERSION_METADATA_KEY,
            b"kat.materialization.version",
        )
        self.assertEqual(text_ftrace.MATERIALIZATION_VERSION, "text-ftrace-v1")
        self.assertTrue(issubclass(text_ftrace.DecodeError, RuntimeError))
        self.assertFalse(hasattr(kat_datasource, "decode_text_ftrace"))

    def test_decode_publishes_typed_parquet_relations(self) -> None:
        from kat_datasource import text_ftrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "trace.ftrace"
            destination = root / "relations"
            source.write_text(
                _HEADER
                + "worker-7 ( 7) [002] d.... 1.0: sched_wakeup: "
                "comm=target pid=8 prio=120 target_cpu=003\n",
                encoding="utf-8",
            )

            report = text_ftrace.decode(source, destination, "fixture_clock")
            self.assertEqual(report.unsupported_event_names, ())
            self.assertEqual(
                sorted(path.name for path in destination.iterdir()),
                [
                    "text_ftrace_event.parquet",
                    "text_ftrace_event_occurrence.parquet",
                    "text_ftrace_event_sched_wakeup.parquet",
                    "text_ftrace_header.parquet",
                ],
            )

    def test_decode_reports_unknown_events_and_accepts_no_supported_events(self) -> None:
        from kat_datasource import text_ftrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "unknown.ftrace"
            destination = root / "relations"
            source.write_text(
                _HEADER
                + "worker-7 ( 7) [002] d.... 1.0: z_event: value=1\n"
                + "worker-7 ( 7) [002] d.... 2.0: a_event: value=2\n"
                + "worker-7 ( 7) [002] d.... 3.0: z_event: value=3\n",
                encoding="utf-8",
            )

            report = text_ftrace.decode(source, destination, "fixture_clock")

            self.assertEqual(report.unsupported_event_names, ("a_event", "z_event"))
            self.assertEqual(
                sorted(path.name for path in destination.iterdir()),
                [
                    "text_ftrace_header.parquet",
                    "text_ftrace_unsupported_event.parquet",
                ],
            )

    def test_decode_error_is_specific_and_leaves_no_destination(self) -> None:
        from kat_datasource import text_ftrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "invalid.ftrace"
            destination = root / "relations"
            source.write_text("not ftrace\n", encoding="utf-8")

            with self.assertRaises(text_ftrace.DecodeError):
                text_ftrace.decode(source, destination, "fixture_clock")

            self.assertFalse(destination.exists())

    def test_native_decode_releases_the_interpreter(self) -> None:
        from kat_datasource import text_ftrace

        event_count = 100_000
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "large.ftrace"
            destination = root / "relations"
            event = (
                "worker-7 ( 7) [002] d.... 1.0: sched_wakeup: "
                "comm=target pid=8 prio=120 target_cpu=003\n"
            )
            source.write_text(
                _HEADER + event * event_count,
                encoding="utf-8",
            )

            stop = threading.Event()
            progress = [0]

            def advance() -> None:
                while not stop.is_set():
                    progress[0] += 1

            worker = threading.Thread(target=advance)
            worker.start()
            time.sleep(0.05)
            before = progress[0]
            try:
                text_ftrace.decode(source, destination, "fixture_clock")
            finally:
                after = progress[0]
                stop.set()
                worker.join()

            self.assertGreater(after, before)


if __name__ == "__main__":
    unittest.main()
