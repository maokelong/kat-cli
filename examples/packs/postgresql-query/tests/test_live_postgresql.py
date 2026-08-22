from datetime import date, datetime, time, timezone
from decimal import Decimal

import pyarrow as pa


def test_live_postgresql_smoke(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            """
            SELECT
                current_database() AS database_name,
                current_user AS user_name,
                version() AS server_version,
                current_setting('transaction_read_only') AS transaction_read_only,
                COALESCE(
                    (
                        SELECT ssl::text
                        FROM pg_stat_ssl
                        WHERE pid = pg_backend_pid()
                    ),
                    'false'
                ) AS tls_enabled
            """,
        ],
    )

    table = outputs["postgresql_result"]
    assert table.num_rows == 1
    assert table.column_names == [
        "database_name",
        "user_name",
        "server_version",
        "transaction_read_only",
        "tls_enabled",
    ]
    row = table.to_pylist()[0]
    assert row["database_name"]
    assert row["user_name"]
    assert row["server_version"].startswith("PostgreSQL ")
    assert row["transaction_read_only"] in {"on", "off"}
    assert row["tls_enabled"] in {"true", "false"}


def test_live_postgresql_executes_pack_sql_file(kat_run):
    outputs = kat_run(workflow="query-postgresql-file")

    table = outputs["postgresql_result"]
    assert table.num_rows == 1
    assert table.column_names == ["query_source", "database_name", "user_name"]
    row = table.to_pylist()[0]
    assert row["query_source"] == "pack-sql-file"
    assert row["database_name"]
    assert row["user_name"]


def test_live_postgresql_preserves_null(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            "SELECT 'present'::text AS present_value, NULL::text AS missing_value",
        ],
    )

    table = outputs["postgresql_result"]
    assert table.schema.types == [pa.string(), pa.string()]
    assert table.to_pylist() == [
        {"present_value": "present", "missing_value": None}
    ]


def test_live_postgresql_uses_the_server_resolved_type_for_bare_null(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=["--sql", "SELECT NULL AS server_resolved_null"],
    )

    table = outputs["postgresql_result"]
    assert table.schema.types == [pa.string()]
    assert table.to_pylist() == [{"server_resolved_null": None}]


def test_live_postgresql_preserves_zero_row_schema(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            "SELECT NULL::text AS empty_value WHERE FALSE",
        ],
    )

    table = outputs["postgresql_result"]
    assert table.num_rows == 0
    assert table.column_names == ["empty_value"]
    assert table.schema.field("empty_value").type == pa.string()


def test_live_postgresql_preserves_supported_scalar_types(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            """
            SELECT
                TRUE::boolean AS boolean_value,
                (-123)::smallint AS smallint_value,
                456::integer AS integer_value,
                7890123456789::bigint AS bigint_value,
                1.25::real AS real_value,
                2.5::double precision AS double_value,
                123.45::numeric(10, 2) AS numeric_value,
                current_user AS name_value,
                'hello'::text AS text_value,
                'world'::varchar(8) AS varchar_value,
                'cat'::char(3) AS char_value,
                decode('00ff', 'hex')::bytea AS bytea_value,
                DATE '2026-08-22' AS date_value,
                TIME '12:34:56.123456' AS time_value,
                TIMESTAMP '2026-08-22 12:34:56.123456' AS timestamp_value,
                TIMESTAMPTZ '2026-08-22 12:34:56.123456+08' AS timestamptz_value
            """,
        ],
    )

    table = outputs["postgresql_result"]
    assert table.schema.types == [
        pa.bool_(),
        pa.int16(),
        pa.int32(),
        pa.int64(),
        pa.float32(),
        pa.float64(),
        pa.decimal128(10, 2),
        pa.string(),
        pa.string(),
        pa.string(),
        pa.string(),
        pa.binary(),
        pa.date32(),
        pa.time64("us"),
        pa.timestamp("us"),
        pa.timestamp("us", tz="UTC"),
    ]
    row = table.to_pylist()[0]
    name_value = row.pop("name_value")
    assert name_value
    assert row == {
        "boolean_value": True,
        "smallint_value": -123,
        "integer_value": 456,
        "bigint_value": 7890123456789,
        "real_value": 1.25,
        "double_value": 2.5,
        "numeric_value": Decimal("123.45"),
        "text_value": "hello",
        "varchar_value": "world",
        "char_value": "cat",
        "bytea_value": b"\x00\xff",
        "date_value": date(2026, 8, 22),
        "time_value": time(12, 34, 56, 123456),
        "timestamp_value": datetime(2026, 8, 22, 12, 34, 56, 123456),
        "timestamptz_value": datetime(
            2026, 8, 22, 4, 34, 56, 123456, tzinfo=timezone.utc
        ),
    }
