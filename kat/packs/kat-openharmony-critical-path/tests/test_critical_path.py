"""PACK 私有规则测试，统一通过 ``kat test --pack-dir`` 执行。"""

import sqlite3

import pytest

from kat.pack.datasources.trace_streamer import TraceStreamerSQLiteProvider
from kat.pack.helpers import critical_path
from kat.pack.helpers.critical_path import (
    CriticalPathError,
    TraceStreamerFacts,
    _Walker,
    locate_first_actual_frame,
)
from kat.pack.workflows.extract_critical_path import extract_critical_path_workflow
from kat.pack.workflows.locate_first_actual_frame import locate_first_actual_frame_workflow


def state(start, end, name, io_wait=None, blocked_function=None):
    return {"start": start, "end": end, "state": name, "io_wait": io_wait, "blocked_function": blocked_function}


class FakeFacts:
    def __init__(self, *, states, wakers=None, sched=None, callstacks=None, metadata=None):
        self._states = states
        self._wakers = wakers or {}
        self._sched = sched or {}
        self._callstacks = callstacks or {}
        self._metadata = metadata or {}

    def first_frame(self, process_name):
        return {"frame_id": 1, "itid": 1, "ipid": 10, "ts": 100, "dur": 10, "pid": 1000, "process_name": process_name, "callstack_id": 1}

    def metadata(self, itid):
        return self._metadata.get(itid, {"itid": itid, "ipid": 10, "tid": itid, "thread_name": f"thread-{itid}", "pid": 1000, "process_name": ".demo"})

    def states(self, itid, start, end):
        return [row for row in self._states.get(itid, []) if row["start"] < end and row["end"] > start]

    def sched(self, itid, start, end):
        return self._sched.get(itid, [])

    def callstacks(self, itid, start, end):
        return self._callstacks.get(itid, [])

    def waker(self, itid, end):
        value = self._wakers.get((itid, end))
        if isinstance(value, Exception):
            raise value
        return value


def frame(frame_id, start, duration):
    return {
        "id": frame_id,
        "itid": frame_id,
        "ts": start,
        "dur": duration,
        "callstack_id": None,
        "ipid": 10,
        "type": 0,
    }


def trace_streamer_facts(tmp_path, frames, *, root_thread_ipid=10):
    database = tmp_path / "trace-streamer.db"
    connection = sqlite3.connect(database)
    try:
        connection.executescript("""
            CREATE TABLE process(ipid INTEGER NOT NULL, pid INTEGER NOT NULL, name TEXT NOT NULL);
            CREATE TABLE frame_slice(
                id INTEGER NOT NULL,
                itid INTEGER NOT NULL,
                ts INTEGER NOT NULL,
                dur INTEGER NOT NULL,
                callstack_id INTEGER,
                ipid INTEGER NOT NULL,
                type INTEGER NOT NULL
            );
            CREATE TABLE thread(
                itid INTEGER NOT NULL,
                ipid INTEGER NOT NULL,
                tid INTEGER NOT NULL,
                name TEXT NOT NULL
            );
        """)
        connection.executemany(
            "INSERT INTO process VALUES (?, ?, ?)",
            [(10, 1000, ".demo"), (20, 2000, ".other")],
        )
        connection.executemany(
            "INSERT INTO frame_slice VALUES (:id, :itid, :ts, :dur, :callstack_id, :ipid, :type)",
            frames,
        )
        connection.executemany(
            "INSERT INTO thread VALUES (?, ?, ?, ?)",
            [
                (itid, root_thread_ipid, itid, f"thread-{itid}")
                for itid in sorted({item["itid"] for item in frames})
            ],
        )
        connection.commit()
    finally:
        connection.close()
    return TraceStreamerFacts(
        TraceStreamerSQLiteProvider(sqlite_path=str(database.resolve()))
    )


def run(facts, **kwargs):
    return _Walker(facts, 1, 100, 110, kwargs.get("max_depth", 8), kwargs.get("minimum", 0)).run()


@pytest.mark.parametrize(("frames", "expected_frame_id"), [
    ([frame(1, 100, 100), frame(2, 150, 10)], 2),
    ([frame(1, 150, 50), frame(2, 100, 100)], 2),
    ([frame(2, 100, 100), frame(1, 100, 100)], 1),
])
def test_first_frame_selects_earliest_completion_with_stable_ties(tmp_path, frames, expected_frame_id):
    selected = trace_streamer_facts(tmp_path, frames).first_frame(".demo")

    assert selected["frame_id"] == expected_frame_id


