from __future__ import annotations

from datetime import datetime, timezone
from decimal import Decimal
from urllib.parse import quote

from adbc_driver_postgresql import dbapi
import pyarrow as pa
import pytest

import kat
from kat import datasource as ds
from kat.pack.datasources.postgresql import PostgreSQLProvider


def _uri(service: str, database: str) -> str:
    return (
        f"postgresql:///{quote(database, safe='')}"
        f"?service={quote(service, safe='')}"
    )


def _direct_table(
    service: str,
    database: str,
    sql: str,
    params: tuple[object, ...] | None = None,
) -> pa.Table:
    connection = dbapi.connect(_uri(service, database), autocommit=True)
    try:
        cursor = connection.cursor()
        try:
            cursor.execute(sql, params)
            reader = cursor.fetch_record_batch()
            try:
                return reader.read_all()
            finally:
                reader.close()
        finally:
            cursor.close()
    finally:
        connection.close()


def _query(
    config,
    database: str,
    sql: str,
    *,
    params: object | None = None,
    service: str | None = None,
) -> ds.Table:
    provider = PostgreSQLProvider(
        service=service or config.readonly_profile,
    )
    return provider.query(sql, database=database, params=params)


def _readonly_session_identity(config) -> tuple[str, str]:
    table = _query(
        config,
        config.telemetry_database,
        """
        SELECT
            current_user::TEXT AS role_name,
            current_setting('application_name')::TEXT AS application_name
        """,
    )
    return table["role_name"][0], table["application_name"][0]


def _role_sessions(
    config,
    role_name: str,
    application_name: str,
) -> set[int]:
    table = _direct_table(
        config.writer_profile,
        config.telemetry_database,
        """
        SELECT pid::BIGINT AS pid
        FROM pg_stat_activity
        WHERE usename = $1
          AND datname = current_database()
          AND application_name = $2
        """,
        (role_name, application_name),
    )
    return set(table.column("pid").to_pylist())


def test_parameterized_types_return_one_repeatable_eager_table(
    postgresql_config,
):
    instant = datetime(
        2026,
        8,
        28,
        1,
        2,
        3,
        456789,
        tzinfo=timezone.utc,
    )
    amount = Decimal("12345678901234567890.123456789012345678")
    expected = [
        {
            "integer_value": 42,
            "float_value": 2.5,
            "bool_value": True,
            "text_value": "bound text",
            "timestamp_value": kat.WallClockTimestamp(
                "2026-08-28T01:02:03.456789Z"
            ),
            "decimal_value": amount,
            "null_value": None,
        }
    ]

    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        """
        SELECT
            $1::BIGINT AS integer_value,
            $2::DOUBLE PRECISION AS float_value,
            $3::BOOLEAN AS bool_value,
            $4::TEXT AS text_value,
            $5::TIMESTAMPTZ AS timestamp_value,
            12345678901234567890.123456789012345678::NUMERIC(38, 18)
                AS decimal_value,
            $6::BIGINT AS null_value
        """,
        params=(42, 2.5, True, "bound text", instant, None),
    )

    assert type(result) is ds.Table
    assert result.columns == tuple(expected[0])
    assert len(result) == 1
    first_read = result.to_rows()
    assert first_read == expected
    first_read[0]["text_value"] = "changed outside the Table"
    assert result.to_rows() == expected

    arrow = result.to_arrow()
    assert arrow.schema.field("timestamp_value").type == pa.timestamp(
        "ns", tz="UTC"
    )
    assert arrow.schema.field("decimal_value").type == pa.decimal128(38, 18)


def test_large_result_is_fully_materialized_before_query_returns(
    postgresql_config,
):
    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        "SELECT value::BIGINT FROM generate_series(1, 200000) AS value",
    )

    assert len(result) == 200_000
    assert result["value"][0] == 1
    assert result["value"][-1] == 200_000


def test_zero_rows_keep_a_nonempty_schema(postgresql_config):
    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        "SELECT 1::BIGINT AS value WHERE FALSE",
    )

    assert result.columns == ("value",)
    assert len(result) == 0
    assert result.to_arrow().schema.field("value").type == pa.int64()


def test_remote_join_filter_and_aggregate_execute_as_one_source_query(
    postgresql_config,
):
    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        """
        SELECT
            r.process_id,
            COUNT(*)::BIGINT AS observation_count
        FROM observation AS o
        JOIN thread_registry AS r USING (thread_id)
        WHERE o.observed_at >= $1
          AND o.observed_at < $2
        GROUP BY r.process_id
        ORDER BY r.process_id
        """,
        params=(100, 220),
    )

    assert result.to_rows() == [
        {"process_id": 10, "observation_count": 2},
        {"process_id": 20, "observation_count": 2},
        {"process_id": 30, "observation_count": 1},
    ]


