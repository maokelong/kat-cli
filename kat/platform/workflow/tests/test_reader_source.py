from __future__ import annotations

import inspect
from pathlib import Path
import tempfile
import unittest

from datafusion import SessionContext
from datafusion.catalog import Schema, SchemaProvider, Table
import pyarrow as pa

import kat
from kat._reader import _adapt_schema_provider, _reader_source_operation


def reader(
    schema: pa.Schema,
    batches: list[pa.RecordBatch],
) -> pa.RecordBatchReader:
    return pa.RecordBatchReader.from_batches(schema, batches)


class MethodTableNamesProvider(SchemaProvider):
    def __init__(self, table: Table) -> None:
        self._table = table

    def table_names(self) -> set[str]:
        return {"events"}

    def table_exist(self, name: str) -> bool:
        return name == "events"

    def table(self, name: str) -> Table | None:
        return self._table if name == "events" else None


class PropertyTableNamesProvider(SchemaProvider):
    def __init__(self, table: Table) -> None:
        self._table = table
        self.table_names_calls = 0

    @property
    def table_names(self) -> tuple[str, ...]:  # type: ignore[override]
        self.table_names_calls += 1
        return ("events",)

    def table_exist(self, name: str) -> bool:
        return name == "events"

    def table(self, name: str) -> Table | None:
        return self._table if name == "events" else None


class PlainSchemaLookalike:
    def __init__(self) -> None:
        self.table_names_calls = 0

    @property
    def table_names(self) -> tuple[str, ...]:
        self.table_names_calls += 1
        return ("events",)

    def table(self, name: str) -> None:
        return None


class ReaderSourceTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_public_helper_has_one_mapping_argument(self) -> None:
        signature = inspect.signature(kat.schema_from_readers)
        self.assertEqual(list(signature.parameters), ["factories"])
        self.assertEqual(
            signature.parameters["factories"].kind,
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
        )

    def test_mapping_is_copied_and_discovery_does_not_call_factories(self) -> None:
        calls = 0

        def factory() -> pa.RecordBatchReader:
            nonlocal calls
            calls += 1
            return reader(pa.schema([("value", pa.int64())]), [])

        factories = {"z_events": factory, "a_events": factory}
        provider = kat.schema_from_readers(factories)
        factories.clear()

        self.assertEqual(provider.table_names, ("a_events", "z_events"))
        self.assertTrue(provider.table_exist("a_events"))
        self.assertIsNone(provider.table("missing"))
        self.assertEqual(calls, 0)
        with self.assertRaisesRegex(RuntimeError, "KAT Source operation"):
            provider.table("a_events")

    def test_reader_is_staged_once_and_supports_multiple_and_empty_batches(self) -> None:
        calls = {"events": 0, "empty": 0}
        schema = pa.schema([("value", pa.int64())])

        def events() -> pa.RecordBatchReader:
            calls["events"] += 1
            return reader(
                schema,
                [
                    pa.record_batch([[1, 2]], schema=schema),
                    pa.record_batch([[3]], schema=schema),
                ],
            )

        def empty() -> pa.RecordBatchReader:
            calls["empty"] += 1
            return reader(schema, [])

        provider = kat.schema_from_readers({"events": events, "empty": empty})
        session = SessionContext()
        with _reader_source_operation(session, staging_parent=self.root) as staging:
            first = provider.table("events")
            second = provider.table("events")
            empty_table = provider.table("empty")
            self.assertIs(first, second)
            assert first is not None and empty_table is not None
            values = pa.Table.from_batches(session.read_table(first).collect())
            empty_values = pa.Table.from_batches(
                session.read_table(empty_table).collect(),
                schema=schema,
            )
            self.assertEqual(values.to_pydict(), {"value": [1, 2, 3]})
            self.assertEqual(empty_values.schema, schema)
            self.assertEqual(empty_values.num_rows, 0)
            self.assertEqual(len(list(staging.glob("*.parquet"))), 2)
        self.assertFalse(staging.exists())
        self.assertEqual(calls, {"events": 1, "empty": 1})

        second_session = SessionContext()
        with _reader_source_operation(second_session, staging_parent=self.root):
            self.assertIsNotNone(provider.table("events"))
        self.assertEqual(calls["events"], 2)

    def test_datafusion_54_can_query_the_property_shaped_provider(self) -> None:
        calls = 0
        schema = pa.schema([("value", pa.int64())])

        def events() -> pa.RecordBatchReader:
            nonlocal calls
            calls += 1
            return reader(schema, [pa.record_batch([[2, 1]], schema=schema)])

        provider = kat.schema_from_readers({"events": events})
        session = SessionContext()
        session.catalog().register_schema("raw", provider)
        self.assertEqual(calls, 0)
        with _reader_source_operation(session, staging_parent=self.root):
            batches = session.sql(
                "SELECT value FROM raw.events ORDER BY value"
            ).collect()
        self.assertEqual(pa.Table.from_batches(batches).to_pydict(), {"value": [1, 2]})
        self.assertEqual(calls, 1)

    def test_method_table_names_provider_is_adapted_for_datafusion_54(self) -> None:
        session = SessionContext()
        dataframe = session.from_arrow(pa.table({"value": [4]}))
        provider = _adapt_schema_provider(MethodTableNamesProvider(Table(dataframe)))
        self.assertEqual(provider.table_names, ("events",))
        session.catalog().register_schema("legacy", provider)
        batches = session.sql("SELECT value FROM legacy.events").collect()
        self.assertEqual(pa.Table.from_batches(batches).to_pydict(), {"value": [4]})

    def test_official_datafusion_schema_is_adapted_and_queryable(self) -> None:
        session = SessionContext()
        schema = Schema.memory_schema(session)
        dataframe = session.from_arrow(pa.table({"value": [6]}))
        schema.register_table("events", Table(dataframe))

        provider = _adapt_schema_provider(schema)
        self.assertEqual(provider.table_names, {"events"})
        session.catalog().register_schema("official", provider)
        batches = session.sql("SELECT value FROM official.events").collect()

        self.assertEqual(pa.Table.from_batches(batches).to_pydict(), {"value": [6]})

    def test_query_adapter_does_not_enumerate_property_table_names(self) -> None:
        session = SessionContext()
        dataframe = session.from_arrow(pa.table({"value": [4]}))
        original = PropertyTableNamesProvider(Table(dataframe))

        provider = _adapt_schema_provider(original)
        self.assertEqual(original.table_names_calls, 0)
        session.catalog().register_schema("lazy", provider)
        batches = session.sql("SELECT value FROM lazy.events").collect()

        self.assertEqual(pa.Table.from_batches(batches).to_pydict(), {"value": [4]})
        self.assertEqual(original.table_names_calls, 0)

    def test_plain_schema_lookalike_is_rejected_without_property_access(self) -> None:
        lookalike = PlainSchemaLookalike()

        with self.assertRaisesRegex(TypeError, "DataFusion schema provider"):
            _adapt_schema_provider(lookalike)

        self.assertEqual(lookalike.table_names_calls, 0)

    def test_failure_is_cached_and_partial_staging_is_removed(self) -> None:
        calls = 0
        schema = pa.schema([("value", pa.int64())])

        def broken_batches():
            yield pa.record_batch([[1]], schema=schema)
            raise ValueError("decoder failed")

        def factory() -> pa.RecordBatchReader:
            nonlocal calls
            calls += 1
            return pa.RecordBatchReader.from_batches(schema, broken_batches())

        provider = kat.schema_from_readers({"events": factory})
        session = SessionContext()
        with _reader_source_operation(session, staging_parent=self.root) as staging:
            with self.assertRaisesRegex(ValueError, "decoder failed"):
                provider.table("events")
            self.assertEqual(list(staging.iterdir()), [])
            with self.assertRaisesRegex(RuntimeError, "failed earlier"):
                provider.table("events")
            self.assertEqual(calls, 1)

    def test_invalid_mapping_and_factory_result_fail_directly(self) -> None:
        with self.assertRaises(TypeError):
            kat.schema_from_readers([])  # type: ignore[arg-type]
        with self.assertRaises(ValueError):
            kat.schema_from_readers({"Bad-Name": lambda: None})  # type: ignore[dict-item]
        with self.assertRaises(TypeError):
            kat.schema_from_readers({"events": object()})  # type: ignore[dict-item]

        provider = kat.schema_from_readers({"events": lambda: object()})  # type: ignore[return-value]
        with _reader_source_operation(SessionContext(), staging_parent=self.root):
            with self.assertRaisesRegex(TypeError, "RecordBatchReader"):
                provider.table("events")


if __name__ == "__main__":
    unittest.main()
