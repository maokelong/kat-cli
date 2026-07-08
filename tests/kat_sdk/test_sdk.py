from __future__ import annotations

import sys
import unittest
from pathlib import Path

SDK_ROOT = Path(__file__).resolve().parents[2] / "python" / "kat_sdk"
sys.path.insert(0, str(SDK_ROOT))

import kat


class FakeChannel:
    def __init__(self) -> None:
        self.queries = []
        self.logs = []

    def query(self, sql, params):
        self.queries.append((sql, dict(params)))
        return {"query_id": f"q{len(self.queries)}"}

    def preview(self, query_id, limit):
        return [{"query_id": query_id, "limit": limit}]

    def rows(self, query_id, max_rows):
        return [{"query_id": query_id, "max_rows": max_rows}]

    def log(self, level, message, fields):
        self.logs.append((level, message, dict(fields)))


class KatSdkTests(unittest.TestCase):
    def tearDown(self) -> None:
        kat.reset_runtime()

    def test_decorators_attach_workflow_contract(self) -> None:
        @kat.workflow(title="Generic thread critical path")
        @kat.option("--root-itid", help="Root internal thread id", default=0)
        @kat.option("--start-ts", help="Start timestamp", required=True)
        def extract(root_itid=0, start_ts=0):
            return None

        spec = kat.get_workflow_spec(extract)

        self.assertIsNotNone(spec)
        self.assertEqual(spec.title, "Generic thread critical path")
        self.assertEqual([option.name for option in spec.options], ["root_itid", "start_ts"])
        self.assertEqual(spec.options[1].flags, ("--start-ts",))
        self.assertTrue(spec.options[1].required)

    def test_query_forwards_to_runtime_and_returns_handle(self) -> None:
        channel = FakeChannel()
        kat.bind_runtime(channel)

        result = kat.query("select * from thread where itid = :itid", itid=7)

        self.assertIsInstance(result, kat.QueryResult)
        self.assertEqual(result.query_id, "q1")
        self.assertEqual(channel.queries, [("select * from thread where itid = :itid", {"itid": 7})])

    def test_query_result_reads_bounded_rows(self) -> None:
        channel = FakeChannel()
        kat.bind_runtime(channel)
        result = kat.query("select 1")

        self.assertEqual(result.preview(limit=2), [{"query_id": "q1", "limit": 2}])
        self.assertEqual(result.rows(max_rows=3), [{"query_id": "q1", "max_rows": 3}])

    def test_log_forwards_to_runtime(self) -> None:
        channel = FakeChannel()
        kat.bind_runtime(channel)

        kat.log("loaded facts", rows=12)

        self.assertEqual(channel.logs, [("info", "loaded facts", {"rows": 12})])

    def test_validate_workflow_return_accepts_only_query_results(self) -> None:
        channel = FakeChannel()
        kat.bind_runtime(channel)
        result = kat.query("select 1")

        self.assertEqual(kat.validate_workflow_return({"artifact": result}), {"artifact": result})
        with self.assertRaises(TypeError):
            kat.validate_workflow_return({"bad": [{"not": "query result"}]})


if __name__ == "__main__":
    unittest.main()
