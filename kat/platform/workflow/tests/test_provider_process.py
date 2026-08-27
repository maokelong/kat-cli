from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

import pyarrow as pa
import pyarrow.parquet as pq


class ProviderProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.candidate_index = 0

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(
        self, request: object
    ) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
                "-B",
                "-X",
                "utf8",
                "-u",
                "-m",
                "_kat_runtime",
                "--request",
                str(request_path),
                "--response",
                str(response_path),
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env={**os.environ, "NO_COLOR": "1"},
        )
        self.assertTrue(
            response_path.is_file(),
            completed.stderr.decode(errors="replace"),
        )
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def pack(self, body: str) -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "entry.py").write_text(body, encoding="utf-8")
        return pack.resolve()

    def candidate(self) -> tuple[str, Path, Path]:
        data_home = self.root / "data-home"
        runs = data_home / "runs"
        runs.mkdir(parents=True)
        self.candidate_index += 1
        candidate_id = f"019f6e00-0000-7000-8000-{self.candidate_index:012x}"
        candidate = runs / candidate_id
        candidate.mkdir()
        return candidate_id, candidate.resolve(), data_home.resolve()

    def request(
        self,
        pack: Path,
        candidate_id: str,
        candidate: Path,
        data_home: Path,
        dataset: dict[str, object] | None = None,
    ) -> dict[str, object]:
        request: dict[str, object] = {
            "operation": "run_workflow",
            "pack_name": "example",
            "pack_path": str(pack),
            "workflow_name": "analyze",
            "arguments": [],
            "candidate_id": candidate_id,
            "candidate_path": str(candidate),
            "datasource_root": str(data_home / "datasources" / "example"),
        }
        if dataset is not None:
            request["dataset"] = dataset
        return request

    def test_custom_source_executor_can_publish_one_table(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ExampleExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("event_id", pa.int64()), ("label", pa.string())])
        yield pa.RecordBatchReader.from_batches(
            schema,
            [pa.record_batch([[1, 2], ["start", "stop"]], schema=schema)],
        )

    def close(self):
        pass


@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables=[],
)
def analyze(ctx: kat.Context):
    """Publish one source query result."""
    return ctx.provider(ExampleExecutor()).query(
        "SELECT event_id, label FROM source_events",
        name="events",
    )
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["outputs"],
            {
                "events": {
                    "columns": [
                        {"name": "event_id", "type": "int64"},
                        {"name": "label", "type": "string"},
                    ],
                    "row_count": 2,
                }
            },
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "events.parquet").to_pydict(),
            {"event_id": [1, 2], "label": ["start", "stop"]},
        )

    def test_provider_tables_can_be_fused_by_context_sql(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ValuesExecutor:
    def __init__(self, table):
        self.table = table

    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield pa.RecordBatchReader.from_batches(
            self.table.schema,
            self.table.to_batches(),
        )

    def close(self):
        pass


@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables=[],
)
def analyze(ctx: kat.Context):
    """Join two localized source results."""
    left = pa.table({"id": [1, 2], "value": [10, 20]})
    right = pa.table({"id": [2, 3], "label": ["two", "three"]})
    ctx.provider(ValuesExecutor(left)).query("left source", name="left_rows")
    ctx.provider(ValuesExecutor(right)).query("right source", name="right_rows")
    return ctx.sql(
        """
        SELECT l.id, l.value, r.label
        FROM left_rows AS l
        JOIN right_rows AS r USING (id)
        ORDER BY l.id
        """
    )
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"id": [2], "value": [20], "label": ["two"]},
        )
        self.assertFalse((candidate / "outputs" / "left_rows.parquet").exists())
        self.assertFalse((candidate / "outputs" / "right_rows.parquet").exists())

    def test_provider_query_streams_multiple_batches_into_one_table(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class BatchedExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("value", pa.int64())])
        yield pa.RecordBatchReader.from_batches(
            schema,
            [
                pa.record_batch([[1, 2]], schema=schema),
                pa.record_batch([[3]], schema=schema),
            ],
        )

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish all source batches."""
    return ctx.provider(BatchedExecutor()).query("batched", name="values")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["values"]["row_count"], 3)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "values.parquet").to_pydict(),
            {"value": [1, 2, 3]},
        )

    def test_provider_query_preserves_schema_for_zero_rows(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class EmptyExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("event_id", pa.int64()), ("label", pa.string())])
        yield pa.RecordBatchReader.from_batches(schema, [])

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish an empty source query result."""
    return ctx.provider(EmptyExecutor()).query("empty", name="empty_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["outputs"]["empty_rows"],
            {
                "columns": [
                    {"name": "event_id", "type": "int64"},
                    {"name": "label", "type": "string"},
                ],
                "row_count": 0,
            },
        )
        table = pq.read_table(candidate / "outputs" / "empty_rows.parquet")
        self.assertEqual(table.num_rows, 0)
        self.assertEqual(table.schema.names, ["event_id", "label"])

    def test_caught_provider_query_failure_still_prevents_publication(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class FailingExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("value", pa.int64())])

        def batches():
            yield pa.record_batch([[1]], schema=schema)
            raise RuntimeError("source stream failed")

        yield pa.RecordBatchReader.from_batches(schema, batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Attempt to recover from a failed source query."""
    try:
        ctx.provider(FailingExecutor()).query("fails", name="partial_rows")
    except RuntimeError:
        pass
    return ctx.from_arrow(pa.table({"value": [99]}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertIn("source stream failed", json.dumps(response))
        self.assertEqual(
            list((candidate / "outputs").glob("*.parquet")),
            [],
        )
        self.assertFalse((candidate / "manifest.json").exists())

    def test_missing_query_name_uses_the_raw_sql_sha256(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ExampleExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("value", pa.int64())])
        yield pa.RecordBatchReader.from_batches(
            schema,
            [pa.record_batch([[1]], schema=schema)],
        )

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Use the deterministic default query result name."""
    return ctx.provider(ExampleExecutor()).query("SELECT stable")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        expected = (
            "q_ece51ea143aa28c71fbcf15cc2d6e7d3"
            "eeb48125cbc9d20228a3b610314b155d"
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(set(response["result"]["outputs"]), {expected})
        self.assertTrue((candidate / "outputs" / f"{expected}.parquet").is_file())

    def test_query_result_name_conflict_fails_before_source_execution(self) -> None:
        marker = self.root / "conflicting-source-entered"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class FirstExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("value", pa.int64())])
        yield pa.RecordBatchReader.from_batches(
            schema,
            [pa.record_batch([[1]], schema=schema)],
        )

    def close(self):
        pass


