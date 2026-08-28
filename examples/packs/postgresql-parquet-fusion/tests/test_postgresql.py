from __future__ import annotations

from datetime import datetime
from pathlib import Path
from urllib.parse import quote

from adbc_driver_postgresql import dbapi
from datafusion import SessionContext
import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from kat.pack.helpers.datasources import postgresql as postgresql_module
from kat.pack.helpers.datasources.postgresql import PostgreSQLExecutor


def _uri(profile: str, database: str) -> str:
    return (
        f"postgresql:///{quote(database, safe='')}"
        f"?service={quote(profile, safe='')}"
    )


def _direct_table(
    profile: str,
    database: str,
    sql: str,
    params: tuple[object, ...] | None = None,
) -> pa.Table:
    connection = dbapi.connect(_uri(profile, database), autocommit=True)
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


def _scratch(root: Path, name: str) -> Path:
    scratch = root / name
    scratch.mkdir()
    return scratch


def _executor_table(
    profile: str,
    database: str,
    sql: str,
    *,
    params: object | None = None,
    scratch: Path,
) -> pa.Table:
    executor = PostgreSQLExecutor(profile=profile, database=database)
    try:
        with executor.execute(sql, params, scratch=scratch) as reader:
            assert type(reader) is pa.RecordBatchReader
            return reader.read_all()
    finally:
        executor.close()


def _readonly_session_identity(config, tmp_path: Path) -> tuple[str, str]:
    table = _executor_table(
        config.readonly_profile,
        config.telemetry_database,
        """
        SELECT
            current_user::TEXT AS role_name,
            current_setting('application_name')::TEXT AS application_name
        """,
        scratch=_scratch(tmp_path, "identity"),
    )
    return (
        table.column("role_name")[0].as_py(),
        table.column("application_name")[0].as_py(),
    )


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


def test_parameterized_type_matrix_streams_through_parquet_and_datafusion(
    postgresql_config,
    tmp_path,
):
    timestamp = datetime(2026, 8, 28, 1, 2, 3, 456789)
    expected_schema = pa.schema(
        [
            ("integer_value", pa.int64()),
            ("float_value", pa.float64()),
            ("bool_value", pa.bool_()),
            ("text_value", pa.string()),
            ("timestamp_value", pa.timestamp("us")),
            ("null_value", pa.int64()),
        ]
    )
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )
    parquet_path = tmp_path / "type-matrix.parquet"

    with executor.execute(
        """
        SELECT
            $1::BIGINT AS integer_value,
            $2::DOUBLE PRECISION AS float_value,
            $3::BOOLEAN AS bool_value,
            $4::TEXT AS text_value,
            $5::TIMESTAMP AS timestamp_value,
            $6::BIGINT AS null_value
        """,
        (42, 2.5, True, "bound text", timestamp, None),
        scratch=_scratch(tmp_path, "type-matrix-scratch"),
    ) as reader:
        assert reader.schema == expected_schema
        with pq.ParquetWriter(parquet_path, reader.schema) as sink:
            for batch in reader:
                sink.write_batch(batch)
    executor.close()

    session = SessionContext()
    session.register_parquet("type_matrix", str(parquet_path))
    batches = session.sql("SELECT * FROM type_matrix").collect()
    result = pa.Table.from_batches(batches)
    assert result.schema == pa.schema(
        [
            ("integer_value", pa.int64()),
            ("float_value", pa.float64()),
            ("bool_value", pa.bool_()),
            ("text_value", pa.string_view()),
            ("timestamp_value", pa.timestamp("us")),
            ("null_value", pa.int64()),
        ]
    )
    assert result.to_pylist() == [
        {
            "integer_value": 42,
            "float_value": 2.5,
            "bool_value": True,
            "text_value": "bound text",
            "timestamp_value": timestamp,
            "null_value": None,
        }
    ]


