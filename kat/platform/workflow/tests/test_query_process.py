from __future__ import annotations

from decimal import Decimal
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


class QueryProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.run_path = (self.root / "run").resolve()
        (self.run_path / "outputs").mkdir(parents=True)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def parquet(self, table: pa.Table, *, output_name: str = "main") -> Path:
        path = self.run_path / "outputs" / f"{output_name}.parquet"
        pq.write_table(table, path)
        return path

    def request(
        self,
        sql: str,
        *,
        dataset: dict[str, object] | None = None,
    ) -> dict[str, object]:
        request: dict[str, object] = {
            "operation": "query_run",
            "run_path": str(self.run_path),
            "outputs": ["main"],
            "sql": sql,
        }
        if dataset is not None:
            request["dataset"] = dataset
        return request

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
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def test_registers_published_outputs_and_available_dataset(self) -> None:
        self.parquet(pa.table({"value": pa.array([2, 1], type=pa.int64())}))
        dataset_root = (self.root / "dataset").resolve()
        dataset_root.mkdir()
        events = dataset_root / "events.parquet"
        pq.write_table(pa.table({"value": pa.array([3], type=pa.int64())}), events)

        cases = [
            (
                self.request("SELECT value FROM output.main ORDER BY value"),
                [["1"], ["2"]],
            ),
            (
                self.request(
                    """
                    SELECT value FROM output.main
                    UNION ALL
                    SELECT value FROM dataset.events
                    ORDER BY value
                    """,
                    dataset={
                        "path": str(dataset_root),
                        "tables": {"events": str(events.resolve())},
                    },
                ),
                [["1"], ["2"], ["3"]],
            ),
        ]
        for request, expected_rows in cases:
            with self.subTest(dataset="dataset" in request):
                completed, response = self.run_runtime(request)
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "success", response)
                self.assertEqual(set(response["result"]), {"columns", "rows"})
                self.assertEqual(response["result"]["rows"], expected_rows)

    def test_scalar_projection_is_lossless_and_positional(self) -> None:
        schema = pa.schema(
            [
                pa.field("signed", pa.int64()),
                pa.field("unsigned", pa.uint64()),
                pa.field("small", pa.int32()),
                pa.field("amount", pa.decimal128(10, 3)),
                pa.field("wide_amount", pa.decimal256(40, 4)),
                pa.field("ratio", pa.float64()),
                pa.field("text", pa.string()),
                pa.field("at", pa.timestamp("ns", tz="UTC")),
                pa.field("empty", pa.string()),
            ]
        )
        self.parquet(
            pa.Table.from_arrays(
                [
                    pa.array([-(2**63)], type=pa.int64()),
                    pa.array([2**64 - 1], type=pa.uint64()),
                    pa.array([7], type=pa.int32()),
                    pa.array([Decimal("123.450")], type=pa.decimal128(10, 3)),
                    pa.array([Decimal("-1.2300")], type=pa.decimal256(40, 4)),
                    pa.array([1.25], type=pa.float64()),
                    pa.array(["查询"], type=pa.string()),
                    pa.array([2**63 - 1], type=pa.timestamp("ns", tz="UTC")),
                    pa.array([None], type=pa.string()),
                ],
                schema=schema,
            )
        )

        _, response = self.run_runtime(
            self.request("SELECT *, signed AS signed_copy FROM output.main")
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["rows"],
            [
                [
                    "-9223372036854775808",
                    "18446744073709551615",
                    7,
                    "123.450",
                    "-1.2300",
                    1.25,
                    "查询",
                    "2262-04-11T23:47:16.854775807Z",
                    None,
                    "-9223372036854775808",
                ]
            ],
        )
        self.assertEqual(
            [column["name"] for column in response["result"]["columns"]],
            [
                "signed",
                "unsigned",
                "small",
                "amount",
                "wide_amount",
                "ratio",
                "text",
                "at",
                "empty",
                "signed_copy",
            ],
        )

    def test_read_only_and_projection_errors_fail_whole_query(self) -> None:
        self.parquet(pa.table({"value": [1]}))
        for sql, expected in [
            ("CREATE TABLE altered AS SELECT 1", "ddl not supported"),
            ("SELECT CAST('NaN' AS DOUBLE) FROM output.main", "non-finite"),
        ]:
            with self.subTest(sql=sql):
                _, response = self.run_runtime(self.request(sql))
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)
                self.assertIn(expected, json.dumps(response).lower())

        self.parquet(pa.table({"value": pa.array([b"x"], type=pa.binary())}))
        _, response = self.run_runtime(self.request("SELECT * FROM output.main"))
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertIn("not supported", json.dumps(response).lower())

    def test_default_datafusion_sources_are_not_artificially_blocked(self) -> None:
        self.parquet(pa.table({"value": [0]}))
        source = self.root / "trusted-local-source.parquet"
        pq.write_table(pa.table({"value": [1, 2, 3]}), source)
        _, response = self.run_runtime(
            self.request(f"SELECT value FROM '{source.as_posix()}' ORDER BY value")
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["rows"], [["1"], ["2"], ["3"]])

    def test_results_beyond_previous_row_and_byte_limits_succeed(self) -> None:
        values = [f"{index:04d}-{'x' * 300}" for index in range(1_001)]
        self.parquet(pa.table({"value": values}))

        _, response = self.run_runtime(
            self.request("SELECT value FROM output.main ORDER BY value")
        )

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(len(response["result"]["rows"]), 1_001)
        self.assertGreater(
            len(json.dumps(response, ensure_ascii=False).encode("utf-8")),
            256 * 1_024,
        )

if __name__ == "__main__":
    unittest.main()
