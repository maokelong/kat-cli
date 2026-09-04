from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass, field
import json
import logging
import math
from pathlib import Path
import re
import threading
from typing import BinaryIO, Iterator
import uuid

import kat

from kat._identifiers import valid_table_name


_CANONICAL_INTEGER = re.compile(r"(?:0|-[1-9][0-9]*|[1-9][0-9]*)\Z")
_MIN_INT64 = -(2**63)
_MAX_INT64 = 2**63 - 1
_MAX_CALL_ID = 2**64 - 1
_CHANNEL_FAILURE = "Nested Workflow control channel failed"
_LOGGER = logging.getLogger(__name__)

type TaggedScalar = dict[str, object]


@dataclass(frozen=True)
class _SuccessResponse:
    relations: dict[str, Path]


@dataclass(frozen=True)
class _FailureResponse:
    message: str


@dataclass(frozen=True)
class _TestSessionResponse:
    identifier: str


@dataclass(frozen=True)
class _TestRunResponse:
    identifier: str
    candidate_id: str
    candidate_path: Path
    datasource_root: Path
    scratch_root: Path


@dataclass(frozen=True)
class _CompleteResponse:
    pass


type _CallResponse = (
    _SuccessResponse
    | _FailureResponse
    | _TestSessionResponse
    | _TestRunResponse
    | _CompleteResponse
)


@dataclass
class _PendingCall:
    event: threading.Event = field(default_factory=threading.Event)
    response: _CallResponse | None = None


class _TestRunScope:
    def __init__(
        self,
        client: _NestedRunClient,
        response: _TestRunResponse,
    ) -> None:
        self._client = client
        self._identifier = response.identifier
        self.candidate_id = response.candidate_id
        self.candidate_path = response.candidate_path
        self.datasource_root = response.datasource_root
        self.scratch_root = response.scratch_root

    def run(
        self,
        pack_name: str,
        workflow_name: str,
        inputs: dict[str, object],
    ) -> dict[str, Path]:
        return self._client._run(
            pack_name,
            workflow_name,
            inputs,
            test_run_id=self._identifier,
        )


class _TestSessionScope:
    def __init__(self, client: _NestedRunClient, identifier: str) -> None:
        self._client = client
        self._identifier = identifier

    @contextmanager
    def workflow(
        self,
        pack_name: str,
        workflow_name: str,
    ) -> Iterator[_TestRunScope]:
        response = self._client._exchange(
            {
                "operation": "begin_test_run",
                "test_session_id": self._identifier,
                "pack_name": pack_name,
                "workflow_name": workflow_name,
            }
        )
        if isinstance(response, _FailureResponse):
            raise RuntimeError("PACK test Workflow scope is unavailable")
        if not isinstance(response, _TestRunResponse):
            self._client._invalid_response_kind()
        assert isinstance(response, _TestRunResponse)
        execution = _TestRunScope(self._client, response)
        try:
            yield execution
        except BaseException:
            self._close_run(execution._identifier, suppress=True)
            raise
        else:
            self._close_run(execution._identifier, suppress=False)

    def _close_run(self, identifier: str, *, suppress: bool) -> None:
        try:
            response = self._client._exchange(
                {
                    "operation": "end_test_run",
                    "test_run_id": identifier,
                }
            )
            if isinstance(response, _FailureResponse):
                raise RuntimeError("PACK test Workflow scope could not be closed")
            if not isinstance(response, _CompleteResponse):
                self._client._invalid_response_kind()
        except BaseException:
            if not suppress:
                raise
            _LOGGER.exception("PACK test Workflow scope cleanup failed")


