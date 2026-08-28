from __future__ import annotations

from datetime import datetime, timezone
from decimal import Decimal
import gc
from pathlib import Path
import tempfile
import unittest

import pyarrow as pa
import pyarrow.parquet as pq

from kat import Duration, WallClockTimestamp
from kat import datasource as ds


class DatasourceParquetTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_parquet(
        self,
        path: Path,
        fields: list[pa.Field],
        values: dict[str, list[object]],
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        schema = pa.schema(fields)
        arrays = [pa.array(values[field.name], type=field.type) for field in fields]
        pq.write_table(pa.Table.from_arrays(arrays, schema=schema), path)

    def test_writer_creates_flat_multitable_catalog_with_batches_and_empty_table(
        self,
    ) -> None:
        schema = ds.Schema(
            {
                "events": {
                    "value": int,
                    "label": str | None,
                    "observed_at": datetime,
                    "amount": Decimal,
                },
                "empty_rows": {"payload": bytes | None},
                "threads": {"thread_id": int},
            }
        )
        destination = self.root / "catalog"

        with ds.write(schema, destination=destination) as writer:
            writer.write("threads", thread_id=[20])
            writer.write(
                "events",
                value=[1],
                label=[None],
                observed_at=[datetime(2026, 8, 28, tzinfo=timezone.utc)],
                amount=[Decimal("1.25")],
            )
            writer.write(
                "events",
                value=[2],
                label=["second"],
                observed_at=[
                    datetime(
                        2026,
                        8,
                        28,
                        8,
                        tzinfo=timezone.utc,
                    )
                ],
                amount=[Decimal("2.500000000000000000")],
            )

        self.assertEqual(
            sorted(path.name for path in destination.iterdir()),
            ["empty_rows.parquet", "events.parquet", "threads.parquet"],
        )
        physical = pq.read_schema(destination / "events.parquet")
        self.assertEqual(physical.field("value"), pa.field("value", pa.int64(), False))
        self.assertEqual(physical.field("label"), pa.field("label", pa.string(), True))
        self.assertEqual(
            physical.field("observed_at"),
            pa.field("observed_at", pa.timestamp("ns", tz="UTC"), False),
        )
        self.assertEqual(
            physical.field("amount"),
            pa.field("amount", pa.decimal128(38, 18), False),
        )
        self.assertEqual(pq.read_table(destination / "empty_rows.parquet").num_rows, 0)

        result = ds.open(schema, root=destination).query(
            "SELECT value, label, amount FROM events WHERE value >= $minimum ORDER BY value",
            params={"minimum": 1},
        )
        self.assertEqual(
            result.to_rows(),
            [
                {"value": 1, "label": None, "amount": Decimal("1.250000000000000000")},
                {
                    "value": 2,
                    "label": "second",
                    "amount": Decimal("2.500000000000000000"),
                },
            ],
        )

    def test_writer_requires_a_new_path_and_strict_lists(self) -> None:
        schema = ds.Schema({"events": {"value": int, "label": str}})
        existing = self.root / "existing"
        existing.mkdir()
        sentinel = existing / "belongs-to-the-caller.txt"
        sentinel.write_text("keep", encoding="utf-8")

        with self.assertRaises(FileExistsError):
            with ds.write(schema, destination=existing):
                pass
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

        destination = self.root / "catalog"
        with self.assertRaisesRegex(TypeError, "list"):
            with ds.write(schema, destination=destination) as writer:
                writer.write("events", value=(1,), label=["one"])
        self.assertFalse(destination.exists())

    def test_writer_failure_poison_survives_a_caught_error_and_cleans_up(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        destination = self.root / "catalog"
        first_error: BaseException | None = None

        with self.assertRaisesRegex(TypeError, "value"):
            with ds.write(schema, destination=destination) as writer:
                try:
                    writer.write("events", value=["not-an-int"])
                except TypeError as error:
                    first_error = error
                with self.assertRaises(TypeError) as retry:
                    writer.write("events", value=[1])
                self.assertIs(retry.exception, first_error)

        self.assertFalse(destination.exists())

    def test_writer_body_failure_cleans_up_and_keeps_the_body_error(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        destination = self.root / "catalog"

        with self.assertRaisesRegex(RuntimeError, "parser failed"):
            with ds.write(schema, destination=destination) as writer:
                writer.write("events", value=[1])
                raise RuntimeError("parser failed")

        self.assertFalse(destination.exists())

    def test_open_root_discovers_only_direct_lowercase_parquet_files(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        catalog_root = self.root / "catalog"
        self.write_parquet(
            catalog_root / "events.parquet",
            [pa.field("value", pa.int32(), False)],
            {"value": [1]},
        )
        self.write_parquet(
            catalog_root / "nested" / "ignored.parquet",
            [pa.field("value", pa.int32(), False)],
            {"value": [99]},
        )
        (catalog_root / "notes.txt").write_text("ignored", encoding="utf-8")
        (catalog_root / "upper.PARQUET").write_bytes(b"ignored")

        result = ds.open(schema, root=catalog_root).query("SELECT value FROM events")

        self.assertEqual(result["value"], (1,))

        self.write_parquet(
            catalog_root / "extra.parquet",
            [pa.field("value", pa.int32(), False)],
            {"value": [2]},
        )
        with self.assertRaisesRegex(ValueError, "table set"):
            ds.open(schema, root=catalog_root)

    def test_open_tables_recurses_parts_without_hive_inference(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        parts = self.root / "parts"
        self.write_parquet(
            parts / "z" / "02.parquet",
            [pa.field("value", pa.uint16(), False)],
            {"value": [2]},
        )
        self.write_parquet(
            parts / "partition=seven" / "01.parquet",
            [pa.field("value", pa.uint16(), False)],
            {"value": [1]},
        )
        (parts / "ignored.json").write_text("{}", encoding="utf-8")

        catalog = ds.open(schema, tables={"events": parts})

        self.assertEqual(
            catalog.query("SELECT value FROM events ORDER BY value")["value"],
            (1, 2),
        )
        with self.assertRaises(Exception):
            catalog.query("SELECT partition FROM events")

        (self.root / "empty").mkdir()
        with self.assertRaisesRegex(ValueError, "at least one"):
            ds.open(schema, tables={"events": self.root / "empty"})

    def test_open_requires_exactly_one_location_form_and_exact_table_keys(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        file = self.root / "events.parquet"
        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [1]},
        )

        with self.assertRaisesRegex(TypeError, "exactly one"):
            ds.open(schema)
        with self.assertRaisesRegex(TypeError, "exactly one"):
            ds.open(schema, root=self.root, tables={"events": file})
        with self.assertRaisesRegex(ValueError, "table set"):
            ds.open(schema, tables={"other": file})
        with self.assertRaisesRegex(TypeError, "Path"):
            ds.open(schema, tables={"events": str(file)})

    def test_open_explicit_single_file_does_not_require_a_parquet_suffix(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        file = self.root / "opaque-parser-output"
        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [4]},
        )

        result = ds.open(schema, tables={"events": file}).query(
            "SELECT value FROM events"
        )

        self.assertEqual(result["value"], (4,))

    def test_open_validates_logical_types_columns_order_and_nullability(self) -> None:
        accepted = [
            (bool, pa.bool_()),
            (int, pa.int8()),
            (int, pa.uint64()),
            (float, pa.float32()),
            (float, pa.float64()),
            (str, pa.string()),
            (str, pa.large_string()),
            (bytes, pa.binary()),
            (bytes, pa.large_binary()),
            (datetime, pa.timestamp("ns", tz="UTC")),
            (Decimal, pa.decimal128(20, 4)),
            (Decimal, pa.decimal256(60, 30)),
        ]
        string_view = getattr(pa, "string_view", None)
        if string_view is not None:
            accepted.append((str, string_view()))

        for index, (logical, physical) in enumerate(accepted):
            with self.subTest(logical=logical, physical=physical):
                file = self.root / f"accepted-{index}.parquet"
                self.write_parquet(
                    file,
                    [pa.field("value", physical, False)],
                    {"value": []},
                )
                ds.open(ds.Schema({"events": {"value": logical}}), tables={"events": file})

        nullable_file = self.root / "nullable.parquet"
        self.write_parquet(
            nullable_file,
            [pa.field("value", pa.int64(), True)],
            {"value": []},
        )
        with self.assertRaisesRegex(TypeError, "nullable"):
            ds.open(ds.Schema({"events": {"value": int}}), tables={"events": nullable_file})
        ds.open(
            ds.Schema({"events": {"value": int | None}}),
            tables={"events": nullable_file},
        )

        required_file = self.root / "required.parquet"
        self.write_parquet(
            required_file,
            [pa.field("value", pa.int64(), False)],
            {"value": []},
        )
        ds.open(
            ds.Schema({"events": {"value": int | None}}),
            tables={"events": required_file},
        )

        wrong_order = self.root / "wrong-order.parquet"
        self.write_parquet(
            wrong_order,
            [
                pa.field("second", pa.int64(), False),
                pa.field("first", pa.int64(), False),
            ],
            {"second": [], "first": []},
        )
        with self.assertRaisesRegex(ValueError, "columns"):
            ds.open(
                ds.Schema({"events": {"first": int, "second": int}}),
                tables={"events": wrong_order},
            )

        wrong_type = self.root / "wrong-type.parquet"
        self.write_parquet(
            wrong_type,
            [pa.field("value", pa.float16(), False)],
            {"value": []},
        )
        with self.assertRaisesRegex(TypeError, "type"):
            ds.open(
                ds.Schema({"events": {"value": float}}),
                tables={"events": wrong_type},
            )

    def test_open_requires_identical_part_schemas_and_valid_footers(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        parts = self.root / "parts"
        self.write_parquet(
            parts / "a.parquet",
            [pa.field("value", pa.int32(), False)],
            {"value": [1]},
        )
        self.write_parquet(
            parts / "b.parquet",
            [pa.field("value", pa.int64(), False)],
            {"value": [2]},
        )
        with self.assertRaisesRegex(TypeError, "same physical schema"):
            ds.open(schema, tables={"events": parts})

        damaged = self.root / "damaged.parquet"
        damaged.write_bytes(b"not parquet")
        with self.assertRaises(Exception):
            ds.open(schema, tables={"events": damaged})

    def test_open_ignores_arrow_schema_and_field_metadata_between_parts(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        parts = self.root / "parts"
        first = pa.schema(
            [pa.field("value", pa.int64(), False, metadata={b"field": b"first"})],
            metadata={b"schema": b"first"},
        )
        second = pa.schema(
            [pa.field("value", pa.int64(), False, metadata={b"field": b"second"})],
            metadata={b"schema": b"second"},
        )
        parts.mkdir()
        pq.write_table(
            pa.Table.from_arrays([pa.array([1], type=pa.int64())], schema=first),
            parts / "a.parquet",
        )
        pq.write_table(
            pa.Table.from_arrays([pa.array([2], type=pa.int64())], schema=second),
            parts / "b.parquet",
        )

        result = ds.open(schema, tables={"events": parts}).query(
            "SELECT value FROM events ORDER BY value"
        )

        self.assertEqual(result["value"], (1, 2))

    def test_catalog_query_is_eager_detached_reusable_and_live(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        file = self.root / "events.parquet"
        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [1, 2]},
        )
        catalog = ds.open(schema, tables={"events": file})

        first = catalog.query("SELECT value FROM events ORDER BY value")
        with self.assertRaises(Exception):
            catalog.query("SELECT missing FROM events")
        self.assertEqual(
            catalog.query("SELECT SUM(value) AS total FROM events")["total"],
            (3,),
        )

        file.unlink()
        del catalog
        gc.collect()
        self.assertEqual(first.to_rows(), [{"value": 1}, {"value": 2}])

        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [7]},
        )
        live = ds.open(schema, tables={"events": file})
        self.assertEqual(live.query("SELECT value FROM events")["value"], (7,))
        file.unlink()
        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [8]},
        )
        self.assertEqual(live.query("SELECT value FROM events")["value"], (8,))

    def test_catalog_query_accepts_read_only_single_statements_and_named_scalars(self) -> None:
        schema = ds.Schema({"events": {"value": int}})
        file = self.root / "events.parquet"
        self.write_parquet(
            file,
            [pa.field("value", pa.int64(), False)],
            {"value": [1]},
        )
        catalog = ds.open(schema, tables={"events": file})

        self.assertEqual(
            catalog.query(
                "SELECT $truth AS truth, $number AS number, $ratio AS ratio, "
                "$text AS text, $duration AS duration, $instant AS instant",
                params={
                    "truth": True,
                    "number": 3,
                    "ratio": 1.5,
                    "text": "value",
                    "duration": Duration("2ns"),
                    "instant": WallClockTimestamp("2026-08-28T00:00:00.000000001Z"),
                },
            )["number"],
            (3,),
        )
        self.assertEqual(catalog.query("VALUES (1)")["column1"], (1,))
        self.assertGreater(len(catalog.query("DESCRIBE events")), 0)
        self.assertGreater(len(catalog.query("EXPLAIN SELECT * FROM events")), 0)
        self.assertGreater(len(catalog.query("SHOW TABLES")), 0)

        for sql in (
            "SELECT 1; SELECT 2",
            "CREATE TABLE bad AS VALUES (1)",
            "INSERT INTO events VALUES (2)",
            "COPY events TO 'bad.parquet'",
            "SET datafusion.execution.batch_size = 1",
        ):
            with self.subTest(sql=sql), self.assertRaises(Exception):
                catalog.query(sql)

        with self.assertRaisesRegex(TypeError, "non-empty"):
            catalog.query(" ")
        with self.assertRaisesRegex(ValueError, "parameter name"):
            catalog.query("SELECT 1", params={"Bad-Name": 1})
        with self.assertRaisesRegex(TypeError, "finite float"):
            catalog.query("SELECT $value", params={"value": float("inf")})
        with self.assertRaises(TypeError):
            catalog.query("SHOW FUNCTIONS")


if __name__ == "__main__":
    unittest.main()
