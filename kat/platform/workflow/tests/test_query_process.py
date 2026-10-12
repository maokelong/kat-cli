from __future__ import annotations

import json
import os
from decimal import Decimal
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
        outputs: dict[str, Path] | None = None,
        result_name: str | None = None,
    ) -> dict[str, object]:
        selected_outputs = outputs or {
            "main": self.run_path / "outputs" / "main.parquet"
        }
        return {
            "operation": "query_run",
            "outputs": {name: str(path.resolve()) for name, path in selected_outputs.items()},
            "sql": sql,
            "result_path": str(
                (self.root / (result_name or f"query-{uuid.uuid4()}.ndjson")).resolve()
            ),
        }

    def run_runtime(
        self, request: dict[str, object]
    ) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
        completed, response = self.run_runtime_text(request)
        return completed, json.loads(response)

    def run_runtime_text(
        self, request: dict[str, object]
    ) -> tuple[subprocess.CompletedProcess[bytes], str]:
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
        return completed, response_path.read_text(encoding="utf-8")

    def test_inlines_small_result_from_registered_run_outputs(self) -> None:
        main = self.parquet(
            pa.table({"key": pa.array([2, 1], type=pa.int64())})
        )
        other = self.parquet(
            pa.table({"key": pa.array([1], type=pa.int64()), "label": ["one"]}),
            output_name="other",
        )
        request = self.request(
            """
            SELECT main.key, other.label
            FROM output.main AS main
            LEFT JOIN output.other AS other USING (key)
            ORDER BY main.key
            """,
            outputs={"main": main, "other": other},
        )

        completed, response = self.run_runtime(request)

        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "format": "json",
                    "columns": [
                        {"name": "key", "type": "int64"},
                        {"name": "label", "type": "string_view"},
                    ],
                    "rows": [{"key": 1, "label": "one"}, {"key": 2}],
                },
            },
        )
        self.assertEqual(
            Path(request["result_path"]).read_bytes(),
            b'{"key":1,"label":"one"}\n{"key":2}\n',
        )

    def test_native_ndjson_semantics_and_zero_rows(self) -> None:
        schema = pa.schema(
            [
                pa.field("binary", pa.binary()),
                pa.field("finite", pa.float64()),
                pa.field("nan", pa.float64()),
                pa.field("missing", pa.string()),
                pa.field(
                    "nested",
                    pa.struct(
                        [
                            pa.field("x", pa.int64()),
                            pa.field("missing", pa.string()),
                        ]
                    ),
                ),
            ]
        )
        self.parquet(
            pa.Table.from_arrays(
                [
                    pa.array([b"\xde\x00\xff"], type=pa.binary()),
                    pa.array([1.25], type=pa.float64()),
                    pa.array([float("nan")], type=pa.float64()),
                    pa.array([None], type=pa.string()),
                    pa.array(
                        [{"x": 1, "missing": None}],
                        type=schema.field("nested").type,
                    ),
                ],
                schema=schema,
            )
        )

        request = self.request("SELECT * FROM output.main", result_name="native.ndjson")
        _, response = self.run_runtime(request)

        self.assertEqual(
            response,
            {
                "status": "success",
                "result": {
                    "format": "json",
                    "columns": [
                        {"name": "binary", "type": "binary_view"},
                        {"name": "finite", "type": "double"},
                        {"name": "nan", "type": "double"},
                        {"name": "missing", "type": "string_view"},
                        {
                            "name": "nested",
                            "type": "struct<x: int64, missing: string>",
                        },
                    ],
                    "rows": [
                        {
                            "binary": "de00ff",
                            "finite": 1.25,
                            "nan": None,
                            "nested": {"x": 1},
                        }
                    ],
                },
            },
        )
        self.assertEqual(
            Path(request["result_path"]).read_bytes(),
            b'{"binary":"de00ff","finite":1.25,"nan":null,"nested":{"x":1}}\n',
        )

        empty_request = self.request(
            "SELECT * FROM output.main WHERE FALSE",
            result_name="empty.ndjson",
        )
        _, empty_response = self.run_runtime(empty_request)
        self.assertEqual(empty_response["status"], "success", empty_response)
        self.assertEqual(empty_response["result"]["format"], "json")
        self.assertEqual(empty_response["result"]["rows"], [])
        self.assertEqual(Path(empty_request["result_path"]).read_bytes(), b"")

    def test_inline_response_preserves_the_native_numeric_lexeme(self) -> None:
        precise = Decimal("12345678901234567890.123456789012345678")
        self.parquet(
            pa.table(
                {
                    "precise": pa.array(
                        [precise],
                        type=pa.decimal128(38, 18),
                    )
                }
            )
        )
        request = self.request(
            "SELECT precise FROM output.main",
            result_name="precise.ndjson",
        )

        completed, response = self.run_runtime_text(request)

        self.assertEqual(completed.returncode, 0, completed.stderr.decode())
        native_row = Path(request["result_path"]).read_text(encoding="utf-8").strip()
        self.assertEqual(
            native_row,
            '{"precise":12345678901234567890.123456789012345678}',
        )
        self.assertIn(f'"rows":[{native_row}]', response)

    def test_row_threshold_is_inclusive_and_does_not_truncate(self) -> None:
        self.parquet(pa.table({"value": list(range(101))}))

        inline_request = self.request(
            "SELECT value FROM output.main ORDER BY value LIMIT 100",
            result_name="one-hundred.ndjson",
        )
        _, inline_response = self.run_runtime(inline_request)
        file_request = self.request(
            "SELECT value FROM output.main ORDER BY value",
            result_name="one-hundred-one.ndjson",
        )
        _, file_response = self.run_runtime(file_request)

        self.assertEqual(inline_response["status"], "success", inline_response)
        self.assertEqual(inline_response["result"]["format"], "json")
        self.assertEqual(len(inline_response["result"]["rows"]), 100)
        self.assertEqual(file_response["status"], "success", file_response)
        self.assertEqual(file_response["result"]["format"], "ndjson")
        self.assertNotIn("rows", file_response["result"])
        self.assertEqual(len(Path(file_request["result_path"]).read_bytes().splitlines()), 101)

    def test_byte_threshold_is_inclusive_even_for_one_row(self) -> None:
        for size, expected_format in [(65_536, "json"), (65_537, "ndjson")]:
            with self.subTest(size=size):
                output_name = f"bytes_{size}"
                output = self.parquet(
                    pa.table({"value": ["x" * (size - 13)]}),
                    output_name=output_name,
                )
                request = self.request(
                    f"SELECT value FROM output.{output_name}",
                    outputs={output_name: output},
                    result_name=f"{size}.ndjson",
                )

                _, response = self.run_runtime(request)

                self.assertEqual(response["status"], "success", response)
                self.assertEqual(Path(request["result_path"]).stat().st_size, size)
                self.assertEqual(response["result"]["format"], expected_format)
                if expected_format == "json":
                    self.assertEqual(len(response["result"]["rows"]), 1)
                else:
                    self.assertNotIn("rows", response["result"])

    def test_rejects_duplicate_struct_sibling_names_before_writing(self) -> None:
        self.parquet(pa.table({"value": [1]}))
        for sql in [
            "SELECT 1 AS duplicate, 2 AS duplicate",
            "SELECT named_struct('duplicate', 1, 'duplicate', 2) AS nested",
            "SELECT named_struct('outer', named_struct('duplicate', 1, 'duplicate', 2)) AS nested",
        ]:
            with self.subTest(sql=sql):
                request = self.request(sql)
                _, response = self.run_runtime(request)
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)
                self.assertIn("unique", json.dumps(response).lower())
                self.assertFalse(Path(request["result_path"]).exists())

        self.parquet(
            pa.table(
                {
                    "dynamic_keys": pa.array(
                        [[("duplicate", 1), ("duplicate", 2)]],
                        type=pa.map_(pa.string(), pa.int64()),
                    )
                }
            )
        )
        map_request = self.request("SELECT dynamic_keys FROM output.main")
        _, map_response = self.run_runtime(map_request)
        self.assertEqual(map_response["status"], "success", map_response)

    def test_fresh_session_allows_only_read_only_sql_and_registered_relations(self) -> None:
        output = self.parquet(pa.table({"value": [1]}))
        source = self.root / "trusted-local-source.parquet"
        pq.write_table(pa.table({"value": [1, 2, 3]}), source)

        allowed = [
            "SELECT value FROM output.main",
            "WITH selected AS (SELECT value FROM output.main) SELECT * FROM selected",
            "VALUES (1)",
            "DESCRIBE output.main",
            "EXPLAIN SELECT * FROM output.main",
            "SHOW datafusion.execution.batch_size",
            "SELECT * FROM range(0, 2)",
            "SELECT * FROM generate_series(0, 1)",
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'output'",
        ]
        for sql in allowed:
            with self.subTest(allowed=sql):
                request = self.request(sql, outputs={"main": output})
                _, response = self.run_runtime(request)
                self.assertEqual(response["status"], "success", response)
                self.assertTrue(Path(request["result_path"]).is_file())

        blocked = [
            f"SELECT * FROM '{source.as_posix()}'",
            "SELECT * FROM dataset.events",
            "SELECT * FROM unregistered",
            "CREATE TABLE altered AS SELECT 1",
            "INSERT INTO output.main VALUES (2)",
            "COPY output.main TO 'copy.parquet' STORED AS PARQUET",
            "SET datafusion.execution.batch_size = 1",
            "SELECT 1; SELECT 2",
        ]
        for sql in blocked:
            with self.subTest(blocked=sql):
                request = self.request(sql, outputs={"main": output})
                _, response = self.run_runtime(request)
                self.assertEqual(response["status"], "failure", response)
                self.assertNotIn("result", response)

    def test_results_beyond_previous_row_and_byte_limits_succeed(self) -> None:
        values = [f"{index:04d}-{'x' * 300}" for index in range(1_001)]
        self.parquet(pa.table({"value": values}))

        request = self.request(
            "SELECT value FROM output.main ORDER BY value",
            result_name="large.ndjson",
        )
        _, response = self.run_runtime(request)

        self.assertEqual(response["status"], "success", response)
        self.assertEqual(set(response["result"]), {"format", "columns"})
        self.assertEqual(response["result"]["format"], "ndjson")
        result = Path(request["result_path"]).read_bytes()
        self.assertEqual(len(result.splitlines()), 1_001)
        self.assertGreater(len(result), 256 * 1_024)

if __name__ == "__main__":
    unittest.main()