class _NestedRunClient:
    """Thread-safe client for one Runtime's private nested-run JSONL channel."""

    def __init__(self, reader: BinaryIO, writer: BinaryIO) -> None:
        self._reader = reader
        self._writer = writer
        self._state_lock = threading.Lock()
        self._write_lock = threading.Lock()
        self._next_call_id = 0
        self._pending: dict[int, _PendingCall] = {}
        self._poisoned = False
        self._reader_thread: threading.Thread | None = None

    def run(
        self,
        pack_name: str,
        workflow_name: str,
        inputs: dict[str, object],
    ) -> dict[str, Path]:
        return self._run(pack_name, workflow_name, inputs, test_run_id=None)

    def _run(
        self,
        pack_name: str,
        workflow_name: str,
        inputs: dict[str, object],
        *,
        test_run_id: str | None,
    ) -> dict[str, Path]:
        if type(pack_name) is not str or not pack_name:
            raise kat.RunError("Nested Workflow PACK name is invalid")
        if type(workflow_name) is not str or not workflow_name:
            raise kat.RunError("Nested Workflow name is invalid")
        try:
            encoded_inputs = _encode_inputs(inputs)
        except (TypeError, ValueError):
            raise kat.RunError("Nested Workflow inputs are invalid") from None

        request: dict[str, object] = {
            "pack_name": pack_name,
            "workflow_name": workflow_name,
            "inputs": encoded_inputs,
        }
        if test_run_id is not None:
            request.update(
                operation="run_workflow",
                test_run_id=test_run_id,
            )
        response = self._exchange(request)

        if isinstance(response, _FailureResponse):
            raise kat.RunError(response.message)
        if not isinstance(response, _SuccessResponse):
            self._invalid_response_kind()
        assert isinstance(response, _SuccessResponse)
        return response.relations

    @contextmanager
    def test_session(self) -> Iterator[_TestSessionScope]:
        response = self._exchange({"operation": "begin_test_session"})
        if isinstance(response, _FailureResponse):
            raise RuntimeError("PACK test Session scope is unavailable")
        if not isinstance(response, _TestSessionResponse):
            self._invalid_response_kind()
        assert isinstance(response, _TestSessionResponse)
        session = _TestSessionScope(self, response.identifier)
        try:
            yield session
        except BaseException:
            self._close_test_session(session._identifier, suppress=True)
            raise
        else:
            self._close_test_session(session._identifier, suppress=False)

    def _close_test_session(self, identifier: str, *, suppress: bool) -> None:
        try:
            response = self._exchange(
                {
                    "operation": "end_test_session",
                    "test_session_id": identifier,
                }
            )
            if isinstance(response, _FailureResponse):
                raise RuntimeError("PACK test Session scope could not be closed")
            if not isinstance(response, _CompleteResponse):
                self._invalid_response_kind()
        except BaseException:
            if not suppress:
                raise
            _LOGGER.exception("PACK test Session scope cleanup failed")

    def _exchange(self, request: dict[str, object]) -> _CallResponse:
        call_id, pending = self._register_call()
        frame = {"call_id": call_id, **request}
        try:
            try:
                self._write_frame(frame)
            except (Exception, SystemExit) as error:
                self._poison(f"request write failed: {type(error).__name__}")
            else:
                self._ensure_reader()
            return self._await_response(pending)
        finally:
            with self._state_lock:
                self._pending.pop(call_id, None)

    def _invalid_response_kind(self) -> None:
        self._poison("response kind does not match its request")
        raise kat.RunError(_CHANNEL_FAILURE)

    def _register_call(self) -> tuple[int, _PendingCall]:
        with self._state_lock:
            if self._poisoned:
                raise kat.RunError(_CHANNEL_FAILURE)
            call_id = self._next_call_id
            if call_id > _MAX_CALL_ID:
                self._poisoned = True
                for pending in self._pending.values():
                    if pending.response is None:
                        pending.response = _FailureResponse(_CHANNEL_FAILURE)
                        pending.event.set()
                raise kat.RunError(_CHANNEL_FAILURE)
            self._next_call_id += 1
            pending = _PendingCall()
            self._pending[call_id] = pending
            return call_id, pending

    def close(self) -> bool:
        """Poison future calls and report whether the response reader is quiescent."""
        self._poison("Runtime channel closed", log=False)
        with self._state_lock:
            reader_thread = self._reader_thread
            return reader_thread is None or not reader_thread.is_alive()

    def _write_frame(self, value: object) -> None:
        payload = (
            json.dumps(
                value,
                ensure_ascii=False,
                separators=(",", ":"),
                allow_nan=False,
            ).encode("utf-8")
            + b"\n"
        )
        with self._write_lock:
            remaining = memoryview(payload)
            while remaining:
                written = self._writer.write(remaining)
                if type(written) is not int or written <= 0:
                    raise OSError("private Runtime channel write made no progress")
                remaining = remaining[written:]
            self._writer.flush()

    def _ensure_reader(self) -> None:
        with self._state_lock:
            if self._poisoned or self._reader_thread is not None:
                return
            reader_thread = threading.Thread(
                target=self._read_responses,
                name="kat-nested-run-responses",
                daemon=True,
            )
            self._reader_thread = reader_thread
        try:
            reader_thread.start()
        except (Exception, SystemExit) as error:
            with self._state_lock:
                if self._reader_thread is reader_thread:
                    self._reader_thread = None
            self._poison(f"response reader failed to start: {type(error).__name__}")

    def _read_responses(self) -> None:
        try:
            while True:
                with self._state_lock:
                    if self._poisoned:
                        return
                    if not any(
                        pending.response is None for pending in self._pending.values()
                    ):
                        # 必须与调用登记共用锁，否则退出 reader 的缝隙会遗留无人读取的调用。
                        self._reader_thread = None
                        return
                try:
                    line = self._reader.readline()
                    call_id, response = _decode_response(line)
                except (Exception, SystemExit) as error:
                    self._poison(f"response read failed: {type(error).__name__}")
                    return
                self._dispatch(call_id, response)
        finally:
            with self._state_lock:
                if self._reader_thread is threading.current_thread():
                    self._reader_thread = None

    def _await_response(self, pending: _PendingCall) -> _CallResponse:
        pending.event.wait()
        response = pending.response
        if response is None:
            raise kat.RunError(_CHANNEL_FAILURE)
        return response

    def _dispatch(self, call_id: int, response: _CallResponse) -> None:
        invalid = False
        with self._state_lock:
            pending = self._pending.get(call_id)
            if pending is None or pending.response is not None:
                invalid = True
            else:
                pending.response = response
                pending.event.set()
        if invalid:
            self._poison("response call_id is unknown or already completed")

    def _poison(self, detail: str, *, log: bool = True) -> None:
        with self._state_lock:
            if self._poisoned:
                return
            self._poisoned = True
            if log:
                _LOGGER.error("%s: %s", _CHANNEL_FAILURE, detail)
            for pending in self._pending.values():
                if pending.response is None:
                    pending.response = _FailureResponse(_CHANNEL_FAILURE)
                    pending.event.set()


