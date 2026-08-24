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

from _source_dataset import (
    materialized_dataset_request,
    write_materialized_source,
)


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

    def external_pack(self, name: str, value: int) -> tuple[Path, Path]:
        pack = self.root / f"pack-{name}"
        (pack / "sources").mkdir(parents=True)
        (pack / "helpers").mkdir()
        (pack / "SOURCES.md").write_text("External facts.\n", encoding="utf-8")
        (pack / "helpers" / "shared.py").write_text(
            f"VALUE = {value}\n",
            encoding="utf-8",
        )
        marker = self.root / f"{name}-source-called"
        (pack / "sources" / "facts.py").write_text(
            f'''from pathlib import Path
import kat
import pyarrow as pa
from ..helpers.shared import VALUE

@kat.source(name="facts")
def provide():
    Path({str(marker)!r}).write_text("called", encoding="utf-8")
    batch = pa.record_batch({{"value": pa.array([VALUE], type=pa.int64())}})
    return kat.schema_from_readers({{
        "events": lambda: pa.RecordBatchReader.from_batches(batch.schema, [batch]),
    }})
''',
            encoding="utf-8",
        )
        return pack.resolve(), marker

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
            "pack_search": {"candidates": {}, "issues": []},
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
        tables = write_materialized_source(
            dataset_root,
            pack="example",
            source="facts",
            tables={"events": pa.table({"value": pa.array([3], type=pa.int64())})},
        )

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
                    SELECT value FROM example.facts.events
                    ORDER BY value
                    """,
                    dataset=materialized_dataset_request(
                        dataset_root,
                        pack="example",
                        source="facts",
                        tables=tables,
                    ),
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

    def test_dataset_identity_cannot_replace_the_private_run_output_catalog(self) -> None:
        self.parquet(pa.table({"value": pa.array([1], type=pa.int64())}))
        dataset_root = (self.root / "colliding-dataset").resolve()
        tables = write_materialized_source(
            dataset_root,
            pack="datafusion",
            source="output",
            tables={"main": pa.table({"value": pa.array([2], type=pa.int64())})},
        )

        completed, response = self.run_runtime(
            self.request(
                """
                SELECT value FROM output.main
                UNION ALL
                SELECT value FROM datafusion.output.main
                ORDER BY value
                """,
                dataset=materialized_dataset_request(
                    dataset_root,
                    pack="datafusion",
                    source="output",
                    tables=tables,
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["rows"], [["1"], ["2"]])

    def test_partial_materialization_never_falls_back_to_external_source(self) -> None:
        self.parquet(pa.table({"value": [0]}))
        dataset_root = (self.root / "partial-dataset").resolve()
        tables = write_materialized_source(
            dataset_root,
            pack="example",
            source="facts",
            tables={"snapshots": pa.table({"value": [1]})},
        )
        source_pack = self.root / "source-pack"
        (source_pack / "sources").mkdir(parents=True)
        (source_pack / "SOURCES.md").write_text("Facts.\n", encoding="utf-8")
        marker = self.root / "external-source-loaded"
        (source_pack / "sources" / "facts.py").write_text(
            f"from pathlib import Path\nPath({str(marker)!r}).write_text('loaded')\n",
            encoding="utf-8",
        )
        request = self.request(
            "SELECT * FROM example.facts.mappings",
            dataset=materialized_dataset_request(
                dataset_root,
                pack="example",
                source="facts",
                tables=tables,
            ),
        )
        request["pack_search"]["candidates"] = {
            "example": [str(source_pack.resolve())]
        }

        completed, response = self.run_runtime(request)

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("mappings", json.dumps(response))
        self.assertFalse(marker.exists(), "Materialized Source must fully shadow External")

    def test_query_joins_two_external_packs_and_does_not_call_unreferenced_source(
        self,
    ) -> None:
        self.parquet(pa.table({"value": [0]}))
        packs = {
            name: self.external_pack(name, value)
            for name, value in (("alpha", 1), ("beta", 2), ("gamma", 3))
        }
        dataset = self.root / "external-dataset"
        dataset.mkdir()
        request = self.request(
            """
            SELECT alpha.value AS alpha_value, beta.value AS beta_value
            FROM alpha.facts.events AS alpha
            CROSS JOIN beta.facts.events AS beta
            """,
            dataset={
                "path": str(dataset.resolve()),
                "sources": [
                    {
                        "pack": name,
                        "source": "facts",
                        "kind": "external",
                        "arguments": [],
                        "working_directory": str(self.root.resolve()),
                    }
                    for name in sorted(packs)
                ],
            },
        )
        request["pack_search"]["candidates"] = {
            name: [str(pack.resolve())]
            for name, (pack, _marker) in sorted(packs.items())
        }
        request["pack_search"]["candidates"]["unrelated"] = [
            str((self.root / "unrelated-a").absolute()),
            str((self.root / "unrelated-b").absolute()),
        ]
        request["pack_search"]["issues"] = ["unrelated PACK manifest is invalid"]

        completed, response = self.run_runtime(request)

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["rows"], [["1", "2"]])
        self.assertTrue(packs["alpha"][1].exists())
        self.assertTrue(packs["beta"][1].exists())
        self.assertFalse(packs["gamma"][1].exists())

    def test_unreferenced_external_pack_failures_do_not_block_output_or_materialized_tables(
        self,
    ) -> None:
        self.parquet(pa.table({"value": [1]}))
        dataset_root = (self.root / "mixed-dataset").resolve()
        tables = write_materialized_source(
            dataset_root,
            pack="example",
            source="facts",
            tables={"events": pa.table({"value": [2]})},
        )
        dataset = materialized_dataset_request(
            dataset_root,
            pack="example",
            source="facts",
            tables=tables,
        )
        dataset["sources"].append(
            {
                "pack": "zeta",
                "source": "facts",
                "kind": "external",
                "arguments": [],
                "working_directory": str(self.root.resolve()),
            }
        )

        for sql, expected in (
            ("SELECT value FROM output.main", [["1"]]),
            ("SELECT value FROM example.facts.events", [["2"]]),
        ):
            with self.subTest(sql=sql):
                request = self.request(sql, dataset=dataset)
                request["pack_search"] = {
                    "candidates": {
                        "zeta": [
                            str((self.root / "zeta-a").absolute()),
                            str((self.root / "zeta-b").absolute()),
                        ]
                    },
                    "issues": ["another PACK manifest is invalid"],
                }

                completed, response = self.run_runtime(request)

                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "success", response)
                self.assertEqual(response["result"]["rows"], expected)

    def test_referenced_external_pack_discovery_failure_is_reported_lazily(self) -> None:
        self.parquet(pa.table({"value": [0]}))
        dataset = self.root / "external-failure-dataset"
        dataset.mkdir()
        common = {
            "path": str(dataset.resolve()),
            "sources": [
                {
                    "pack": "zeta",
                    "source": "facts",
                    "kind": "external",
                    "arguments": [],
                    "working_directory": str(self.root.resolve()),
                }
            ],
        }
        cases = (
            (
                {
                    "candidates": {
                        "zeta": [
                            str((self.root / "zeta-a").absolute()),
                            str((self.root / "zeta-b").absolute()),
                        ]
                    },
                    "issues": [],
                },
                "ambiguous PACK discovery",
            ),
            (
                {
                    "candidates": {},
                    "issues": ["failed to parse PACK manifest"],
                },
                "PACK discovery reported",
            ),
        )
        for pack_search, evidence in cases:
            with self.subTest(evidence=evidence):
                request = self.request(
                    "SELECT * FROM zeta.facts.events",
                    dataset=common,
                )
                request["pack_search"] = pack_search

                completed, response = self.run_runtime(request)

                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr.decode(errors="replace"),
                )
                self.assertEqual(response["status"], "failure", response)
                self.assertIn(evidence, json.dumps(response))

    def test_bound_source_with_relative_helper_is_queryable_without_a_current_pack(
        self,
    ) -> None:
        pack, marker = self.external_pack("bound", 7)
        completed, response = self.run_runtime(
            {
                "operation": "bind_source",
                "pack_name": "bound",
                "pack_path": str(pack),
                "source_name": "facts",
                "arguments": [],
                "argument_base": str(self.root.resolve()),
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response, {"status": "success", "result": {}})
        self.assertFalse(marker.exists(), "bind must not instantiate the Source Provider")
        (pack / "SOURCES.md").unlink()

        dataset = self.root / "bound-dataset"
        dataset.mkdir()
        completed, response = self.run_runtime(
            {
                "operation": "query_dataset",
                "dataset": {
                    "path": str(dataset.resolve()),
                    "sources": [
                        {
                            "pack": "bound",
                            "source": "facts",
                            "kind": "external",
                            "arguments": [],
                            "working_directory": str(self.root.resolve()),
                        }
                    ],
                },
                "pack_search": {
                    "candidates": {"bound": [str(pack)]},
                    "issues": [],
                },
                "sql": "SELECT value FROM bound.facts.events",
            }
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "success", response)
        self.assertEqual(response["result"]["rows"], [["7"]])
        self.assertTrue(marker.exists())

    def test_materialized_table_directory_is_rejected(self) -> None:
        self.parquet(pa.table({"value": [0]}))
        dataset_root = (self.root / "fragmented-dataset").resolve()
        events = dataset_root / "sources" / "example" / "facts" / "tables" / "events.parquet"
        events.mkdir(parents=True)
        pq.write_table(pa.table({"value": [1]}), events / "part.parquet")

        completed, response = self.run_runtime(
            self.request(
                "SELECT value FROM example.facts.events",
                dataset=materialized_dataset_request(
                    dataset_root,
                    pack="example",
                    source="facts",
                    tables={"events": events},
                ),
            )
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(response["status"], "failure", response)
        self.assertIn("canonical file", json.dumps(response).lower())

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