def test_large_result_is_delivered_as_multiple_record_batches(
    postgresql_config,
    tmp_path,
):
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )
    with executor.execute(
        "SELECT value::BIGINT FROM generate_series(1, 200000) AS value",
        None,
        scratch=_scratch(tmp_path, "multi-batch"),
    ) as reader:
        batches = list(reader)
    executor.close()

    assert len(batches) > 1
    assert sum(batch.num_rows for batch in batches) == 200_000


def test_zero_rows_keep_a_nonempty_schema(postgresql_config, tmp_path):
    result = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.telemetry_database,
        "SELECT 1::BIGINT AS value WHERE FALSE",
        scratch=_scratch(tmp_path, "zero-rows"),
    )

    assert result.schema == pa.schema([("value", pa.int64())])
    assert result.num_rows == 0


def test_server_observes_a_read_only_transaction(postgresql_config, tmp_path):
    result = _executor_table(
        postgresql_config.writer_profile,
        postgresql_config.telemetry_database,
        "SELECT current_setting('transaction_read_only')::TEXT AS value",
        scratch=_scratch(tmp_path, "read-only-setting"),
    )

    assert result.to_pydict() == {"value": ["on"]}


def test_read_only_transaction_rejects_every_supported_write_shape(
    postgresql_config,
    tmp_path,
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
    statements = {
        "insert_returning": (
            "INSERT INTO write_guard(value) VALUES (99) RETURNING value"
        ),
        "ddl": "CREATE TABLE kat_forbidden_write(value INTEGER)",
        "copy_from": "COPY write_guard(value) FROM STDIN",
        "data_modifying_cte": (
            "WITH changed AS ("
            "INSERT INTO write_guard(value) VALUES (99) RETURNING value"
            ") SELECT value FROM changed"
        ),
    }

    for name, sql in statements.items():
        with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
            _executor_table(
                postgresql_config.writer_profile,
                postgresql_config.telemetry_database,
                sql,
                scratch=_scratch(tmp_path, name),
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
    tmp_path,
):
    result = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.telemetry_database,
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
        scratch=_scratch(tmp_path, "minimum-role"),
    )

    assert result.to_pylist() == [
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

    control = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.control_database,
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
        scratch=_scratch(tmp_path, "minimum-control-role"),
    )
    assert control.to_pylist() == [
        {"can_select": True, "can_write": False, "can_create": False}
    ]


def test_database_override_and_remote_fixture_keys_are_exact(
    postgresql_config,
    tmp_path,
):
    telemetry = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.telemetry_database,
        """
        SELECT
            current_database()::TEXT AS database_name,
            count(*) = count(DISTINCT thread_id) AS thread_ids_unique
        FROM thread_registry
        """,
        scratch=_scratch(tmp_path, "unique-threads"),
    )
    control = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.control_database,
        """
        SELECT
            current_database()::TEXT AS database_name,
            count(*) = count(DISTINCT process_id) AS process_ids_unique
        FROM process_registry
        """,
        scratch=_scratch(tmp_path, "unique-processes"),
    )

    assert telemetry.to_pylist() == [
        {
            "database_name": postgresql_config.telemetry_database,
            "thread_ids_unique": True,
        }
    ]
    assert control.to_pylist() == [
        {
            "database_name": postgresql_config.control_database,
            "process_ids_unique": True,
        }
    ]


def test_prepared_result_path_rejects_multiple_commands(
    postgresql_config,
    tmp_path,
):
    with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
        _executor_table(
            postgresql_config.readonly_profile,
            postgresql_config.telemetry_database,
            "SELECT 1 AS value; SELECT 2 AS value",
            scratch=_scratch(tmp_path, "multiple-commands"),
        )


