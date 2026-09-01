from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal

import kat
import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from kat import dataprovider as dp
from kat.pack.datasources import postgresql as postgresql_module
from kat.pack.datasources.postgresql import PostgreSQLProvider


_PRIVATE_SERVICE = "private-service-sentinel"
_PRIVATE_DATABASE = "private-database-sentinel"
_PRIVATE_PASSWORD = "private-password-sentinel"
_PRIVATE_CONNECTION = (
    "postgresql:///private-database-sentinel"
    f"?service=private-service-sentinel&password={_PRIVATE_PASSWORD}"
)


class _FakeStatement:
    def set_options(self, **options) -> None:
        assert options


class _FakeReader:
    def __init__(self, backend: _FakeBackend) -> None:
        self._backend = backend
        self.closed = False

    def read_all(self) -> pa.Table:
        if self._backend.failure in {"read", "read-and-close"}:
            raise RuntimeError(_PRIVATE_CONNECTION)
        if self._backend.result is not None:
            return self._backend.result
        return pa.table({"value": pa.array([7], type=pa.int64())})

    def close(self) -> None:
        self.closed = True
        if self._backend.failure in {"reader-close", "read-and-close"}:
            raise RuntimeError(_PRIVATE_CONNECTION)


class _FakeCursor:
    def __init__(self, backend: _FakeBackend, kind: str) -> None:
        self._backend = backend
        self._kind = kind
        self.adbc_statement = _FakeStatement()
        self.closed = False

    def execute(self, sql, params=None) -> None:
        del sql, params
        if self._backend.failure == self._kind:
            raise RuntimeError(_PRIVATE_CONNECTION)

    def fetch_record_batch(self) -> _FakeReader:
        reader = _FakeReader(self._backend)
        self._backend.reader = reader
        return reader

    def close(self) -> None:
        self.closed = True
        if self._backend.failure == f"{self._kind}-close":
            raise RuntimeError(_PRIVATE_CONNECTION)


class _FakeConnection:
    def __init__(self, backend: _FakeBackend) -> None:
        self._backend = backend
        self.setup = _FakeCursor(backend, "setup")
        self.query = _FakeCursor(backend, "query")
        self._cursors = iter((self.setup, self.query))
        self.rolled_back = False
        self.closed = False

    def cursor(self) -> _FakeCursor:
        return next(self._cursors)

    def rollback(self) -> None:
        self.rolled_back = True
        if self._backend.failure == "rollback":
            raise RuntimeError(_PRIVATE_CONNECTION)

    def close(self) -> None:
        self.closed = True
        if self._backend.failure == "connection-close":
            raise RuntimeError(_PRIVATE_CONNECTION)


@dataclass
class _FakeBackend:
    failure: str | None = None
    result: pa.Table | None = None
    reader: _FakeReader | None = None
    connection: _FakeConnection = field(init=False)

    def __post_init__(self) -> None:
        self.connection = _FakeConnection(self)


def _install_backend(
    monkeypatch,
    *,
    failure: str | None = None,
    result: pa.Table | None = None,
) -> _FakeBackend:
    backend = _FakeBackend(failure=failure, result=result)

    def connect(*args, **kwargs):
        del args, kwargs
        if backend.failure == "connect":
            raise RuntimeError(_PRIVATE_CONNECTION)
        return backend.connection

    monkeypatch.setattr(postgresql_module.dbapi, "connect", connect)
    return backend


def _assert_all_acquired_resources_are_closed(backend: _FakeBackend) -> None:
    assert backend.connection.setup.closed
    assert backend.connection.query.closed
    assert backend.connection.rolled_back
    assert backend.connection.closed
    if backend.reader is not None:
        assert backend.reader.closed


def test_postgresql_provider_is_a_pack_owned_ordinary_class():
    provider = PostgreSQLProvider(service="not-connected")

    assert type(provider) is PostgreSQLProvider
    assert not hasattr(provider, "context")


def test_local_catalog_and_eager_table_share_one_fusion_query(tmp_path):
    placement = tmp_path / "thread_placement.parquet"
    pq.write_table(
        pa.table(
            {
                "thread_id": pa.array([101], type=pa.int64()),
                "cpu": pa.array([0], type=pa.int64()),
            }
        ),
        placement,
    )
    telemetry = dp.Table.from_arrow(
        pa.table(
            {
                "thread_id": pa.array([101], type=pa.int64()),
                "observed_at": pa.array([100], type=pa.int64()),
            }
        )
    )

    result = dp.DataFusionProvider(
        tables={"telemetry": telemetry},
        catalog=dp.open(tables={"thread_placement": placement}),
    ).query(
        """
        SELECT t.thread_id, placement.cpu
        FROM telemetry AS t
        JOIN thread_placement AS placement USING (thread_id)
        """,
    )

    assert result.to_rows() == [{"thread_id": 101, "cpu": 0}]


def test_public_query_returns_a_table_after_closing_query_resources(monkeypatch):
    backend = _install_backend(monkeypatch)
    provider = PostgreSQLProvider(service="service")

    result = provider.query("SELECT 7", database="database")

    assert type(result) is dp.Table
    assert result.to_rows() == [{"value": 7}]
    _assert_all_acquired_resources_are_closed(backend)


