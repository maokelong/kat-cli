#!/usr/bin/env python3
"""Verify Payload smoke responses and the exact persisted NDJSON result."""

from __future__ import annotations

import json
import sys
from pathlib import Path


EXPECTED_NDJSON = b'{"clock_domain":"boottime","clock_value":123456}\n'


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(
            "usage: verify_payload_smoke.py INSPECT_RESPONSE TEST_RESPONSE "
            "RUN_RESPONSE QUERY_RESPONSE",
            file=sys.stderr,
        )
        return 2
    inspect_response, test_response, run_response, query_response = (
        json.loads(Path(path).read_text(encoding="utf-8-sig")) for path in argv
    )
    if inspect_response["status"] != "success":
        raise RuntimeError("PACK inspection did not succeed")
    if test_response["status"] != "success":
        raise RuntimeError("PACK tests did not succeed")
    if test_response["result"]["summary"] != {"passed": 1}:
        raise RuntimeError("PACK tests did not run the smoke contract exactly once")
    if run_response["status"] != "success":
        raise RuntimeError("Workflow Run did not succeed")
    if run_response["result"]["outputs"]["main"]["row_count"] != 1:
        raise RuntimeError("Workflow Run did not publish exactly one main row")
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
