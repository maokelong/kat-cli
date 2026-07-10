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


def state_row(itid, ts, dur, state):
    return {
        "itid": itid, "ts": ts, "dur": dur, "state": state, "cpu": 1,
        "arg_setid": None, "iowait": None, "blocked_caller": None,
    }


def dependency_facts(*, root_states, wakeups, waker_name, waker_states):
    return fake_facts(
        metadata={
            1: {"itid": 1, "tid": 10, "thread_name": "root", "pid": 100, "process_name": "app"},
            2: {"itid": 2, "tid": 20, "thread_name": waker_name, "pid": 100, "process_name": "app"},
        },
        states={1: root_states, 2: waker_states},
        wakeups={1: wakeups},
        sched={2: [{"itid": 2, "ts": 0, "dur": 400, "ts_end": 400,
                    "cpu": 1, "priority": 120, "end_state": "R"}]},
        callstacks={2: []},
    )


def run_compute(facts, *, root_itid, start_ts, end_ts, max_depth=8):
    compute = capability("compute", "extract_critical_path")
    request = compute.__globals__["CriticalPathRequest"](
        root_itid, start_ts, end_ts, max_depth=max_depth
    )
    return compute(facts.provider, request)


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


def test_compute_walks_backward_and_emits_forward_sequence_edges():
    facts = fake_facts(
        metadata={1: {"itid": 1, "tid": 10, "thread_name": "root", "pid": 100, "process_name": "app"}},
        states={1: [
            {"itid": 1, "ts": 0, "dur": 200, "state": "Running", "cpu": 1,
             "arg_setid": None, "iowait": None, "blocked_caller": None},
            {"itid": 1, "ts": 200, "dur": 300, "state": "R", "cpu": 1,
             "arg_setid": None, "iowait": None, "blocked_caller": None},
        ]},
        sched={1: [{"itid": 1, "ts": 0, "dur": 200, "ts_end": 200,
                    "cpu": 1, "priority": 120, "end_state": "R"}]},
        callstacks={1: [
            {"itid": 1, "ts": 0, "dur": 200, "name": "root_stack"},
            {"itid": 1, "ts": 50, "dur": 20, "name": "short_stack"},
        ]},
    )
    compute = capability("compute", "extract_critical_path")
    request = compute.__globals__["CriticalPathRequest"](1, 0, 500)

    first = compute(facts.provider, request)
    second = compute(facts.provider, request)
    nodes = rows(first.nodes)
    edges = rows(first.edges)

    assert [(n["state"], n["classification"]) for n in nodes] == [
        ("R", "scheduler_wait"), ("Running", "self_running")
    ]
    assert next(n for n in nodes if n["state"] == "Running")["callstack_name"] == "root_stack"
    assert edges == [{
        "edge_id": 1,
        "from_node_id": 2,
        "to_node_id": 1,
        "from_itid": 1,
        "to_itid": 1,
        "parent_depth": 0,
        "child_depth": 0,
        "wakeup_ts": None,
        "edge_type": "sequence",
        "confidence": "fact",
        "reason": "thread_state_order",
    }]
    assert rows(second.nodes) == nodes


def test_wait_to_runnable_recurses_into_unique_waker():
    facts = dependency_facts(
        root_states=[state_row(1, 0, 400, "S"), state_row(1, 400, 100, "R")],
        wakeups=[{"wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"}],
        waker_name="worker",
        waker_states=[state_row(2, 0, 400, "Running")],
    )
    result = run_compute(facts, root_itid=1, start_ts=0, end_ts=500, max_depth=8)
    nodes = rows(result.nodes)
    edges = rows(result.edges)

    wait = next(node for node in nodes if node["itid"] == 1 and node["state"] == "S")
    worker = next(node for node in nodes if node["itid"] == 2)
    assert wait["classification"] == "waiting_for_waker"
    assert any(edge["edge_type"] == "wakeup"
               and edge["from_node_id"] == worker["node_id"]
               and edge["to_node_id"] == wait["node_id"]
               and edge["wakeup_ts"] == 400 for edge in edges)


