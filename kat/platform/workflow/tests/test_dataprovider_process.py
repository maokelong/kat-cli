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

    def pack(self, body: str) -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "entry.py").write_text(
            f'''import kat
import pyarrow as pa
from kat import dataprovider as dp

@kat.workflow(
    name="analyze",
    description="Exercise the Data Provider Runtime boundary.",
)
def analyze(ctx: kat.Context):
    """Exercise the standard Output boundary."""
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
        self, pack: Path, candidate_id: str, candidate: Path
    ) -> dict[str, object]:
        return {
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

    def test_standard_outputs_accept_table_and_exact_non_empty_dict(self) -> None:
        for name, body, expected in [
            (
                "single",
                '''    table = dp.Table.from_arrow(pa.table({"value": [1]}))
    return table''',
                {"main": {"value": [1]}},
            ),
            (
                "mapping",
                '''    first = dp.Table.from_arrow(pa.table({"value": [1]}))
    empty = dp.Table.from_arrow(pa.table({"value": pa.array([], type=pa.int64())}))
    return {"first": first, "empty": empty}''',
                {"first": {"value": [1]}, "empty": {"value": []}},
            ),
        ]:
            with self.subTest(name=name):
                pack = self.pack(body)
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
                    {
                        output_name: pq.read_table(
                            candidate / "outputs" / f"{output_name}.parquet"
                        ).to_pydict()
                        for output_name in response["result"]["outputs"]
                    },
                    expected,
                )

    def test_standard_outputs_reject_legacy_and_inexact_values_before_writing(
        self,
    ) -> None:
        cases = {
            "dataframe": '''    import pyarrow as pa
    from datafusion import SessionContext
    return SessionContext().from_arrow(pa.table({"value": [1]}))''',
            "pyarrow": '''    import pyarrow as pa
    return pa.table({"value": [1]})''',
            "empty": "    return {}",
            "scalar": "    return 1",
            "list": "    return []",
            "path": '''    from pathlib import Path
    return Path("not-an-output")''',
            "catalog": '''    import pyarrow.parquet as pq
    catalog_root = ctx.datasource_root / "catalog"
    catalog_root.mkdir(parents=True)
    pq.write_table(pa.table({"value": [1]}), catalog_root / "events.parquet")
    return dp.open(root=catalog_root)''',
            "write-sink": '''    ctx.datasource_root.mkdir(parents=True, exist_ok=True)
    schema = dp.Schema({"events": {"value": int}})
    with dp.write(schema, destination=ctx.datasource_root / "facts") as sink:
        sink["events"].append(value=1)
    return sink''',
            "mixed": '''    import pyarrow as pa
    from datafusion import SessionContext
    table = dp.Table.from_arrow(pa.table({"value": [1]}))
    frame = SessionContext().from_arrow(pa.table({"value": [2]}))
    return {"table": table, "frame": frame}''',
            "dict-subclass": '''    class Outputs(dict):
        pass
    table = dp.Table.from_arrow(pa.table({"value": [1]}))
    return Outputs(main=table)''',
            "table-like": '''    class TableLike:
        def to_arrow(self):
            return pa.table({"value": [1]})
    return TableLike()''',
        }
        for name, body in cases.items():
            with self.subTest(name=name):
                pack = self.pack(body)
                candidate_id, candidate = self.candidate()

                completed, response = self.run_runtime(
                    self.request(pack, candidate_id, candidate)
                )

                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure", response)
                self.assertFalse((candidate / "outputs").exists())


if __name__ == "__main__":
    unittest.main()
