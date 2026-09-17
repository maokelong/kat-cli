from __future__ import annotations

import unittest
from datetime import datetime, timedelta, timezone
from decimal import Decimal
import math
from types import MappingProxyType

import pyarrow as pa

from kat import dataprovider as dp
from kat import WallClockTimestamp


class SchemaTest(unittest.TestCase):
    def test_schema_copies_freezes_and_preserves_the_declaration_order(self) -> None:
        columns: dict[str, object] = {
            "Observed At": datetime,
            "value": int | None,
        }
        declarations = {"events": columns, "metrics": {"ratio": float}}

        schema = dp.Schema(declarations)
        columns["late"] = str
        declarations["other"] = {"value": int}

        self.assertEqual(schema.tables, ("events", "metrics"))
        self.assertEqual(
            tuple(schema["events"].items()),
            (("Observed At", datetime), ("value", int | None)),
        )
        self.assertIsInstance(schema["events"], MappingProxyType)
        with self.assertRaises(TypeError):
            schema["events"]["value"] = str  # type: ignore[index]
        with self.assertRaises(KeyError):
            schema["missing"]

    def test_schema_rejects_empty_or_invalid_declarations(self) -> None:
        invalid = [
            {},
            {"events": {}},
            {"BadName": {"value": int}},
            {"con": {"value": int}},
            {"events": {"": int}},
            {"events": {"\ud800": str}},
            {"events": {1: int}},
            {"events": {"value": object}},
            {"events": {"value": int | str}},
            {"events": {"value": int | str | None}},
        ]
        for declaration in invalid:
            with self.subTest(declaration=declaration), self.assertRaises(
                (TypeError, ValueError)
            ):
                dp.Schema(declaration)  # type: ignore[arg-type]

    def test_schema_accepts_all_seven_types_and_nullable_variants(self) -> None:
        logical_types = (bool, int, float, str, bytes, datetime, Decimal)
        declaration = {
            "values": {
                **{value.__name__: value for value in logical_types},
                **{f"optional_{value.__name__}": value | None for value in logical_types},
            }
        }
        schema = dp.Schema(declaration)
        self.assertEqual(tuple(schema["values"]), tuple(declaration["values"]))

    def test_schema_does_not_create_mutable_tables(self) -> None:
        schema = dp.Schema({"events": {"timestamp": int}})

        self.assertFalse(hasattr(schema, "create"))

    def test_schema_cannot_be_subclassed(self) -> None:
        with self.assertRaisesRegex(TypeError, "cannot be subclassed"):

            class DerivedSchema(dp.Schema):
                pass