@pytest.mark.parametrize(
    ("sql", "expected"),
    [
        ("SELECT ';'::TEXT AS value", ";"),
        ("SELECT 1::BIGINT AS value /* ; */", 1),
        ("SELECT $$;$$::TEXT AS value", ";"),
    ],
)
def test_semicolons_inside_postgresql_text_are_not_split(
    postgresql_config,
    tmp_path,
    sql,
    expected,
):
    result = _executor_table(
        postgresql_config.readonly_profile,
        postgresql_config.telemetry_database,
        sql,
        scratch=_scratch(tmp_path, f"semicolon-{type(expected).__name__}"),
    )

    assert result.column("value").to_pylist() == [expected]


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT %s::BIGINT AS value",
        "SELECT :value::BIGINT AS value",
    ],
)
def test_non_postgresql_parameter_syntax_is_rejected(
    postgresql_config,
    tmp_path,
    sql,
):
    with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
        _executor_table(
            postgresql_config.readonly_profile,
            postgresql_config.telemetry_database,
            sql,
            params=(1,),
            scratch=_scratch(tmp_path, f"invalid-syntax-{len(sql)}"),
        )


def test_zero_column_command_cannot_become_a_table(postgresql_config, tmp_path):
    with pytest.raises(RuntimeError, match="must return at least one column"):
        _executor_table(
            postgresql_config.readonly_profile,
            postgresql_config.telemetry_database,
            "SET LOCAL statement_timeout = '1s'",
            scratch=_scratch(tmp_path, "zero-column"),
        )


@pytest.mark.parametrize(
    "params",
    [
        {"value": 1},
        "value",
        b"value",
        [[1]],
        pa.table({"value": [1]}),
        pa.record_batch([[1]], names=["value"]),
        pa.RecordBatchReader.from_batches(pa.schema([("value", pa.int64())]), []),
    ],
)
def test_query_rejects_non_single_positional_parameter_sets(params, tmp_path):
    executor = PostgreSQLExecutor(profile="not-read", database="not-read")

    try:
        with pytest.raises(TypeError, match="single positional sequence"):
            with executor.execute(
                "SELECT $1::BIGINT AS value",
                params,
                scratch=_scratch(tmp_path, "invalid-params"),
            ):
                pass
    finally:
        if isinstance(params, pa.RecordBatchReader):
            params.close()
        executor.close()


def test_execute_is_inert_until_context_entry(postgresql_config, tmp_path):
    role_name, application_name = _readonly_session_identity(
        postgresql_config, tmp_path
    )
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )

    manager = executor.execute(
        "SELECT pg_backend_pid()::BIGINT AS pid",
        None,
        scratch=_scratch(tmp_path, "inert"),
    )
    assert _role_sessions(postgresql_config, role_name, application_name) == baseline

    with manager as reader:
        pid = reader.read_all().column("pid")[0].as_py()
        assert pid in _role_sessions(
            postgresql_config, role_name, application_name
        )
    assert _role_sessions(postgresql_config, role_name, application_name) == baseline
    executor.close()


def test_base_exception_during_enter_closes_every_acquired_resource(
    monkeypatch,
    tmp_path,
):
    events: list[str] = []

    class Statement:
        def set_options(self, **options):
            assert options
            events.append("query.options")

    class Cursor:
        def __init__(self, kind: str) -> None:
            self.kind = kind
            self.adbc_statement = Statement()

        def execute(self, sql, params=None):
            del sql, params
            events.append(f"{self.kind}.execute")
            if self.kind == "query":
                raise KeyboardInterrupt

        def close(self):
            events.append(f"{self.kind}.close")

    class Connection:
        def __init__(self) -> None:
            self.cursors = iter((Cursor("setup"), Cursor("query")))

        def cursor(self):
            return next(self.cursors)

        def rollback(self):
            events.append("connection.rollback")

        def close(self):
            events.append("connection.close")

    monkeypatch.setattr(
        postgresql_module.dbapi,
        "connect",
        lambda *args, **kwargs: Connection(),
    )
    executor = PostgreSQLExecutor(profile="unused", database="unused")

    with pytest.raises(KeyboardInterrupt):
        with executor.execute(
            "SELECT 1",
            None,
            scratch=_scratch(tmp_path, "base-exception"),
        ):
            pass

    assert events == [
        "setup.execute",
        "setup.close",
        "query.options",
        "query.execute",
        "query.close",
        "connection.rollback",
        "connection.close",
    ]