def test_missing_and_ambiguous_wakers_do_not_create_edges():
    for wakeups, uncertainty in [
        ([], "missing_waker"),
        ([
            {"wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"},
            {"wakeup_ts": 400, "target_itid": 1, "waker_itid": 3, "name": "sched_wakeup"},
        ], "ambiguous_waker"),
    ]:
        facts = dependency_facts(
            root_states=[state_row(1, 0, 400, "S"), state_row(1, 400, 100, "R")],
            wakeups=wakeups,
            waker_name="worker",
            waker_states=[state_row(2, 0, 400, "Running")],
        )
        result = run_compute(facts, root_itid=1, start_ts=0, end_ts=500)
        wait = next(node for node in rows(result.nodes) if node["state"] == "S")
        assert wait["uncertainty"] == uncertainty
        assert not any(edge["edge_type"] == "wakeup" for edge in rows(result.edges))


@pytest.mark.parametrize(
    ("max_depth", "waker_name", "reason"),
    [(0, "worker", "max_depth"), (8, "udk-irq", "udk_irq")],
)
def test_depth_and_irq_create_terminal_child_without_querying_child(max_depth, waker_name, reason):
    facts = dependency_facts(
        root_states=[state_row(1, 0, 400, "S"), state_row(1, 400, 100, "R")],
        wakeups=[{"wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"}],
        waker_name=waker_name,
        waker_states=[state_row(2, 0, 400, "Running")],
    )
    result = run_compute(facts, root_itid=1, start_ts=0, end_ts=500, max_depth=max_depth)
    assert any(node["termination_reason"] == reason for node in rows(result.nodes))
    assert not any(call[0] == "thread_state_segments" and call[1][0] == 2 for call in facts.calls)


def test_repeated_wakeup_key_is_reported_as_cycle():
    compute = capability("compute", "extract_critical_path")
    follow = compute.__globals__["_follow_waker"]
    state_type = compute.__globals__["TraversalState"]
    state = state_type(visited_wakeups={(1, 2, 400)})
    result = follow(
        state=state, waiter_itid=1, waker_itid=2, wakeup_ts=400,
        waiting_node_id=1, frame_depth=0, max_depth=8, waker_name="worker",
        blocking_context_node_id=None, inherited_blocked_caller=None, start_ts=0,
    )
    assert result is None
    assert state.nodes[-1].termination_reason == "cycle_detected"


@pytest.mark.parametrize(("state_name", "iowait", "blocked_caller", "classification", "uncertainty"), [
    ("D-IO", None, None, "io_block", None),
    ("D", 1, None, "io_block", None),
    ("D-NIO", 0, "mutex_wait", "non_io_block", None),
    ("D", 0, None, "unknown", "unsupported_state"),
    ("S", None, None, "unknown", "unsupported_state"),
])
def test_compute_classifies_base_blocked_states(
    state_name, iowait, blocked_caller, classification, uncertainty
):
    facts = fake_facts(states={1: [{
        "itid": 1, "ts": 0, "dur": 10, "state": state_name, "cpu": 1,
        "arg_setid": None, "iowait": iowait, "blocked_caller": blocked_caller,
    }]})
    compute = capability("compute", "extract_critical_path")
    request = compute.__globals__["CriticalPathRequest"](1, 0, 10)

    node = rows(compute(facts.provider, request).nodes)[0]

    assert node["classification"] == classification
    assert node["uncertainty"] == uncertainty


def test_running_without_sched_evidence_is_unknown():
    facts = fake_facts(states={1: [{
        "itid": 1, "ts": 0, "dur": 10, "state": "Running", "cpu": 1,
        "arg_setid": None, "iowait": None, "blocked_caller": None,
    }]})
    compute = capability("compute", "extract_critical_path")
    request = compute.__globals__["CriticalPathRequest"](1, 0, 10)

    node = rows(compute(facts.provider, request).nodes)[0]

    assert node["classification"] == "unknown"
    assert node["uncertainty"] == "missing_sched_evidence"


def dataframe_with_schema(rows, schema):
    return SessionContext().from_arrow(pa.Table.from_pylist(rows, schema=schema))