def _decode_response(line: object) -> tuple[int, _CallResponse]:
    if type(line) is not bytes or not line or not line.endswith(b"\n"):
        raise ValueError("nested Workflow response must be one complete JSONL frame")
    value = json.loads(line, parse_constant=_reject_json_constant)
    if type(value) is not dict:
        raise TypeError("nested Workflow response must be an object")
    call_id = value.get("call_id")
    if type(call_id) is not int or not 0 <= call_id <= _MAX_CALL_ID:
        raise ValueError("nested Workflow response call_id is invalid")
    status = value.get("status")
    if status == "failure":
        if set(value) != {"call_id", "status", "message"}:
            raise ValueError("nested Workflow failure response fields are invalid")
        message = value["message"]
        if type(message) is not str or not message:
            raise TypeError("nested Workflow failure message is invalid")
        return call_id, _FailureResponse(message)
    if status != "success":
        raise ValueError("nested Workflow response status is invalid")
    if set(value) == {"call_id", "status", "test_session_id"}:
        identifier = value["test_session_id"]
        if type(identifier) is not str or not identifier:
            raise TypeError("PACK test Session capability is invalid")
        return call_id, _TestSessionResponse(identifier)
    if set(value) == {
        "call_id",
        "status",
        "test_run_id",
        "candidate_id",
        "candidate_path",
        "datasource_root",
        "scratch_root",
    }:
        return call_id, _decode_test_run_response(value)
    if set(value) == {"call_id", "status"}:
        return call_id, _CompleteResponse()
    if set(value) != {"call_id", "status", "relations"}:
        raise ValueError("nested Workflow success response fields are invalid")
    raw_relations = value["relations"]
    if type(raw_relations) is not list or not raw_relations:
        raise TypeError("nested Workflow relations must be a non-empty array")
    relations: dict[str, Path] = {}
    names: list[str] = []
    for raw_relation in raw_relations:
        if type(raw_relation) is not dict or set(raw_relation) != {"name", "path"}:
            raise ValueError("nested Workflow relation fields are invalid")
        name = raw_relation["name"]
        path = raw_relation["path"]
        if type(name) is not str or not valid_table_name(name):
            raise ValueError("nested Workflow relation name is invalid")
        if type(path) is not str or not path or not Path(path).is_absolute():
            raise ValueError("nested Workflow relation path is invalid")
        if name in relations:
            raise ValueError("nested Workflow relation names must be unique")
        names.append(name)
        relations[name] = Path(path)
    if names != sorted(names):
        raise ValueError("nested Workflow relations must be sorted by name")
    return call_id, _SuccessResponse(relations)


