from __future__ import annotations

import math
import unittest
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from types import MappingProxyType

import pyarrow as pa

from kat import datasource as ds
from kat import WallClockTimestamp


class SchemaTest(unittest.TestCase):
    def test_schema_copies_freezes_and_preserves_the_declaration_order(self) -> None:
        columns: dict[str, object] = {
            "Observed At": datetime,
            "value": int | None,
        }
        declarations = {"events": columns, "metrics": {"ratio": float}}

        schema = ds.Schema(declarations)
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
            {"events": {1: int}},
            {"events": {"value": object}},
            {"events": {"value": int | str}},
            {"events": {"value": int | str | None}},
        ]
        for declaration in invalid:
            with self.subTest(declaration=declaration), self.assertRaises(
                (TypeError, ValueError)
            ):
                ds.Schema(declaration)  # type: ignore[arg-type]

    def test_schema_accepts_all_seven_types_and_nullable_variants(self) -> None:
        logical_types = (bool, int, float, str, bytes, datetime, Decimal)
        declaration = {
            "values": {
                **{value.__name__: value for value in logical_types},
                **{f"optional_{value.__name__}": value | None for value in logical_types},
            }
        }
        schema = ds.Schema(declaration)
        self.assertEqual(tuple(schema["values"]), tuple(declaration["values"]))

    def test_create_returns_one_empty_appendable_table_per_declaration(self) -> None:
        schema = ds.Schema(
            {
                "capture": {"tracer": str, "cpu_count": int},
                "events": {"timestamp": int, "name": str | None},
            }
        )

        tables = schema.create()

        self.assertIs(type(tables), dict)
        self.assertEqual(tuple(tables), ("capture", "events"))
        self.assertEqual(tables["capture"].columns, ("tracer", "cpu_count"))
        self.assertEqual(len(tables["capture"]), 0)
        tables["capture"].append(tracer="nop", cpu_count=4)
        self.assertEqual(
            tables["capture"].to_rows(),
            [{"tracer": "nop", "cpu_count": 4}],
        )