def test_frame_root_thread_must_belong_to_the_frame_process(tmp_path):
    facts = trace_streamer_facts(tmp_path, [frame(1, 100, 10)], root_thread_ipid=20)

    with pytest.raises(CriticalPathError, match="frame process"):
        locate_first_actual_frame(facts, ".demo")


def test_negative_clock_values_fail_closed(tmp_path):
    with pytest.raises(CriticalPathError, match="non-negative"):
        locate_first_actual_frame(trace_streamer_facts(tmp_path, [frame(1, -1, 10)]), ".demo")

    with pytest.raises(ValueError, match="non-negative"):
        critical_path.extract_critical_path(FakeFacts(states={}), 1, -1, 10)


def test_wakeup_recurses_and_keeps_parent_before_child():
    facts = FakeFacts(states={1: [state(100, 110, "D")], 2: [state(100, 110, "Running")]}, wakers={(1, 110): 2}, sched={2: [{"ts": 100, "dur": 10, "cpu": 1, "priority": 120}]})
    segments, _ = run(facts)
    rows = segments.to_pylist()
    assert [(row["segment_id"], row["parent_segment_id"], row["depth"], row["relation_to_parent"]) for row in rows] == [(0, None, 0, "root"), (1, 0, 1, "wakeup")]


def test_upstream_states_form_one_path_with_only_the_boundary_segment_as_waker():
    facts = FakeFacts(
        states={
            1: [state(100, 110, "D")],
            2: [state(100, 105, "D"), state(105, 110, "Running")],
            3: [state(100, 105, "Running")],
        },
        wakers={(1, 110): 2, (2, 105): 3},
        sched={
            2: [{"ts": 105, "dur": 5, "cpu": 1, "priority": 120}],
            3: [{"ts": 100, "dur": 5, "cpu": 2, "priority": 110}],
        },
    )

    segments, _ = run(facts)

    assert [
        (
            row["itid"],
            row["start_ts"],
            row["end_ts"],
            row["parent_segment_id"],
            row["depth"],
            row["relation_to_parent"],
        )
        for row in segments.to_pylist()
    ] == [
        (1, 100, 110, None, 0, "root"),
        (2, 105, 110, 0, 1, "wakeup"),
        (2, 100, 105, 1, 1, "same_thread"),
        (3, 100, 105, 2, 2, "wakeup"),
    ]


def test_runnable_is_a_scheduler_wait_not_a_wakeup_dependency():
    segments, _ = run(FakeFacts(states={1: [state(100, 110, "R")]}))
    row = segments.to_pylist()[0]
    assert (row["segment_kind"], row["termination_reason"]) == ("scheduling_wait", "scheduling_wait")


def test_missing_wakeup_is_a_path_termination():
    segments, _ = run(FakeFacts(states={1: [state(100, 110, "D")]}))
    assert segments.to_pylist()[0]["termination_reason"] == "missing_wakeup"


def test_callstack_boundaries_do_not_split_scheduling_segments():
    facts = FakeFacts(
        states={1: [state(100, 110, "Running")]},
        sched={1: [{"ts": 100, "dur": 10, "cpu": 2, "priority": 110}]},
        callstacks={1: [
            {"id": 7, "parent_id": None, "depth": 0, "ts": 100, "dur": 5, "function_name": "business"},
            {"id": 8, "parent_id": 7, "depth": 1, "ts": 105, "dur": 5, "function_name": "H:FFRTTask"},
        ]},
    )
    segments, evidence = run(facts)
    assert [(row["start_ts"], row["end_ts"], row["cpu"]) for row in segments.to_pylist()] == [(100, 110, 2)]
    assert [(row["segment_id"], row["callstack_id"], row["business_category"]) for row in evidence.to_pylist()] == [(0, 7, "application"), (0, 8, "runtime")]