def test_each_query_uses_and_closes_a_distinct_backend(
    postgresql_config,
    tmp_path,
):
    role_name, application_name = _readonly_session_identity(
        postgresql_config, tmp_path
    )
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )
    pids = []

    for index in range(2):
        with executor.execute(
            "SELECT pg_backend_pid()::BIGINT AS pid",
            None,
            scratch=_scratch(tmp_path, f"connection-{index}"),
        ) as reader:
            pid = reader.read_all().column("pid")[0].as_py()
        pids.append(pid)
        assert pid not in _role_sessions(
            postgresql_config, role_name, application_name
        )
        assert (
            _role_sessions(postgresql_config, role_name, application_name)
            == baseline
        )
    executor.close()

    assert pids[0] != pids[1]


def test_partial_consumption_and_query_errors_leave_no_session(
    postgresql_config,
    tmp_path,
):
    role_name, application_name = _readonly_session_identity(
        postgresql_config, tmp_path
    )
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )

    with executor.execute(
        """
        SELECT pg_backend_pid()::BIGINT AS pid, value::BIGINT
        FROM generate_series(1, 200000) AS value
        """,
        None,
        scratch=_scratch(tmp_path, "partial-stream"),
    ) as reader:
        pid = reader.read_next_batch().column("pid")[0].as_py()
    assert pid not in _role_sessions(
        postgresql_config, role_name, application_name
    )

    with pytest.raises(RuntimeError, match="PostgreSQL query failed"):
        with executor.execute(
            "SELECT missing_column FROM missing_relation",
            None,
            scratch=_scratch(tmp_path, "query-error"),
        ):
            pass
    assert _role_sessions(postgresql_config, role_name, application_name) == baseline
    executor.close()


def test_stream_errors_are_sanitized_and_leave_no_session(
    postgresql_config,
    tmp_path,
):
    role_name, application_name = _readonly_session_identity(
        postgresql_config, tmp_path
    )
    baseline = _role_sessions(postgresql_config, role_name, application_name)
    executor = PostgreSQLExecutor(
        profile=postgresql_config.readonly_profile,
        database=postgresql_config.telemetry_database,
    )

    with executor.execute(
        """
        SELECT
            pg_backend_pid()::BIGINT AS pid,
            value::BIGINT,
            CASE
                WHEN value = 150000 THEN 1 / (value - 150000)
                ELSE 1
            END::BIGINT AS guarded
        FROM generate_series(1, 200000) AS value
        """,
        None,
        scratch=_scratch(tmp_path, "stream-error"),
    ) as reader:
        pid = reader.read_next_batch().column("pid")[0].as_py()
        with pytest.raises(RuntimeError, match="PostgreSQL result stream failed"):
            reader.read_all()
    assert pid not in _role_sessions(
        postgresql_config, role_name, application_name
    )
    assert _role_sessions(postgresql_config, role_name, application_name) == baseline
    executor.close()


def test_connection_errors_do_not_expose_profile_or_database_values(tmp_path):
    profile = "missing-private-profile-sentinel"
    database = "private-database-sentinel"
    executor = PostgreSQLExecutor(profile=profile, database=database)

    with pytest.raises(RuntimeError) as captured:
        with executor.execute(
            "SELECT 1::BIGINT AS value",
            None,
            scratch=_scratch(tmp_path, "safe-error"),
        ):
            pass
    executor.close()

    rendered = str(captured.value)
    assert "PostgreSQL query failed" in rendered
    assert profile not in rendered
    assert database not in rendered


def test_close_is_idempotent_and_prevents_later_entry(tmp_path):
    executor = PostgreSQLExecutor(profile="not-read", database="not-read")
    manager = executor.execute(
        "SELECT 1::BIGINT AS value",
        None,
        scratch=_scratch(tmp_path, "closed"),
    )

    executor.close()
    executor.close()
    with pytest.raises(RuntimeError, match="closed"):
        with manager:
            pass
