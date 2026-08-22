from __future__ import annotations

from dataclasses import dataclass
from datetime import date, datetime, time, timedelta, timezone
from decimal import Decimal
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import pyarrow as pa

from kat.common.sql import postgresql


@dataclass(frozen=True)
class _Column:
    name: str
    type_code: int
    precision: int | None = None
    scale: int | None = None
    type_display: str | None = None


@dataclass(frozen=True)
class _Result:
    description: tuple[_Column, ...] | None
    rows: tuple[tuple[object, ...], ...] = ()


class _Cursor:
    def __init__(self, results: list[_Result]) -> None:
        self._results = results
        self._current: _Result | None = None
        self.executed_sql: str | None = None
        self.executed_parameters: object = None
        self.closed = False

    @property
    def description(self) -> tuple[_Column, ...] | None:
        assert self._current is not None
        return self._current.description

    def execute(self, sql: str, parameters: object = None) -> None:
        self.executed_sql = sql
        self.executed_parameters = parameters

    def results(self):
        for result in self._results:
            self._current = result
            yield self

    def fetchall(self) -> list[tuple[object, ...]]:
        assert self._current is not None
        return list(self._current.rows)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.closed = True


class _Connection:
    def __init__(self, cursor: _Cursor) -> None:
        self._cursor = cursor
        self.closed = False

    def cursor(self) -> _Cursor:
        return self._cursor

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.closed = True


class _Context:
    def __init__(self) -> None:
        self.table: pa.Table | None = None
        self.frame = object()

    def from_arrow(self, table: pa.Table):
        self.table = table
        return self.frame


