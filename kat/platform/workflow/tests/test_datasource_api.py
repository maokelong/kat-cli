from __future__ import annotations

import math
import unittest
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from types import MappingProxyType

import pyarrow as pa

import kat
from kat import datasource as ds


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


class PythonTableTest(unittest.TestCase):
    def test_columns_and_rows_build_the_same_isolated_reusable_table(self) -> None:
        schema = {"id": int, "label": str | None}
        ids = [1, 2]
        labels = ["one", None]
        from_columns = ds.table(
            schema=schema,
            columns={"label": labels, "id": ids},
        )
        from_rows = ds.table(schema=schema, rows=[(1, "one"), (2, None)])
        ids[0] = 99
        labels.append("late")

        for table in (from_columns, from_rows):
            with self.subTest(table=table):
                self.assertEqual(len(table), 2)
                self.assertEqual(table.columns, ("id", "label"))
                self.assertFalse(ds.to_arrow(table).schema.field("id").nullable)
                self.assertTrue(ds.to_arrow(table).schema.field("label").nullable)
                self.assertEqual(table["id"], (1, 2))
                self.assertEqual(table["label"], ("one", None))
                first = table.to_rows()
                first[0]["id"] = 99
                first.append({"id": 3, "label": "three"})
                self.assertEqual(
                    table.to_rows(),
                    [{"id": 1, "label": "one"}, {"id": 2, "label": None}],
                )

    def test_table_requires_exactly_one_input_and_valid_shapes(self) -> None:
        with self.assertRaises(ValueError):
            ds.table(schema={"value": int})
        with self.assertRaises(ValueError):
            ds.table(schema={"value": int}, columns={"value": []}, rows=[])
        with self.assertRaises(ValueError):
            ds.table(schema={}, rows=[])
        with self.assertRaises(ValueError):
            ds.table(schema={"left": int, "right": int}, columns={"left": [1]})
        with self.assertRaises(ValueError):
            ds.table(
                schema={"left": int, "right": int},
                columns={"left": [1], "right": [2, 3]},
            )
        with self.assertRaises(ValueError):
            ds.table(schema={"left": int, "right": int}, rows=[(1,)])
        with self.assertRaises(TypeError):
            ds.table(schema={"value": int}, columns={"value": (1,)})  # type: ignore[arg-type]

    def test_python_values_use_exact_types_ranges_and_nullability(self) -> None:
        cases = [
            ({"value": bool}, [1]),
            ({"value": int}, [True]),
            ({"value": int}, [2**63]),
            ({"value": int}, [-(2**63) - 1]),
            ({"value": float}, [1]),
            ({"value": str}, [str.__new__(type("Text", (str,), {}), "value")]),
            ({"value": bytes}, [bytearray(b"value")]),
            ({"value": int}, [None]),
        ]
        for schema, values in cases:
            with self.subTest(schema=schema, values=values), self.assertRaises(
                (TypeError, ValueError)
            ):
                ds.table(schema=schema, columns={"value": values})

        table = ds.table(
            schema={
                "flag": bool,
                "number": int,
                "ratio": float,
                "text": str,
                "payload": bytes,
                "optional": int | None,
            },
            rows=[(True, -(2**63), math.inf, "value", b"bytes", None)],
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
        table = ds.table(schema={"at": datetime}, columns={"at": [value]})
        self.assertEqual(
            table["at"],
            (kat.WallClockTimestamp("2026-08-28T04:30:00.123456Z"),),
        )
        self.assertEqual(ds.to_arrow(table).schema.field("at").type, pa.timestamp("ns", tz="UTC"))

        for invalid in (
            datetime(2026, 8, 28, 12, 30),
            datetime(1600, 1, 1, tzinfo=timezone.utc),
            datetime(2300, 1, 1, tzinfo=timezone.utc),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                ds.table(schema={"at": datetime}, columns={"at": [invalid]})

    def test_decimal_uses_exact_decimal128_38_18_rescaling(self) -> None:
        values = [
            Decimal("1.2"),
            Decimal("1.2300000000000000000"),
            Decimal("99999999999999999999.999999999999999999"),
        ]
        table = ds.table(schema={"value": Decimal}, columns={"value": values})
        self.assertEqual(
            ds.to_arrow(table).schema.field("value").type,
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
                ds.table(schema={"value": Decimal}, columns={"value": [invalid]})


class ArrowTableTest(unittest.TestCase):
    def test_bridge_preserves_the_table_and_all_admitted_physical_types(self) -> None:
        arrays = [
            pa.array([True], type=pa.bool_()),
            pa.array([-1], type=pa.int8()),
            pa.array([1], type=pa.uint64()),
            pa.array([1.5], type=pa.float16()),
            pa.array([1.5], type=pa.float32()),
            pa.array([1.5], type=pa.float64()),
            pa.array(["text"], type=pa.string()),
            pa.array(["text"], type=pa.large_string()),
            pa.array(["text"], type=pa.string_view()),
            pa.array([b"bytes"], type=pa.binary()),
            pa.array([b"bytes"], type=pa.large_binary()),
            pa.array([0], type=pa.timestamp("ns", tz="UTC")),
            pa.array([Decimal("1.20")], type=pa.decimal128(10, 2)),
            pa.array([Decimal("1.20")], type=pa.decimal256(50, 2)),
        ]
        fields = [
            pa.field(f"column_{index}", array.type, nullable=True)
            for index, array in enumerate(arrays)
        ]
        arrow = pa.Table.from_arrays(arrays, schema=pa.schema(fields))

        table = ds.from_arrow(arrow)

        self.assertIs(ds.to_arrow(table), arrow)
        self.assertEqual(table.columns, tuple(field.name for field in fields))
        self.assertEqual(table["column_11"], (kat.WallClockTimestamp("1970-01-01T00:00:00Z"),))
        self.assertEqual(table["column_12"], (Decimal("1.20"),))

    def test_timestamp_projection_preserves_all_nine_fractional_digits(self) -> None:
        arrow = pa.table(
            {"at": pa.array([1_725_000_000_123_456_789], type=pa.timestamp("ns", tz="UTC"))}
        )
        table = ds.from_arrow(arrow)
        self.assertEqual(
            table["at"],
            (kat.WallClockTimestamp("2024-08-30T06:40:00.123456789Z"),),
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
        ]
        for arrow in invalid_tables:
            with self.subTest(schema=arrow.schema), self.assertRaises(
                (TypeError, ValueError)
            ):
                ds.from_arrow(arrow)

    def test_non_nullable_fields_scan_nulls_across_all_chunks(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.chunked_array([pa.array([1]), pa.array([None], type=pa.int64())])],
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )
        with self.assertRaises(ValueError):
            ds.from_arrow(arrow)

    def test_metadata_is_not_a_table_contract(self) -> None:
        arrow = pa.Table.from_arrays(
            [pa.array([1])],
            schema=pa.schema(
                [pa.field("value", pa.int64(), metadata={b"field": b"ignored"})],
                metadata={b"schema": b"ignored"},
            ),
        )
        table = ds.from_arrow(arrow)
        self.assertEqual(table["value"], (1,))

    def test_bridge_requires_the_documented_exact_objects(self) -> None:
        with self.assertRaises(TypeError):
            ds.from_arrow(pa.record_batch([[1]], names=["value"]))  # type: ignore[arg-type]
        with self.assertRaises(TypeError):
            ds.to_arrow(object())  # type: ignore[arg-type]


if __name__ == "__main__":
    unittest.main()
