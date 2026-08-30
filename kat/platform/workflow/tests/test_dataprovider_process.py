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


class DataProviderRuntimeProcessTest(unittest.TestCase):
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
from kat import dataprovider as dp

@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables={required_tables},
)
def analyze(ctx: kat.Context):
    """Exercise the Data Provider Runtime boundary."""
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

    def test_ctx_sql_rejects_datasource_binding_mappings(self) -> None:
        pack = self.pack(
            '''    for keyword in ("tables", "params"):
        try:
            ctx.sql("SELECT 1 AS value", **{keyword: {}})
        except TypeError:
            pass
        else:
            raise AssertionError(f"accepted Datasource binding keyword: {keyword}")
    return ctx.from_arrow(__import__("pyarrow").table({"value": [1]}))'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["main"]["row_count"], 1)
        self.assertEqual(
            pq.read_table(candidate / "outputs" / "main.parquet").to_pydict(),
            {"value": [1]},
        )

    def test_ctx_sql_registers_only_granted_dataset_tables_for_every_call(
        self,
    ) -> None:
        pack = self.pack(
            '''    from datafusion import DataFrame
    first = ctx.sql("SELECT value FROM events ORDER BY value")
    second = ctx.sql(
        "SELECT value FROM events WHERE value >= $minimum ORDER BY value",
        minimum=2,
    )
    assert isinstance(first, DataFrame)
    assert isinstance(second, DataFrame)
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
    table = dp.Table({{"value": int}})
    table.append(value=1)
    return table''',
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

    def test_output_accepts_table_and_dataframe_in_one_exact_dict(self) -> None:
        pack = self.pack(
            '''    table = dp.Table({"value": int})
    table.append(value=1)
    table.append(value=2)
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
    table = dp.Table({"value": int})
    table.append(value=1)
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
