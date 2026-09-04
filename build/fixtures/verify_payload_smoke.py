#!/usr/bin/env python3
"""Verify cross-PACK same-Session Runs and the exact persisted NDJSON result."""

from __future__ import annotations

import json
import sys
from pathlib import Path


EXPECTED_NDJSON = b'{"clock_domain":"boottime","clock_value":123456}\n'


def main(argv: list[str]) -> int:
    if len(argv) != 5:
        print(
            "usage: verify_payload_smoke.py INSPECT_RESPONSE TEST_RESPONSE "
            "FIRST_RUN_RESPONSE SECOND_RUN_RESPONSE QUERY_RESPONSE",
            file=sys.stderr,
        )
        return 2
    inspect_response, test_response, first_run, second_run, query_response = (
        json.loads(Path(path).read_text(encoding="utf-8-sig")) for path in argv
    )
    if inspect_response["status"] != "success":
        raise RuntimeError("PACK inspection did not succeed")
    if test_response["status"] != "success":
        raise RuntimeError("PACK tests did not succeed")
    if test_response["result"]["summary"] != {"passed": 1}:
        raise RuntimeError("PACK tests did not run the smoke contract exactly once")
    for run_response in (first_run, second_run):
        if run_response["status"] != "success":
            raise RuntimeError("Workflow Run did not succeed")
        if run_response["result"]["outputs"]["main"]["row_count"] != 1:
            raise RuntimeError("Workflow Run did not publish exactly one main row")
    first_result = first_run["result"]
    second_result = second_run["result"]
    if first_result["session_id"] != second_result["session_id"]:
        raise RuntimeError("Workflow Runs did not share one Analysis Session")
    if first_result["run_id"] == second_result["run_id"]:
        raise RuntimeError("Workflow Runs did not receive distinct Run IDs")
    if query_response["status"] != "success":
        raise RuntimeError("Run Output query did not succeed")
    result = query_response["result"]
    if result["format"] != "ndjson":
        raise RuntimeError("Run Output query did not return NDJSON")
    if Path(result["path"]).read_bytes() != EXPECTED_NDJSON:
        raise RuntimeError("Run Output query returned unexpected NDJSON content")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
