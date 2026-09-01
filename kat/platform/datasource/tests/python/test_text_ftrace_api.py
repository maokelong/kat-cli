from __future__ import annotations

import pathlib
import tempfile
import threading
import time
import unittest


_HEADER = """# tracer: nop
#
# entries-in-buffer/entries-written: {entries}/{entries}   #P:4
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
        self.assertEqual(text_ftrace.__all__, ("decode", "DecodeError"))
        self.assertTrue(issubclass(text_ftrace.DecodeError, RuntimeError))
        self.assertFalse(hasattr(kat_datasource, "decode_text_ftrace"))

    def test_decode_publishes_typed_parquet_relations(self) -> None:
        from kat_datasource import text_ftrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "trace.ftrace"
            destination = root / "relations"
            source.write_text(
                _HEADER.format(entries=1)
                + "worker-7 ( 7) [002] d.... 1.0: sched_wakeup: "
                "comm=target pid=8 prio=120 target_cpu=003\n",
                encoding="utf-8",
            )

            self.assertIsNone(
                text_ftrace.decode(source, destination, "fixture_clock")
            )
            self.assertEqual(
                sorted(path.name for path in destination.iterdir()),
                [
                    "text_ftrace_event.parquet",
                    "text_ftrace_event_occurrence.parquet",
                    "text_ftrace_event_sched_wakeup.parquet",
                    "text_ftrace_header.parquet",
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
                _HEADER.format(entries=event_count) + event * event_count,
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