def test_blocked_segment_keeps_available_callstack_evidence():
    facts = FakeFacts(
        states={1: [state(100, 110, "D")]},
        callstacks={1: [
            {"id": 9, "parent_id": None, "depth": 0, "ts": 100, "dur": 10, "function_name": "wait_for_reply"},
        ]},
    )

    segments, evidence = run(facts)

    assert segments.to_pylist()[0]["uncertainty_reason"] is None
    assert [(row["segment_id"], row["callstack_id"], row["function_name"]) for row in evidence.to_pylist()] == [
        (0, 9, "wait_for_reply"),
    ]


def test_partial_callstack_coverage_is_explicitly_uncertain():
    facts = FakeFacts(
        states={1: [state(100, 110, "Running")]},
        sched={1: [{"ts": 100, "dur": 10, "cpu": 2, "priority": 110}]},
        callstacks={1: [
            {"id": 10, "parent_id": None, "depth": 0, "ts": 100, "dur": 5, "function_name": "partial"},
        ]},
    )

    segments, evidence = run(facts)

    assert segments.to_pylist()[0]["uncertainty_reason"] == "incomplete_callstack_coverage"
    assert evidence.to_pylist()[0]["duration_ns"] == 5


def test_conflicting_scheduler_coverage_fails_instead_of_looking_missing():
    facts = FakeFacts(
        states={1: [state(100, 110, "Running")]},
        sched={1: [
            {"ts": 100, "dur": 10, "cpu": 1, "priority": 110},
            {"ts": 100, "dur": 10, "cpu": 2, "priority": 120},
        ]},
    )

    with pytest.raises(CriticalPathError, match="conflicting sched slices"):
        run(facts)


def test_partial_root_state_coverage_marks_real_segments_uncertain():
    facts = FakeFacts(
        states={1: [state(100, 105, "Running")]},
        sched={1: [{"ts": 100, "dur": 5, "cpu": 2, "priority": 110}]},
        callstacks={1: [
            {"id": 11, "parent_id": None, "depth": 0, "ts": 100, "dur": 5, "function_name": "partial_state"},
        ]},
    )

    segments, _ = run(facts)

    assert segments.to_pylist()[0]["uncertainty_reason"] == "incomplete_thread_state_coverage"


def test_missing_root_state_coverage_fails_without_synthetic_segments():
    with pytest.raises(CriticalPathError, match="root itid 1 has no state coverage"):
        run(FakeFacts(states={}))


def test_overlapping_thread_states_fail_as_conflicting_fact_shape():
    with pytest.raises(CriticalPathError, match="overlapping thread states for itid 1"):
        run(FakeFacts(states={1: [
            state(100, 107, "D"),
            state(105, 110, "R"),
        ]}))


def test_unknown_thread_state_fails_as_unsupported_fact_shape():
    with pytest.raises(CriticalPathError, match="unsupported thread state 'Unknown'"):
        run(FakeFacts(states={1: [state(100, 110, "Unknown")]}))


def test_callstack_evidence_is_clipped_to_each_segment_and_thread():
    facts = FakeFacts(
        states={1: [state(100, 105, "D"), state(105, 110, "D")]},
        callstacks={
            1: [{"id": 15, "parent_id": None, "depth": 0, "ts": 95, "dur": 20, "function_name": "root"}],
            2: [{"id": 16, "parent_id": None, "depth": 0, "ts": 100, "dur": 10, "function_name": "other"}],
        },
    )

    _, evidence = run(facts)

    assert [
        (row["segment_id"], row["callstack_id"], row["start_ts"], row["end_ts"])
        for row in evidence.to_pylist()
    ] == [(0, 15, 100, 105), (1, 15, 105, 110)]


def test_io_worker_and_interrupt_use_adapter_categories_and_boundaries():
    facts = FakeFacts(
        states={
            1: [state(100, 105, "D"), state(105, 110, "D")],
            2: [state(100, 105, "Running")],
            3: [state(105, 110, "Running")],
        },
        wakers={(1, 105): 2, (1, 110): 3},
        metadata={
            1: {"itid": 1, "tid": 1, "thread_name": "ui", "pid": 1, "process_name": ".demo"},
            2: {"itid": 2, "tid": 2, "thread_name": "hmfs", "pid": 2, "process_name": "kernel"},
            3: {"itid": 3, "tid": 3, "thread_name": "udk-irq", "pid": 3, "process_name": "kernel"},
        },
        callstacks={2: [{"id": 2, "parent_id": None, "depth": 0, "ts": 100, "dur": 5, "function_name": "io"}], 3: [{"id": 3, "parent_id": None, "depth": 0, "ts": 105, "dur": 5, "function_name": "irq"}]},
    )
    segments, evidence = run(facts)
    rows = segments.to_pylist()
    assert rows[-1]["termination_reason"] == "interrupt_boundary"
    assert [row["business_category"] for row in evidence.to_pylist()] == ["io", "interrupt"]


