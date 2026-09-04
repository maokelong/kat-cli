from __future__ import annotations

import concurrent.futures
import json
import math
import os
from pathlib import Path
import threading
import time
import unittest

import kat

from _kat_runtime.rpc import _NestedRunClient, _decode_inputs, _encode_inputs


class _ShortGuardedWriter:
    def __init__(self, raw: object) -> None:
        self._raw = raw
        self._guard = threading.Lock()
        self._active = 0
        self.maximum_active = 0

    def write(self, data: bytes | memoryview) -> int:
        with self._guard:
            self._active += 1
            self.maximum_active = max(self.maximum_active, self._active)
        try:
            time.sleep(0.001)
            return self._raw.write(bytes(data[:3]))  # type: ignore[attr-defined]
        finally:
            with self._guard:
                self._active -= 1

    def flush(self) -> None:
        self._raw.flush()  # type: ignore[attr-defined]


class _GuardedReader:
    def __init__(self, raw: object) -> None:
        self._raw = raw
        self._guard = threading.Lock()
        self._active = 0
        self.maximum_active = 0

    def readline(self) -> bytes:
        with self._guard:
            self._active += 1
            self.maximum_active = max(self.maximum_active, self._active)
        try:
            time.sleep(0.001)
            return self._raw.readline()  # type: ignore[attr-defined,no-any-return]
        finally:
            with self._guard:
                self._active -= 1


class _SignallingReader:
    def __init__(self, raw: object) -> None:
        self._raw = raw
        self.started = threading.Event()

    def readline(self) -> bytes:
        self.started.set()
        return self._raw.readline()  # type: ignore[attr-defined,no-any-return]


class _FailingSecondWriter:
    def __init__(self, raw: object) -> None:
        self._raw = raw
        self._writes = 0

    def write(self, data: bytes | memoryview) -> int:
        self._writes += 1
        if self._writes == 2:
            raise OSError("test write failure")
        return self._raw.write(data)  # type: ignore[attr-defined,no-any-return]

    def flush(self) -> None:
        self._raw.flush()  # type: ignore[attr-defined]


class NestedRunScalarProtocolTest(unittest.TestCase):
    def test_tagged_scalars_round_trip_without_json_type_coercion(self) -> None:
        values = {
            "text": "5",
            "minimum": -(2**63),
            "maximum": 2**63 - 1,
            "ratio": 1.25,
            "enabled": True,
            "window": kat.Duration("0.125ms"),
            "at": kat.WallClockTimestamp("2026-07-14T16:30:00+08:00"),
            "optional": None,
        }

        encoded = _encode_inputs(values)

        self.assertEqual(
            encoded,
            {
                "text": {"type": "string", "value": "5"},
                "minimum": {"type": "int64", "value": "-9223372036854775808"},
                "maximum": {"type": "int64", "value": "9223372036854775807"},
                "ratio": {"type": "float64", "value": 1.25},
                "enabled": {"type": "boolean", "value": True},
                "window": {"type": "duration", "value": "0.125ms"},
                "at": {
                    "type": "wall_clock_timestamp",
                    "value": "2026-07-14T08:30:00Z",
                },
                "optional": {"type": "none"},
            },
        )
        decoded = _decode_inputs(encoded)
        self.assertEqual(decoded, values)
        self.assertIs(type(decoded["minimum"]), int)
        self.assertIs(type(decoded["ratio"]), float)
        self.assertIs(type(decoded["enabled"]), bool)
        self.assertIs(type(decoded["window"]), kat.Duration)
        self.assertIs(type(decoded["at"]), kat.WallClockTimestamp)

    def test_tagged_scalars_reject_values_outside_the_closed_contract(self) -> None:
        class StringSubclass(str):
            pass

        class DurationSubclass(kat.Duration):
            pass

        class WallClockTimestampSubclass(kat.WallClockTimestamp):
            pass

        for value in (
            2**63,
            -(2**63) - 1,
            math.inf,
            -math.inf,
            math.nan,
            1.0j,
            b"bytes",
            ["value"],
            {"nested": "value"},
            StringSubclass("value"),
            DurationSubclass("5ms"),
            WallClockTimestampSubclass("2026-07-14T08:30:00Z"),
        ):
            with self.subTest(value=value), self.assertRaises((TypeError, ValueError)):
                _encode_inputs({"value": value})

    def test_tagged_scalar_decoder_requires_exact_tags_fields_and_payloads(self) -> None:
        invalid = (
            {"value": {"type": "none", "value": None}},
            {"value": {"type": "int64", "value": 1}},
            {"value": {"type": "int64", "value": "01"}},
            {"value": {"type": "int64", "value": "9223372036854775808"}},
            {"value": {"type": "float64", "value": 1}},
            {"value": {"type": "float64", "value": math.inf}},
            {"value": {"type": "boolean", "value": 1}},
            {"value": {"type": "duration", "value": "5"}},
            {
                "value": {
                    "type": "wall_clock_timestamp",
                    "value": "2026-07-14T08:30:00",
                }
            },
            {"value": {"type": "unknown", "value": "value"}},
        )
        for value in invalid:
            with self.subTest(value=value), self.assertRaises((TypeError, ValueError)):
                _decode_inputs(value)