class PostgreSqlCommonTest(unittest.TestCase):
    def test_execute_sql_text_returns_context_frame_from_one_short_connection(self) -> None:
        cursor = _Cursor(
            [
                _Result(
                    description=(_Column("answer", 23, type_display="int4"),),
                    rows=((42,),),
                )
            ]
        )
        connection = _Connection(cursor)
        context = _Context()
        parameters = {"input": 42}

        with mock.patch.object(
            postgresql.psycopg, "connect", return_value=connection
        ) as connect:
            frame = postgresql.execute_sql_text(
                context,
                sql_text="SELECT %(input)s::integer AS answer",
                parameters=parameters,
            )

        self.assertIs(frame, context.frame)
        self.assertEqual(
            context.table,
            pa.Table.from_arrays(
                [pa.array([42], type=pa.int32())], names=["answer"]
            ),
        )
        connect.assert_called_once_with(autocommit=True)
        self.assertEqual(
            cursor.executed_sql, "SELECT %(input)s::integer AS answer"
        )
        self.assertIs(cursor.executed_parameters, parameters)
        self.assertTrue(cursor.closed)
        self.assertTrue(connection.closed)

    def test_execute_sql_file_accepts_bom_path_and_rereads_each_call(self) -> None:
        first_cursor = _Cursor(
            [_Result((_Column("answer", 23, type_display="int4"),), ((1,),))]
        )
        second_cursor = _Cursor(
            [_Result((_Column("answer", 23, type_display="int4"),), ((2,),))]
        )
        context = _Context()

        with tempfile.TemporaryDirectory() as temporary:
            sql_file = Path(temporary) / "answer.sql"
            sql_file.write_bytes(b"\xef\xbb\xbfSELECT 1::integer AS answer")
            with mock.patch.object(
                postgresql.psycopg,
                "connect",
                side_effect=[_Connection(first_cursor), _Connection(second_cursor)],
            ):
                postgresql.execute_sql_file(context, sql_file)
                sql_file.write_text(
                    "SELECT 2::integer AS answer", encoding="utf-8"
                )
                postgresql.execute_sql_file(context, sql_file)

        self.assertEqual(
            first_cursor.executed_sql, "SELECT 1::integer AS answer"
        )
        self.assertEqual(
            second_cursor.executed_sql, "SELECT 2::integer AS answer"
        )

    def test_execute_sql_file_preserves_standard_file_errors(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary) / "missing.sql"
            invalid_utf8 = Path(temporary) / "invalid.sql"
            invalid_utf8.write_bytes(b"SELECT '\xff'")

            with mock.patch.object(postgresql.psycopg, "connect") as connect:
                with self.assertRaises(FileNotFoundError):
                    postgresql.execute_sql_file(_Context(), missing)
                with self.assertRaises(UnicodeDecodeError):
                    postgresql.execute_sql_file(_Context(), invalid_utf8)
                connect.assert_not_called()

    def test_supported_postgresql_types_preserve_arrow_schema_values_and_nulls(self) -> None:
        columns = (
            _Column("boolean_value", 16, type_display="bool"),
            _Column("smallint_value", 21, type_display="int2"),
            _Column("integer_value", 23, type_display="int4"),
            _Column("bigint_value", 20, type_display="int8"),
            _Column("real_value", 700, type_display="float4"),
            _Column("double_value", 701, type_display="float8"),
            _Column(
                "numeric_value",
                1700,
                precision=10,
                scale=2,
                type_display="numeric(10,2)",
            ),
            _Column("name_value", 19, type_display="name"),
            _Column("text_value", 25, type_display="text"),
            _Column("varchar_value", 1043, type_display="varchar"),
            _Column("char_value", 1042, type_display="bpchar"),
            _Column("binary_value", 17, type_display="bytea"),
            _Column("date_value", 1082, type_display="date"),
            _Column("time_value", 1083, type_display="time"),
            _Column("timestamp_value", 1114, type_display="timestamp"),
            _Column("timestamptz_value", 1184, type_display="timestamptz"),
        )
        values = (
            True,
            -12,
            345,
            9_876_543_210,
            1.25,
            -2.5,
            Decimal("12345.67"),
            "analyst",
            "plain text",
            "bounded text",
            "xy  ",
            b"\x00\xff",
            date(2026, 1, 2),
            time(3, 4, 5, 600000),
            datetime(2026, 1, 2, 3, 4, 5, 600000),
            datetime(
                2026,
                1,
                2,
                8,
                4,
                5,
                600000,
                tzinfo=timezone(timedelta(hours=8)),
            ),
        )
        cursor = _Cursor(
            [_Result(columns, (values, tuple(None for _ in columns)))]
        )
        context = _Context()

        with mock.patch.object(
            postgresql.psycopg,
            "connect",
            return_value=_Connection(cursor),
        ):
            postgresql.execute_sql_text(context, "SELECT supported_types")

        assert context.table is not None
        self.assertEqual(
            context.table.schema,
            pa.schema(
                [
                    pa.field("boolean_value", pa.bool_()),
                    pa.field("smallint_value", pa.int16()),
                    pa.field("integer_value", pa.int32()),
                    pa.field("bigint_value", pa.int64()),
                    pa.field("real_value", pa.float32()),
                    pa.field("double_value", pa.float64()),
                    pa.field("numeric_value", pa.decimal128(10, 2)),
                    pa.field("name_value", pa.string()),
                    pa.field("text_value", pa.string()),
                    pa.field("varchar_value", pa.string()),
                    pa.field("char_value", pa.string()),
                    pa.field("binary_value", pa.binary()),
                    pa.field("date_value", pa.date32()),
                    pa.field("time_value", pa.time64("us")),
                    pa.field("timestamp_value", pa.timestamp("us")),
                    pa.field(
                        "timestamptz_value", pa.timestamp("us", tz="UTC")
                    ),
                ]
            ),
        )
        self.assertEqual(
            context.table.to_pydict(),
            {
                "boolean_value": [True, None],
                "smallint_value": [-12, None],
                "integer_value": [345, None],
                "bigint_value": [9_876_543_210, None],
                "real_value": [1.25, None],
                "double_value": [-2.5, None],
                "numeric_value": [Decimal("12345.67"), None],
                "name_value": ["analyst", None],
                "text_value": ["plain text", None],
                "varchar_value": ["bounded text", None],
                "char_value": ["xy  ", None],
                "binary_value": [b"\x00\xff", None],
                "date_value": [date(2026, 1, 2), None],
                "time_value": [time(3, 4, 5, 600000), None],
                "timestamp_value": [
                    datetime(2026, 1, 2, 3, 4, 5, 600000),
                    None,
                ],
                "timestamptz_value": [
                    datetime(
                        2026,
                        1,
                        2,
                        0,
                        4,
                        5,
                        600000,
                        tzinfo=timezone.utc,
                    ),
                    None,
                ],
            },
        )

    def test_result_shape_requires_one_nonempty_uniquely_named_rowset(self) -> None:
        valid = _Result(
            (_Column("answer", 23, type_display="int4"),), ((42,),)
        )
        invalid_results = {
            "no rowset": [_Result(None)],
            "multiple rowsets": [valid, valid],
            "zero columns": [_Result(())],
            "empty column name": [
                _Result((_Column("", 23, type_display="int4"),), ((42,),))
            ],
            "duplicate column name": [
                _Result(
                    (
                        _Column("answer", 23, type_display="int4"),
                        _Column("answer", 23, type_display="int4"),
                    ),
                    ((42, 43),),
                )
            ],
        }

        for label, results in invalid_results.items():
            with self.subTest(label=label):
                cursor = _Cursor(results)
                with (
                    mock.patch.object(
                        postgresql.psycopg,
                        "connect",
                        return_value=_Connection(cursor),
                    ),
                    self.assertRaises(ValueError),
                ):
                    postgresql.execute_sql_text(_Context(), "arbitrary SQL")

        cursor = _Cursor([_Result(None), valid, _Result(None)])
        context = _Context()
        with mock.patch.object(
            postgresql.psycopg,
            "connect",
            return_value=_Connection(cursor),
        ):
            postgresql.execute_sql_text(context, "commands; SELECT 42; command")
        assert context.table is not None
        self.assertEqual(context.table.to_pydict(), {"answer": [42]})

    def test_invalid_input_shapes_fail_before_opening_a_connection(self) -> None:
        invalid_calls = {
            "bytes SQL text": lambda: postgresql.execute_sql_text(
                _Context(), b"SELECT 1"  # type: ignore[arg-type]
            ),
            "non-mapping parameters": lambda: postgresql.execute_sql_text(
                _Context(), "SELECT 1", []  # type: ignore[arg-type]
            ),
            "non-string parameter key": lambda: postgresql.execute_sql_text(
                _Context(), "SELECT 1", {1: "value"}  # type: ignore[dict-item]
            ),
            "bytes SQL path": lambda: postgresql.execute_sql_file(
                _Context(), b"C:\\query.sql"  # type: ignore[arg-type]
            ),
        }
        for label, call in invalid_calls.items():
            with self.subTest(label=label):
                with (
                    mock.patch.object(postgresql.psycopg, "connect") as connect,
                    self.assertRaises(TypeError),
                ):
                    call()
                connect.assert_not_called()

        with (
            mock.patch.object(postgresql.psycopg, "connect") as connect,
            self.assertRaises(ValueError),
        ):
            postgresql.execute_sql_file(_Context(), "relative.sql")
        connect.assert_not_called()

    def test_unsupported_types_and_numeric_values_fail_with_column_context(self) -> None:
        invalid_columns_and_values = [
            (_Column("uuid_value", 2950, type_display="uuid"), "uuid-value"),
            (
                _Column("numeric_unbounded", 1700, type_display="numeric"),
                Decimal("1"),
            ),
            (
                _Column(
                    "numeric_wide",
                    1700,
                    precision=39,
                    scale=2,
                    type_display="numeric(39,2)",
                ),
                Decimal("1.00"),
            ),
            (
                _Column(
                    "numeric_scale",
                    1700,
                    precision=10,
                    scale=11,
                    type_display="numeric(10,11)",
                ),
                Decimal("0"),
            ),
            (
                _Column(
                    "numeric_nan",
                    1700,
                    precision=10,
                    scale=2,
                    type_display="numeric(10,2)",
                ),
                Decimal("NaN"),
            ),
            (
                _Column(
                    "numeric_infinity",
                    1700,
                    precision=10,
                    scale=2,
                    type_display="numeric(10,2)",
                ),
                Decimal("Infinity"),
            ),
        ]

        for column, value in invalid_columns_and_values:
            with self.subTest(column=column.name):
                cursor = _Cursor([_Result((column,), ((value,),))])
                with (
                    mock.patch.object(
                        postgresql.psycopg,
                        "connect",
                        return_value=_Connection(cursor),
                    ),
                    self.assertRaisesRegex(ValueError, column.name),
                ):
                    postgresql.execute_sql_text(_Context(), "SELECT unsupported")

    def test_zero_row_result_uses_postgresql_description_for_arrow_schema(self) -> None:
        cursor = _Cursor(
            [
                _Result(
                    (
                        _Column("count", 20, type_display="int8"),
                        _Column("label", 25, type_display="text"),
                        _Column(
                            "ratio",
                            1700,
                            precision=8,
                            scale=3,
                            type_display="numeric(8,3)",
                        ),
                    )
                )
            ]
        )
        context = _Context()

        with mock.patch.object(
            postgresql.psycopg,
            "connect",
            return_value=_Connection(cursor),
        ):
            postgresql.execute_sql_text(context, "SELECT zero_rows")

        assert context.table is not None
        self.assertEqual(context.table.num_rows, 0)
        self.assertEqual(
            context.table.schema,
            pa.schema(
                [
                    pa.field("count", pa.int64()),
                    pa.field("label", pa.string()),
                    pa.field("ratio", pa.decimal128(8, 3)),
                ]
            ),
        )


if __name__ == "__main__":
    unittest.main()
