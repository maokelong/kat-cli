from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import uuid

import pyarrow as pa
import pyarrow.parquet as pq

from kat import Duration, WallClockTimestamp
from kat import dataprovider as dp


class DataProviderParquetTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_parquet(
        self,
        path: Path,
        table: pa.Table,
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        pq.write_table(table, path)

    def test_open_root_discovers_a_schema_less_read_only_catalog(self) -> None:
        catalog_root = self.root / "catalog"
        self.write_parquet(
            catalog_root / "threads.parquet",
            pa.table({"thread_id": [2]}),
        )
        self.write_parquet(
            catalog_root / "events.parquet",
            pa.table({"value": [1]}),
        )

        catalog = dp.open(root=catalog_root)

        self.assertEqual(catalog.tables, ("events", "threads"))
        self.assertFalse(hasattr(catalog, "query"))
        self.assertFalse(hasattr(catalog, "schema"))
        self.assertFalse(hasattr(catalog, "close"))
        with self.assertRaisesRegex(TypeError, "dp.open"):
            dp.Catalog({})  # type: ignore[arg-type]

    def test_open_root_rejects_a_linked_catalog_directory(self) -> None:
        target = self.root / "outside-catalog"
        self.write_parquet(target / "events.parquet", pa.table({"value": [1]}))
        linked = self.root / "linked-catalog"
        if os.name == "nt":
            result = subprocess.run(
                ["cmd.exe", "/d", "/c", "mklink", "/j", str(linked), str(target)],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                self.skipTest(f"could not create a Windows junction: {result.stderr}")
        else:
            linked.symlink_to(target, target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "ordinary directory"):
            dp.open(root=linked)

    def test_open_root_rejects_a_linked_relation_file(self) -> None:
        catalog = self.root / "catalog"
        catalog.mkdir()
        target = self.root / "outside.parquet"
        self.write_parquet(target, pa.table({"value": [1]}))
        linked = catalog / "events.parquet"
        try:
            linked.symlink_to(target)
        except OSError as error:
            if os.name == "nt" and getattr(error, "winerror", None) == 1314:
                self.skipTest("creating file symlinks requires Windows developer mode")
            raise

        with self.assertRaisesRegex(ValueError, "ordinary Parquet file"):
            dp.open(root=catalog)

    def test_datafusion_provider_reuses_immutable_memory_bindings(
        self,
    ) -> None:
        events = dp.Table.from_arrow(pa.table({"value": [1]}))
        bindings = {"events": events}
        fusion = dp.DataFusionProvider(tables=bindings)

        first = fusion.query("SELECT value FROM events ORDER BY value")
        bindings.clear()
        second = fusion.query("SELECT value FROM events ORDER BY value")

        self.assertEqual(first["value"], (1,))
        self.assertEqual(second["value"], (1,))

    def test_explicit_parts_catalog_is_queried_without_hive_inference(self) -> None:
        parts = self.root / "parts"
        self.write_parquet(
            parts / "z" / "02.parquet",
            pa.table({"value": pa.array([2], type=pa.uint16())}),
        )
        self.write_parquet(
            parts / "partition=seven" / "01.parquet",
            pa.table({"value": pa.array([1], type=pa.uint16())}),
        )
        (parts / "ignored.json").write_text("{}", encoding="utf-8")

        catalog = dp.open(tables={"events": parts})
        result = dp.DataFusionProvider(catalog=catalog).query(
            "SELECT value FROM events ORDER BY value"
        )

        self.assertEqual(catalog.tables, ("events",))
        self.assertEqual(result["value"], (1, 2))
        with self.assertRaises(Exception):
            dp.DataFusionProvider(catalog=catalog).query(
                "SELECT partition FROM events"
            )

    def test_datafusion_provider_joins_memory_and_parquet_relations(self) -> None:
        file = self.root / "owners.parquet"
        self.write_parquet(
            file,
            pa.table(
                {
                    "thread_id": pa.array([1, 2], type=pa.int64()),
                    "owner": ["render", "system"],
                }
            ),
        )
        samples = dp.Table.from_arrow(
            pa.table({"thread_id": [1], "cpu": [0.75]})
        )

        result = dp.DataFusionProvider(
            tables={"samples": samples},
            catalog=dp.open(tables={"owners": file}),
        ).query(
            "SELECT o.owner, s.cpu FROM samples s "
            "JOIN owners o USING (thread_id)"
        )

        self.assertEqual(result.to_rows(), [{"owner": "render", "cpu": 0.75}])

    def test_datafusion_provider_rejects_invalid_or_overlapping_bindings(self) -> None:
        table = dp.Table.from_arrow(pa.table({"value": [1]}))
        file = self.root / "events.parquet"
        self.write_parquet(file, pa.table({"value": [1]}))
        catalog = dp.open(tables={"events": file})

        with self.assertRaisesRegex(ValueError, "requires"):
            dp.DataFusionProvider()
        with self.assertRaisesRegex(ValueError, "relation name"):
            dp.DataFusionProvider(tables={"Bad-Name": table})
        with self.assertRaisesRegex(TypeError, "dp.Table"):
            dp.DataFusionProvider(tables={"events": object()})
        with self.assertRaisesRegex(ValueError, "overlap"):
            dp.DataFusionProvider(tables={"events": table}, catalog=catalog)
        with self.assertRaisesRegex(TypeError, "cannot be subclassed"):
            class CustomCatalog(dp.Catalog):
                pass

        fusion = dp.DataFusionProvider(tables={"events": table})
        for method in ("register", "remove", "replace"):
            with self.subTest(method=method):
                self.assertFalse(hasattr(fusion, method))

    def test_query_normalizes_the_explicit_scalar_parameter_set(self) -> None:
        fusion = dp.DataFusionProvider(
            tables={"unused": dp.Table.from_arrow(pa.table({"value": [1]}))}
        )

        result = fusion.query(
            "SELECT $truth AS truth, $number AS number, $ratio AS ratio, "
            "$text AS text, $payload AS payload, $instant AS instant, "
            "$wall AS wall, $amount AS amount, $duration AS duration",
            params={
                "truth": True,
                "number": 3,
                "ratio": 1.5,
                "text": "value",
                "payload": b"bytes",
                "instant": datetime(
                    2026,
                    8,
                    29,
                    8,
                    tzinfo=timezone(timedelta(hours=8)),
                ),
                "wall": WallClockTimestamp("2026-08-29T00:00:00.000000001Z"),
                "amount": Decimal("1E-40"),
                "duration": Duration("2ns"),
            },
        )

        self.assertEqual(
            result.to_rows(),
            [
                {
                    "truth": True,
                    "number": 3,
                    "ratio": 1.5,
                    "text": "value",
                    "payload": b"bytes",
                    "instant": WallClockTimestamp("2026-08-29T00:00:00Z"),
                    "wall": WallClockTimestamp("2026-08-29T00:00:00.000000001Z"),
                    "amount": Decimal("1E-40"),
                    "duration": 2,
                }
            ],
        )

    def test_query_rejects_ambiguous_parameters_without_poisoning_the_provider(
        self,
    ) -> None:
        fusion = dp.DataFusionProvider(
            tables={"unused": dp.Table.from_arrow(pa.table({"value": [1]}))}
        )
        invalid = (
            None,
            2**63,
            float("inf"),
            bytearray(b"bytes"),
            memoryview(b"bytes"),
            datetime(2026, 8, 29),
            timedelta(seconds=1),
            Decimal("NaN"),
            Decimal("1E-77"),
            Decimal("1E+1000000"),
            pa.scalar(1),
            [1],
        )

        for value in invalid:
            with self.subTest(value=value), self.assertRaises((TypeError, ValueError)):
                fusion.query("SELECT $value AS value", params={"value": value})

        with self.assertRaisesRegex(ValueError, "parameter name"):
            fusion.query("SELECT 1", params={"Bad-Name": 1})
        self.assertEqual(fusion.query("SELECT 7 AS value")["value"], (7,))

    def test_wide_catalog_requires_a_standard_planned_result_schema(self) -> None:
        file = self.root / "wide.parquet"
        self.write_parquet(
            file,
            pa.table(
                {
                    "items": pa.array([[1, 2]], type=pa.list_(pa.int64())),
                    "details": pa.array(
                        [{"label": "ready"}],
                        type=pa.struct([pa.field("label", pa.string())]),
                    ),
                    "day": pa.array([0], type=pa.date32()),
                    "elapsed": pa.array([5], type=pa.duration("ns")),
                }
            ),
        )
        fusion = dp.DataFusionProvider(
            catalog=dp.open(tables={"wide": file})
        )

        with self.assertRaisesRegex(TypeError, "unsupported Arrow type"):
            fusion.query("SELECT * FROM wide")

        result = fusion.query(
            "SELECT cardinality(items) AS item_count, "
            "details['label'] AS label, CAST(day AS VARCHAR) AS day_text, "
            "CAST(elapsed AS BIGINT) AS elapsed_ns FROM wide"
        )
        self.assertEqual(
            result.to_rows(),
            [
                {
                    "item_count": 2,
                    "label": "ready",
                    "day_text": "1970-01-01",
                    "elapsed_ns": 5,
                }
            ],
        )

    def test_query_accepts_one_read_only_statement_and_rejects_session_mutation(
        self,
    ) -> None:
        events = dp.Table.from_arrow(pa.table({"value": [1]}))
        fusion = dp.DataFusionProvider(tables={"events": events})

        self.assertEqual(fusion.query("VALUES (1)")["column1"], (1,))
        self.assertGreater(len(fusion.query("DESCRIBE events")), 0)
        self.assertGreater(len(fusion.query("EXPLAIN SELECT * FROM events")), 0)
        self.assertGreater(len(fusion.query("SHOW TABLES")), 0)

        for sql in (
            "SELECT 1; SELECT 2",
            "CREATE TABLE bad AS VALUES (1)",
            "INSERT INTO events VALUES (2)",
            "COPY events TO 'bad.parquet'",
            "SET datafusion.execution.batch_size = 1",
            "SHOW FUNCTIONS",
        ):
            with self.subTest(sql=sql), self.assertRaises(Exception):
                fusion.query(sql)

        with self.assertRaisesRegex(TypeError, "non-empty"):
            fusion.query(" ")

    def test_open_validates_discovery_paths_names_footers_and_part_schemas(self) -> None:
        root = self.root / "root"
        self.write_parquet(root / "events.parquet", pa.table({"value": [1]}))
        self.write_parquet(root / "nested" / "ignored.parquet", pa.table({"value": [2]}))
        (root / "notes.txt").write_text("ignored", encoding="utf-8")

        self.assertEqual(dp.open(root=root).tables, ("events",))
        with self.assertRaisesRegex(TypeError, "exactly one"):
            dp.open()
        with self.assertRaisesRegex(TypeError, "exactly one"):
            dp.open(root=root, tables={"events": root / "events.parquet"})
        catalog = dp.open(tables={})
        self.assertEqual(catalog.tables, ())
        fusion = dp.DataFusionProvider(catalog=catalog)
        self.assertEqual(fusion.query("SELECT 42 AS answer")["answer"], (42,))
        with self.assertRaises(Exception):
            fusion.query("SELECT * FROM missing")
        empty_root = self.root / "not-materialized"
        empty_root.mkdir()
        with self.assertRaisesRegex(ValueError, "at least one"):
            dp.open(root=empty_root)
        with self.assertRaisesRegex(TypeError, "Path"):
            dp.open(tables={"events": str(root / "events.parquet")})

        self.write_parquet(root / "Bad-Name.parquet", pa.table({"value": [1]}))
        with self.assertRaisesRegex(ValueError, "table name"):
            dp.open(root=root)

        parts = self.root / "mismatched"
        self.write_parquet(
            parts / "a.parquet",
            pa.table({"value": pa.array([1], type=pa.int32())}),
        )
        self.write_parquet(
            parts / "b.parquet",
            pa.table({"value": pa.array([2], type=pa.int64())}),
        )
        with self.assertRaisesRegex(TypeError, "same physical schema"):
            dp.open(tables={"events": parts})

        damaged = self.root / "damaged.parquet"
        damaged.write_bytes(b"not parquet")
        with self.assertRaises(Exception):
            dp.open(tables={"events": damaged})

        empty_column = self.root / "empty-column.parquet"
        self.write_parquet(
            empty_column,
            pa.Table.from_arrays([pa.array([1])], names=[""]),
        )
        with self.assertRaisesRegex(ValueError, "column name"):
            dp.open(tables={"events": empty_column})

        extension = self.root / "extension.parquet"
        self.write_parquet(
            extension,
            pa.table(
                {
                    "value": pa.array(
                        [uuid.UUID(int=0)],
                        type=pa.uuid(),
                    )
                }
            ),
        )
        with self.assertRaisesRegex(TypeError, "unsupported Catalog Arrow type"):
            dp.open(tables={"events": extension})


if __name__ == "__main__":
    unittest.main()
