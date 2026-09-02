from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

import pyarrow.parquet as pq


class WorkflowExecutionProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

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
        (pack / "workflows" / "entry.py").write_text(
            f'''import kat
import pyarrow as pa

@kat.workflow(
    name="analyze",
    description="Analyze the provided facts.",
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
        candidate_id = f"019f6e00-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        candidate = self.root / "runs" / candidate_id
        candidate.mkdir(parents=True)
        return candidate_id, candidate.resolve()

    def request(
        self,
        pack: Path,
        candidate_id: str,
        candidate: Path,
        arguments: list[str] | None = None,
    ) -> dict[str, object]:
        return {
            "operation": "run_workflow",
            "pack_name": "example",
            "pack_path": str(pack),
            "workflow_name": "analyze",
            "arguments": arguments or [],
            "candidate_id": candidate_id,
            "candidate_path": str(candidate),
            "datasource_root": str(
                (self.root / "datasources" / "example").resolve(strict=False)
            ),
        }

    def test_run_uses_only_datasource_root_and_writes_standard_table_outputs(
        self,
    ) -> None:
        pack = self.pack(
            '''    import logging
    logging.getLogger("pack").info("executed")
    root = ctx.datasource_root
    selected = kat.dataprovider.Table.from_arrow(pa.table({"value": [minimum]}))
    empty = kat.dataprovider.Table.from_arrow(
        pa.table({"value": pa.array([], type=pa.int64())})
    )
    assert root.name == "example"
    return {"selected_rows": selected, "empty_rows": empty}'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(
                pack,
                candidate_id,
                candidate,
                arguments=["--minimum", "2", "--window", "0.005s"],
            )
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "success", response)
        self.assertIn(
            f"candidate={candidate_id} pack=example workflow=analyze pack: executed",
            completed.stderr.decode(errors="replace"),
        )
        result = response["result"]
        self.assertEqual(
            result["effective_inputs"],
            {"minimum": "2", "window": "5000000"},
        )
        self.assertEqual(set(result["outputs"]), {"selected_rows", "empty_rows"})
        self.assertEqual(result["outputs"]["selected_rows"]["row_count"], 1)
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

    def test_run_does_not_turn_arrow_chunks_into_parquet_row_groups(self) -> None:
        pack = self.pack(
            '''    values = pa.chunked_array([
        pa.array([1, 2], type=pa.int64()),
        pa.array([3], type=pa.int64()),
    ])
    table = kat.dataprovider.Table.from_arrow(
        pa.Table.from_arrays([values], names=["value"])
    )
    return table'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["outputs"]["main"],
            {
                "columns": [{"name": "value", "type": "int64"}],
                "row_count": 3,
            },
        )
        output = candidate / "outputs" / "main.parquet"
        metadata = pq.read_metadata(output)
        self.assertEqual(metadata.num_row_groups, 1)
        self.assertEqual(
            [
                metadata.row_group(index).column(0).compression
                for index in range(metadata.num_row_groups)
            ],
            ["ZSTD"],
        )
        self.assertFalse((candidate / "manifest.json").exists())

    def test_run_request_rejects_removed_dataset_field(self) -> None:
        pack = self.pack(
            '''    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))
    return table'''
        )
        for legacy_dataset in (None, {"path": "private", "tables": {}}):
            with self.subTest(dataset=legacy_dataset):
                candidate_id, candidate = self.candidate()
                completed, response = self.run_runtime(
                    self.request(pack, candidate_id, candidate)
                    | {"dataset": legacy_dataset}
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertEqual(
                    response["error"]["message"],
                    "Runtime Request is invalid",
                )

    def test_run_rejects_a_non_uuidv7_candidate_without_exposing_it(self) -> None:
        pack = self.pack(
            '''    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))
    return table'''
        )
        candidate_id = "private-candidate"
        candidate = self.root / "runs" / candidate_id
        candidate.mkdir(parents=True)

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate.resolve())
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure", response)
        self.assertEqual(response["error"]["message"], "Runtime Request is invalid")
        self.assertNotIn(candidate_id, json.dumps(response, ensure_ascii=False))

    def test_run_request_rejects_unowned_datasource_roots(self) -> None:
        pack = self.pack(
            '''    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))
    return table'''
        )
        for root in (
            self.root / "datasources" / "other-pack",
            self.root / "other-data-home" / "datasources" / "example",
        ):
            with self.subTest(root=root):
                candidate_id, candidate = self.candidate()
                request = self.request(pack, candidate_id, candidate)
                request["datasource_root"] = str(root.resolve(strict=False))

                completed, response = self.run_runtime(request)

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertEqual(
                    response["error"]["message"],
                    "Runtime Request is invalid",
                )

    def test_run_request_accepts_a_linked_runs_directory(self) -> None:
        external_runs = self.root / "external-runs"
        external_runs.mkdir()
        runs = self.root / "runs"
        if os.name == "nt":
            created = subprocess.run(
                [
                    "cmd.exe",
                    "/d",
                    "/c",
                    "mklink",
                    "/J",
                    str(runs),
                    str(external_runs),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(
                created.returncode,
                0,
                created.stderr.decode(errors="replace"),
            )
        else:
            try:
                runs.symlink_to(external_runs, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"directory link creation is unavailable: {error}")

        pack = self.pack(
            '''    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [7]}))
    return table'''
        )
        candidate_id = f"019f6e00-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        (runs / candidate_id).mkdir()
        candidate = (external_runs / candidate_id).resolve()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["outputs"]["main"]["row_count"], 1)

    def test_workflow_system_exit_is_a_runtime_failure(self) -> None:
        pack = self.pack('    raise SystemExit("Workflow requested exit")')
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("Workflow requested exit", response["error"]["causes"])
        self.assertNotIn("result", response)

    def test_output_io_failure_logs_private_cause_but_returns_public_diagnostic(
        self,
    ) -> None:
        pack = self.pack(
            '''    import kat.dataprovider._parquet_writer as writer_module
    def fail_write(path, schema, *args, **kwargs):
        raise ValueError(f"private output path: {path}")
    writer_module.pq.ParquetWriter = fail_write
    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [7]}))
    return table'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
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

    def test_output_footer_failure_uses_the_shared_private_writer_boundary(
        self,
    ) -> None:
        pack = self.pack(
            '''    import kat.dataprovider._parquet_writer as writer_module
    def fail_footer(path):
        raise ValueError(f"private footer path: {path}")
    writer_module.pq.read_metadata = fail_footer
    table = kat.dataprovider.Table.from_arrow(pa.table({"value": [7]}))
    return table'''
        )
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate)
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
        self.assertIn("private footer path", operation_log)
        self.assertIn(str(candidate), operation_log)

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
                    f'''    table = kat.dataprovider.Table.from_arrow(
        pa.table({{"value": [1]}})
    )
    return {{"{reserved}": table}}'''
                )
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(pack, candidate_id, candidate)
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)
                self.assertFalse((candidate / "outputs").exists())

    def test_run_rejects_workflow_entry_imports_independent_of_entry_order(
        self,
    ) -> None:
        pack = self.root / "entry-import-pack"
        workflows = pack / "workflows"
        workflows.mkdir(parents=True)
        (workflows / "a.py").write_text(
            """import pyarrow as pa
from kat import Context, dataprovider, workflow
@workflow(name='a', description='A.')
def analyze(ctx: Context):
    \"\"\"A.\"\"\"
    return dataprovider.Table.from_arrow(pa.table({'value': [1]}))
""",
            encoding="utf-8",
        )
        (workflows / "b.py").write_text(
            """import pyarrow as pa
from kat import Context, dataprovider, workflow
from kat.pack.workflows.a import analyze
@workflow(name='b', description='B.')
def other(ctx: Context):
    \"\"\"B.\"\"\"
    return dataprovider.Table.from_arrow(pa.table({'value': [1]}))
""",
            encoding="utf-8",
        )
        pack = pack.resolve(strict=True)
        candidate_id, candidate = self.candidate()

        completed, response = self.run_runtime(
            self.request(pack, candidate_id, candidate) | {"workflow_name": "b"}
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


if __name__ == "__main__":
    unittest.main()