class PythonTableTest(unittest.TestCase):
    def test_constructor_copies_schema_and_reads_are_reusable_and_isolated(self) -> None:
        schema: dict[str, object] = {"id": int, "label": str | None}
        table = ds.Table(schema)
        schema["late"] = bytes

        table.append(label="one", id=1)
        table.append(id=2, label=None)

        self.assertEqual(len(table), 2)
        self.assertEqual(table.columns, ("id", "label"))
        self.assertFalse(table.to_arrow().schema.field("id").nullable)
        self.assertTrue(table.to_arrow().schema.field("label").nullable)
        self.assertEqual(table["id"], (1, 2))
        self.assertEqual(table["label"], ("one", None))
        first = table.to_rows()
        first[0]["id"] = 99
        first.append({"id": 3, "label": "three"})
        self.assertEqual(
            table.to_rows(),
            [{"id": 1, "label": "one"}, {"id": 2, "label": None}],
        )

    def test_append_requires_one_complete_valid_row_and_fails_atomically(self) -> None:
        table = ds.Table({"left": int, "right": str, "optional": bytes | None})
        table.append(left=1, right="one", optional=None)
        before = table.to_arrow()

        invalid_rows = [
            {"left": 2, "right": "two"},
            {"left": 2, "right": "two", "optional": None, "extra": 3},
            {"left": True, "right": "two", "optional": None},
            {"left": 2**63, "right": "two", "optional": None},
            {"left": 2, "right": str.__new__(type("Text", (str,), {}), "two"), "optional": None},
            {"left": 2, "right": "two", "optional": bytearray(b"x")},
        ]
        for row in invalid_rows:
            with self.subTest(row=row), self.assertRaises((TypeError, ValueError)):
                table.append(**row)
            self.assertIs(table.to_arrow(), before)
            self.assertEqual(
                table.to_rows(),
                [{"left": 1, "right": "one", "optional": None}],
            )

    def test_python_values_use_exact_types_ranges_and_nullability(self) -> None:
        cases = [
            (bool, 1),
            (int, True),
            (int, 2**63),
            (int, -(2**63) - 1),
            (float, 1),
            (str, str.__new__(type("Text", (str,), {}), "value")),
            (bytes, bytearray(b"value")),
            (int, None),
        ]
        for annotation, value in cases:
            with self.subTest(annotation=annotation, value=value), self.assertRaises(
                (TypeError, ValueError)
            ):
                ds.Table({"value": annotation}).append(value=value)

        table = ds.Table(
            {
                "flag": bool,
                "number": int,
                "ratio": float,
                "text": str,
                "payload": bytes,
                "optional": int | None,
            }
        )
        table.append(
            flag=True,
            number=-(2**63),
            ratio=math.inf,
            text="value",
            payload=b"bytes",
            optional=None,
        )
        self.assertEqual(table["ratio"], (math.inf,))

    def test_datetime_is_normalized_to_utc_and_naive_or_out_of_range_fails(self) -> None:
        value = datetime(
            2026,
            8,
            28,
            12,
            30,
            0,
            123456,
            tzinfo=timezone(timedelta(hours=8)),
        )
        table = ds.Table({"at": datetime})
        table.append(at=value)
        self.assertEqual(
            table["at"],
            (WallClockTimestamp("2026-08-28T04:30:00.123456Z"),),
        )
        self.assertEqual(table.to_arrow().schema.field("at").type, pa.timestamp("ns", tz="UTC"))

        for invalid in (
            datetime(2026, 8, 28, 12, 30),
            datetime(1600, 1, 1, tzinfo=timezone.utc),
            datetime(2300, 1, 1, tzinfo=timezone.utc),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                ds.Table({"at": datetime}).append(at=invalid)

    def test_decimal_uses_exact_decimal128_38_18_rescaling(self) -> None:
        values = [
            Decimal("1.2"),
            Decimal("1.2300000000000000000"),
            Decimal("99999999999999999999.999999999999999999"),
        ]
        table = ds.Table({"value": Decimal})
        for value in values:
            table.append(value=value)
        self.assertEqual(
            table.to_arrow().schema.field("value").type,
            pa.decimal128(38, 18),
        )
        self.assertEqual(
            table["value"],
            (
                Decimal("1.200000000000000000"),
                Decimal("1.230000000000000000"),
                Decimal("99999999999999999999.999999999999999999"),
            ),
        )

        for invalid in (
            Decimal("1.0000000000000000001"),
            Decimal("NaN"),
            Decimal("Infinity"),
            Decimal("100000000000000000000"),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                ds.Table({"value": Decimal}).append(value=invalid)


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

        table = ds.Table.from_arrow(arrow)

        self.assertIs(table.to_arrow(), arrow)
        self.assertEqual(table.columns, tuple(field.name for field in fields))
        self.assertEqual(table["timestamp"], (WallClockTimestamp("1970-01-01T00:00:00Z"),))
        self.assertEqual(table["decimal128"], (Decimal("1.20"),))

    def test_timestamp_projection_preserves_all_nine_fractional_digits(self) -> None:
        arrow = pa.table(
            {"at": pa.array([1_725_000_000_123_456_789], type=pa.timestamp("ns", tz="UTC"))}
        )
        table = ds.Table.from_arrow(arrow)
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
                ds.Table.from_arrow(arrow)

    def test_non_nullable_fields_scan_nulls_across_all_chunks(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.chunked_array([pa.array([1]), pa.array([None], type=pa.int64())])],
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )
        with self.assertRaises(ValueError):
            ds.Table.from_arrow(arrow)

    def test_metadata_is_not_a_table_contract(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.array([1])],
            schema=pa.schema(
                [pa.field("value", pa.int64(), metadata={b"field": b"ignored"})],
                metadata={b"schema": b"ignored"},
            ),
        )
        table = ds.Table.from_arrow(arrow)
        self.assertEqual(table["value"], (1,))

    def test_bridge_requires_the_documented_exact_objects(self) -> None:
        with self.assertRaises(TypeError):
            ds.Table.from_arrow(pa.record_batch([[1]], names=["value"]))  # type: ignore[arg-type]

    def test_physical_append_preserves_schema_chunks_and_old_snapshot(self) -> None:
        arrow = pa.table({"value": pa.array([1], type=pa.int8())})
        table = ds.Table.from_arrow(arrow)
        before = table.to_arrow()
        before_buffer = before.column("value").chunk(0).buffers()[1]

        table.append(value=2)
        after = table.to_arrow()

        self.assertEqual(after.schema, arrow.schema)
        self.assertEqual(after.column("value").num_chunks, 2)
        self.assertEqual(after.column("value").to_pylist(), [1, 2])
        self.assertEqual(before.column("value").to_pylist(), [1])
        self.assertEqual(
            after.column("value").chunk(0).buffers()[1].address,
            before_buffer.address,
        )

        table.append(value=3)
        self.assertEqual(table["value"], (1, 2, 3))
        self.assertEqual(after.column("value").to_pylist(), [1, 2])

    def test_physical_append_accepts_every_documented_python_value_family(self) -> None:
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
        arrow = pa.Table.from_arrays(
            [pa.array([], type=field.type) for field in schema],
            schema=schema,
        )
        table = ds.Table.from_arrow(arrow)

        table.append(**{name: value for name, (_, value) in fields_and_values.items()})

        self.assertEqual(table.to_arrow().schema, schema)
        self.assertEqual(table["signed"], (-128,))
        self.assertEqual(table["unsigned"], (2**64 - 1,))
        self.assertAlmostEqual(table["float16"][0], 1.099609375)
        self.assertEqual(
            table["timestamp"],
            (WallClockTimestamp("2026-08-28T04:30:00.123456789Z"),),
        )
        self.assertEqual(table["decimal256"], (Decimal("1.20000000000000000000"),))
        self.assertEqual(table["nullable"], (None,))

        timestamp = ds.Table.from_arrow(
            pa.Table.from_arrays(
                [pa.array([], type=pa.timestamp("ns", tz="UTC"))],
                schema=pa.schema(
                    [pa.field("at", pa.timestamp("ns", tz="UTC"), nullable=False)]
                ),
            )
        )
        timestamp.append(
            at=datetime(2026, 8, 28, 12, 30, tzinfo=timezone(timedelta(hours=8)))
        )
        self.assertEqual(
            timestamp["at"],
            (WallClockTimestamp("2026-08-28T04:30:00Z"),),
        )

    def test_physical_append_rejects_coercion_range_and_rounding(self) -> None:
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
            (pa.binary(), bytearray(b"bytes")),
            (pa.timestamp("ns", tz="UTC"), datetime(2026, 8, 28)),
            (pa.timestamp("ns", tz="UTC"), "2026-08-28T00:00:00Z"),
            (pa.timestamp("ns", tz="UTC"), 0),
            (pa.decimal128(4, 2), Decimal("1.001")),
            (pa.decimal128(4, 2), Decimal("100.00")),
            (pa.decimal128(4, 2), Decimal("NaN")),
            (pa.decimal128(4, 2), 1),
        ]
        for data_type, value in cases:
            with self.subTest(data_type=data_type, value=value):
                arrow = pa.Table.from_arrays(
                    [pa.array([], type=data_type)],
                    schema=pa.schema([pa.field("value", data_type, nullable=False)]),
                )
                table = ds.Table.from_arrow(arrow)
                before = table.to_arrow()
                with self.assertRaises((TypeError, ValueError)):
                    table.append(value=value)
                self.assertIs(table.to_arrow(), before)
                self.assertEqual(len(table), 0)

        non_nullable = ds.Table.from_arrow(
            pa.Table.from_arrays(
                [pa.array([], type=pa.int64())],
                schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
            )
        )
        with self.assertRaises(ValueError):
            non_nullable.append(value=None)

    def test_physical_multi_column_append_is_atomic(self) -> None:
        schema = pa.schema(
            [
                pa.field("number", pa.int8(), nullable=False),
                pa.field("label", pa.string(), nullable=False),
            ]
        )
        table = ds.Table.from_arrow(
            pa.Table.from_arrays(
                [pa.array([], type=field.type) for field in schema],
                schema=schema,
            )
        )
        table.append(number=1, label="one")
        before = table.to_arrow()

        with self.assertRaises(TypeError):
            table.append(number=2, label=b"two")

        self.assertIs(table.to_arrow(), before)
        self.assertEqual(table.to_rows(), [{"number": 1, "label": "one"}])


if __name__ == "__main__":
    unittest.main()
