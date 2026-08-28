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


class DatasourceRuntimeProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(
        self, request: dict[str, object]
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
            response_path.is_file(), completed.stderr.decode(errors="replace")
        )
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def pack(self, body: str, *, required_tables: str = "[]") -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "entry.py").write_text(
            f'''import kat
from kat import datasource as ds

@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables={required_tables},
)
def analyze(ctx: kat.Context):
    """Exercise the Datasource Runtime boundary."""
{body}
''',
            encoding="utf-8",
        )
        return pack.resolve()

    def candidate(self) -> tuple[str, Path]:
        candidate_id = f"019f6e00-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        candidate = self.root / "runs" / candidate_id
        candidate.mkdir(parents=True)
        return candidate_id, candidate.resolve()

    def request(
        self,
        pack: Path,
        candidate_id: str,
        candidate: Path,
        *,
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
            "datasource_root": str(
                (self.root / "datasources" / "example").resolve(strict=False)
            ),
        }
        if dataset is not None:
            request["dataset"] = dataset
        return request

    def test_ctx_sql_fuses_explicit_tables_and_separate_scalar_params(self) -> None:
        pack = self.pack(
            '''    source = ds.table(
        schema={"value": int, "label": str},
        columns={"value": [1, 3, 2], "label": ["one", "three", "two"]},
    )
    result = ctx.sql(
        "SELECT label, value FROM source_rows "
        "WHERE value >= $source_rows ORDER BY value",
        tables={"source_rows": source},
        params={"source_rows": 2},
    )
    assert result.to_rows() == [
        {"label": "two", "value": 2},
        {"label": "three", "value": 3},
    ]
    return result'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["main"]["row_count"], 2)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"label": ["two", "three"], "value": [2, 3]},
        )

    def test_ctx_sql_registers_only_granted_dataset_tables_for_every_call(
        self,
    ) -> None:
        pack = self.pack(
            '''    first = ctx.sql("SELECT value FROM events ORDER BY value")
    second = ctx.sql(
        "SELECT value FROM events WHERE value >= $minimum ORDER BY value",
        params={"minimum": 2},
    )
    assert first["value"] == (1, 2, 3)
    return second''',
            required_tables="['events']",
        )
        dataset = self.root / "dataset"
        dataset.mkdir()
        events = dataset / "events.parquet"
        secret = dataset / "secret.parquet"
        pq.write_table(pa.table({"value": [3, 1, 2]}), events)
        pq.write_table(pa.table({"value": [99]}), secret)
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "events": str(events.resolve()),
                        "secret": str(secret.resolve()),
                    },
                },
            )
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [2, 3]},
        )

    def test_required_dataset_is_validated_before_workflow_user_code(self) -> None:
        marker = self.root / "workflow-ran.txt"
        pack = self.pack(
            f'''    __import__("pathlib").Path({str(marker)!r}).write_text("ran")
    return ds.table(schema={{"value": int}}, columns={{"value": [1]}})''',
            required_tables="['events']",
        )
        dataset = self.root / "damaged-dataset"
        dataset.mkdir()
        events = dataset / "events.parquet"
        events.write_bytes(b"not a Parquet file")
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {"events": str(events.resolve())},
                },
            )
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertFalse(marker.exists())

    def test_ctx_sql_call_state_is_isolated_and_failure_does_not_poison_context(
        self,
    ) -> None:
        pack = self.pack(
            '''    source = ds.table(schema={"value": int}, columns={"value": [7]})
    first = ctx.sql("SELECT * FROM rows", tables={"rows": source})
    try:
        ctx.sql("SELECT * FROM rows")
    except Exception:
        pass
    else:
        raise AssertionError("a relation from an earlier call must not remain registered")
    return ctx.sql("SELECT * FROM rows", tables={"rows": first})'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [7]},
        )

    def test_ctx_sql_validates_call_local_relation_names_before_planning(self) -> None:
        pack = self.pack(
            '''    source = ds.table(schema={"value": int}, columns={"value": [7]})
    for invalid_name in ("BadName", "bad-name", "_rows", 7):
        try:
            ctx.sql("this is not sql", tables={invalid_name: source})
        except ValueError:
            pass
        else:
            raise AssertionError(f"accepted invalid relation name: {invalid_name!r}")
    return source'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [7]},
        )

    def test_ctx_sql_rejects_dataset_relation_conflict_before_planning(self) -> None:
        pack = self.pack(
            '''    replacement = ds.table(
        schema={"value": int}, columns={"value": [99]}
    )
    try:
        ctx.sql("this is not sql", tables={"events": replacement})
    except ValueError as error:
        assert "events" in str(error)
    else:
        raise AssertionError("an explicit relation must not shadow a Dataset grant")
    return ctx.sql("SELECT value FROM events")''',
            required_tables="['events']",
        )
        dataset = self.root / "conflict-dataset"
        dataset.mkdir()
        events = dataset / "events.parquet"
        pq.write_table(pa.table({"value": [1]}), events)
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {"events": str(events.resolve())},
                },
            )
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [1]},
        )

    def test_output_accepts_table_and_dataframe_in_one_exact_dict(self) -> None:
        pack = self.pack(
            '''    table = ds.table(schema={"value": int}, columns={"value": [1, 2]})
    frame = ctx.from_arrow(__import__("pyarrow").table({"label": ["legacy"]}))
    return {"table_rows": table, "legacy_rows": frame}'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            set(response["result"]["outputs"]), {"table_rows", "legacy_rows"}
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "table_rows.parquet").to_pydict(),
            {"value": [1, 2]},
        )
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "legacy_rows.parquet").to_pydict(),
            {"label": ["legacy"]},
        )

    def test_output_rejects_a_dict_subclass(self) -> None:
        pack = self.pack(
            '''    class Outputs(dict):
        pass
    table = ds.table(schema={"value": int}, columns={"value": [1]})
    return Outputs(main=table)'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertFalse((candidate / "outputs" / "main.parquet").exists())


if __name__ == "__main__":
    unittest.main()