def test_query_workflow_publishes_a_fake_adbc_result(monkeypatch, kat_run):
    backend = _install_backend(
        monkeypatch,
        result=pa.table(
            {
                "thread_id": pa.array([101], type=pa.int64()),
                "clock_domain": pa.array(
                    ["fixture.observation_clock"], type=pa.string()
                ),
                "clock_value": pa.array([100], type=pa.int64()),
                "cpu_usage": pa.array([0.75], type=pa.float64()),
            }
        ),
    )

    output = kat_run(
        workflow="query-observations",
        arguments=[
            "--service",
            "fixture-service",
            "--database",
            "telemetry",
            "--clock-domain",
            "fixture.observation_clock",
            "--start-clock-value",
            "100",
            "--end-clock-value",
            "200",
        ],
    )["main"]

    assert output.to_pylist() == [
        {
            "thread_id": 101,
            "clock_domain": "fixture.observation_clock",
            "clock_value": 100,
            "cpu_usage": 0.75,
        }
    ]
    _assert_all_acquired_resources_are_closed(backend)


def test_public_query_normalizes_adbc_numeric_and_absolute_timestamp(monkeypatch):
    numeric_type = pa.opaque(pa.string(), "numeric", "PostgreSQL")
    numeric = pa.ExtensionArray.from_storage(
        numeric_type,
        pa.array(["12.340", None]),
    )
    arrow = pa.Table.from_arrays(
        [
            numeric,
            pa.array([0, None], type=pa.timestamp("us", tz="UTC")),
            pa.array(["12.340", None], type=pa.string()),
        ],
        names=["amount", "observed_at", "text_value"],
    )
    backend = _install_backend(monkeypatch, result=arrow)

    result = PostgreSQLProvider(service="service").query(
        "SELECT typed values",
        database="database",
    )

    assert result.to_rows() == [
        {
            "amount": Decimal("12.340000000000000000"),
            "observed_at": kat.WallClockTimestamp("1970-01-01T00:00:00Z"),
            "text_value": "12.340",
        },
        {"amount": None, "observed_at": None, "text_value": None},
    ]
    physical = result.to_arrow().schema
    assert physical.field("amount").type == pa.decimal128(38, 18)
    assert physical.field("observed_at").type == pa.timestamp("ns", tz="UTC")
    assert physical.field("text_value").type == pa.string()
    _assert_all_acquired_resources_are_closed(backend)


def test_public_query_rejects_naive_timestamp_after_closing_resources(monkeypatch):
    arrow = pa.table(
        {"observed_at": pa.array([0], type=pa.timestamp("us"))}
    )
    backend = _install_backend(monkeypatch, result=arrow)

    with pytest.raises(RuntimeError, match="TIMESTAMP WITHOUT TIME ZONE"):
        PostgreSQLProvider(service="service").query(
            "SELECT local timestamp",
            database="database",
        )

    _assert_all_acquired_resources_are_closed(backend)


def test_empty_adbc_numeric_keeps_the_canonical_decimal_schema(monkeypatch):
    numeric_type = pa.opaque(pa.string(), "numeric", "PostgreSQL")
    numeric = pa.ExtensionArray.from_storage(
        numeric_type,
        pa.array([], type=pa.string()),
    )
    backend = _install_backend(
        monkeypatch,
        result=pa.Table.from_arrays([numeric], names=["amount"]),
    )

    result = PostgreSQLProvider(service="service").query(
        "SELECT empty numeric",
        database="database",
    )

    assert len(result) == 0
    assert result.to_arrow().schema.field("amount").type == pa.decimal128(
        38, 18
    )
    _assert_all_acquired_resources_are_closed(backend)


@pytest.mark.parametrize(
    "failure",
    [
        "query",
        "read",
        "reader-close",
        "read-and-close",
        "query-close",
        "rollback",
        "connection-close",
    ],
)
def test_public_query_failures_close_resources_without_exposing_connection_values(
    monkeypatch,
    failure,
):
    backend = _install_backend(monkeypatch, failure=failure)
    provider = PostgreSQLProvider(service=_PRIVATE_SERVICE)

    with pytest.raises(RuntimeError) as captured:
        provider.query("SELECT 7", database=_PRIVATE_DATABASE)

    rendered = str(captured.value)
    assert "PostgreSQL query" in rendered
    assert _PRIVATE_SERVICE not in rendered
    assert _PRIVATE_DATABASE not in rendered
    assert _PRIVATE_PASSWORD not in rendered
    assert _PRIVATE_CONNECTION not in rendered
    if failure == "read-and-close":
        assert "cleanup" not in rendered
    _assert_all_acquired_resources_are_closed(backend)


@pytest.mark.parametrize("failure", ["setup", "setup-close"])
def test_setup_failures_close_the_connection_without_exposing_values(
    monkeypatch,
    failure,
):
    backend = _install_backend(monkeypatch, failure=failure)

    with pytest.raises(RuntimeError, match="PostgreSQL query failed") as captured:
        PostgreSQLProvider(service=_PRIVATE_SERVICE).query(
            "SELECT 7",
            database=_PRIVATE_DATABASE,
        )

    assert _PRIVATE_CONNECTION not in str(captured.value)
    assert backend.connection.setup.closed
    assert not backend.connection.query.closed
    assert backend.connection.rolled_back
    assert backend.connection.closed


def test_connect_failure_does_not_expose_connection_values(monkeypatch):
    _install_backend(monkeypatch, failure="connect")

    with pytest.raises(RuntimeError, match="PostgreSQL query failed") as captured:
        PostgreSQLProvider(service=_PRIVATE_SERVICE).query(
            "SELECT 7",
            database=_PRIVATE_DATABASE,
        )

    assert _PRIVATE_CONNECTION not in str(captured.value)


@pytest.mark.parametrize(
    "params",
    [
        {"value": 1},
        "value",
        b"value",
        [[1]],
        pa.table({"value": [1]}),
    ],
)
def test_query_rejects_non_single_positional_parameter_sets(params):
    provider = PostgreSQLProvider(service="not-read")

    with pytest.raises(TypeError, match="single positional sequence"):
        provider.query(
            "SELECT $1::BIGINT AS value",
            database="not-read",
            params=params,
        )