def _decode_test_run_response(value: dict[str, object]) -> _TestRunResponse:
    identifier = value["test_run_id"]
    candidate_id = value["candidate_id"]
    raw_paths = {
        name: value[name]
        for name in ("candidate_path", "datasource_root", "scratch_root")
    }
    if type(identifier) is not str or not identifier:
        raise TypeError("PACK test Workflow capability is invalid")
    if type(candidate_id) is not str or not _is_canonical_uuid7(candidate_id):
        raise TypeError("PACK test candidate identity is invalid")
    if any(
        type(path) is not str or not Path(path).is_absolute()
        for path in raw_paths.values()
    ):
        raise TypeError("PACK test capability paths are invalid")
    candidate_path = Path(raw_paths["candidate_path"])
    datasource_root = Path(raw_paths["datasource_root"])
    scratch_root = Path(raw_paths["scratch_root"])
    session_root = candidate_path.parent.parent
    if (
        candidate_path.name != candidate_id
        or candidate_path.parent.name != "runs"
        or datasource_root.name != "materializations"
        or datasource_root.parent != session_root
        or scratch_root.name != candidate_id
        or scratch_root.parent.name != "scratch"
        or scratch_root.parent.parent != session_root
    ):
        raise ValueError("PACK test capability paths do not match one Session")
    return _TestRunResponse(
        identifier=identifier,
        candidate_id=candidate_id,
        candidate_path=candidate_path,
        datasource_root=datasource_root,
        scratch_root=scratch_root,
    )


def _is_canonical_uuid7(value: str) -> bool:
    try:
        identity = uuid.UUID(value)
    except ValueError:
        return False
    return identity.version == 7 and str(identity) == value


def _reject_json_constant(value: str) -> object:
    raise ValueError(f"invalid JSON constant: {value}")


def _encode_inputs(inputs: dict[str, object]) -> dict[str, TaggedScalar]:
    if type(inputs) is not dict:
        raise TypeError("nested Workflow inputs must be a dict")
    encoded: dict[str, TaggedScalar] = {}
    for name, value in inputs.items():
        if type(name) is not str or not name:
            raise TypeError("nested Workflow input names must be non-empty strings")
        encoded[name] = _encode_scalar(value)
    return encoded


def _encode_scalar(value: object) -> TaggedScalar:
    if value is None:
        return {"type": "none"}
    if type(value) is kat.Duration:
        return {"type": "duration", "value": str(value)}
    if type(value) is kat.WallClockTimestamp:
        return {"type": "wall_clock_timestamp", "value": str(value)}
    if type(value) is str:
        return {"type": "string", "value": value}
    if type(value) is int:
        if not _MIN_INT64 <= value <= _MAX_INT64:
            raise ValueError("nested Workflow integer input is outside int64 range")
        return {"type": "int64", "value": str(value)}
    if type(value) is float:
        if not math.isfinite(value):
            raise ValueError("nested Workflow float input must be finite")
        return {"type": "float64", "value": value}
    if type(value) is bool:
        return {"type": "boolean", "value": value}
    raise TypeError("nested Workflow input has an unsupported type")


def _decode_inputs(value: object) -> dict[str, object]:
    if type(value) is not dict:
        raise TypeError("nested Workflow inputs must be an object")
    decoded: dict[str, object] = {}
    for name, tagged in value.items():
        if type(name) is not str or not name:
            raise TypeError("nested Workflow input names must be non-empty strings")
        decoded[name] = _decode_scalar(tagged)
    return decoded


def _decode_scalar(tagged: object) -> object:
    if type(tagged) is not dict:
        raise TypeError("nested Workflow input must be a tagged object")
    tag = tagged.get("type")
    if type(tag) is not str:
        raise TypeError("nested Workflow input tag must be a string")
    if tag == "none":
        if set(tagged) != {"type"}:
            raise ValueError("none input fields are invalid")
        return None
    if set(tagged) != {"type", "value"}:
        raise ValueError("tagged input fields are invalid")
    payload = tagged["value"]
    if tag == "string":
        if type(payload) is not str:
            raise TypeError("string input payload must be a string")
        return payload
    if tag == "int64":
        if type(payload) is not str or _CANONICAL_INTEGER.fullmatch(payload) is None:
            raise TypeError("int64 input payload must be a canonical decimal string")
        decoded = int(payload)
        if not _MIN_INT64 <= decoded <= _MAX_INT64:
            raise ValueError("int64 input payload is outside range")
        return decoded
    if tag == "float64":
        if type(payload) is not float or not math.isfinite(payload):
            raise TypeError("float64 input payload must be a finite JSON number")
        return payload
    if tag == "boolean":
        if type(payload) is not bool:
            raise TypeError("boolean input payload must be a boolean")
        return payload
    if tag == "duration":
        if type(payload) is not str:
            raise TypeError("duration input payload must be a string")
        return kat.Duration(payload)
    if tag == "wall_clock_timestamp":
        if type(payload) is not str:
            raise TypeError("wall-clock input payload must be a string")
        return kat.WallClockTimestamp(payload)
    raise ValueError("nested Workflow input tag is unsupported")
