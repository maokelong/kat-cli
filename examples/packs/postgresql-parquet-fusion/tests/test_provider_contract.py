from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal

import kat
import pyarrow as pa
import pytest

from kat import datasource as ds
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
        if self._kind == "query" and self._backend.failure == "query":
            raise RuntimeError(_PRIVATE_CONNECTION)

    def fetch_record_batch(self) -> _FakeReader:
        reader = _FakeReader(self._backend)
        self._backend.reader = reader
        return reader

    def close(self) -> None:
        self.closed = True


class _FakeConnection:
    def __init__(self, backend: _FakeBackend) -> None:
        self.setup = _FakeCursor(backend, "setup")
        self.query = _FakeCursor(backend, "query")
        self._cursors = iter((self.setup, self.query))
        self.rolled_back = False
        self.closed = False

    def cursor(self) -> _FakeCursor:
        return next(self._cursors)

    def rollback(self) -> None:
        self.rolled_back = True

    def close(self) -> None:
        self.closed = True


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


def test_public_query_returns_a_table_after_closing_query_resources(monkeypatch):
    backend = _install_backend(monkeypatch)
    provider = PostgreSQLProvider(service="service")

    result = provider.query("SELECT 7", database="database")

    assert type(result) is ds.Table
    assert result.to_rows() == [{"value": 7}]
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
    physical = ds.to_arrow(result).schema
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
    assert ds.to_arrow(result).schema.field("amount").type == pa.decimal128(
        38, 18
    )
    _assert_all_acquired_resources_are_closed(backend)


@pytest.mark.parametrize(
    "failure",
    ["query", "read", "reader-close", "read-and-close"],
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
