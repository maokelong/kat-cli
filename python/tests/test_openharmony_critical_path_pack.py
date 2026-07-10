import json
import os
import subprocess
import sys
from pathlib import Path

import pyarrow as pa
import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
PACK_ROOT = REPO_ROOT / "packs" / "openharmony-critical-path"
sys.path[:0] = [str(SDK_ROOT), str(RUNTIME_ROOT)]

from datafusion import SessionContext
from kat import Kat
from kat_runtime.pack_loader import load_pack_modules


def capability(kind: str, name: str):
    for module in load_pack_modules(PACK_ROOT):
        for value in vars(module).values():
            metadata = getattr(value, "__kat_capability__", None)
            if metadata and metadata["kind"] == kind and metadata["name"] == name:
                return value
    raise AssertionError(f"missing {kind}: {name}")


def rows(dataframe) -> list[dict]:
    result: list[dict] = []
    for batch in dataframe.collect():
        result.extend(batch.to_pylist())
    return result


METADATA_SCHEMA = pa.schema([
    ("itid", pa.int64()), ("tid", pa.int64()), ("thread_name", pa.string()),
    ("pid", pa.int64()), ("process_name", pa.string()),
])
STATE_SCHEMA = pa.schema([
    ("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()), ("state", pa.string()),
    ("cpu", pa.int64()), ("arg_setid", pa.int64()), ("iowait", pa.int64()),
    ("blocked_caller", pa.string()),
])
WAKEUP_SCHEMA = pa.schema([
    ("wakeup_ts", pa.int64()), ("target_itid", pa.int64()),
    ("waker_itid", pa.int64()), ("name", pa.string()),
])
SCHED_SCHEMA = pa.schema([
    ("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()), ("ts_end", pa.int64()),
    ("cpu", pa.int64()), ("priority", pa.int64()), ("end_state", pa.string()),
])
CALLSTACK_SCHEMA = pa.schema([
    ("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()), ("name", pa.string()),
])


class FakeFactBundle:
    def __init__(self, provider, calls: list[tuple[str, tuple]]):
        self.provider = provider
        self.calls = calls


def fake_facts(*, metadata=None, states=None, wakeups=None, sched=None, callstacks=None):
    metadata = metadata or {}
    states = states or {}
    wakeups = wakeups or {}
    sched = sched or {}
    callstacks = callstacks or {}
    calls: list[tuple[str, tuple]] = []
    compute = capability("compute", "extract_critical_path")
    provider_type = compute.__globals__["FactProvider"]

    def answer(name, mapping, schema):
        def callback(*args):
            calls.append((name, args))
            value = mapping.get(args[0], [])
            if isinstance(value, dict):
                value = [value]
            return SessionContext().from_arrow(pa.Table.from_pylist(value, schema=schema))
        return callback

    provider = provider_type(
        thread_metadata=answer("thread_metadata", metadata, METADATA_SCHEMA),
        thread_state_segments=answer("thread_state_segments", states, STATE_SCHEMA),
        wakeup_edges=answer("wakeup_edges", wakeups, WAKEUP_SCHEMA),
        sched_slices=answer("sched_slices", sched, SCHED_SCHEMA),
        callstack_slices=answer("callstack_slices", callstacks, CALLSTACK_SCHEMA),
    )
    return FakeFactBundle(provider, calls)


def test_compute_rejects_invalid_request_before_querying_facts():
    compute = capability("compute", "extract_critical_path")
    facts = fake_facts()
    request_type = compute.__globals__["CriticalPathRequest"]

    with pytest.raises(ValueError, match="start_ts must be less than end_ts"):
        compute(facts.provider, request_type(root_itid=1, start_ts=5, end_ts=5))
    with pytest.raises(ValueError, match="max_depth must be non-negative"):
        compute(facts.provider, request_type(root_itid=1, start_ts=0, end_ts=5, max_depth=-1))
    assert facts.calls == []


def test_missing_state_and_target_not_found_keep_typed_artifacts():
    compute = capability("compute", "extract_critical_path")
    request_type = compute.__globals__["CriticalPathRequest"]
    result = compute(fake_facts(states={}).provider, request_type(root_itid=1, start_ts=0, end_ts=10))
    assert rows(result.nodes)[0]["termination_reason"] == "missing_state"
    assert rows(result.edges) == []

    target_not_found = compute.__globals__["target_not_found_result"]()
    assert rows(target_not_found.nodes)[0]["termination_reason"] == "target_not_found"
    assert rows(target_not_found.edges) == []


def register(ctx: SessionContext, name: str, data: list[dict], schema: pa.Schema) -> None:
    ctx.from_arrow(pa.Table.from_pylist(data, schema=schema), name)


def test_openharmony_critical_path_pack_is_discoverable():
    result = subprocess.run(
        [sys.executable, "-m", "kat_runtime.worker.discovery", "--pack-root", str(PACK_ROOT)],
        env=os.environ | {"PYTHONPATH": os.pathsep.join([str(SDK_ROOT), str(RUNTIME_ROOT)])},
        text=True,
        capture_output=True,
        check=True,
    )
    manifest = json.loads(result.stdout)
    workflow_names = {item["name"] for item in manifest["workflows"]}
    assert {"critical_path", "wechat_first_frame_critical_path"} <= workflow_names
    assert "extract_critical_path" in {item["name"] for item in manifest["computes"]}


def test_thread_facts_filter_window_and_decode_blocking_args():
    ctx = SessionContext()
    register(ctx, "thread", [{"itid": 1, "tid": 10, "name": "root", "ipid": 100}],
             pa.schema([("itid", pa.int64()), ("tid", pa.int64()), ("name", pa.string()), ("ipid", pa.int64())]))
    register(ctx, "process", [{"ipid": 100, "pid": 1000, "name": "app"}],
             pa.schema([("ipid", pa.int64()), ("pid", pa.int64()), ("name", pa.string())]))
    register(ctx, "thread_state", [
        {"itid": 1, "ts": 100, "dur": 300, "state": "D-IO", "cpu": 3, "arg_setid": 7},
        {"itid": 1, "ts": 900, "dur": 50, "state": "Running", "cpu": 2, "arg_setid": None},
    ], pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()),
                  ("state", pa.string()), ("cpu", pa.int64()), ("arg_setid", pa.int64())]))
    register(ctx, "data_dict", [
        {"id": 1, "data": "iowait"}, {"id": 2, "data": "caller"}, {"id": 100, "data": "vfs_wait"},
    ], pa.schema([("id", pa.int64()), ("data", pa.string())]))
    register(ctx, "args", [
        {"key": 1, "datatype": 0, "value": 1, "argset": 7},
        {"key": 2, "datatype": 1, "value": 100, "argset": 7},
    ], pa.schema([("key", pa.int64()), ("datatype", pa.int64()), ("value", pa.int64()), ("argset", pa.int64())]))
    kat = Kat(ctx=ctx)

    assert rows(capability("fact", "thread_metadata")(kat, 1)) == [{
        "itid": 1, "tid": 10, "thread_name": "root", "pid": 1000, "process_name": "app"
    }]
    assert rows(capability("fact", "thread_state_segments")(kat, 1, 0, 500)) == [{
        "itid": 1, "ts": 100, "dur": 300, "state": "D-IO", "cpu": 3,
        "arg_setid": 7, "iowait": 1, "blocked_caller": "vfs_wait"
    }]