class ImmutableTableTest(unittest.TestCase):
    def test_table_can_only_be_created_from_completion_factories(self) -> None:
        with self.assertRaisesRegex(TypeError, "Table.from_arrow.*Table.from_rows"):
            dp.Table({"value": int})

    def test_completed_table_has_no_append_capability(self) -> None:
        arrow = pa.table({"value": pa.array([1], type=pa.int64())})
        table = dp.Table.from_arrow(arrow)

        self.assertFalse(hasattr(table, "append"))
        self.assertIs(table.to_arrow(), arrow)

    def test_table_can_be_created_eagerly_from_completed_rows(self) -> None:
        schema = pa.schema(
            [
                pa.field("id", pa.int64(), nullable=False),
                pa.field("label", pa.string(), nullable=True),
            ],
            metadata={b"source": b"query"},
        )
        rows = [{"label": "one", "id": 1}, {"label": None, "id": 2}]

        table = dp.Table.from_rows(rows, schema=schema)
        rows[0]["id"] = 99

        self.assertEqual(
            table.to_rows(),
            [{"id": 1, "label": "one"}, {"id": 2, "label": None}],
        )
        self.assertTrue(table.to_arrow().schema.equals(schema, check_metadata=True))

    def test_from_rows_requires_mapping_rows_with_exact_fields(self) -> None:
        schema = pa.schema(
            [
                pa.field("id", pa.int64(), nullable=False),
                pa.field("label", pa.string(), nullable=True),
            ]
        )

        invalid_rows = [
            [(1, "one")],
            [{"id": 1}],
            [{"id": 1, "label": "one", "extra": True}],
        ]
        for rows in invalid_rows:
            with self.subTest(rows=rows), self.assertRaises((TypeError, ValueError)):
                dp.Table.from_rows(rows, schema=schema)  # type: ignore[arg-type]

    def test_from_rows_accepts_every_admitted_physical_value_family(self) -> None:
        fields_and_values = {
            "boolean": (pa.bool_(), True),
            "signed": (pa.int8(), -128),
            "unsigned": (pa.uint64(), 2**64 - 1),
            "float16": (pa.float16(), 1.1),
            "float32": (pa.float32(), -3.25),
            "float64": (pa.float64(), math.inf),
            "utf8": (pa.string(), "text"),
            "large_utf8": (pa.large_string(), "text"),
            "utf8_view": (pa.string_view(), "text"),
            "binary": (pa.binary(), b"bytes"),
            "large_binary": (pa.large_binary(), b"bytes"),
            "timestamp": (
                pa.timestamp("ns", tz="UTC"),
                WallClockTimestamp("2026-08-28T04:30:00.123456789Z"),
            ),
            "decimal128": (pa.decimal128(10, 2), Decimal("12.30")),
            "decimal256": (pa.decimal256(50, 20), Decimal("1.2")),
            "nullable": (pa.int16(), None),
        }
        schema = pa.schema(
            [
                pa.field(name, data_type, nullable=True)
                for name, (data_type, _) in fields_and_values.items()
            ]
        )

        table = dp.Table.from_rows(
            [{name: value for name, (_, value) in fields_and_values.items()}],
            schema=schema,
        )

        self.assertEqual(table.to_arrow().schema, schema)
        self.assertEqual(table["signed"], (-128,))
        self.assertEqual(table["unsigned"], (2**64 - 1,))
        self.assertAlmostEqual(table["float16"][0], 1.099609375)
        self.assertEqual(
            table["timestamp"],
            (WallClockTimestamp("2026-08-28T04:30:00.123456789Z"),),
        )
        self.assertEqual(
            table["decimal256"],
            (Decimal("1.20000000000000000000"),),
        )
        self.assertEqual(table["nullable"], (None,))

        aware = dp.Table.from_rows(
            [
                {
                    "at": datetime(
                        2026,
                        8,
                        28,
                        12,
                        30,
                        tzinfo=timezone(timedelta(hours=8)),
                    )
                }
            ],
            schema=pa.schema(
                [pa.field("at", pa.timestamp("ns", tz="UTC"), nullable=False)]
            ),
        )
        self.assertEqual(
            aware["at"],
            (WallClockTimestamp("2026-08-28T04:30:00Z"),),
        )

    def test_from_rows_rejects_coercion_range_null_and_rounding(self) -> None:
        cases = [
            (pa.bool_(), 1),
            (pa.int8(), True),
            (pa.int8(), -129),
            (pa.int8(), 128),
            (pa.uint8(), -1),
            (pa.uint8(), 256),
            (pa.float16(), 1),
            (pa.float16(), 70_000.0),
            (pa.float32(), 3.5e38),
            (pa.string(), str.__new__(type("Text", (str,), {}), "text")),
            (pa.string(), "\ud800"),
            (pa.binary(), bytearray(b"bytes")),
            (pa.timestamp("ns", tz="UTC"), datetime(2026, 8, 28)),
            (pa.timestamp("ns", tz="UTC"), "2026-08-28T00:00:00Z"),
            (pa.timestamp("ns", tz="UTC"), 0),
            (pa.timestamp("ns", tz="UTC"), datetime(1600, 1, 1, tzinfo=timezone.utc)),
            (pa.timestamp("ns", tz="UTC"), datetime(2300, 1, 1, tzinfo=timezone.utc)),
            (pa.decimal128(4, 2), Decimal("1.001")),
            (pa.decimal128(4, 2), Decimal("100.00")),
            (pa.decimal128(4, 2), Decimal("NaN")),
            (pa.decimal128(4, 2), 1),
        ]
        for data_type, value in cases:
            with self.subTest(data_type=data_type, value=value), self.assertRaises(
                (TypeError, ValueError)
            ):
                dp.Table.from_rows(
                    [{"value": value}],
                    schema=pa.schema(
                        [pa.field("value", data_type, nullable=False)]
                    ),
                )

        with self.assertRaisesRegex(ValueError, "row 0, column 'value'.*not nullable"):
            dp.Table.from_rows(
                [{"value": None}],
                schema=pa.schema(
                    [pa.field("value", pa.int64(), nullable=False)]
                ),
            )

    def test_from_rows_validates_schema_before_consuming_and_supports_empty_rows(
        self,
    ) -> None:
        consumed = False

        def rows():
            nonlocal consumed
            consumed = True
            yield {"value": 1}

        with self.assertRaisesRegex(TypeError, "pyarrow.Schema"):
            dp.Table.from_rows(rows(), schema={"value": int})  # type: ignore[arg-type]
        self.assertFalse(consumed)

        schema = pa.schema(
            [pa.field("value", pa.int16(), nullable=False)],
            metadata={b"source": b"empty-query"},
        )
        table = dp.Table.from_rows(iter(()), schema=schema)
        self.assertEqual(len(table), 0)
        self.assertTrue(table.to_arrow().schema.equals(schema, check_metadata=True))

    def test_from_rows_consumes_a_row_iterable_once(self) -> None:
        class Rows:
            calls = 0

            def __iter__(self):
                self.calls += 1
                if self.calls > 1:
                    raise AssertionError("rows iterable was consumed more than once")
                yield {"value": 7}

        rows = Rows()
        table = dp.Table.from_rows(
            rows,
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )

        self.assertEqual(table.to_rows(), [{"value": 7}])
        self.assertEqual(rows.calls, 1)

    def test_reads_are_reusable_without_copying_the_arrow_backing(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.array([1, 2]), pa.array(["one", None])],
            schema=pa.schema(
                [
                    pa.field("id", pa.int64(), nullable=False),
                    pa.field("label", pa.string(), nullable=True),
                ]
            ),
        )
        table = dp.Table.from_arrow(arrow)

        self.assertEqual(len(table), 2)
        self.assertEqual(table.columns, ("id", "label"))
        self.assertEqual(table["id"], (1, 2))
        self.assertEqual(table["label"], ("one", None))
        first = table.to_rows()
        first[0]["id"] = 99
        first.append({"id": 3, "label": "three"})

        self.assertEqual(
            table.to_rows(),
            [{"id": 1, "label": "one"}, {"id": 2, "label": None}],
        )
        self.assertIs(table.to_arrow(), arrow)

    def test_table_cannot_be_subclassed_or_have_attributes_reassigned(self) -> None:
        with self.assertRaisesRegex(TypeError, "cannot be subclassed"):

            class DerivedTable(dp.Table):
                pass

        table = dp.Table.from_arrow(pa.table({"value": [1]}))
        with self.assertRaisesRegex(AttributeError, "immutable"):
            table.extra = "value"  # type: ignore[attr-defined]


