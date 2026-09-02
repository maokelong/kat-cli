from pathlib import Path
import sqlite3

import pyarrow as pa
import pytest

from kat import dataprovider as dp
from kat.pack.datasources.trace_streamer import TraceStreamerSQLiteProvider


def _database(path: Path) -> Path:
    connection = sqlite3.connect(path)
    try:
        connection.execute("CREATE TABLE event(value INTEGER NOT NULL)")
        connection.commit()
    finally:
        connection.close()
    return path.resolve(strict=True)


def test_provider_declares_inspection_metadata():
    declaration = TraceStreamerSQLiteProvider.__kat_provider__

    assert declaration.name == "trace-streamer-sqlite"
    assert declaration.description
    guide = Path(__file__).parents[1] / "knowledge" / declaration.guide
    assert guide.is_file()
    assert guide.read_text(encoding="utf-8").startswith("# Trace Streamer SQLite Provider")


def test_provider_requires_an_exact_absolute_regular_file(tmp_path: Path):
    database = _database(tmp_path / "trace.db")

    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))

    assert type(provider) is TraceStreamerSQLiteProvider
    with pytest.raises(ValueError, match="absolute"):
        TraceStreamerSQLiteProvider(sqlite_path="trace.db")
    with pytest.raises(ValueError, match="exist"):
        TraceStreamerSQLiteProvider(sqlite_path=str(tmp_path / "missing.db"))
    with pytest.raises(ValueError, match="regular file"):
        TraceStreamerSQLiteProvider(sqlite_path=str(tmp_path))
    with pytest.raises(ValueError, match="exact"):
        TraceStreamerSQLiteProvider(
            sqlite_path=str(database.parent / "child" / ".." / database.name)
        )


def test_query_binds_named_parameters_and_returns_a_physical_table(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    connection = sqlite3.connect(database)
    try:
        connection.executemany("INSERT INTO event VALUES (?)", [(1,), (2,), (3,)])
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))

    result = provider.query(
        "SELECT value FROM event WHERE value >= :minimum ORDER BY value",
        schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        params={"minimum": 2},
    )

    assert type(result) is dp.Table
    assert result.to_rows() == [{"value": 2}, {"value": 3}]


def test_query_requires_a_named_mapping_and_exact_physical_schema(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    connection = sqlite3.connect(database)
    try:
        connection.execute("INSERT INTO event VALUES (128)")
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))

    with pytest.raises(TypeError, match="named mapping"):
        provider.query(
            "SELECT value FROM event WHERE value = ?",
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
            params=[128],
        )
    with pytest.raises(ValueError, match="exactly match schema order"):
        provider.query(
            "SELECT value AS actual FROM event",
            schema=pa.schema([pa.field("expected", pa.int64(), nullable=False)]),
        )
    with pytest.raises(ValueError, match="column 'value'.*int8"):
        provider.query(
            "SELECT value FROM event",
            schema=pa.schema([pa.field("value", pa.int8(), nullable=False)]),
        )
    with pytest.raises(TypeError, match="column 'value'.*exact type int"):
        provider.query(
            "SELECT 1.5 AS value",
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )
    with pytest.raises(TypeError, match="column 'value'.*exact type float"):
        provider.query(
            "SELECT 1 AS value",
            schema=pa.schema([pa.field("value", pa.float64(), nullable=False)]),
        )
    with pytest.raises(ValueError, match="column 'value'.*overflows float"):
        provider.query(
            "SELECT 1e40 AS value",
            schema=pa.schema([pa.field("value", pa.float32(), nullable=False)]),
        )


def test_query_preserves_the_declared_schema_for_empty_results_and_rejects_nulls(
    tmp_path: Path,
):
    database = _database(tmp_path / "trace.db")
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))
    schema = pa.schema(
        [pa.field("value", pa.int64(), nullable=False)],
        metadata={b"source": b"trace-streamer"},
    )

    result = provider.query(
        "SELECT value FROM event WHERE 0",
        schema=schema,
    )

    assert result.to_rows() == []
    assert result.to_arrow().schema.equals(schema, check_metadata=True)
    with pytest.raises(ValueError, match="column 'value'.*not nullable"):
        provider.query("SELECT NULL AS value", schema=schema)


def test_query_rejects_attach_ddl_dml_and_pragma(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    attached = tmp_path / "attached.db"
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))
    empty_schema = pa.schema([pa.field("value", pa.int64())])

    forbidden = [
        ("ATTACH DATABASE :path AS escaped", {"path": str(attached)}),
        ("CREATE TABLE escaped(value INTEGER)", {}),
        ("INSERT INTO event VALUES (1)", {}),
        ("PRAGMA user_version", {}),
    ]
    for sql, params in forbidden:
        with pytest.raises(sqlite3.DatabaseError, match="(?i)(authorized|readonly)"):
            provider.query(sql, schema=empty_schema, params=params)

    assert not attached.exists()