@pytest.mark.parametrize(("fact_name", "replacement", "expected_detail"), [
    (
        "thread_metadata",
        lambda *_: dataframe_with_schema(
            [{"itid": 1, "tid": 10, "thread_name": "root", "pid": 100}],
            pa.schema([("itid", pa.int64()), ("tid", pa.int64()),
                       ("thread_name", pa.string()), ("pid", pa.int64())]),
        ),
        "missing required columns: process_name",
    ),
    (
        "thread_state_segments",
        lambda *_: dataframe_with_schema(
            [{"itid": 1, "ts": 0, "state": "Running", "cpu": 1,
              "arg_setid": None, "iowait": None, "blocked_caller": None}],
            pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("state", pa.string()),
                       ("cpu", pa.int64()), ("arg_setid", pa.int64()), ("iowait", pa.int64()),
                       ("blocked_caller", pa.string())]),
        ),
        "missing required columns: dur",
    ),
    (
        "thread_state_segments",
        lambda *_: dataframe_with_schema(
            [{"itid": 1, "ts": 0, "dur": "10", "state": "Running", "cpu": 1,
              "arg_setid": None, "iowait": None, "blocked_caller": None}],
            pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.string()),
                       ("state", pa.string()), ("cpu", pa.int64()), ("arg_setid", pa.int64()),
                       ("iowait", pa.int64()), ("blocked_caller", pa.string())]),
        ),
        "incompatible column type: dur",
    ),
    (
        "sched_slices",
        lambda *_: dataframe_with_schema(
            [{"itid": 1, "ts": 0, "dur": 10, "ts_end": 10, "cpu": 1, "end_state": "R"}],
            pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64()),
                       ("ts_end", pa.int64()), ("cpu", pa.int64()), ("end_state", pa.string())]),
        ),
        "missing required columns: priority",
    ),
    (
        "callstack_slices",
        lambda *_: dataframe_with_schema(
            [{"itid": 1, "ts": 0, "dur": 10}],
            pa.schema([("itid", pa.int64()), ("ts", pa.int64()), ("dur", pa.int64())]),
        ),
        "missing required columns: name",
    ),
])
def test_compute_reports_fact_contract_errors_with_context(fact_name, replacement, expected_detail):
    facts = fake_facts(
        states={1: [{"itid": 1, "ts": 0, "dur": 10, "state": "Running", "cpu": 1,
                    "arg_setid": None, "iowait": None, "blocked_caller": None}]},
        sched={1: [{"itid": 1, "ts": 0, "dur": 10, "ts_end": 10,
                    "cpu": 1, "priority": 120, "end_state": "R"}]},
        callstacks={1: [{"itid": 1, "ts": 0, "dur": 10, "name": "stack"}]},
    )
    compute = capability("compute", "extract_critical_path")
    provider_type = compute.__globals__["FactProvider"]
    exception_type = compute.__globals__["FactContractError"]
    provider_values = vars(facts.provider).copy()
    provider_values[fact_name] = replacement
    provider = provider_type(**provider_values)

    with pytest.raises(exception_type) as raised:
        compute(provider, compute.__globals__["CriticalPathRequest"](1, 0, 10))

    message = str(raised.value)
    assert fact_name in message
    assert "itid=1" in message
    assert "start_ts=0" in message
    assert "end_ts=10" in message
    assert expected_detail in message


def test_compute_preserves_fact_collection_error_as_cause():
    facts = fake_facts()
    compute = capability("compute", "extract_critical_path")
    provider_type = compute.__globals__["FactProvider"]
    exception_type = compute.__globals__["FactContractError"]

    def broken_metadata(_itid):
        raise OSError("fact backend unavailable")

    provider_values = vars(facts.provider).copy()
    provider_values["thread_metadata"] = broken_metadata

    with pytest.raises(exception_type) as raised:
        compute(
            provider_type(**provider_values),
            compute.__globals__["CriticalPathRequest"](1, 0, 10),
        )

    assert "capability=thread_metadata" in str(raised.value)
    assert "itid=1, start_ts=0, end_ts=10" in str(raised.value)
    assert isinstance(raised.value.__cause__, OSError)


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