def test_scheduling_callstack_and_frame_facts_are_bounded():
    ctx = SessionContext()
    register_fact_tables_for_window_test(ctx)
    kat = Kat(ctx=ctx)

    assert rows(capability("fact", "wakeup_edges")(kat, 1, 100, 500)) == [{
        "wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"
    }]
    assert rows(capability("fact", "sched_slices")(kat, 1, 100, 500)) == [{
        "itid": 1, "ts": 200, "dur": 100, "ts_end": 300, "cpu": 4,
        "priority": 120, "end_state": "R",
    }]
    assert rows(capability("fact", "callstack_slices")(kat, 1, 100, 500)) == [{
        "itid": 1, "ts": 200, "dur": 100, "name": "root_stack",
    }]
    assert rows(capability("fact", "first_frame_window")(kat, ".tencent.wechat")) == [{
        "root_itid": 1, "start_ts": 200, "end_ts": 500
    }]


def register_fact_tables_for_window_test(ctx: SessionContext) -> None:
    register(ctx, "instant", [
        {"ts": 400, "name": "sched_wakeup", "ref": 1, "wakeup_from": 2, "ref_type": "itid"},
        {"ts": 900, "name": "sched_wakeup", "ref": 1, "wakeup_from": 3, "ref_type": "itid"},
    ], pa.schema([("ts", pa.int64()), ("name", pa.string()), ("ref", pa.int64()),
                  ("wakeup_from", pa.int64()), ("ref_type", pa.string())]))
    register(ctx, "sched_slice", [
        {"itid": 1, "ts": 200, "dur": 100, "ts_end": 300, "cpu": 4, "priority": 120, "end_state": "R"},
        {"itid": 1, "ts": 800, "dur": 50, "ts_end": 850, "cpu": 5, "priority": 120, "end_state": "S"},
    ], pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()),
                  ("ts_end", pa.int64()), ("cpu", pa.int64()), ("priority", pa.int64()),
                  ("end_state", pa.string())]))
    register(ctx, "callstack", [
        {"callid": 1, "ts": 200, "dur": 100, "name": "root_stack"},
        {"callid": 1, "ts": 800, "dur": 50, "name": "late_stack"},
    ], pa.schema([("callid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()), ("name", pa.string())]))
    register(ctx, "thread", [
        {"itid": 1, "tid": 10, "name": "main", "ipid": 100, "is_main_thread": 1},
        {"itid": 2, "tid": 20, "name": "worker", "ipid": 100, "is_main_thread": 0},
    ], pa.schema([("itid", pa.int64()), ("tid", pa.int64()), ("name", pa.string()),
                  ("ipid", pa.int64()), ("is_main_thread", pa.int64())]))
    register(ctx, "process", [{"ipid": 100, "pid": 1000, "name": ".tencent.wechat"}],
             pa.schema([("ipid", pa.int64()), ("pid", pa.int64()), ("name", pa.string())]))
    register(ctx, "frame_slice", [
        {"itid": 2, "ipid": 100, "ts": 100, "dur": 100, "type": 0},
        {"itid": 1, "ipid": 100, "ts": 200, "dur": 300, "type": 0},
    ], pa.schema([("itid", pa.int64()), ("ipid", pa.int64()), ("ts", pa.int64()),
                  ("dur", pa.int64()), ("type", pa.int64())]))