def test_server_observes_a_read_only_transaction(postgresql_config):
    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        "SELECT current_setting('transaction_read_only')::TEXT AS value",
        service=postgresql_config.writer_profile,
    )

    assert result["value"] == ("on",)


def test_prepared_execution_rejects_multiple_commands_but_not_semicolons_in_data(
    postgresql_config,
):
    provider = PostgreSQLProvider(service=postgresql_config.writer_profile)
    database = postgresql_config.telemetry_database
    before = _direct_table(
        postgresql_config.writer_profile,
        database,
        "SELECT count(*)::BIGINT AS count FROM write_guard",
    ).column("count")[0].as_py()

    assert provider.query(
        "SELECT 'text;not-a-command'::TEXT AS value",
        database=database,
    ).to_rows() == [{"value": "text;not-a-command"}]

    for sql in (
        "SELECT 1 AS first; SELECT 2 AS second",
        (
            "SET TRANSACTION READ WRITE; "
            "INSERT INTO write_guard(value) VALUES (99) RETURNING value"
        ),
    ):
        with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
            provider.query(sql, database=database)

    after = _direct_table(
        postgresql_config.writer_profile,
        database,
        "SELECT count(*)::BIGINT AS count FROM write_guard",
    ).column("count")[0].as_py()
    assert after == before


def test_read_only_transaction_rejects_every_supported_write_shape(
    postgresql_config,
):
    writer_capabilities = _direct_table(
        postgresql_config.writer_profile,
        postgresql_config.telemetry_database,
        """
        SELECT
            has_table_privilege(
                current_user, 'write_guard', 'INSERT'
            ) AS can_insert,
            has_schema_privilege(
                current_user, 'public', 'CREATE'
            ) AS can_create
        """,
    )
    assert writer_capabilities.to_pylist() == [
        {"can_insert": True, "can_create": True}
    ]
    before = _direct_table(
        postgresql_config.writer_profile,
        postgresql_config.telemetry_database,
        "SELECT count(*)::BIGINT AS count FROM write_guard",
    ).column("count")[0].as_py()
    provider = PostgreSQLProvider(service=postgresql_config.writer_profile)
    statements = [
        "INSERT INTO write_guard(value) VALUES (99) RETURNING value",
        "CREATE TABLE kat_forbidden_write(value INTEGER)",
        "COPY write_guard(value) FROM STDIN",
        (
            "WITH changed AS ("
            "INSERT INTO write_guard(value) VALUES (99) RETURNING value"
            ") SELECT value FROM changed"
        ),
    ]

    for sql in statements:
        with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
            provider.query(
                sql,
                database=postgresql_config.telemetry_database,
            )

    after = _direct_table(
        postgresql_config.writer_profile,
        postgresql_config.telemetry_database,
        """
        SELECT
            count(*)::BIGINT AS count,
            to_regclass('public.kat_forbidden_write') IS NOT NULL AS ddl_visible
        FROM write_guard
        """,
    )
    assert after.to_pylist() == [{"count": before, "ddl_visible": False}]


def test_readonly_profile_has_minimum_role_and_table_privileges(
    postgresql_config,
):
    provider = PostgreSQLProvider(service=postgresql_config.readonly_profile)
    telemetry = provider.query(
        """
        SELECT
            role.rolsuper,
            role.rolcreaterole,
            role.rolcreatedb,
            role.rolreplication,
            role.rolbypassrls,
            has_table_privilege(current_user, 'observation', 'SELECT')
                AND has_table_privilege(current_user, 'thread_registry', 'SELECT')
                AND has_table_privilege(current_user, 'write_guard', 'SELECT')
                AS can_select,
            has_table_privilege(current_user, 'observation', 'INSERT')
                OR has_table_privilege(current_user, 'observation', 'UPDATE')
                OR has_table_privilege(current_user, 'observation', 'DELETE')
                OR has_table_privilege(current_user, 'observation', 'TRUNCATE')
                OR has_table_privilege(current_user, 'thread_registry', 'INSERT')
                OR has_table_privilege(current_user, 'thread_registry', 'UPDATE')
                OR has_table_privilege(current_user, 'thread_registry', 'DELETE')
                OR has_table_privilege(current_user, 'thread_registry', 'TRUNCATE')
                OR has_table_privilege(current_user, 'write_guard', 'INSERT')
                OR has_table_privilege(current_user, 'write_guard', 'UPDATE')
                OR has_table_privilege(current_user, 'write_guard', 'DELETE')
                OR has_table_privilege(current_user, 'write_guard', 'TRUNCATE')
                AS can_write,
            has_schema_privilege(current_user, 'public', 'CREATE') AS can_create
        FROM pg_roles AS role
        WHERE role.rolname = current_user
        """,
        database=postgresql_config.telemetry_database,
    )

    assert telemetry.to_rows() == [
        {
            "rolsuper": False,
            "rolcreaterole": False,
            "rolcreatedb": False,
            "rolreplication": False,
            "rolbypassrls": False,
            "can_select": True,
            "can_write": False,
            "can_create": False,
        }
    ]

    control = provider.query(
        """
        SELECT
            has_table_privilege(
                current_user, 'process_registry', 'SELECT'
            ) AS can_select,
            has_table_privilege(current_user, 'process_registry', 'INSERT')
                OR has_table_privilege(current_user, 'process_registry', 'UPDATE')
                OR has_table_privilege(current_user, 'process_registry', 'DELETE')
                OR has_table_privilege(current_user, 'process_registry', 'TRUNCATE')
                AS can_write,
            has_schema_privilege(
                current_user, 'public', 'CREATE'
            ) AS can_create
        """,
        database=postgresql_config.control_database,
    )
    assert control.to_rows() == [
        {"can_select": True, "can_write": False, "can_create": False}
    ]