def test_minimum_segment_stops_recursion_without_removing_the_observation():
    facts = FakeFacts(states={1: [state(100, 110, "D")]}, wakers={(1, 110): 2})
    segments, _ = run(facts, minimum=11)
    assert [(row["segment_id"], row["termination_reason"]) for row in segments.to_pylist()] == [(0, "min_segment_threshold")]


def test_depth_and_cycles_terminate_published_segments():
    depth_segments, _ = run(FakeFacts(states={1: [state(100, 110, "D")]}, wakers={(1, 110): 2}), max_depth=0)
    cycle_segments, _ = run(FakeFacts(states={1: [state(100, 110, "D")]}, wakers={(1, 110): 1}))
    assert depth_segments.to_pylist()[0]["termination_reason"] == "max_depth"
    assert cycle_segments.to_pylist()[-1]["termination_reason"] == "cycle"


def test_ambiguous_wakeup_fails_as_conflicting_fact_shape():
    with pytest.raises(CriticalPathError, match="two wakers"):
        run(FakeFacts(
            states={1: [state(100, 110, "D")]},
            wakers={(1, 110): CriticalPathError("two wakers")},
        ))


def test_missing_upstream_coverage_is_uncertain():
    missing, _ = run(FakeFacts(
        states={1: [state(100, 110, "D")]},
        wakers={(1, 110): 2},
        callstacks={1: [
            {"id": 12, "parent_id": None, "depth": 0, "ts": 100, "dur": 10, "function_name": "wait"},
        ]},
    ))
    assert missing.to_pylist()[0]["termination_reason"] is None
    assert missing.to_pylist()[0]["uncertainty_reason"] == "missing_upstream_thread_state_coverage"


def test_partial_upstream_state_coverage_marks_the_dependency_uncertain():
    facts = FakeFacts(
        states={1: [state(100, 110, "D")], 2: [state(100, 105, "Running")]},
        wakers={(1, 110): 2},
        sched={2: [{"ts": 100, "dur": 5, "cpu": 2, "priority": 110}]},
        callstacks={
            1: [{"id": 13, "parent_id": None, "depth": 0, "ts": 100, "dur": 10, "function_name": "wait"}],
            2: [{"id": 14, "parent_id": None, "depth": 0, "ts": 100, "dur": 5, "function_name": "wake"}],
        },
    )

    segments, _ = run(facts)

    assert segments.to_pylist()[0]["uncertainty_reason"] == "incomplete_upstream_thread_state_coverage"


def test_helpers_return_provider_tables_and_workflows_publish_provider_parameters():
    facts = FakeFacts(states={1: [state(100, 110, "Running")]})

    frame_window = locate_first_actual_frame(facts, ".demo").to_rows()[0]
    outputs = critical_path.extract_critical_path(
        facts,
        frame_window["root_itid"],
        frame_window["start_ts"],
        frame_window["end_ts"],
    )

    assert dict(extract_critical_path_workflow.__kat_workflow__.parameters) == {
        "sqlite_path": "Absolute path to a Trace Streamer SQLite database.",
        "root_itid": "Root thread internal ID from frame_window.root_itid.",
        "start_ts": "Window start from frame_window.start_ts in boottime nanoseconds.",
        "end_ts": "Window end from frame_window.end_ts in boottime nanoseconds.",
        "max_depth": "Maximum upstream wakeup depth.",
        "min_segment_ms": "Minimum duration before recursive tracing continues.",
    }
    assert set(outputs) == {"critical_path_segments", "critical_path_callstack_evidence"}
    assert outputs["critical_path_segments"].to_rows()[0]["uncertainty_reason"] == "missing_sched_coverage,missing_callstack_evidence"
    assert outputs["critical_path_callstack_evidence"].to_rows() == []
