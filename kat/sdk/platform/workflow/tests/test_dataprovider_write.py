from __future__ import annotations

import os
import tempfile
import threading
import unittest
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from unittest import mock

import pyarrow.parquet as pq

from kat import dataprovider as dp
from kat.dataprovider import _parquet_writer as parquet_writer_module
from kat.dataprovider import _write as write_module


class DataProviderWriteTest(unittest.TestCase):
    def test_multi_relation_batches_publish_as_one_queryable_catalog(self) -> None:
        schema = dp.Schema(
            {
                "events": {"sequence": int, "label": str},
                "capture": {"clock": str},
                "empty_rows": {"payload": bytes | None},
            }
        )

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 2
        ), mock.patch.object(write_module, "_MAX_BATCH_BYTES", 1_000_000):
            destination = Path(parent) / "facts"
            with dp.write(schema, destination=destination) as sink:
                sink["events"].append(sequence=1, label="start")
                sink["capture"].append(clock="boot")
                sink["events"].append(sequence=2, label="stop")
                sink["capture"].append(clock="monotonic")
                sink["events"].append(sequence=3, label="tail")
                self.assertFalse(destination.exists())

            self.assertEqual(
                sorted(path.name for path in destination.iterdir()),
                ["capture.parquet", "empty_rows.parquet", "events.parquet"],
            )
            catalog = dp.open(root=destination)
            self.assertEqual(catalog.tables, ("capture", "empty_rows", "events"))
            events = dp.DataFusionProvider(catalog=catalog).query(
                "SELECT sequence, label FROM events ORDER BY sequence"
            )
            self.assertEqual(
                events.to_rows(),
                [
                    {"sequence": 1, "label": "start"},
                    {"sequence": 2, "label": "stop"},
                    {"sequence": 3, "label": "tail"},
                ],
            )
            capture = dp.DataFusionProvider(catalog=catalog).query(
                "SELECT clock FROM capture"
            )
            self.assertEqual(
                capture.to_rows(),
                [{"clock": "boot"}, {"clock": "monotonic"}],
            )
            self.assertEqual(
                pq.read_table(destination / "events.parquet")["sequence"].to_pylist(),
                [1, 2, 3],
            )
            self.assertEqual(
                pq.ParquetFile(destination / "events.parquet").metadata.num_row_groups,
                2,
            )
            self.assertEqual(
                pq.read_table(destination / "empty_rows.parquet").num_rows,
                0,
            )

    def test_write_accepts_self_as_a_schema_column_name(self) -> None:
        schema = dp.Schema({"events": {"self": int}})

        with tempfile.TemporaryDirectory() as parent:
            destination = Path(parent) / "facts"
            with dp.write(schema, destination=destination) as sink:
                sink["events"].append(**{"self": 7})

            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                'SELECT "self" FROM events'
            )
            self.assertEqual(result.to_rows(), [{"self": 7}])

    def test_invalid_row_is_atomic_and_does_not_poison_the_write_transaction(
        self,
    ) -> None:
        table_schema = {
            "enabled": bool,
            "count": int,
            "ratio": float,
            "label": str,
            "payload": bytes,
            "observed_at": datetime,
            "amount": Decimal | None,
        }
        schema = dp.Schema({"events": table_schema})

        with tempfile.TemporaryDirectory() as parent:
            destination = Path(parent) / "facts"
            with dp.write(schema, destination=destination) as sink:
                valid = {
                    "enabled": True,
                    "count": 7,
                    "ratio": 1.5,
                    "label": "valid",
                    "payload": b"ok",
                    "observed_at": datetime(2026, 9, 2, tzinfo=timezone.utc),
                    "amount": Decimal("12.5"),
                }
                missing = dict(valid)
                missing.pop("label")
                invalid_rows = (
                    ("missing field", missing, ValueError, "exactly match"),
                    (
                        "extra field",
                        {**valid, "extra": 1},
                        ValueError,
                        "exactly match",
                    ),
                    (
                        "exact Python type",
                        {**valid, "count": True},
                        TypeError,
                        "exact type int, got bool",
                    ),
                    (
                        "integer range",
                        {**valid, "count": 2**63},
                        ValueError,
                        "signed int64 range",
                    ),
                    (
                        "nullability",
                        {**valid, "label": None},
                        ValueError,
                        "not nullable",
                    ),
                    (
                        "aware datetime",
                        {**valid, "observed_at": datetime(2026, 9, 2)},
                        ValueError,
                        "aware datetime",
                    ),
                    (
                        "finite Decimal",
                        {**valid, "amount": Decimal("NaN")},
                        ValueError,
                        "finite Decimal",
                    ),
                )
                for label, row, error_type, message in invalid_rows:
                    with self.subTest(label=label), self.assertRaisesRegex(
                        error_type, message
                    ):
                        sink["events"].append(**row)

                sink["events"].append(**valid)

            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT label, count, amount FROM events"
            )
            self.assertEqual(
                result.to_rows(),
                [
                    {
                        "label": "valid",
                        "count": 7,
                        "amount": Decimal("12.500000000000000000"),
                    }
                ],
            )

    def test_write_transaction_and_relation_handles_are_one_shot_and_thread_confined(
        self,
    ) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent:
            destination = Path(parent) / "facts"
            candidate = dp.write(schema, destination=destination)
            with self.assertRaisesRegex(RuntimeError, "active context"):
                candidate["events"]

            with candidate as sink:
                relation = sink["events"]
                for name in ("finish", "flush", "close", "catalog"):
                    with self.subTest(transaction_attribute=name):
                        self.assertFalse(hasattr(sink, name))
                for name in ("to_arrow", "to_rows", "query", "close"):
                    with self.subTest(relation_attribute=name):
                        self.assertFalse(hasattr(relation, name))
                with self.assertRaises(KeyError):
                    sink["unknown"]

                errors: list[BaseException] = []

                def append_from_another_thread() -> None:
                    try:
                        relation.append(value=1)
                    except BaseException as error:
                        errors.append(error)

                thread = threading.Thread(target=append_from_another_thread)
                thread.start()
                thread.join()
                self.assertEqual(len(errors), 1)
                self.assertIsInstance(errors[0], RuntimeError)
                self.assertIn("owner thread", str(errors[0]))
                relation.append(value=2)

            with self.assertRaisesRegex(RuntimeError, "active context"):
                relation.append(value=3)
            with self.assertRaisesRegex(RuntimeError, "one-shot"):
                candidate.__enter__()

            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT value FROM events"
            )
            self.assertEqual(result.to_rows(), [{"value": 2}])

    def test_entering_thread_becomes_the_write_owner(self) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent:
            destination = Path(parent) / "facts"
            transaction = dp.write(schema, destination=destination)
            errors: list[BaseException] = []

            def produce() -> None:
                try:
                    with transaction as sink:
                        sink["events"].append(value=7)
                except BaseException as error:
                    errors.append(error)

            producer = threading.Thread(target=produce)
            producer.start()
            producer.join(5)

            self.assertFalse(producer.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual(
                pq.read_table(destination / "events.parquet").to_pylist(),
                [{"value": 7}],
            )

    def test_bounded_batches_become_row_groups_without_losing_rows(self) -> None:
        schema = dp.Schema({"events": {"sequence": int, "payload": bytes}})

        with tempfile.TemporaryDirectory() as parent, mock.patch(
            "kat.dataprovider._write._MAX_BATCH_ROWS", 3
        ), mock.patch("kat.dataprovider._write._MAX_BATCH_BYTES", 20):
            destination = Path(parent) / "facts"
            with dp.write(schema, destination=destination) as sink:
                for sequence, payload in enumerate(
                    (b"a", b"long-enough", b"b", b"c", b"d"), start=1
                ):
                    sink["events"].append(sequence=sequence, payload=payload)

            metadata = pq.ParquetFile(destination / "events.parquet").metadata
            self.assertEqual(metadata.num_row_groups, 2)
            self.assertEqual(
                pq.read_table(destination / "events.parquet")["sequence"].to_pylist(),
                [1, 2, 3, 4, 5],
            )
            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT sequence, payload FROM events ORDER BY sequence"
            )
            self.assertEqual(
                result.to_rows(),
                [
                    {"sequence": 1, "payload": b"a"},
                    {"sequence": 2, "payload": b"long-enough"},
                    {"sequence": 3, "payload": b"b"},
                    {"sequence": 4, "payload": b"c"},
                    {"sequence": 5, "payload": b"d"},
                ],
            )

    def test_parser_and_writer_overlap_until_bounded_backpressure(self) -> None:
        schema = dp.Schema({"events": {"sequence": int}})
        write_started = threading.Event()
        release_write = threading.Event()
        second_batch_accepted = threading.Event()
        third_batch_started = threading.Event()
        third_batch_accepted = threading.Event()
        producer_errors: list[BaseException] = []
        original_write_rows = parquet_writer_module._ParquetRelationWriter.write_rows

        def block_first_write(
            writer: parquet_writer_module._ParquetRelationWriter,
            rows: tuple[tuple[object | None, ...], ...],
        ) -> None:
            if rows[0][0] == 1:
                write_started.set()
                if not release_write.wait(5):
                    raise TimeoutError("test did not release the first write")
            original_write_rows(writer, rows)

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            write_module, "_MAX_BATCH_BYTES", 1024
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=block_first_write,
        ):
            destination = Path(parent) / "facts"

            def produce() -> None:
                try:
                    with dp.write(schema, destination=destination) as sink:
                        sink["events"].append(sequence=1)
                        if not write_started.wait(5):
                            raise TimeoutError("writer did not start")
                        sink["events"].append(sequence=2)
                        second_batch_accepted.set()
                        third_batch_started.set()
                        sink["events"].append(sequence=3)
                        third_batch_accepted.set()
                except BaseException as error:
                    producer_errors.append(error)

            producer = threading.Thread(target=produce)
            producer.start()
            try:
                self.assertTrue(write_started.wait(5))
                self.assertTrue(second_batch_accepted.wait(5))
                self.assertTrue(third_batch_started.wait(5))
                self.assertFalse(third_batch_accepted.is_set())
            finally:
                release_write.set()
                producer.join(5)

            self.assertFalse(producer.is_alive())
            self.assertEqual(producer_errors, [])
            self.assertTrue(third_batch_accepted.is_set())
            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT sequence FROM events ORDER BY sequence"
            )
            self.assertEqual(
                result.to_rows(),
                [{"sequence": 1}, {"sequence": 2}, {"sequence": 3}],
            )

    def test_writer_failure_wakes_a_producer_blocked_by_a_full_queue(self) -> None:
        schema = dp.Schema({"events": {"sequence": int}})
        write_started = threading.Event()
        fail_write = threading.Event()
        second_batch_accepted = threading.Event()
        third_batch_started = threading.Event()
        producer_finished = threading.Event()
        producer_errors: list[BaseException] = []

        def fail_first_write(
            _writer: parquet_writer_module._ParquetRelationWriter,
            _rows: tuple[tuple[object | None, ...], ...],
        ) -> None:
            write_started.set()
            if not fail_write.wait(5):
                raise TimeoutError("test did not trigger the writer failure")
            raise OSError("simulated disk failure")

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            write_module, "_MAX_BATCH_BYTES", 1024
        ), mock.patch.object(
            write_module, "_QUEUE_WAIT_SECONDS", 2.0
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=fail_first_write,
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"

            def produce() -> None:
                try:
                    with dp.write(schema, destination=destination) as sink:
                        sink["events"].append(sequence=1)
                        if not write_started.wait(5):
                            raise TimeoutError("writer did not start")
                        sink["events"].append(sequence=2)
                        second_batch_accepted.set()
                        third_batch_started.set()
                        sink["events"].append(sequence=3)
                except BaseException as error:
                    producer_errors.append(error)
                finally:
                    producer_finished.set()

            producer = threading.Thread(target=produce)
            producer.start()
            try:
                self.assertTrue(second_batch_accepted.wait(5))
                self.assertTrue(third_batch_started.wait(5))
                self.assertFalse(producer_finished.wait(0.1))
                fail_write.set()
                self.assertTrue(
                    producer_finished.wait(0.75),
                    "writer failure did not wake the blocked producer",
                )
            finally:
                fail_write.set()
                producer.join(5)

            self.assertFalse(producer.is_alive())
            self.assertEqual(len(producer_errors), 1)
            self.assertIsInstance(producer_errors[0], OSError)
            self.assertIn("simulated disk failure", str(producer_errors[0]))
            self.assertFalse(destination.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_background_failure_is_primary_when_the_body_succeeds(self) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=OSError("simulated encoder failure"),
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with self.assertRaisesRegex(OSError, "simulated encoder failure"):
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)

            self.assertFalse(destination.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_body_failure_remains_primary_when_the_writer_also_fails(self) -> None:
        class ParserError(Exception):
            pass

        schema = dp.Schema({"events": {"value": int}})
        writer_entered = threading.Event()
        original_close = parquet_writer_module._ParquetRelationWriter.close

        def fail_write(
            _writer: parquet_writer_module._ParquetRelationWriter,
            _rows: tuple[tuple[object | None, ...], ...],
        ) -> None:
            writer_entered.set()
            raise OSError("simulated background failure")

        def fail_close(
            writer: parquet_writer_module._ParquetRelationWriter,
        ) -> None:
            original_close(writer)
            raise OSError("simulated close failure")

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=fail_write,
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "close",
            autospec=True,
            side_effect=fail_close,
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with self.assertRaisesRegex(ParserError, "parser stopped") as raised:
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)
                    self.assertTrue(writer_entered.wait(5))
                    raise ParserError("parser stopped")

            notes = getattr(raised.exception, "__notes__", [])
            self.assertTrue(
                any("simulated background failure" in note for note in notes),
                notes,
            )
            self.assertTrue(
                any("simulated close failure" in note for note in notes),
                notes,
            )
            self.assertFalse(destination.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_invalid_inputs_fail_before_any_candidate_is_created(self) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            with self.assertRaisesRegex(TypeError, "dp.Schema"):
                dp.write(  # type: ignore[arg-type]
                    {"events": object()},
                    destination=parent_path / "legacy-eager-write",
                )
            with self.assertRaisesRegex(TypeError, "dp.Schema"):
                dp.write(  # type: ignore[arg-type]
                    object(), destination=parent_path / "facts"
                )
            with self.assertRaisesRegex(TypeError, "pathlib.Path"):
                dp.write(  # type: ignore[arg-type]
                    schema, destination=str(parent_path / "facts")
                )
            with self.assertRaisesRegex(ValueError, "parent"):
                dp.write(
                    schema,
                    destination=parent_path / "missing" / "facts",
                )
            truncated_destination = parent_path / "nul-prefix"
            with self.assertRaisesRegex(ValueError, "NUL"):
                dp.write(
                    schema,
                    destination=parent_path / "nul-prefix\0ignored",
                )
            self.assertFalse(truncated_destination.exists())

            destination = parent_path / "facts"
            destination.mkdir()
            marker = destination / "owned-by-someone-else"
            marker.write_text("keep", encoding="utf-8")
            with self.assertRaises(FileExistsError):
                dp.write(schema, destination=destination)
            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
            self.assertEqual(tuple(parent_path.iterdir()), (destination,))

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            destination = parent_path / "facts"
            candidate = dp.write(schema, destination=destination)
            destination.mkdir()
            with self.assertRaises(FileExistsError):
                candidate.__enter__()
            self.assertEqual(tuple(parent_path.iterdir()), (destination,))
            destination.rmdir()
            with self.assertRaisesRegex(RuntimeError, "one-shot"):
                candidate.__enter__()

    def test_publish_race_never_replaces_a_competing_destination(self) -> None:
        schema = dp.Schema({"events": {"value": int}})
        original_rename = write_module._rename_no_replace

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            destination = parent_path / "facts"

            def create_competitor_then_rename(source: Path, target: Path) -> None:
                target.mkdir()
                (target / "owned-by-someone-else").write_text("keep", encoding="utf-8")
                original_rename(source, target)

            with mock.patch.object(
                write_module,
                "_rename_no_replace",
                side_effect=create_competitor_then_rename,
            ), self.assertRaises(OSError):
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)

            self.assertEqual(
                (destination / "owned-by-someone-else").read_text(encoding="utf-8"),
                "keep",
            )
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_relative_destination_is_anchored_when_write_is_created(self) -> None:
        schema = dp.Schema({"events": {"value": int}})
        original_cwd = Path.cwd()

        with (
            tempfile.TemporaryDirectory() as parent,
            tempfile.TemporaryDirectory() as elsewhere,
        ):
            parent_path = Path(parent)
            elsewhere_path = Path(elsewhere)
            try:
                os.chdir(parent_path)
                transaction = dp.write(schema, destination=Path("facts"))
                with transaction as sink:
                    sink["events"].append(value=7)
                    os.chdir(elsewhere_path)

                destination = parent_path / "facts"
                self.assertEqual(
                    pq.read_table(destination / "events.parquet")["value"].to_pylist(),
                    [7],
                )
                self.assertFalse((elsewhere_path / "facts").exists())
            finally:
                os.chdir(original_cwd)

    def test_dangling_destination_symlink_is_never_followed(self) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            missing_target = parent_path / "missing" / "facts"
            destination = parent_path / "facts"
            try:
                destination.symlink_to(missing_target, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"directory symlinks are unavailable: {error}")

            with self.assertRaises(FileExistsError):
                dp.write(schema, destination=destination)

            self.assertTrue(destination.is_symlink())
            self.assertFalse(missing_target.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_successful_publish_is_not_rolled_back_by_a_late_interrupt(self) -> None:
        schema = dp.Schema({"events": {"value": int}})
        original_rename = write_module._rename_no_replace

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            destination = parent_path / "facts"

            def rename_then_interrupt(source: Path, target: Path) -> None:
                original_rename(source, target)
                raise KeyboardInterrupt("after publication")

            with mock.patch.object(
                write_module,
                "_rename_no_replace",
                side_effect=rename_then_interrupt,
            ), self.assertRaisesRegex(KeyboardInterrupt, "after publication"):
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)

            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT value FROM events"
            )
            self.assertEqual(result.to_rows(), [{"value": 1}])
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_body_cancellation_waits_for_current_io_and_discards_queued_work(
        self,
    ) -> None:
        class ParserError(Exception):
            pass

        schema = dp.Schema({"events": {"sequence": int}})
        write_started = threading.Event()
        release_write = threading.Event()
        cancellation_started = threading.Event()
        producer_finished = threading.Event()
        written_batches: list[int] = []
        producer_errors: list[BaseException] = []
        original_write_rows = parquet_writer_module._ParquetRelationWriter.write_rows

        def block_first_write(
            writer: parquet_writer_module._ParquetRelationWriter,
            rows: tuple[tuple[object | None, ...], ...],
        ) -> None:
            written_batches.append(rows[0][0])
            write_started.set()
            if not release_write.wait(5):
                raise TimeoutError("test did not release the current write")
            original_write_rows(writer, rows)

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=block_first_write,
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"

            def produce() -> None:
                try:
                    with dp.write(schema, destination=destination) as sink:
                        sink["events"].append(sequence=1)
                        if not write_started.wait(5):
                            raise TimeoutError("writer did not start")
                        sink["events"].append(sequence=2)
                        cancellation_started.set()
                        raise ParserError("cancel parsing")
                except BaseException as error:
                    producer_errors.append(error)
                finally:
                    producer_finished.set()

            producer = threading.Thread(target=produce)
            producer.start()
            try:
                self.assertTrue(cancellation_started.wait(5))
                self.assertFalse(producer_finished.is_set())
                self.assertFalse(destination.exists())
            finally:
                release_write.set()
                producer.join(5)

            self.assertFalse(producer.is_alive())
            self.assertEqual(len(producer_errors), 1)
            self.assertIsInstance(producer_errors[0], ParserError)
            self.assertEqual(written_batches, [1])
            self.assertFalse(destination.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-write-")
                    for path in parent_path.iterdir()
                )
            )

    def test_normal_exit_with_a_full_queue_drains_without_deadlock(self) -> None:
        schema = dp.Schema({"events": {"sequence": int}})
        write_started = threading.Event()
        release_write = threading.Event()
        body_finished = threading.Event()
        producer_finished = threading.Event()
        producer_errors: list[BaseException] = []
        original_write_rows = parquet_writer_module._ParquetRelationWriter.write_rows

        def block_first_write(
            writer: parquet_writer_module._ParquetRelationWriter,
            rows: tuple[tuple[object | None, ...], ...],
        ) -> None:
            if rows[0][0] == 1:
                write_started.set()
                if not release_write.wait(5):
                    raise TimeoutError("test did not release the current write")
            original_write_rows(writer, rows)

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module, "_MAX_BATCH_ROWS", 1
        ), mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "write_rows",
            autospec=True,
            side_effect=block_first_write,
        ):
            destination = Path(parent) / "facts"

            def produce() -> None:
                try:
                    with dp.write(schema, destination=destination) as sink:
                        sink["events"].append(sequence=1)
                        if not write_started.wait(5):
                            raise TimeoutError("writer did not start")
                        sink["events"].append(sequence=2)
                        body_finished.set()
                except BaseException as error:
                    producer_errors.append(error)
                finally:
                    producer_finished.set()

            producer = threading.Thread(target=produce)
            producer.start()
            try:
                self.assertTrue(body_finished.wait(5))
                self.assertFalse(producer_finished.is_set())
            finally:
                release_write.set()
                producer.join(5)

            self.assertFalse(producer.is_alive())
            self.assertEqual(producer_errors, [])
            result = dp.DataFusionProvider(catalog=dp.open(root=destination)).query(
                "SELECT sequence FROM events ORDER BY sequence"
            )
            self.assertEqual(result.to_rows(), [{"sequence": 1}, {"sequence": 2}])

    def test_writer_initialization_failure_prevents_the_body_from_running(self) -> None:
        schema = dp.Schema({"events": {"value": int}})
        body_ran = False

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            write_module,
            "_ParquetRelationWriter",
            side_effect=OSError("cannot create parquet writer"),
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with self.assertRaisesRegex(OSError, "cannot create parquet writer"):
                with dp.write(schema, destination=destination):
                    body_ran = True

            self.assertFalse(body_ran)
            self.assertFalse(destination.exists())
            self.assertEqual(tuple(parent_path.iterdir()), ())

    def test_writer_close_failure_prevents_publication(self) -> None:
        schema = dp.Schema({"events": {"value": int}})
        original_close = parquet_writer_module._ParquetRelationWriter.close

        def fail_close(
            writer: parquet_writer_module._ParquetRelationWriter,
        ) -> None:
            original_close(writer)
            raise OSError("cannot close parquet writer")

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            parquet_writer_module._ParquetRelationWriter,
            "close",
            autospec=True,
            side_effect=fail_close,
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with self.assertRaisesRegex(OSError, "cannot close parquet writer"):
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)

            self.assertFalse(destination.exists())
            self.assertEqual(tuple(parent_path.iterdir()), ())

    def test_footer_validation_failure_prevents_publication(self) -> None:
        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent, mock.patch.object(
            parquet_writer_module.pq,
            "read_metadata",
            side_effect=OSError("cannot validate parquet footer"),
        ):
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with self.assertRaisesRegex(OSError, "cannot validate parquet footer"):
                with dp.write(schema, destination=destination) as sink:
                    sink["events"].append(value=1)

            self.assertFalse(destination.exists())
            self.assertEqual(tuple(parent_path.iterdir()), ())

    def test_cleanup_failure_is_attached_without_replacing_the_body_error(self) -> None:
        class ParserError(Exception):
            pass

        schema = dp.Schema({"events": {"value": int}})

        with tempfile.TemporaryDirectory() as parent:
            parent_path = Path(parent)
            destination = parent_path / "facts"
            with mock.patch.object(
                write_module.shutil,
                "rmtree",
                side_effect=OSError("cannot remove staging"),
            ), self.assertRaisesRegex(ParserError, "parser stopped") as raised:
                with dp.write(schema, destination=destination):
                    raise ParserError("parser stopped")

            notes = getattr(raised.exception, "__notes__", [])
            self.assertTrue(
                any("cannot remove staging" in note for note in notes),
                notes,
            )
            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
