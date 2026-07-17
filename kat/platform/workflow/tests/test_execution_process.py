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
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def pack(self, body: str, *, required_tables: str = "['events']") -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "entry.py").write_text(
            f'''import kat

@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables={required_tables},
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
        candidate_id = "019f6e00-0000-7000-8000-000000000001"
        candidate = self.root / candidate_id
        candidate.mkdir()
        return candidate_id, candidate.resolve()

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
            "workflow_name": "analyze",
            "arguments": arguments or [],
            "candidate_id": candidate_id,
            "run_path": str(candidate),
        }
        if dataset is not None:
            request["dataset"] = dataset
        return request

    def test_run_enforces_grant_compiles_inputs_and_writes_all_outputs(self) -> None:
        pack = self.pack(
            '''    import logging
    logging.getLogger("pack").info("executed")
    selected = ctx.sql(
        "SELECT value FROM events WHERE value >= $minimum ORDER BY value",
        minimum=minimum,
    )
    empty = ctx.sql("SELECT CAST(NULL AS BIGINT) AS value WHERE FALSE")
    return {"selected_rows": selected, "empty_rows": empty}'''
        )
        dataset = self.root / "dataset"
        dataset.mkdir()
        events = dataset / "events.parquet"
        secret = dataset / "secret.parquet"
        pq.write_table(pa.table({"value": [1, 3, 2]}), events)
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
        self.assertEqual(result["outputs"]["selected_rows"]["row_count"], 2)
        self.assertEqual(result["outputs"]["empty_rows"]["row_count"], 0)
        self.assertEqual(
            result["outputs"]["empty_rows"]["columns"],
            [{"name": "value", "type": "int64"}],
        )
        self.assertEqual(len(list((candidate / "outputs").glob("*.parquet"))), 2)
        self.assertFalse((candidate / "manifest.json").exists())

    def test_empty_grant_runs_without_dataset_and_extra_table_is_not_visible(self) -> None:
        no_dataset_pack = self.pack(
            '    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))',
            required_tables="[]",
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
        dataset.mkdir()
        events = dataset / "events.parquet"
        secret = dataset / "secret.parquet"
        pq.write_table(pa.table({"value": [1]}), events)
        pq.write_table(pa.table({"value": [99]}), secret)
        completed, response = self.run_runtime(
            self.request(
                grant_pack,
                isolated_id,
                isolated_root.resolve(),
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "events": str(events.resolve()),
                        "secret": str(secret.resolve()),
                    },
                },
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        rendered = json.dumps(response, ensure_ascii=False)
        self.assertNotIn(isolated_id, rendered)
        self.assertNotIn(str(isolated_root.resolve()), rendered)

    def test_run_rejects_a_non_uuidv7_candidate_without_exposing_it(self) -> None:
        pack = self.pack(
            '    return ctx.from_arrow(__import__("pyarrow").table({"value": [7]}))',
            required_tables="[]",
        )
        candidate_id = "private-candidate"
        candidate = self.root / candidate_id
        candidate.mkdir()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate.resolve(), dataset=None)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn(candidate_id, json.dumps(response, ensure_ascii=False))

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
            col("clock_domain"), col("clock_value"), target_domain="realtime"
        ).alias("realtime_clock_value")
    )''',
            required_tables="[]",
        )
        dataset = self.root / "clock-dataset"
        dataset.mkdir()
        definitions = dataset / "clock_domain.parquet"
        snapshots = dataset / "clock_snapshot.parquet"
        pq.write_table(
            pa.Table.from_arrays(
                [
                    pa.array(["monotonic", "realtime"]),
                    pa.array(["monotonic", "realtime"]),
                    pa.array([1_000_000_000, 1_000_000_000], type=pa.uint64()),
                ],
                schema=pa.schema(
                    [
                        pa.field("clock_domain", pa.string(), nullable=False),
                        pa.field("clock_type", pa.string(), nullable=False),
                        pa.field("ticks_per_second", pa.uint64(), nullable=False),
                    ]
                ),
            ),
            definitions,
        )
        pq.write_table(
            pa.Table.from_arrays(
                [
                    pa.array([0, 0], type=pa.uint64()),
                    pa.array(["monotonic", "realtime"]),
                    pa.array([100, 1_000], type=pa.uint64()),
                ],
                schema=pa.schema(
                    [
                        pa.field("snapshot_id", pa.uint64(), nullable=False),
                        pa.field("clock_domain", pa.string(), nullable=False),
                        pa.field("clock_value", pa.uint64(), nullable=False),
                    ]
                ),
            ),
            snapshots,
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "clock_domain": str(definitions.resolve()),
                        "clock_snapshot": str(snapshots.resolve()),
                    },
                },
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        output_id = response["result"]["outputs"]["main"]["output_id"]
        table = pq.read_table(candidate / "outputs" / f"{output_id}.parquet")
        self.assertEqual(table.to_pydict(), {"realtime_clock_value": [1005, None]})

        events = dataset / "events.parquet"
        pq.write_table(
            pa.table(
                {
                    "clock_domain": pa.array(["monotonic"], type=pa.string()),
                    "clock_value": pa.array([105], type=pa.uint64()),
                    "target_domain": pa.array(["realtime"], type=pa.string()),
                }
            ),
            events,
        )
        column_target_pack = self.pack(
            '''    return ctx.sql(
        "SELECT clock_domain FROM events ORDER BY "
        "kat_convert_clock(clock_domain, clock_value, target_domain)"
    )'''
        )
        column_target_id = "019f6e00-0000-7000-8000-000000000004"
        column_target_candidate = self.root / column_target_id
        column_target_candidate.mkdir()
        completed, response = self.run_runtime(
            self.request(
                column_target_pack,
                column_target_id,
                column_target_candidate.resolve(),
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "events": str(events.resolve()),
                        "clock_domain": str(definitions.resolve()),
                        "clock_snapshot": str(snapshots.resolve()),
                    },
                },
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("string literal", response["error"]["causes"][0])
        self.assertNotIn("result", response)

        window_target_pack = self.pack(
            '''    return ctx.sql(
        "SELECT row_number() OVER (PARTITION BY "
        "kat_convert_clock(clock_domain, clock_value, target_domain) "
        "ORDER BY clock_value) AS row_number FROM events"
    )'''
        )
        window_target_id = "019f6e00-0000-7000-8000-000000000006"
        window_target_candidate = self.root / window_target_id
        window_target_candidate.mkdir()
        completed, response = self.run_runtime(
            self.request(
                window_target_pack,
                window_target_id,
                window_target_candidate.resolve(),
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "events": str(events.resolve()),
                        "clock_domain": str(definitions.resolve()),
                        "clock_snapshot": str(snapshots.resolve()),
                    },
                },
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("string literal", response["error"]["causes"][0])
        self.assertNotIn("result", response)

        parameter_target_pack = self.pack(
            '''    return ctx.sql(
        "SELECT kat_convert_clock(clock_domain, clock_value, $target) "
        "AS realtime_clock_value FROM events",
        target="realtime",
    )'''
        )
        parameter_target_id = "019f6e00-0000-7000-8000-000000000005"
        parameter_target_candidate = self.root / parameter_target_id
        parameter_target_candidate.mkdir()
        completed, response = self.run_runtime(
            self.request(
                parameter_target_pack,
                parameter_target_id,
                parameter_target_candidate.resolve(),
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "events": str(events.resolve()),
                        "clock_domain": str(definitions.resolve()),
                        "clock_snapshot": str(snapshots.resolve()),
                    },
                },
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("string literal", response["error"]["causes"][0])
        self.assertNotIn("result", response)

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
        invalid_id = "019f6e00-0000-7000-8000-000000000003"
        invalid_candidate = self.root / invalid_id
        invalid_candidate.mkdir()
        completed, response = self.run_runtime(
            self.request(
                pack,
                invalid_id,
                invalid_candidate.resolve(),
                dataset={
                    "path": str(dataset.resolve()),
                    "tables": {
                        "clock_domain": str(definitions.resolve()),
                        "clock_snapshot": str(snapshots.resolve()),
                    },
                },
            )
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)


if __name__ == "__main__":
    unittest.main()