def test_one_provider_queries_two_databases_of_the_same_service(
    postgresql_config,
):
    provider = PostgreSQLProvider(service=postgresql_config.readonly_profile)

    telemetry = provider.query(
        """
        SELECT
            current_database()::TEXT AS database_name,
            count(*) = count(DISTINCT thread_id) AS keys_are_unique
        FROM thread_registry
        """,
        database=postgresql_config.telemetry_database,
    )
    control = provider.query(
        """
        SELECT
            current_database()::TEXT AS database_name,
            count(*) = count(DISTINCT process_id) AS keys_are_unique
        FROM process_registry
        """,
        database=postgresql_config.control_database,
    )

    assert telemetry.to_rows() == [
        {
            "database_name": postgresql_config.telemetry_database,
            "keys_are_unique": True,
        }
    ]
    assert control.to_rows() == [
        {
            "database_name": postgresql_config.control_database,
            "keys_are_unique": True,
        }
    ]


def test_positional_parameters_are_not_sql_text_substitution(
    postgresql_config,
):
    sentinel = "Robert'); DELETE FROM write_guard; --"
    result = _query(
        postgresql_config,
        postgresql_config.telemetry_database,
        """
        SELECT
            $1::TEXT AS value,
            length($1::TEXT)::BIGINT AS character_count
        """,
        params=(sentinel,),
    )

    assert result.to_rows() == [
        {"value": sentinel, "character_count": len(sentinel)}
    ]


def test_timestamp_without_timezone_requires_explicit_source_sql_conversion(
    postgresql_config,
):
    with pytest.raises(RuntimeError, match="TIMESTAMP WITHOUT TIME ZONE"):
        _query(
            postgresql_config,
            postgresql_config.telemetry_database,
            "SELECT TIMESTAMP '2026-08-28 01:02:03.456789' AS value",
        )


def test_zero_column_command_cannot_become_a_table(postgresql_config):
    with pytest.raises(RuntimeError, match="at least one column"):
        _query(
            postgresql_config,
            postgresql_config.telemetry_database,
            "SET LOCAL statement_timeout = '1s'",
        )


def test_query_resources_are_closed_before_table_is_returned(
    postgresql_config,
):
    role_name, application_name = _readonly_session_identity(postgresql_config)
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    provider = PostgreSQLProvider(service=postgresql_config.readonly_profile)

    result = provider.query(
        "SELECT pg_backend_pid()::BIGINT AS pid",
        database=postgresql_config.telemetry_database,
    )
    pid = result["pid"][0]

    assert result.to_rows() == [{"pid": pid}]
    assert pid not in _role_sessions(
        postgresql_config,
        role_name,
        application_name,
    )
    assert _role_sessions(
        postgresql_config,
        role_name,
        application_name,
    ) == baseline


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT missing_column FROM missing_relation",
        """
        SELECT
            value::BIGINT,
            CASE
                WHEN value = 150000 THEN 1 / (value - 150000)
                ELSE 1
            END::BIGINT AS guarded
        FROM generate_series(1, 200000) AS value
        """,
    ],
    ids=["sql-error", "result-read-error"],
)
def test_query_failures_release_the_session_and_allow_a_later_query(
    postgresql_config,
    sql,
):
    role_name, application_name = _readonly_session_identity(postgresql_config)
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    provider = PostgreSQLProvider(service=postgresql_config.readonly_profile)

    with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
        provider.query(
            sql,
            database=postgresql_config.telemetry_database,
        )

    assert _role_sessions(
        postgresql_config,
        role_name,
        application_name,
    ) == baseline
    recovered = provider.query(
        "SELECT 7::BIGINT AS value",
        database=postgresql_config.telemetry_database,
    )
    assert recovered.to_rows() == [{"value": 7}]