class ConflictingExecutor:
    def execute(self, sql, params, *, scratch):
        Path({marker.as_posix()!r}).write_text("entered", encoding="utf-8")
        raise AssertionError("conflicting source must not execute")

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reject a duplicate result name before source I/O."""
    first = ctx.provider(FirstExecutor()).query("first", name="events")
    try:
        ctx.provider(ConflictingExecutor()).query("second", name="events")
    except ValueError:
        pass
    return first
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertIn("query result name is already in use", json.dumps(response))
        self.assertFalse(marker.exists())
        self.assertFalse((candidate / "outputs" / "events.parquet").exists())

    def test_table_and_dataframe_outputs_can_share_one_mapping(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ExampleExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({"group_id": [1, 1, 2], "value": [4, 6, 5]})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish a source Table and a derived DataFrame."""
    raw = ctx.provider(ExampleExecutor()).query("raw", name="raw_rows")
    summary = ctx.sql(
        "SELECT group_id, SUM(value) AS total FROM raw_rows GROUP BY group_id"
    )
    return {"raw_rows": raw, "summary": summary}
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(set(response["result"]["outputs"]), {"raw_rows", "summary"})
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "raw_rows.parquet").num_rows,
            3,
        )
        summary = pq.read_table(candidate / "outputs" / "summary.parquet")
        self.assertEqual(
            sorted(zip(summary["group_id"].to_pylist(), summary["total"].to_pylist())),
            [(1, 10), (2, 5)],
        )

    def test_table_output_key_must_equal_the_table_name(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ExampleExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({"value": [1]})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reject an alias for a localized Table."""
    table = ctx.provider(ExampleExecutor()).query("raw", name="raw_rows")
    lazy_failure = ctx.sql("SELECT CAST('not-an-integer' AS BIGINT) AS value")
    return {"lazy_failure": lazy_failure, "alias": table}
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        rendered = json.dumps(response)
        self.assertIn("must equal Table.name", rendered)
        self.assertNotIn("not-an-integer", rendered)
        self.assertFalse((candidate / "outputs" / "raw_rows.parquet").exists())

    def test_dataframe_output_name_cannot_shadow_a_provider_table(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class ExampleExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({"value": [1]})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reject a DataFrame Output that would overwrite its input Table."""
    ctx.provider(ExampleExecutor()).query("raw", name="main")
    return ctx.sql("SELECT * FROM main")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertIn(
            "DataFrame Output names conflict with Provider Tables",
            json.dumps(response),
        )
        self.assertFalse((candidate / "outputs" / "main.parquet").exists())

    def test_external_parquet_source_is_independent_after_context_exit(self) -> None:
        source = self.root / "borrowed.parquet"
        pq.write_table(
            __import__("pyarrow").table({"value": [7, 8]}),
            source,
        )
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class ParquetExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        source = Path({source.as_posix()!r})
        try:
            yield kat.ParquetSource(source)
        finally:
            source.unlink()

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish an independently owned Parquet source."""
    return ctx.provider(ParquetExecutor()).query("borrowed", name="copied_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertFalse(source.exists())
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "copied_rows.parquet").to_pydict(),
            {"value": [7, 8]},
        )

    def test_zero_row_parquet_source_preserves_its_schema(self) -> None:
        source = self.root / "empty.parquet"
        pq.write_table(
            pa.Table.from_batches([], schema=pa.schema([("value", pa.int64())])),
            source,
        )
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class EmptyParquetExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield kat.ParquetSource(Path({source.as_posix()!r}))

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish an empty ParquetSource with an explicit Schema."""
    return ctx.provider(EmptyParquetExecutor()).query("empty", name="empty_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["outputs"]["empty_rows"],
            {
                "columns": [{"name": "value", "type": "int64"}],
                "row_count": 0,
            },
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "empty_rows.parquet").schema,
            pa.schema([("value", pa.int64())]),
        )

    def test_scratch_parquet_source_transfers_ownership_before_context_exit(self) -> None:
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa
import pyarrow.parquet as pq


class ScratchExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        source = scratch / "result.parquet"
        pq.write_table(pa.table({"value": [3, 4]}), source)
        try:
            yield kat.ParquetSource(source)
        finally:
            if source.exists():
                raise AssertionError("scratch source ownership was not transferred")

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish a scratch-owned Parquet source."""
    return ctx.provider(ScratchExecutor()).query("scratch", name="moved_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "moved_rows.parquet").to_pydict(),
            {"value": [3, 4]},
        )

    def test_parquet_dataset_source_publishes_as_one_table(self) -> None:
        source = self.root / "parts"
        source.mkdir()
        pq.write_table(pa.table({"value": [1, 2]}), source / "part-0.parquet")
        pq.write_table(pa.table({"value": [3]}), source / "part-1.parquet")
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class DatasetExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield kat.ParquetSource(Path({source.as_posix()!r}))

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish a sharded single-table Parquet dataset."""
    return ctx.provider(DatasetExecutor()).query("parts", name="all_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        output = candidate / "outputs" / "all_rows.parquet"
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["all_rows"]["row_count"], 3)
        self.assertTrue(output.is_dir())
        self.assertEqual(
            sorted(pq.read_table(output)["value"].to_pylist()),
            [1, 2, 3],
        )
        self.assertTrue(source.is_dir())

    def test_parquet_source_rejects_hard_links(self) -> None:
        original = self.root / "original.parquet"
        linked = self.root / "linked.parquet"
        pq.write_table(pa.table({"value": [1]}), original)
        os.link(original, linked)
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class LinkedExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield kat.ParquetSource(Path({linked.as_posix()!r}))

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reject shared Parquet ownership."""
    return ctx.provider(LinkedExecutor()).query("linked", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertIn("must not contain hard links", json.dumps(response))
        self.assertTrue(original.is_file())
        self.assertTrue(linked.is_file())
        self.assertFalse((candidate / "outputs" / "rows.parquet").exists())

    def test_parquet_source_rejects_a_multi_table_root(self) -> None:
        source = self.root / "multi-table"
        source.mkdir()
        pq.write_table(pa.table({"event_id": [1]}), source / "events.parquet")
        pq.write_table(pa.table({"thread_id": [2]}), source / "threads.parquet")
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class MultiTableExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield kat.ParquetSource(Path({source.as_posix()!r}))

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reject a root that has not been reduced to one table."""
    return ctx.provider(MultiTableExecutor()).query("root", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertIn("one identical Arrow Schema", json.dumps(response))
        self.assertFalse((candidate / "outputs" / "rows.parquet").exists())

    def test_executor_close_warning_does_not_replace_successful_outputs(self) -> None:
        first_closed = self.root / "first-closed"
        second_closed = self.root / "second-closed"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class FailingCloseExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        Path({first_closed.as_posix()!r}).write_text("closed", encoding="utf-8")
        raise RuntimeError("first close failed")


class SuccessfulCloseExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [2]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        Path({second_closed.as_posix()!r}).write_text("closed", encoding="utf-8")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Publish despite best-effort operation cleanup failure."""
    ctx.provider(FailingCloseExecutor())
    return ctx.provider(SuccessfulCloseExecutor()).query("rows", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertTrue(first_closed.is_file())
        self.assertTrue(second_closed.is_file())
        self.assertIn(
            "Source executor cleanup failed",
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "rows.parquet").to_pydict(),
            {"value": [2]},
        )

    def test_one_executor_bound_twice_is_closed_once(self) -> None:
        close_log = self.root / "shared-executor-close-log"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class SharedExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """One executor object has one operation-level cleanup."""
    executor = SharedExecutor()
    ctx.provider(executor)
    return ctx.provider(executor).query("rows", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_executor_close_warning_does_not_replace_the_workflow_failure(self) -> None:
        pack = self.pack(
            '''import kat


class FailingCloseExecutor:
    def execute(self, sql, params, *, scratch):
        raise AssertionError("not queried")

    def close(self):
        raise RuntimeError("cleanup sentinel")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Keep the Workflow error as the public failure."""
    ctx.provider(FailingCloseExecutor())
    raise ValueError("primary workflow sentinel")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        rendered = json.dumps(response)
        self.assertIn("primary workflow sentinel", rendered)
        self.assertNotIn("cleanup sentinel", rendered)
        self.assertIn("cleanup sentinel", completed.stderr.decode(errors="replace"))
        self.assertFalse((candidate / "manifest.json").exists())

    def test_provider_table_can_join_a_legacy_dataset_relation(self) -> None:
        dataset = self.root / "legacy-dataset"
        dataset.mkdir()
        legacy_rows = dataset / "legacy_rows.parquet"
        pq.write_table(
            pa.table({"id": [1, 2], "legacy_value": [10, 20]}),
            legacy_rows,
        )
        pack = self.pack(
            '''from contextlib import contextmanager

import kat
import pyarrow as pa


class NewSourceExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({"id": [2, 3], "source_label": ["two", "three"]})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=["legacy_rows"])
def analyze(ctx: kat.Context):
    """Join one migration-era Dataset relation with one Provider Table."""
    ctx.provider(NewSourceExecutor()).query("new source", name="source_rows")
    return ctx.sql(
        """
        SELECT legacy.id, legacy.legacy_value, source.source_label
        FROM legacy_rows AS legacy
        JOIN source_rows AS source USING (id)
        ORDER BY legacy.id
        """
    )
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                data_home,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {"legacy_rows": str(legacy_rows.resolve())},
                },
            )
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"id": [2], "legacy_value": [20], "source_label": ["two"]},
        )
        self.assertFalse((candidate / "outputs" / "source_rows.parquet").exists())
        self.assertTrue(legacy_rows.is_file())

    def test_legacy_relation_name_conflict_prevents_source_execution(self) -> None:
        dataset = self.root / "legacy-name-conflict"
        dataset.mkdir()
        legacy_rows = dataset / "legacy_rows.parquet"
        pq.write_table(pa.table({"value": [1]}), legacy_rows)
        marker = self.root / "legacy-conflict-source-entered"
        pack = self.pack(
            f'''from pathlib import Path

import kat


class MustNotExecute:
    def execute(self, sql, params, *, scratch):
        Path({marker.as_posix()!r}).write_text("entered", encoding="utf-8")
        raise AssertionError("source I/O must not begin")

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=["legacy_rows"])
def analyze(ctx: kat.Context):
    """A Provider cannot shadow an existing Dataset relation."""
    try:
        ctx.provider(MustNotExecute()).query("conflict", name="legacy_rows")
    except ValueError:
        pass
    return ctx.sql("SELECT * FROM legacy_rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                data_home,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {"legacy_rows": str(legacy_rows.resolve())},
                },
            )
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertIn("query result name is already in use", json.dumps(response))
        self.assertFalse(marker.exists())
        self.assertFalse((candidate / "manifest.json").exists())

    def test_one_provider_result_supports_multiple_fusion_queries_without_requery(
        self,
    ) -> None:
        execute_log = self.root / "provider-execute-log"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class CountingExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        with Path({execute_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("executed\\n")
        table = pa.table({{"value": [1, 2, 3]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Reuse one eager Provider result in two local fusion plans."""
    ctx.provider(CountingExecutor()).query("source", name="source_rows")
    return {{
        "even_rows": ctx.sql("SELECT value FROM source_rows WHERE value % 2 = 0"),
        "sum_rows": ctx.sql("SELECT SUM(value) AS total FROM source_rows"),
    }}
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(execute_log.read_text(encoding="utf-8"), "executed\n")
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "even_rows.parquet").to_pydict(),
            {"value": [2]},
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "sum_rows.parquet").to_pydict(),
            {"total": [6]},
        )
        self.assertFalse((candidate / "outputs" / "source_rows.parquet").exists())


if __name__ == "__main__":
    unittest.main()
