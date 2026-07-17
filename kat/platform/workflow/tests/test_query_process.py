from __future__ import annotations

import json
import os
from pathlib import Path
from decimal import Decimal
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
        self.run_id = "019f6e00-0000-7000-8000-000000000021"
        self.run_path = (self.root / self.run_id).resolve()
        (self.run_path / "outputs").mkdir(parents=True)
        self.output_id = "0123456789abcdef0123456789abcdef"

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def parquet(self, table: pa.Table, *, output_id: str | None = None) -> Path:
        path = self.run_path / "outputs" / f"{output_id or self.output_id}.parquet"
        pq.write_table(table, path)
        return path

    def request(
        self,
        sql: str,
        *,
        dataset: dict[str, object] | None = None,
    ) -> dict[str, object]:
        return {
            "operation": "query_run",
            "run_id": self.run_id,
            "run_path": str(self.run_path),
            "outputs": {"main": self.output_id},
            "dataset": dataset or {"status": "not_provided"},
            "sql": sql,
        }

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

    def test_output_query_succeeds_for_all_dataset_states_and_registers_current_dataset(
        self,
    ) -> None:
        self.parquet(pa.table({"value": pa.array([2, 1], type=pa.int64())}))
        dataset_root = (self.root / "dataset").resolve()
        dataset_root.mkdir()
        events = dataset_root / "events.parquet"
        pq.write_table(pa.table({"value": pa.array([3], type=pa.int64())}), events)
        cases = [
            ({"status": "not_provided"}, "SELECT value FROM output.main ORDER BY value", [["1"], ["2"]]),
            (
                {
                    "status": "unavailable",
                    "path": str(dataset_root),
                    "cause": "Dataset marker is missing",
                },
                "SELECT sum(value) AS total FROM output.main",
                [["3"]],
            ),
            (
                {
                    "status": "available",
                    "path": str(dataset_root),
                    "tables": {"events": str(events.resolve())},
                },
                "SELECT value FROM output.main UNION ALL SELECT value FROM dataset.events ORDER BY value",
                [["1"], ["2"], ["3"]],
            ),
        ]
        for dataset, sql, expected_rows in cases:
            with self.subTest(status=dataset["status"]):
                completed, response = self.run_runtime(
                    self.request(sql, dataset=dataset)
                )
                self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
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
                    pa.array([1.25], type=pa.float64()),
                    pa.array(["鏌ヨ"], type=pa.string()),
                    pa.array([1_500_000_001], type=pa.timestamp("ns", tz="UTC")),
                    pa.array([None], type=pa.string()),
                ],
                schema=schema,
            )
        )

        _, response = self.run_runtime(self.request("SELECT *, signed AS signed_copy FROM output.main"))

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(
            response["result"]["rows"],
            [[
                "-9223372036854775808",
                "18446744073709551615",
                7,
                {
                    "decimal": {
                        "bits": 128,
                        "unscaled": "123450",
                        "precision": 10,
                        "scale": 3,
                    }
                },
                1.25,
                "鏌ヨ",
                "1970-01-01T00:00:01.500000001Z",
                None,
                "-9223372036854775808",
            ]],
        )
        self.assertEqual(
            [column["name"] for column in response["result"]["columns"]],
            ["signed", "unsigned", "small", "amount", "ratio", "text", "at", "empty", "signed_copy"],
        )

    def test_limits_read_only_and_unsupported_types_fail_whole(self) -> None:
        self.parquet(pa.table({"value": list(range(1_001))}))
        _, row_limit = self.run_runtime(self.request("SELECT * FROM output.main"))
        self.assertEqual(row_limit["status"], "failure", row_limit)
        self.assertNotIn("result", row_limit)
        self.assertIn("row limit", json.dumps(row_limit).lower())

        for sql, expected in [
            ("CREATE TABLE altered AS SELECT 1", "ddl not supported"),
            ("SELECT CAST(value AS BINARY) FROM output.main LIMIT 1", "unsupported"),
            ("SELECT CAST('NaN' AS DOUBLE) FROM output.main LIMIT 1", "non-finite"),
        ]:
            with self.subTest(sql=sql):
                _, response = self.run_runtime(self.request(sql))
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)
                self.assertIn(expected, json.dumps(response).lower())

    def test_dataset_capability_failure_uses_only_the_recorded_state(self) -> None:
        self.parquet(
            pa.table(
                {
                    "clock_domain": pa.array(["monotonic"], type=pa.string()),
                    "clock_value": pa.array([1], type=pa.uint64()),
                }
            )
        )
        _, response = self.run_runtime(
            self.request(
                "SELECT kat_convert_clock(clock_domain, clock_value, 'realtime') FROM output.main",
                dataset={
                    "status": "unavailable",
                    "path": str((self.root / "removed").resolve()),
                    "cause": "Dataset marker is missing",
                },
            )
        )

        self.assertEqual(response["status"], "failure", response)
        rendered = json.dumps(response, ensure_ascii=False)
        self.assertIn("Dataset marker is missing", rendered)
        self.assertIn("output.*", rendered)
        self.assertNotIn("result", response)

    def test_dataset_table_resolution_uses_typed_current_state(self) -> None:
        self.parquet(pa.table({"value": [1]}))
        cases = [
            (
                {"status": "not_provided"},
                ["did not provide", "output.*", "rerun"],
            ),
            (
                {
                    "status": "unavailable",
                    "path": str((self.root / "removed").resolve()),
                    "cause": "Dataset marker is missing",
                },
                ["unavailable", "Dataset marker is missing", "output.*", "restore"],
            ),
        ]
        for dataset, expected in cases:
            with self.subTest(status=dataset["status"]):
                _, response = self.run_runtime(
                    self.request("SELECT * FROM dataset.events", dataset=dataset)
                )
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)
                rendered = json.dumps(response, ensure_ascii=False)
                self.assertIn("DataFusion", rendered)
                for fragment in expected:
                    self.assertIn(fragment, rendered)

    def test_request_dataset_shapes_are_strict_and_mutually_exclusive(self) -> None:
        self.parquet(pa.table({"value": [1]}))
        invalid = self.request("SELECT * FROM output.main")
        invalid["dataset"] = {"status": "not_provided", "path": "extra"}
        _, response = self.run_runtime(invalid)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)

        unknown = self.request("SELECT * FROM output.main")
        unknown["extra"] = True
        _, response = self.run_runtime(unknown)
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)

    def test_execution_time_limit_fails_without_rows(self) -> None:
        from _kat_runtime import query

        self.parquet(pa.table({"value": [1]}))
        previous = query.QUERY_TIME_LIMIT_SECONDS
        query.QUERY_TIME_LIMIT_SECONDS = 0.0
        try:
            with self.assertRaisesRegex(query.QueryLimitExceeded, "time limit"):
                query.query_run(self.request("SELECT * FROM output.main"))
        finally:
            query.QUERY_TIME_LIMIT_SECONDS = previous

    def test_scalar_helpers_cover_arrow_edge_types(self) -> None:
        from _kat_runtime import query

        self.assertEqual(
            query._json_scalar(pa.array(["large"], type=pa.large_string()), 0),
            "large",
        )
        if hasattr(pa, "string_view"):
            self.assertEqual(
                query._json_scalar(pa.array(["view"], type=pa.string_view()), 0),
                "view",
            )
        self.assertEqual(
            query._json_scalar(
                pa.array([Decimal("-1.2300")], type=pa.decimal256(40, 4)), 0
            ),
            {
                "decimal": {
                    "bits": 256,
                    "unscaled": "-12300",
                    "precision": 40,
                    "scale": 4,
                }
            },
        )
        self.assertEqual(query._timestamp_ns_utc(-1), "1969-12-31T23:59:59.999999999Z")
        for unsupported in [
            pa.timestamp("ms", tz="UTC"),
            pa.timestamp("ns"),
            pa.timestamp("ns", tz="Asia/Shanghai"),
            pa.binary(),
            pa.list_(pa.int64()),
        ]:
            with self.subTest(data_type=unsupported):
                with self.assertRaisesRegex(TypeError, "not supported"):
                    query._validate_result_types(
                        pa.schema([pa.field("value", unsupported)])
                    )
        for unsupported_decimal in [pa.decimal32(8, 2), pa.decimal64(18, 2)]:
            with self.subTest(data_type=unsupported_decimal):
                with self.assertRaisesRegex(TypeError, "not supported"):
                    query._validate_result_types(
                        pa.schema([pa.field("value", unsupported_decimal)])
                    )


if __name__ == "__main__":
    unittest.main()