class ArrowTableTest(unittest.TestCase):
    def test_bridge_preserves_the_table_and_all_admitted_physical_types(self) -> None:
        values_and_types = {
            "boolean": (True, pa.bool_()),
            "int8": (-1, pa.int8()),
            "int16": (-1, pa.int16()),
            "int32": (-1, pa.int32()),
            "int64": (-1, pa.int64()),
            "uint8": (1, pa.uint8()),
            "uint16": (1, pa.uint16()),
            "uint32": (1, pa.uint32()),
            "uint64": (1, pa.uint64()),
            "float16": (1.5, pa.float16()),
            "float32": (1.5, pa.float32()),
            "float64": (1.5, pa.float64()),
            "utf8": ("text", pa.string()),
            "large_utf8": ("text", pa.large_string()),
            "utf8_view": ("text", pa.string_view()),
            "binary": (b"bytes", pa.binary()),
            "large_binary": (b"bytes", pa.large_binary()),
            "timestamp": (0, pa.timestamp("ns", tz="UTC")),
            "decimal128": (Decimal("1.20"), pa.decimal128(10, 2)),
            "decimal256": (Decimal("1.20"), pa.decimal256(50, 2)),
        }
        arrays = [
            pa.array([value], type=data_type)
            for value, data_type in values_and_types.values()
        ]
        fields = [
            pa.field(name, array.type, nullable=True)
            for name, array in zip(values_and_types, arrays, strict=True)
        ]
        arrow = pa.Table.from_arrays(arrays, schema=pa.schema(fields))

        table = dp.Table.from_arrow(arrow)

        self.assertIs(table.to_arrow(), arrow)
        self.assertEqual(table.columns, tuple(field.name for field in fields))
        self.assertEqual(table["timestamp"], (WallClockTimestamp("1970-01-01T00:00:00Z"),))
        self.assertEqual(table["decimal128"], (Decimal("1.20"),))

    def test_timestamp_projection_preserves_all_nine_fractional_digits(self) -> None:
        arrow = pa.table(
            {"at": pa.array([1_725_000_000_123_456_789], type=pa.timestamp("ns", tz="UTC"))}
        )
        table = dp.Table.from_arrow(arrow)
        self.assertEqual(
            table["at"],
            (WallClockTimestamp("2024-08-30T06:40:00.123456789Z"),),
        )

    def test_bridge_rejects_invalid_names_and_unsupported_types(self) -> None:
        invalid_tables = [
            pa.Table.from_arrays([], schema=pa.schema([])),
            pa.Table.from_arrays([pa.array([1])], names=[""]),
            pa.Table.from_arrays([pa.array([1]), pa.array([2])], names=["same", "same"]),
            pa.table({"value": pa.nulls(1)}),
            pa.table({"value": pa.array([[1]], type=pa.list_(pa.int64()))}),
            pa.table({"value": pa.array([1], type=pa.date32())}),
            pa.table({"value": pa.array([1], type=pa.duration("ns"))}),
            pa.table({"value": pa.array([0], type=pa.timestamp("us", tz="UTC"))}),
            pa.table({"value": pa.array([0], type=pa.timestamp("ns"))}),
            pa.table({"value": pa.array([0], type=pa.timestamp("ns", tz="Asia/Shanghai"))}),
            pa.table({"value": pa.array([], type=pa.decimal128(38, -5))}),
            pa.table({"value": pa.array([], type=pa.decimal128(10, 20))}),
            pa.table({"value": pa.array([], type=pa.decimal256(50, -1))}),
            pa.table({"value": pa.array([], type=pa.decimal256(50, 51))}),
        ]
        for arrow in invalid_tables:
            with self.subTest(schema=arrow.schema), self.assertRaises(
                (TypeError, ValueError)
            ):
                dp.Table.from_arrow(arrow)

    def test_non_nullable_fields_scan_nulls_across_all_chunks(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.chunked_array([pa.array([1]), pa.array([None], type=pa.int64())])],
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )
        with self.assertRaises(ValueError):
            dp.Table.from_arrow(arrow)

    def test_metadata_is_not_a_table_contract(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.array([1])],
            schema=pa.schema(
                [pa.field("value", pa.int64(), metadata={b"field": b"ignored"})],
                metadata={b"schema": b"ignored"},
            ),
        )
        table = dp.Table.from_arrow(arrow)
        self.assertEqual(table["value"], (1,))

    def test_bridge_requires_the_documented_exact_objects(self) -> None:
        with self.assertRaises(TypeError):
            dp.Table.from_arrow(pa.record_batch([[1]], names=["value"]))  # type: ignore[arg-type]

if __name__ == "__main__":
    unittest.main()
