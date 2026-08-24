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

from _clock_dataset import write_clock_dataset
from _source_dataset import (
    materialized_dataset_request,
    write_materialized_source,
)


class WorkflowExecutionProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(self, request: object) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
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
        (pack / "workflows" / "entry.py").write_text(
            f'''import kat

@kat.workflow(
    name="analyze",
    title="Analyze",
    parameters={{"minimum": "Minimum", "window": "Window"}},
)
def analyze(ctx: kat.Context, *, minimum: int = 0, window: kat.Duration = "5ms"):
    """Analyze the provided facts."""
{body}
''',
            encoding="utf-8",
        )
        return pack.resolve()

    def candidate(self) -> tuple[str, Path]:
        candidate_id = str(uuid.uuid7())
        candidate = self.root / candidate_id
        candidate.mkdir()
        return candidate_id, candidate.resolve()

    def clock_dataset(self) -> tuple[Path, Path, Path]:
        dataset = self.root / f"clock-dataset-{uuid.uuid4().hex}"
        tables = write_clock_dataset(
            dataset,
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1_000_000_000),
            ],
            snapshots=[(0, "monotonic", 100), (0, "realtime", 1_000)],
        )
        return dataset, tables["clock_domain"], tables["clock_snapshot"]

    def request(
        self,
        pack: Path,
        candidate_id: str,
        candidate: Path,
        *,
        dataset: dict[str, object] | None,
        arguments: list[str] | None = None,
    ) -> dict[str, object]:
        request: dict[str, object] = {
            "operation": "run_workflow",
            "pack_name": "example",
            "pack_path": str(pack),
            "pack_paths": {"example": str(pack)},
            "workflow_name": "analyze",
            "arguments": arguments or [],
            "candidate_id": candidate_id,
            "candidate_path": str(candidate),
        }
        if dataset is not None:
            request["dataset"] = dataset
        return request

    def test_run_enforces_grant_compiles_inputs_and_writes_all_outputs(self) -> None:
        pack = self.pack(
            '''    import logging
    logging.getLogger("pack").info("executed")
    selected = ctx.sql(
        "SELECT value FROM example.facts.events WHERE value >= $minimum ORDER BY value",
        minimum=minimum,
    )
    empty = ctx.sql("SELECT CAST(NULL AS BIGINT) AS value WHERE FALSE")
    return {"selected_rows": selected, "empty_rows": empty}'''
        )
        dataset = self.root / "dataset"
        tables = write_materialized_source(
            dataset,
            pack="example",
            source="facts",
            tables={
                "events": pa.table({"value": [1, 3, 2]}),
                "secret": pa.table({"value": [99]}),
            },
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="facts",
                    tables=tables,
                ),
                arguments=["--minimum", "2", "--window", "0.005s"],
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertIn(
            f"candidate={candidate_id} pack=example workflow=analyze pack: executed",
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "success", response)
        result = response["result"]
        self.assertEqual(result["effective_inputs"], {"minimum": "2", "window": "5000000"})
        self.assertEqual(set(result["outputs"]), {"selected_rows", "empty_rows"})
        self.assertTrue(
            all("output_id" not in output for output in result["outputs"].values())
        )
        self.assertEqual(result["outputs"]["selected_rows"]["row_count"], 2)
        self.assertEqual(result["outputs"]["empty_rows"]["row_count"], 0)
        self.assertEqual(
            result["outputs"]["empty_rows"]["columns"],
            [{"name": "value", "type": "int64"}],
        )
        self.assertEqual(
            {path.name for path in (candidate / "outputs").glob("*.parquet")},
            {"selected_rows.parquet", "empty_rows.parquet"},
        )
        self.assertFalse((candidate / "manifest.json").exists())

    def test_run_queries_an_external_official_datafusion_schema_lazily(self) -> None:
        pack = self.pack(
            '''    return ctx.sql(
        "SELECT value FROM example.official.events ORDER BY value"
    )'''
        )
        (pack / "sources").mkdir()
        (pack / "sources" / "official.py").write_text(
            '''from datafusion.catalog import Schema, Table
import pyarrow as pa
import pyarrow.dataset as ds
from kat import source

@source(name="official")
def provide():
    schema = Schema.memory_schema()
    schema.register_table(
        "events",
        Table(ds.dataset(pa.table({"value": [2, 1]}))),
    )
    return schema
''',
            encoding="utf-8",
        )
        dataset = self.root / "external-official-schema"
        dataset.mkdir()
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "sources": [
                        {
                            "pack": "example",
                            "source": "official",
                            "kind": "external",
                            "arguments": [],
                            "working_directory": str(self.root.resolve()),
                        }
                    ],
                },
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        output = pq.read_table(candidate / "outputs" / "main.parquet")
        self.assertEqual(output.to_pydict(), {"value": [1, 2]})

    def test_run_does_not_resolve_an_unused_external_source(self) -> None:
        pack = self.pack(
            '''    return ctx.sql(
        "SELECT value FROM example.used.events ORDER BY value"
    )'''
        )
        (pack / "sources").mkdir()
        (pack / "sources" / "used.py").write_text(
            '''from datafusion.catalog import Schema, Table
import pyarrow as pa
import pyarrow.dataset as ds
from kat import source

@source(name="used")
def provide():
    schema = Schema.memory_schema()
    schema.register_table(
        "events",
        Table(ds.dataset(pa.table({"value": [2, 1]}))),
    )
    return schema
''',
            encoding="utf-8",
        )
        marker = self.root / "unused-source-entry-called.txt"
        (pack / "sources" / "unused.py").write_text(
            f'''from pathlib import Path
from kat import source

MARKER = Path({str(marker)!r})

@source(name="unused")
def provide():
    MARKER.write_text("called", encoding="utf-8")
    raise AssertionError("unused Source Entry must not run")
''',
            encoding="utf-8",
        )
        dataset = self.root / "external-used-and-unused-sources"
        dataset.mkdir()
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "sources": [
                        {
                            "pack": "example",
                            "source": source,
                            "kind": "external",
                            "arguments": [],
                            "working_directory": str(self.root.resolve()),
                        }
                        for source in ("unused", "used")
                    ],
                },
            )
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [1, 2]},
        )
        self.assertFalse(marker.exists())

    def test_cross_pack_external_source_resolution_does_not_require_guide(self) -> None:
        workflow_pack = self.pack(
            '''    return ctx.sql(
        "SELECT value FROM other.facts.events ORDER BY value"
    )'''
        )
        source_pack = self.root / "guide-free-source-pack"
        (source_pack / "sources").mkdir(parents=True)
        (source_pack / "sources" / "facts.py").write_text(
            '''from datafusion.catalog import Schema, Table
import pyarrow as pa
import pyarrow.dataset as ds
from kat import source

@source(name="facts")
def provide():
    schema = Schema.memory_schema()
    schema.register_table(
        "events",
        Table(ds.dataset(pa.table({"value": [2, 1]}))),
    )
    return schema
''',
            encoding="utf-8",
        )
        dataset = self.root / "cross-pack-external-source"
        dataset.mkdir()
        candidate_id, candidate = self.candidate()
        request = self.request(
            workflow_pack,
            candidate_id,
            candidate,
            dataset={
                "path": str(dataset.resolve()),
                "sources": [
                    {
                        "pack": "other",
                        "source": "facts",
                        "kind": "external",
                        "arguments": [],
                        "working_directory": str(self.root.resolve()),
                    }
                ],
            },
        )
        request["pack_paths"]["other"] = str(source_pack.resolve())

        completed, response = self.run_runtime(request)

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [1, 2]},
        )

    def test_lazy_source_resolution_preserves_the_original_error_chain(self) -> None:
        pack = self.pack('    return ctx.sql("SELECT * FROM example.broken.events")')
        (pack / "sources").mkdir()
        (pack / "sources" / "broken.py").write_text(
            '''from kat import source

@source(name="broken")
def provide():
    raise ValueError("provider setup failed")
''',
            encoding="utf-8",
        )
        dataset = self.root / "external-broken-source"
        dataset.mkdir()
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "sources": [
                        {
                            "pack": "example",
                            "source": "broken",
                            "kind": "external",
                            "arguments": [],
                            "working_directory": str(self.root.resolve()),
                        }
                    ],
                },
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("Source example.broken could not be resolved", response["error"]["causes"])
        self.assertIn("provider setup failed", response["error"]["causes"])

    def test_ffi_schema_export_failure_is_cached_with_its_original_cause(self) -> None:
        pack = self.pack(
            '''    for _ in range(2):
        try:
            ctx.sql("SELECT * FROM example.native.events")
        except Exception:
            pass
    return ctx.sql("SELECT * FROM example.native.events")'''
        )
        (pack / "sources").mkdir()
        marker = self.root / "ffi-export-calls.txt"
        (pack / "sources" / "native.py").write_text(
            f'''from pathlib import Path
from kat import source

MARKER = Path({str(marker)!r})

class BrokenExporter:
    def __datafusion_schema_provider__(self, codec):
        calls = int(MARKER.read_text(encoding="utf-8")) if MARKER.exists() else 0
        MARKER.write_text(str(calls + 1), encoding="utf-8")
        raise ValueError("FFI schema export failed")

@source(name="native")
def provide():
    return BrokenExporter()
''',
            encoding="utf-8",
        )
        dataset = self.root / "external-native-source"
        dataset.mkdir()
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "sources": [
                        {
                            "pack": "example",
                            "source": "native",
                            "kind": "external",
                            "arguments": [],
                            "working_directory": str(self.root.resolve()),
                        }
                    ],
                },
            )
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "failure", response)
        self.assertIn(
            "Source example.native could not be resolved",
            response["error"]["causes"],
        )
        self.assertIn(
            "Source example.native failed earlier in this operation",
            response["error"]["causes"],
        )
        self.assertIn("FFI schema export failed", response["error"]["causes"])
        self.assertEqual(marker.read_text(encoding="utf-8"), "1")

    def test_empty_grant_runs_without_dataset_and_extra_table_is_not_visible(self) -> None:
        no_dataset_pack = self.pack(
            '    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))',
        )
        candidate_id, candidate = self.candidate()
        completed, response = self.run_runtime(
            self.request(no_dataset_pack, candidate_id, candidate, dataset=None)
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["main"]["row_count"], 1)

        isolated_id = "019f6e00-0000-7000-8000-000000000002"
        isolated_root = self.root / isolated_id
        isolated_root.mkdir()
        grant_pack = self.pack('    return ctx.sql("SELECT * FROM secret")')
        dataset = self.root / "grant-dataset"
        tables = write_materialized_source(
            dataset,
            pack="example",
            source="facts",
            tables={
                "events": pa.table({"value": [1]}),
                "secret": pa.table({"value": [99]}),
            },
        )
        completed, response = self.run_runtime(
            self.request(
                grant_pack,
                isolated_id,
                isolated_root.resolve(),
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="facts",
                    tables=tables,
                ),
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        rendered = json.dumps(response, ensure_ascii=False)
        self.assertNotIn(isolated_id, rendered)
        self.assertNotIn(str(isolated_root.resolve()), rendered)

    def test_public_source_name_does_not_create_an_unqualified_table_alias(self) -> None:
        dataset = self.root / "public-source-dataset"
        tables = write_materialized_source(
            dataset,
            pack="example",
            source="public",
            tables={"events": pa.table({"value": [7]})},
        )
        dataset_request = materialized_dataset_request(
            dataset,
            pack="example",
            source="public",
            tables=tables,
        )

        qualified_pack = self.pack(
            '    return ctx.sql("SELECT value FROM public.events")'
        )
        candidate_id, candidate = self.candidate()
        completed, response = self.run_runtime(
            self.request(
                qualified_pack,
                candidate_id,
                candidate,
                dataset=dataset_request,
            )
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [7]},
        )

        unqualified_pack = self.pack('    return ctx.sql("SELECT value FROM events")')
        candidate_id, candidate = self.candidate()
        completed, response = self.run_runtime(
            self.request(
                unqualified_pack,
                candidate_id,
                candidate,
                dataset=dataset_request,
            )
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        rendered = json.dumps(response)
        self.assertIn("events", rendered)
        self.assertIn("not found", rendered.lower())

    def test_run_rejects_materialized_table_directories(self) -> None:
        dataset = self.root / "fragmented-run-dataset"
        events = dataset / "sources" / "example" / "facts" / "tables" / "events.parquet"
        events.mkdir(parents=True)
        pq.write_table(pa.table({"value": [1]}), events / "part.parquet")
        pack = self.pack('    return ctx.sql("SELECT value FROM facts.events")')
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="facts",
                    tables={"events": events},
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("canonical file", json.dumps(response).lower())

    def test_read_only_sql_shapes_are_not_restricted_by_clock_validation(self) -> None:
        pack = self.pack(
            '''    return {
        "explain": ctx.sql("EXPLAIN SELECT value FROM example.facts.events"),
        "tables": ctx.sql("SHOW TABLES"),
        "schemata": ctx.sql(
            "SELECT catalog_name, schema_name FROM information_schema.schemata"
        ),
        "describe": ctx.sql("DESCRIBE example.facts.events"),
        "parameter": ctx.sql("SELECT $value AS value", value=7),
        "timestamp": ctx.sql(
            "SELECT $value AS value",
            value=kat.WallClockTimestamp("2262-04-11T23:47:16.854775807Z"),
        ),
    }'''
        )
        dataset = self.root / "sql-shapes-dataset"
        tables = write_materialized_source(
            dataset,
            pack="example",
            source="facts",
            tables={"events": pa.table({"value": [1]})},
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="facts",
                    tables=tables,
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            set(response["result"]["outputs"]),
            {"explain", "tables", "schemata", "describe", "parameter", "timestamp"},
        )
        table_rows = pq.read_table(candidate / "outputs" / "tables.parquet").to_pylist()
        self.assertTrue(
            any(
                row["table_catalog"] == "example"
                and row["table_schema"] == "facts"
                and row["table_name"] == "events"
                for row in table_rows
            ),
            table_rows,
        )
        schema_rows = pq.read_table(
            candidate / "outputs" / "schemata.parquet"
        ).to_pylist()
        visible_schemas = {
            (row["catalog_name"], row["schema_name"])
            for row in schema_rows
        }
        self.assertIn(("example", "__kat_workflow__"), visible_schemas)
        self.assertIn(("example", "facts"), visible_schemas)

    def test_run_rejects_a_non_uuidv7_candidate_without_exposing_it(self) -> None:
        pack = self.pack(
            '    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))',
        )
        candidate_id = "private-candidate"
        candidate = self.root / candidate_id
        candidate.mkdir()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate.resolve(), dataset=None)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertEqual(response["error"]["message"], "Runtime Request is invalid")
        self.assertNotIn(candidate_id, json.dumps(response, ensure_ascii=False))

    def test_run_request_rejects_explicit_null_and_unowned_dataset_paths(self) -> None:
        pack = self.pack(
            '    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))',
        )
        candidate_id, candidate = self.candidate()
        explicit_null = self.request(
            pack, candidate_id, candidate, dataset=None
        ) | {"dataset": None}

        completed, response = self.run_runtime(explicit_null)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertEqual(response["error"]["message"], "Runtime Request is invalid")

        dataset = self.root / "dataset-ref"
        dataset.mkdir()
        outside = self.root / "outside.parquet"
        pq.write_table(pa.table({"value": [1]}), outside)
        candidate.rmdir()
        candidate.mkdir()
        unowned_table = self.request(
            pack,
            candidate_id,
            candidate,
            dataset={
                "path": str(dataset.resolve()),
                "sources": [
                    {
                        "pack": "example",
                        "source": "facts",
                        "kind": "materialized",
                        "tables": [
                            {"name": "events", "path": str(outside.resolve())}
                        ],
                    }
                ],
            },
        )

        completed, response = self.run_runtime(unowned_table)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertEqual(response["error"]["message"], "Runtime Request is invalid")

    def test_workflow_system_exit_is_a_runtime_failure(self) -> None:
        pack = self.pack(
            '    raise SystemExit("Workflow requested exit")',
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, dataset=None)
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("Workflow requested exit", response["error"]["causes"])
        self.assertNotIn("result", response)

    def test_output_io_failure_logs_private_cause_but_returns_public_diagnostic(
        self,
    ) -> None:
        pack = self.pack(
            '''    import _kat_runtime.outputs as output_module
    class FailingWriter:
        def __init__(self, path, *args, **kwargs):
            self.path = path
            __import__("pathlib").Path(path).write_bytes(b"partial")
        def write_batch(self, batch):
            raise ValueError(f"private output path: {self.path}")
        def close(self):
            pass
    output_module.pq.ParquetWriter = FailingWriter
    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))''',
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, dataset=None)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn(
            "Output 'main' could not be materialized",
            response["error"]["causes"],
        )
        rendered = json.dumps(response, ensure_ascii=False)
        self.assertNotIn(candidate_id, rendered)
        self.assertNotIn(str(candidate), rendered)
        operation_log = completed.stderr.decode(errors="replace")
        self.assertIn("private output path", operation_log)
        self.assertIn(str(candidate), operation_log)
        self.assertNotIn(str(candidate), rendered)

    def test_output_names_are_portable_file_names(self) -> None:
        for reserved in (
            "con",
            "prn",
            "aux",
            "nul",
            "com1",
            "com9",
            "lpt1",
            "lpt9",
        ):
            with self.subTest(reserved=reserved):
                pack = self.pack(
                    f'''    frame = ctx.from_arrow(__import__("pyarrow").table({{"value": [7]}}))
    return {{"{reserved}": frame}}''',
                )
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(pack, candidate_id, candidate, dataset=None)
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)

    def test_run_rejects_workflow_entry_imports_independent_of_entry_order(self) -> None:
        pack = self.root / "entry-import-pack"
        workflows = pack / "workflows"
        workflows.mkdir(parents=True)
        (workflows / "a.py").write_text(
            """from kat import Context, workflow
@workflow(name='a', title='A')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
    return ctx.from_arrow(__import__('pyarrow').table({'value': [1]}))
""",
            encoding="utf-8",
        )
        (workflows / "b.py").write_text(
            """from kat import Context, workflow
from kat.pack.workflows.a import analyze
@workflow(name='b', title='B')
def other(ctx: Context):
    \"\"\"B.\"\"\"
    return ctx.from_arrow(__import__('pyarrow').table({'value': [2]}))
""",
            encoding="utf-8",
        )
        pack = pack.resolve(strict=True)
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, dataset=None)
            | {"workflow_name": "b"}
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertTrue(
            any(
                "must register exactly one Workflow" in cause
                for cause in response["error"]["causes"]
            ),
            response,
        )

    def test_clock_conversion_uses_dataset_evidence_without_granting_tables(self) -> None:
        pack = self.pack(
            '''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({
        "clock_domain": pa.array(["monotonic", None], type=pa.string()),
        "clock_value": pa.array([105, None], type=pa.uint64()),
    }))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="realtime",
        ).alias("realtime_clock_value")
    )''',
        )
        dataset, definitions, snapshots = self.clock_dataset()
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="clocks",
                    tables={
                        "clock_domain": definitions,
                        "clock_snapshot": snapshots,
                    },
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        table = pq.read_table(candidate / "outputs" / "main.parquet")
        self.assertEqual(table.to_pydict(), {"realtime_clock_value": [1005, None]})

    def test_empty_clock_conversion_plans_and_writes_zero_rows(self) -> None:
        dataset, definitions, snapshots = self.clock_dataset()
        cases = [
            ("large_string", "pa.large_string()", "pa.uint64()"),
            ("int64", "pa.string()", "pa.int64()"),
        ]
        for name, domain_type, value_type in cases:
            with self.subTest(name=name):
                empty_pack = self.pack(
                    f'''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({{
        "clock_domain": pa.array([], type={domain_type}),
        "clock_value": pa.array([], type={value_type}),
    }}))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="realtime",
        ).alias("realtime_clock_value")
    )''',
                )
                empty_id, empty_candidate = self.candidate()
                completed, response = self.run_runtime(
                    self.request(
                        empty_pack,
                        empty_id,
                        empty_candidate.resolve(),
                        dataset=materialized_dataset_request(
                            dataset,
                            pack="example",
                            source="clocks",
                            tables={
                                "clock_domain": definitions,
                                "clock_snapshot": snapshots,
                            },
                        ),
                    )
                )
                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "success", response)
                table = pq.read_table(empty_candidate / "outputs" / "main.parquet")
                self.assertEqual(table.to_pydict(), {"realtime_clock_value": []})

    def test_clock_sql_function_is_not_registered(self) -> None:
        dataset, definitions, snapshots = self.clock_dataset()
        events = write_materialized_source(
            dataset,
            pack="example",
            source="clocks",
            tables={
                "events": pa.table(
                    {
                        "clock_domain": pa.array(["monotonic"], type=pa.string()),
                        "clock_value": pa.array([105], type=pa.uint64()),
                    }
                )
            },
        )["events"]
        sql_pack = self.pack(
            '''    return ctx.sql(
        "SELECT kat_convert_clock(clock_domain, clock_value, 'realtime') "
        "AS realtime_clock_value FROM example.clocks.events"
    )'''
        )
        sql_id, sql_candidate = self.candidate()
        completed, response = self.run_runtime(
            self.request(
                sql_pack,
                sql_id,
                sql_candidate.resolve(),
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="clocks",
                    tables={
                        "events": events,
                        "clock_domain": definitions,
                        "clock_snapshot": snapshots,
                    },
                ),
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("Invalid function 'kat_convert_clock'", response["error"]["causes"][0])
        self.assertNotIn("result", response)

    def test_invalid_clock_definitions_fail_the_complete_conversion(self) -> None:
        dataset, definitions, snapshots = self.clock_dataset()
        pq.write_table(
            pa.Table.from_arrays(
                [
                    pa.array(["Bad Name", "realtime"]),
                    pa.array(["monotonic", "realtime"]),
                    pa.array([1_000_000_000, 1_000_000_000], type=pa.uint64()),
                ],
                schema=pq.read_schema(definitions),
            ),
            definitions,
        )
        invalid_pack = self.pack(
            '''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({
        "clock_domain": pa.array(["monotonic"], type=pa.string()),
        "clock_value": pa.array([105], type=pa.uint64()),
    }))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="realtime",
        ).alias("realtime_clock_value")
    )''',
        )
        invalid_id, invalid_candidate = self.candidate()
        completed, response = self.run_runtime(
            self.request(
                invalid_pack,
                invalid_id,
                invalid_candidate.resolve(),
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="clocks",
                    tables={
                        "clock_domain": definitions,
                        "clock_snapshot": snapshots,
                    },
                ),
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)

    def test_context_clock_target_requires_an_exact_non_empty_string(self) -> None:
        cases = [
            ("empty", 'target = ""'),
            ("none", "target = None"),
            ("integer", "target = 7"),
            (
                "subclass",
                'target = type("Domain", (str,), {})("realtime")',
            ),
        ]
        for name, target in cases:
            with self.subTest(name=name):
                pack = self.pack(
                    f'''    import pyarrow as pa
    from datafusion import col
    {target}
    frame = ctx.from_arrow(pa.table({{
        "clock_domain": pa.array(["monotonic"], type=pa.string()),
        "clock_value": pa.array([105], type=pa.uint64()),
    }}))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain=target,
        ).alias("clock_value")
    )''',
                )
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(
                        pack,
                        candidate_id,
                        candidate.resolve(),
                        dataset=None,
                    )
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertIn(
                    "target_domain must be an exact non-empty str",
                    response["error"]["causes"][0],
                )
                self.assertNotIn("result", response)

    def test_context_clock_accepts_safely_coercible_source_types(self) -> None:
        dataset = self.root / "typed-clock-dataset"
        definitions = write_clock_dataset(
            dataset,
            definitions=[("monotonic", "monotonic", 1_000_000_000)],
        )["clock_domain"]
        cases = [
            (
                "large_string",
                "pa.large_string()",
                "pa.uint64()",
            ),
            (
                "string_view",
                "pa.string_view()",
                "pa.uint64()",
            ),
            (
                "int64",
                "pa.string()",
                "pa.int64()",
            ),
        ]
        for name, domain_type, value_type in cases:
            with self.subTest(name=name):
                pack = self.pack(
                    f'''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({{
        "clock_domain": pa.array(["monotonic"], type={domain_type}),
        "clock_value": pa.array([105], type={value_type}),
    }}))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="monotonic",
        ).alias("clock_value")
    )''',
                )
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(
                        pack,
                        candidate_id,
                        candidate.resolve(),
                        dataset=materialized_dataset_request(
                            dataset,
                            pack="example",
                            source="clocks",
                            tables={"clock_domain": definitions},
                        ),
                    )
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "success", response)
                table = pq.read_table(candidate / "outputs" / "main.parquet")
                self.assertEqual(table.to_pydict(), {"clock_value": [105]})

    def test_context_clock_rejects_unsafe_value_coercion(self) -> None:
        dataset, definitions, _snapshots = self.clock_dataset()
        cases = [
            ("negative", "[-1]", "pa.int64()"),
            ("overflow", "[2**64]", "pa.decimal128(20, 0)"),
            ("invalid_text", '["not-a-clock"]', "pa.string()"),
        ]
        for name, values, value_type in cases:
            with self.subTest(name=name):
                pack = self.pack(
                    f'''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({{
        "clock_domain": pa.array(["monotonic"], type=pa.string()),
        "clock_value": pa.array({values}, type={value_type}),
    }}))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="monotonic",
        ).alias("clock_value")
    )''',
                )
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(
                        pack,
                        candidate_id,
                        candidate.resolve(),
                        dataset=materialized_dataset_request(
                            dataset,
                            pack="example",
                            source="clocks",
                            tables={"clock_domain": definitions},
                        ),
                    )
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)

    def test_same_domain_clock_conversion_does_not_read_snapshot_evidence(self) -> None:
        dataset = self.root / "same-domain-clock-dataset"
        definitions = write_clock_dataset(
            dataset,
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1_000_000_000),
            ],
        )["clock_domain"]
        invalid_snapshots = definitions.parent / "clock_snapshot.parquet"
        pq.write_table(pa.table({"invalid": [1]}), invalid_snapshots)
        same_domain_pack = self.pack(
            '''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({
        "clock_domain": pa.array(["monotonic"], type=pa.string()),
        "clock_value": pa.array([105], type=pa.uint64()),
    }))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="monotonic",
        ).alias("monotonic_clock_value")
    )''',
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                same_domain_pack,
                candidate_id,
                candidate,
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="clocks",
                    tables={
                        "clock_domain": definitions,
                        "clock_snapshot": invalid_snapshots,
                    },
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        table = pq.read_table(candidate / "outputs" / "main.parquet")
        self.assertEqual(table.to_pydict(), {"monotonic_clock_value": [105]})

        cross_domain_pack = self.pack(
            '''    import pyarrow as pa
    from datafusion import col
    frame = ctx.from_arrow(pa.table({
        "clock_domain": pa.array(["monotonic"], type=pa.string()),
        "clock_value": pa.array([105], type=pa.uint64()),
    }))
    return frame.select(
        ctx.convert_clock(
            col("clock_domain"),
            col("clock_value"),
            source="clocks",
            target_domain="realtime",
        ).alias("realtime_clock_value")
    )''',
        )
        cross_id = "019f6e00-0000-7000-8000-000000000007"
        cross_candidate = self.root / cross_id
        cross_candidate.mkdir()
        completed, response = self.run_runtime(
            self.request(
                cross_domain_pack,
                cross_id,
                cross_candidate.resolve(),
                dataset=materialized_dataset_request(
                    dataset,
                    pack="example",
                    source="clocks",
                    tables={"clock_domain": definitions},
                ),
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("baseline is incomplete", response["error"]["causes"][0])
        self.assertNotIn("result", response)


if __name__ == "__main__":
    unittest.main()