class NestedRunClientTest(unittest.TestCase):
    def open_channel(self) -> tuple[object, object, object, object]:
        host_request_fd, client_request_fd = os.pipe()
        client_response_fd, host_response_fd = os.pipe()
        streams = (
            os.fdopen(client_response_fd, "rb", buffering=0),
            os.fdopen(client_request_fd, "wb", buffering=0),
            os.fdopen(host_request_fd, "rb", buffering=0),
            os.fdopen(host_response_fd, "wb", buffering=0),
        )
        for stream in streams:
            self.addCleanup(stream.close)
        return streams

    def test_concurrent_calls_accept_out_of_order_responses_with_one_reader_and_writer(
        self,
    ) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        guarded_reader = _GuardedReader(client_reader)
        guarded_writer = _ShortGuardedWriter(client_writer)
        client = _NestedRunClient(guarded_reader, guarded_writer)
        requests: list[dict[str, object]] = []

        def host() -> None:
            for _ in range(2):
                requests.append(json.loads(host_reader.readline()))  # type: ignore[attr-defined]
            for request in reversed(requests):
                workflow_name = request["workflow_name"]
                response = {
                    "call_id": request["call_id"],
                    "status": "success",
                    "relations": [
                        {
                            "name": "main",
                            "path": str(
                                (Path.cwd() / f"{workflow_name}.parquet").resolve()
                            ),
                        }
                    ],
                }
                host_writer.write(  # type: ignore[attr-defined]
                    json.dumps(response, separators=(",", ":")).encode("utf-8")
                    + b"\n"
                )
                host_writer.flush()  # type: ignore[attr-defined]

        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            host_future = pool.submit(host)
            first = pool.submit(client.run, "example", "first", {"value": 1})
            second = pool.submit(client.run, "example", "second", {"value": 2})
            first_relations = first.result(timeout=5)
            second_relations = second.result(timeout=5)
            host_future.result(timeout=5)

        self.assertEqual(
            first_relations,
            {"main": (Path.cwd() / "first.parquet").resolve()},
        )
        self.assertEqual(
            second_relations,
            {"main": (Path.cwd() / "second.parquet").resolve()},
        )
        self.assertEqual({request["call_id"] for request in requests}, {0, 1})
        self.assertEqual(
            {request["workflow_name"] for request in requests}, {"first", "second"}
        )
        for request in requests:
            self.assertEqual(
                set(request),
                {"call_id", "pack_name", "workflow_name", "inputs"},
            )
            self.assertEqual(request["pack_name"], "example")
            expected = 1 if request["workflow_name"] == "first" else 2
            self.assertEqual(
                request["inputs"],
                {"value": {"type": "int64", "value": str(expected)}},
            )
        self.assertEqual(guarded_reader.maximum_active, 1)
        self.assertEqual(guarded_writer.maximum_active, 1)

    def test_test_scope_and_parent_candidate_are_host_owned_capabilities(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        client = _NestedRunClient(client_reader, client_writer)
        root = (Path.cwd() / "test-session").resolve()
        candidate_id = "019f6e00-0000-7000-8000-000000000001"
        requests: list[dict[str, object]] = []

        def respond(response: dict[str, object]) -> None:
            host_writer.write(  # type: ignore[attr-defined]
                json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\n"
            )
            host_writer.flush()  # type: ignore[attr-defined]

        def host() -> None:
            begin_session = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            requests.append(begin_session)
            respond(
                {
                    "call_id": begin_session["call_id"],
                    "status": "success",
                    "test_session_id": "session-capability",
                }
            )
            begin_run = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            requests.append(begin_run)
            respond(
                {
                    "call_id": begin_run["call_id"],
                    "status": "success",
                    "test_run_id": "run-capability",
                    "candidate_id": candidate_id,
                    "candidate_path": str(root / "runs" / candidate_id),
                    "datasource_root": str(root / "materializations"),
                    "scratch_root": str(root / "scratch" / candidate_id),
                }
            )
            nested = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            requests.append(nested)
            respond(
                {
                    "call_id": nested["call_id"],
                    "status": "success",
                    "relations": [
                        {
                            "name": "main",
                            "path": str(root / "runs" / "child" / "outputs" / "main.parquet"),
                        }
                    ],
                }
            )
            end_run = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            requests.append(end_run)
            respond({"call_id": end_run["call_id"], "status": "success"})
            end_session = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            requests.append(end_session)
            respond({"call_id": end_session["call_id"], "status": "success"})

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            host_future = pool.submit(host)
            with client.test_session() as session:
                with session.workflow("exact-pack", "parent") as execution:
                    self.assertEqual(execution.candidate_id, candidate_id)
                    self.assertEqual(
                        execution.candidate_path,
                        root / "runs" / candidate_id,
                    )
                    self.assertEqual(
                        execution.datasource_root,
                        root / "materializations",
                    )
                    self.assertEqual(
                        execution.scratch_root,
                        root / "scratch" / candidate_id,
                    )
                    relations = execution.run("installed-pack", "child", {"value": 5})
            host_future.result(timeout=5)

        self.assertEqual(
            relations,
            {"main": root / "runs" / "child" / "outputs" / "main.parquet"},
        )
        self.assertEqual(
            requests,
            [
                {"call_id": 0, "operation": "begin_test_session"},
                {
                    "call_id": 1,
                    "operation": "begin_test_run",
                    "test_session_id": "session-capability",
                    "pack_name": "exact-pack",
                    "workflow_name": "parent",
                },
                {
                    "call_id": 2,
                    "operation": "run_workflow",
                    "test_run_id": "run-capability",
                    "pack_name": "installed-pack",
                    "workflow_name": "child",
                    "inputs": {"value": {"type": "int64", "value": "5"}},
                },
                {
                    "call_id": 3,
                    "operation": "end_test_run",
                    "test_run_id": "run-capability",
                },
                {
                    "call_id": 4,
                    "operation": "end_test_session",
                    "test_session_id": "session-capability",
                },
            ],
        )

    def test_invalid_response_poisons_all_pending_and_future_calls(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        client = _NestedRunClient(client_reader, client_writer)

        def host() -> None:
            host_reader.readline()  # type: ignore[attr-defined]
            host_reader.readline()  # type: ignore[attr-defined]
            host_writer.write(b'{"status":"success"}\n')  # type: ignore[attr-defined]
            host_writer.flush()  # type: ignore[attr-defined]

        with self.assertLogs("_kat_runtime.rpc", level="ERROR"):
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                host_future = pool.submit(host)
                calls = [
                    pool.submit(client.run, "example", workflow, {})
                    for workflow in ("first", "second")
                ]
                for call in calls:
                    with self.assertRaisesRegex(kat.RunError, "control channel"):
                        call.result(timeout=5)
                host_future.result(timeout=5)

        with self.assertRaisesRegex(kat.RunError, "control channel"):
            client.run("example", "later", {})

    def test_response_eof_wakes_all_pending_calls(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        client = _NestedRunClient(client_reader, client_writer)

        def host() -> None:
            host_reader.readline()  # type: ignore[attr-defined]
            host_reader.readline()  # type: ignore[attr-defined]
            host_writer.close()  # type: ignore[attr-defined]

        with self.assertLogs("_kat_runtime.rpc", level="ERROR"):
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                host_future = pool.submit(host)
                calls = [
                    pool.submit(client.run, "example", workflow, {})
                    for workflow in ("first", "second")
                ]
                for call in calls:
                    with self.assertRaisesRegex(kat.RunError, "control channel"):
                        call.result(timeout=5)
                host_future.result(timeout=5)

    def test_write_side_poison_wakes_a_call_blocked_reading_the_other_half(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        signalling_reader = _SignallingReader(client_reader)
        client = _NestedRunClient(
            signalling_reader,
            _FailingSecondWriter(client_writer),
        )

        with self.assertLogs("_kat_runtime.rpc", level="ERROR"):
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                first = pool.submit(client.run, "example", "first", {})
                host_reader.readline()  # type: ignore[attr-defined]
                self.assertTrue(signalling_reader.started.wait(timeout=5))
                second = pool.submit(client.run, "example", "second", {})
                try:
                    with self.assertRaisesRegex(kat.RunError, "control channel"):
                        second.result(timeout=5)
                    with self.assertRaisesRegex(kat.RunError, "control channel"):
                        first.result(timeout=5)
                finally:
                    host_writer.close()  # type: ignore[attr-defined]

    def test_call_id_exhaustion_wakes_an_already_pending_call(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        client = _NestedRunClient(client_reader, client_writer)
        client._next_call_id = 2**64 - 1

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            pending = pool.submit(client.run, "example", "pending", {})
            request = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            self.assertEqual(request["call_id"], 2**64 - 1)
            exhausted = pool.submit(client.run, "example", "exhausted", {})
            try:
                with self.assertRaisesRegex(kat.RunError, "control channel"):
                    exhausted.result(timeout=5)
                with self.assertRaisesRegex(kat.RunError, "control channel"):
                    pending.result(timeout=5)
            finally:
                host_writer.close()  # type: ignore[attr-defined]

    def test_host_failure_is_a_run_error_without_poisoning_the_channel(self) -> None:
        client_reader, client_writer, host_reader, host_writer = self.open_channel()
        client = _NestedRunClient(client_reader, client_writer)

        def host() -> None:
            first = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            host_writer.write(  # type: ignore[attr-defined]
                json.dumps(
                    {
                        "call_id": first["call_id"],
                        "status": "failure",
                        "message": "Nested Workflow failed",
                    },
                    separators=(",", ":"),
                ).encode("utf-8")
                + b"\n"
            )
            host_writer.flush()  # type: ignore[attr-defined]
            second = json.loads(host_reader.readline())  # type: ignore[attr-defined]
            host_writer.write(  # type: ignore[attr-defined]
                json.dumps(
                    {
                        "call_id": second["call_id"],
                        "status": "success",
                        "relations": [
                            {
                                "name": "main",
                                "path": str((Path.cwd() / "main.parquet").resolve()),
                            }
                        ],
                    },
                    separators=(",", ":"),
                ).encode("utf-8")
                + b"\n"
            )
            host_writer.flush()  # type: ignore[attr-defined]

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            host_future = pool.submit(host)
            with self.assertRaisesRegex(kat.RunError, "Nested Workflow failed"):
                client.run("example", "first", {})
            self.assertEqual(
                client.run("example", "second", {}),
                {"main": (Path.cwd() / "main.parquet").resolve()},
            )
            host_future.result(timeout=5)


if __name__ == "__main__":
    unittest.main()
