# kat-rs 通用关键路径示例 Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 OpenHarmony 示例 Pack 重构为通用关键路径 workflow，使用领域 facts 逐层取数，由 compute 独立维护图遍历、分类和终止状态。

**Architecture:** Workflow 只把绑定 `kat` 的 fact 函数组装成 `FactProvider` 并调用 `extract_critical_path`。Compute 通过 provider 按当前线程和窗口查询 DataFrame facts，在内存中维护 `TraversalState`，最后用独立的 DataFusion `SessionContext` 生成 `path_nodes` 和 `path_edges`。

**Tech Stack:** Python 3.12、datafusion-python 54、PyArrow、pytest、kat Python SDK decorators、kat Python runtime worker。

## Global Constraints

- 公共通用 workflow 名称为 `critical_path`，输入固定为 `root_itid/start_ts/end_ts/max_depth/min_segment_ms`。
- 微信首帧只保留为场景 workflow `wechat_first_frame_critical_path`；`app_name` 和 frame 语义不得进入通用 compute。
- 所有 SQL、表名和 `args/data_dict` 解码只允许出现在 `facts/`。
- 遍历循环、frontier、visited edges、状态分类、IO/IRQ 规则、阻塞上下文和终止条件只允许出现在 `compute/`。
- Workflow 不写 SQL、不维护循环、不解释线程状态。
- Workflow 对 runtime 的返回值保持 `dict[str, DataFrame]`，且只包含 `path_nodes` 与 `path_edges`。
- 事实查询必须限定当前 `itid` 和当前时间窗口；不预加载整个 trace，不新增缓存或 registry。
- 所有 wakeup edge 必须来自实际 `instant` 事实；歧义或缺失时不生成猜测边。
- 不修改 Rust CLI、Python runtime、dataset catalog、artifact runtime 或其他 Pack。
- 保留工作树中与本计划无关的用户删除项和未跟踪文件；每次只暂存任务列出的文件。

---

## File Map

**Create**

- `packs/openharmony-critical-path/facts/threads.py`：线程、进程、状态和阻塞参数事实。
- `packs/openharmony-critical-path/facts/scheduling.py`：wakeup 与 sched slice 事实。
- `packs/openharmony-critical-path/facts/callstacks.py`：调用栈覆盖事实。
- `packs/openharmony-critical-path/facts/frames.py`：微信首帧场景锚点事实。
- `packs/openharmony-critical-path/compute/models.py`：FactProvider、遍历状态和输出行模型。
- `packs/openharmony-critical-path/workflows/critical_path.py`：通用 workflow。
- `python/tests/test_openharmony_critical_path_pack.py`：示例 Pack 的 fact、compute、workflow 和 worker 合同测试。

**Replace**

- `packs/openharmony-critical-path/compute/critical_path.py`：删除递归 SQL，改为 provider 驱动的逐层计算。
- `packs/openharmony-critical-path/workflows/first_frame.py`：收窄为场景 fact + 通用 compute。
- `packs/openharmony-critical-path/pack.py`：只导出两个 workflows 和 compute capability。

**Delete**

- `packs/openharmony-critical-path/facts/trace_streamer.py`。

**Modify**

- `python/tests/test_sdk_runtime_contract.py:258`：移除旧示例 Pack discovery/递归 SQL 测试及其私有 Parquet helper；通用 SDK/runtime 测试保留。

---

### Task 1: 建立领域 Fact 合同

**Files:**

- Create: `packs/openharmony-critical-path/facts/threads.py`
- Create: `packs/openharmony-critical-path/facts/scheduling.py`
- Create: `packs/openharmony-critical-path/facts/callstacks.py`
- Create: `packs/openharmony-critical-path/facts/frames.py`
- Create: `python/tests/test_openharmony_critical_path_pack.py`
- Delete: `packs/openharmony-critical-path/facts/trace_streamer.py`
- Modify: `python/tests/test_sdk_runtime_contract.py:258`

**Interfaces:**

- Produces: `thread_metadata(kat, itid)`、`thread_state_segments(kat, itid, start_ts, end_ts)`、`wakeup_edges(kat, target_itid, start_ts, end_ts)`、`sched_slices(kat, itid, start_ts, end_ts)`、`callstack_slices(kat, itid, start_ts, end_ts)`、`first_frame_window(kat, app_name)`。
- All functions return DataFusion `DataFrame` and are decorated with `@fact`.

- [ ] **Step 1: Move Pack-specific test support into the focused test file**

Create the following fixed test bootstrap and helpers:

```python
import json
import os
import subprocess
import sys
from pathlib import Path

import pyarrow as pa

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


def register(ctx: SessionContext, name: str, data: list[dict], schema: pa.Schema) -> None:
    ctx.from_arrow(pa.Table.from_pylist(data, schema=schema), name)
```

Delete `test_openharmony_critical_path_pack_is_discoverable`, `test_critical_path_clips_segments_and_keeps_edges_consistent` and `_read_parquet` from `python/tests/test_sdk_runtime_contract.py`.

- [ ] **Step 2: Write failing fact contract tests**

Add tests that register one matching row and one out-of-window row for each source. The state fixture must include `args` and `data_dict` rows mapping `iowait=1` and `caller=vfs_wait`:

```python
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
    assert rows(capability("fact", "sched_slices")(kat, 1, 100, 500))[0]["cpu"] == 4
    assert rows(capability("fact", "callstack_slices")(kat, 1, 100, 500))[0]["name"] == "root_stack"
    assert rows(capability("fact", "first_frame_window")(kat, ".tencent.wechat")) == [{
        "root_itid": 1, "start_ts": 200, "end_ts": 500
    }]
```

Implement the fixture helper with the exact rows below. The fact implementations must project the source names to the normalized names asserted in Step 2:

```python
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
```

- [ ] **Step 3: Run tests to verify the new capabilities are missing**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -q
```

Expected: FAIL because `threads.py`, `scheduling.py`, `callstacks.py` and `frames.py` do not exist and the named facts are not discoverable.

- [ ] **Step 4: Implement the minimal fact functions**

Use relative Pack imports and parameterized `kat.sql` calls. `thread_state_segments` must decode source storage but leave clipping and classification to compute:

```python
@fact(title="Thread state segments", description="Raw thread states and decoded blocking arguments")
def thread_state_segments(kat, itid: int, start_ts: int, end_ts: int):
    return kat.sql(
        """
        with decoded as (
          select
            a.argset,
            max(case when key_dict.data = 'iowait' then a.value end) as iowait,
            max(case when key_dict.data = 'caller' and a.datatype = 1 then value_dict.data end) as blocked_caller
          from args a
          join data_dict key_dict on key_dict.id = a.key
          left join data_dict value_dict on value_dict.id = a.value
          group by a.argset
        )
        select s.itid, s.ts, s.dur, s.state, s.cpu, s.arg_setid,
               decoded.iowait, decoded.blocked_caller
        from thread_state s
        left join decoded on decoded.argset = s.arg_setid
        where s.itid = :itid
          and s.ts < :end_ts
          and s.ts + s.dur > :start_ts
        order by s.ts, s.dur
        """,
        itid=itid,
        start_ts=start_ts,
        end_ts=end_ts,
    )
```

Implement the other facts with these exact filters:

- metadata: `thread.itid = :itid` and left join process;
- wakeups: `ref_type='itid'`, `name like 'sched_wakeup%'`, non-null `wakeup_from`, target and `ts between start_ts and end_ts`;
- sched/callstack: interval overlap with the requested window;
- first frame: current ordering by main thread first, then `frame_slice.ts`, with `type=0` and `dur>0`.

Delete `facts/trace_streamer.py` after both replacement facts exist.

- [ ] **Step 5: Run focused and existing Python contracts**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py python/tests/test_sdk_runtime_contract.py -q
```

Expected: all tests selected by the command PASS. No task is committed with a known failing test.

- [ ] **Step 6: Commit the fact boundary**

```powershell
git add packs/openharmony-critical-path/facts python/tests/test_openharmony_critical_path_pack.py python/tests/test_sdk_runtime_contract.py
git commit -m "feat: 建立关键路径领域事实合同"
```

---

### Task 2: 建立 Compute 模型、输入合同与输出 Schema

**Files:**

- Create: `packs/openharmony-critical-path/compute/models.py`
- Replace: `packs/openharmony-critical-path/compute/critical_path.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`

**Interfaces:**

- Consumes: Task 1 fact DataFrames.
- Produces: `CriticalPathRequest`, `FactProvider`, `TraversalFrame`, `TraversalState`, `PathNode`, `PathEdge`, `CriticalPathResult`, `extract_critical_path(facts, request)` and `target_not_found_result()`.

- [ ] **Step 1: Write failing model and validation tests**

```python
import pytest


def empty_frame(schema: pa.Schema):
    return SessionContext().from_arrow(pa.Table.from_pylist([], schema=schema))


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


def state_row(itid, ts, dur, state, *, iowait=None, blocked_caller=None):
    return {
        "itid": itid, "ts": ts, "dur": dur, "state": state, "cpu": 0,
        "arg_setid": None, "iowait": iowait, "blocked_caller": blocked_caller,
    }


def run_compute(facts, *, root_itid, start_ts, end_ts, max_depth=8, min_segment_ms=0.1):
    compute = capability("compute", "extract_critical_path")
    request_type = compute.__globals__["CriticalPathRequest"]
    return compute(
        facts.provider,
        request_type(root_itid, start_ts, end_ts, max_depth, min_segment_ms),
    )


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
    )


def single_state_facts(row):
    return fake_facts(
        metadata={1: {"itid": 1, "tid": 10, "thread_name": "root",
                      "pid": 100, "process_name": "app"}},
        states={1: [row]},
    )


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
```

Define `fake_facts` as a test dataclass-backed factory whose six callbacks append `(fact_name, arguments)` to `calls` and return typed DataFrames. Its default state result is empty.

- [ ] **Step 2: Run the tests to verify the models are absent**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "invalid_request or target_not_found" -q
```

Expected: FAIL because `extract_critical_path`, `CriticalPathRequest` and typed result builders do not exist.

- [ ] **Step 3: Implement exact compute contracts**

`compute/models.py` must define dataclasses with these fields:

```python
@dataclass(frozen=True)
class CriticalPathRequest:
    root_itid: int
    start_ts: int
    end_ts: int
    max_depth: int = 8
    min_segment_ms: float = 0.1


@dataclass(frozen=True)
class FactProvider:
    thread_metadata: Callable[[int], Any]
    thread_state_segments: Callable[[int, int, int], Any]
    wakeup_edges: Callable[[int, int, int], Any]
    sched_slices: Callable[[int, int, int], Any]
    callstack_slices: Callable[[int, int, int], Any]


@dataclass(frozen=True)
class TraversalFrame:
    itid: int
    start_ts: int
    end_ts: int
    depth: int
    wakeup_target_node_id: int | None = None
    blocking_context_node_id: int | None = None
    inherited_blocked_caller: str | None = None
    next_node_id: int | None = None


@dataclass
class TraversalState:
    frontier: list[TraversalFrame] = field(default_factory=list)
    visited_wakeups: set[tuple[int, int, int]] = field(default_factory=set)
    nodes: list[PathNode] = field(default_factory=list)
    edges: list[PathEdge] = field(default_factory=list)
    metadata: dict[int, dict[str, Any]] = field(default_factory=dict)
    next_node_id: int = 1
    next_edge_id: int = 1
```

Define `PathNode` and `PathEdge` with every field from SDD section 12, using nullable Python fields for nullable Arrow columns. `CriticalPathResult` contains `nodes: Any` and `edges: Any`.

Create fixed `pa.Schema` values `PATH_NODE_SCHEMA` and `PATH_EDGE_SCHEMA`. Build both DataFrames through:

```python
def _dataframe(rows: list[dict[str, Any]], schema: pa.Schema):
    ctx = SessionContext()
    return ctx.from_arrow(pa.Table.from_pylist(rows, schema=schema))
```

The minimal `extract_critical_path` validates all scalar inputs, requests only root metadata and states, and returns a `missing_state` terminal node when the state DataFrame is empty. Decorate it as:

```python
@compute(title="Critical path", description="Traverse normalized trace facts into a critical path")
def extract_critical_path(facts: FactProvider, request: CriticalPathRequest) -> CriticalPathResult:
    _validate_request(request)
    state = TraversalState(frontier=[
        TraversalFrame(request.root_itid, request.start_ts, request.end_ts, depth=0)
    ])
    _process_frontier(facts, request, state)
    return _result(state)
```

- [ ] **Step 4: Run model tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "invalid_request or target_not_found" -q
```

Expected: PASS.

- [ ] **Step 5: Commit the compute contract**

```powershell
git add packs/openharmony-critical-path/compute python/tests/test_openharmony_critical_path_pack.py
git commit -m "feat: 建立关键路径计算合同"
```

---

### Task 3: 实现状态遍历、证据选择与 Sequence Edges

**Files:**

- Modify: `packs/openharmony-critical-path/compute/critical_path.py`
- Modify: `packs/openharmony-critical-path/compute/models.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`

**Interfaces:**

- Consumes: `TraversalFrame` and normalized thread/sched/callstack facts.
- Produces: deterministic `PathNode` rows and `sequence` edges.

- [ ] **Step 1: Write failing Running/Runnable traversal test**

```python
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
```

- [ ] **Step 2: Run the state traversal test**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py::test_compute_walks_backward_and_emits_forward_sequence_edges -q
```

Expected: FAIL because the minimal compute only handles missing states.

- [ ] **Step 3: Implement one-segment-at-a-time frontier processing**

`_process_frontier` must pop a frame, collect and validate required columns, clip each state to the frame, select the latest segment, emit one node, then push a continuation frame ending at `segment_start_ts`. This keeps the loop in compute and makes each fact request bounded.

Use these exact decisions:

```python
if state_name == "Running":
    sched = _overlapping_sched(facts, frame.itid, segment_start, segment_end)
    stacks = _overlapping_callstacks(facts, frame.itid, segment_start, segment_end)
    classification = "self_running" if sched else "unknown"
    uncertainty = None if sched else "missing_sched_evidence"
elif state_name in {"R", "R+"}:
    classification = "scheduler_wait"
    uncertainty = None
else:
    classification = _blocked_classification(state_name, row)
    uncertainty = None if classification != "unknown" else "unsupported_state"
```

Select the callstack with greatest overlap duration, then earliest `ts`, then lexical `name`. When an older node is created with `frame.next_node_id`, create a sequence edge from the older node to that newer node. IDs increment only inside `TraversalState`.

- [ ] **Step 4: Run traversal and model tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "walks_backward or invalid_request or target_not_found" -q
```

Expected: PASS.

- [ ] **Step 5: Commit base traversal**

```powershell
git add packs/openharmony-critical-path/compute python/tests/test_openharmony_critical_path_pack.py
git commit -m "feat: 实现关键路径状态遍历"
```

---

### Task 4: 实现 Wakeup 递归与终止条件

**Files:**

- Modify: `packs/openharmony-critical-path/compute/critical_path.py`
- Modify: `packs/openharmony-critical-path/compute/models.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`

**Interfaces:**

- Consumes: normalized wakeup facts and adjacent waiting/Runnable segments.
- Produces: `wakeup` edges, child frames and stable termination reasons.

- [ ] **Step 1: Write failing dependency test**

```python
def test_wait_to_runnable_recurses_into_unique_waker():
    facts = dependency_facts(
        root_states=[
            state_row(1, 0, 400, "S"),
            state_row(1, 400, 100, "R"),
        ],
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
```

Add these explicit terminal tests:

```python
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
        state=state,
        waiter_itid=1,
        waker_itid=2,
        wakeup_ts=400,
        waiting_node_id=1,
        frame_depth=0,
        max_depth=8,
        waker_name="worker",
        blocking_context_node_id=None,
        inherited_blocked_caller=None,
        start_ts=0,
    )
    assert result is None
    assert state.nodes[-1].termination_reason == "cycle_detected"
```

- [ ] **Step 2: Run dependency tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "waker or max_depth or cycle or udk" -q
```

Expected: FAIL because no waiting-to-Runnable dependency handling exists.

- [ ] **Step 3: Implement deterministic waker selection**

When the latest selected segment is Runnable and the immediately previous clipped segment is `S` or starts with `D`:

1. emit both nodes and their sequence edge;
2. query `wakeup_edges(waiter_itid, wait_start, runnable_start)`;
3. keep candidates with `wakeup_ts <= runnable_start`;
4. select the greatest timestamp;
5. reject different wakers at that same timestamp as ambiguous;
6. add `(waiter_itid, waker_itid, wakeup_ts)` to `visited_wakeups` before pushing the child.

Push the same-thread continuation first and the child frame second so the LIFO frontier analyzes the dependency before older same-thread history. Set the child frame’s `wakeup_target_node_id` to the waiting node. On creation of the child’s first node, emit the wakeup edge.

Keep cycle/depth/IRQ branching in one helper with the signature exercised by the tests:

```python
def _follow_waker(
    *,
    state: TraversalState,
    waiter_itid: int,
    waker_itid: int,
    wakeup_ts: int,
    waiting_node_id: int,
    frame_depth: int,
    max_depth: int,
    waker_name: str | None,
    blocking_context_node_id: int | None,
    inherited_blocked_caller: str | None,
    start_ts: int,
) -> TraversalFrame | None:
    key = (waiter_itid, waker_itid, wakeup_ts)
    if key in state.visited_wakeups:
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id,
            wakeup_ts, frame_depth + 1, "cycle_detected", emit_edge=False,
        )
        return None
    if frame_depth >= max_depth:
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id,
            wakeup_ts, frame_depth + 1, "max_depth", emit_edge=True,
        )
        return None
    if waker_name == "udk-irq":
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id,
            wakeup_ts, frame_depth + 1, "udk_irq", emit_edge=True,
        )
        return None
    state.visited_wakeups.add(key)
    return TraversalFrame(
        itid=waker_itid,
        start_ts=start_ts,
        end_ts=wakeup_ts,
        depth=frame_depth + 1,
        wakeup_target_node_id=waiting_node_id,
        blocking_context_node_id=blocking_context_node_id,
        inherited_blocked_caller=inherited_blocked_caller,
    )
```

`_append_terminal_for_dependency` has the exact signature below. It appends a terminal child and, when `emit_edge` is true, a wakeup edge from that child to `waiting_node_id`:

```python
def _append_terminal_for_dependency(
    state: TraversalState,
    waiter_itid: int,
    waker_itid: int,
    waiting_node_id: int,
    wakeup_ts: int,
    depth: int,
    reason: str,
    *,
    emit_edge: bool,
) -> PathNode:
    node = _terminal_node(
        state,
        TraversalFrame(waker_itid, wakeup_ts, wakeup_ts, depth),
        reason,
        itid=waker_itid,
    )
    if emit_edge:
        _append_wakeup_edge(state, node, waiting_node_id, waiter_itid, wakeup_ts)
    return node
```

- [ ] **Step 4: Implement normal terminal nodes**

Use one helper:

```python
def _terminal_node(
    state: TraversalState,
    frame: TraversalFrame,
    reason: str,
    *,
    itid: int | None = None,
    thread_name: str | None = None,
    uncertainty: str | None = None,
) -> PathNode:
    return _append_node(
        state,
        frame,
        itid=itid,
        thread_name=thread_name,
        classification="unknown" if reason != "udk_irq" else "io_block",
        uncertainty=uncertainty or reason,
        termination_reason=reason,
    )
```

Use it for `missing_state`, `max_depth`, `cycle_detected` and `udk_irq`. Missing or ambiguous waker annotates the existing waiting node instead of inventing an edge.

- [ ] **Step 5: Run all compute tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "compute or waker or max_depth or cycle or udk" -q
```

Expected: PASS.

- [ ] **Step 6: Commit dependency traversal**

```powershell
git add packs/openharmony-critical-path/compute python/tests/test_openharmony_critical_path_pack.py
git commit -m "feat: 实现关键路径唤醒递归"
```

---

### Task 5: 实现 IO 分类、阻塞上下文与短片段规则

**Files:**

- Modify: `packs/openharmony-critical-path/compute/critical_path.py`
- Modify: `packs/openharmony-critical-path/compute/models.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`

**Interfaces:**

- Consumes: `iowait`, `blocked_caller` and waker thread metadata.
- Produces: `io_block`, `non_io_block`, inherited blocking fields and min-segment behavior.

- [ ] **Step 1: Write failing classification tests**

Add one parameterized test with these exact cases:

```python
@pytest.mark.parametrize(
    ("state_name", "iowait", "blocked_caller", "expected"),
    [
        ("D-IO", 1, "fscache_wait", "io_block"),
        ("D", 1, "fscache_wait", "io_block"),
        ("D-NIO", 0, "eventfd_read", "non_io_block"),
        ("S", None, None, "unknown"),
    ],
)
def test_block_classification_is_conservative(state_name, iowait, blocked_caller, expected):
    facts = single_state_facts(state_row(
        1, 0, 500, state_name, iowait=iowait, blocked_caller=blocked_caller
    ))
    node = rows(run_compute(facts, root_itid=1, start_ts=0, end_ts=500).nodes)[0]
    assert node["classification"] == expected
```

Add these concrete tests:

```python
@pytest.mark.parametrize(
    ("waker_name", "expected"),
    [("fsverity", "io_block"), ("hmfs_txn", "waiting_for_waker")],
)
def test_io_waker_name_promotes_only_included_threads(waker_name, expected):
    facts = dependency_facts(
        root_states=[state_row(1, 0, 400, "S"), state_row(1, 400, 100, "R")],
        wakeups=[{"wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"}],
        waker_name=waker_name,
        waker_states=[state_row(2, 0, 400, "Running")],
    )
    nodes = rows(run_compute(facts, root_itid=1, start_ts=0, end_ts=500).nodes)
    wait = next(node for node in nodes if node["itid"] == 1 and node["state"] == "S")
    assert wait["classification"] == expected


def test_blocking_context_is_inherited_without_overwriting_child_fact():
    facts = dependency_facts(
        root_states=[
            state_row(1, 0, 400, "D-NIO", blocked_caller="parent_wait"),
            state_row(1, 400, 100, "R"),
        ],
        wakeups=[{"wakeup_ts": 400, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"}],
        waker_name="worker",
        waker_states=[state_row(2, 0, 400, "D-NIO", blocked_caller="child_wait")],
    )
    nodes = rows(run_compute(facts, root_itid=1, start_ts=0, end_ts=500).nodes)
    parent = next(node for node in nodes if node["itid"] == 1 and node["state"] == "D-NIO")
    child = next(node for node in nodes if node["itid"] == 2)
    assert child["blocked_caller"] == "child_wait"
    assert child["inherited_blocked_caller"] == "parent_wait"
    assert child["blocking_context_node_id"] == parent["node_id"]


def test_short_wait_transition_is_kept_but_unrelated_noise_is_filtered():
    facts = dependency_facts(
        root_states=[
            state_row(1, 0, 50_000, "Running"),
            state_row(1, 50_000, 50_000, "S"),
            state_row(1, 100_000, 50_000, "R"),
        ],
        wakeups=[{"wakeup_ts": 100_000, "target_itid": 1, "waker_itid": 2, "name": "sched_wakeup"}],
        waker_name="worker",
        waker_states=[state_row(2, 50_000, 50_000, "Running")],
    )
    result = run_compute(
        facts, root_itid=1, start_ts=0, end_ts=150_000, min_segment_ms=0.1
    )
    nodes = rows(result.nodes)
    assert not any(node["itid"] == 1 and node["state"] == "Running" for node in nodes)
    assert any(node["itid"] == 1 and node["state"] == "S" for node in nodes)
    assert any(edge["edge_type"] == "wakeup" for edge in rows(result.edges))
```

- [ ] **Step 2: Run classification tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "classification or fsverity or hmfs_txn or blocking_context or short" -q
```

Expected: FAIL for IO-waker promotion, inherited context and short-segment retention.

- [ ] **Step 3: Implement exact IO rules**

Define immutable constants in compute:

```python
IO_THREAD_NAMES = frozenset({
    "fsverity", "cdecrypt", "erofs_unzipd", "fsignature", "hmfs",
    "wk:0/0/0", "wk:2/1/0", "wk:0/-20/0",
})
IO_THREAD_EXCLUSIONS = frozenset({"hmfs_txn"})
```

Use exact name equality. Apply classification precedence:

1. `D-IO` or `iowait == 1` -> `io_block`;
2. `D`/`D-NIO` with non-empty `blocked_caller` -> `non_io_block`;
3. waiting with unique waker -> `waiting_for_waker`;
4. otherwise -> `unknown`.

After loading waker metadata, promote the parent waiting node to `io_block` only when the name is in `IO_THREAD_NAMES` and not in `IO_THREAD_EXCLUSIONS`.

- [ ] **Step 4: Implement context propagation and min-segment retention**

When recursing from a D-class waiting node, copy its node ID and `blocked_caller` into the child frame. Every child node copies those values to `blocking_context_node_id` and `inherited_blocked_caller` without changing its own `blocked_caller`.

Compute `min_segment_ns = int(min_segment_ms * 1_000_000)`. A node shorter than the threshold may be omitted only if all are false:

- it is not part of a wait-to-Runnable pair;
- it is not a wakeup source or target;
- it has no `blocked_caller`;
- it has no uncertainty or termination reason.

- [ ] **Step 5: Run complete focused test suite**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -q
```

Expected: PASS.

- [ ] **Step 6: Commit classification rules**

```powershell
git add packs/openharmony-critical-path/compute python/tests/test_openharmony_critical_path_pack.py
git commit -m "feat: 完善关键路径阻塞分类"
```

---

### Task 6: 接入通用与微信 Workflows，并完成 Worker 验收

**Files:**

- Create: `packs/openharmony-critical-path/workflows/critical_path.py`
- Replace: `packs/openharmony-critical-path/workflows/first_frame.py`
- Modify: `packs/openharmony-critical-path/pack.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`
- Test: `crates/kat-rs-cli/tests/pack_run_contract.rs` only if the generic workflow serialization assertion is added; do not change production Rust.

**Interfaces:**

- Consumes: all facts and `extract_critical_path`.
- Produces: public workflows `critical_path` and `wechat_first_frame_critical_path` returning exactly two DataFrames.

- [ ] **Step 1: Write failing discovery and workflow tests**

```python
def test_pack_discovers_generic_boundaries():
    env = os.environ.copy()
    env["PYTHONPATH"] = f"{SDK_ROOT}{os.pathsep}{RUNTIME_ROOT}"
    result = subprocess.run(
        [sys.executable, "-m", "kat_runtime.worker.discovery", "--pack-root", str(PACK_ROOT)],
        env=env, text=True, capture_output=True, check=True,
    )
    manifest = json.loads(result.stdout)
    assert {item["name"] for item in manifest["workflows"]} == {
        "critical_path", "wechat_first_frame_critical_path"
    }
    assert {item["name"] for item in manifest["computes"]} == {"extract_critical_path"}
    assert {item["name"] for item in manifest["facts"]} == {
        "thread_metadata", "thread_state_segments", "wakeup_edges",
        "sched_slices", "callstack_slices", "first_frame_window",
    }
    assert not (PACK_ROOT / "facts" / "trace_streamer.py").exists()


def test_generic_workflow_materializes_only_nodes_and_edges(tmp_path):
    dataset_path = build_integration_dataset(tmp_path)
    run_dir = run_worker(
        tmp_path,
        dataset_path,
        "critical_path",
        {"root_itid": 1, "start_ts": 0, "end_ts": 500, "max_depth": 8, "min_segment_ms": 0.1},
    )
    manifest = json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["status"] == "success"
    assert {item["name"] for item in manifest["artifacts"]} == {"path_nodes", "path_edges"}
```

`build_integration_dataset` must register and write Parquet tables for `thread_state`, `thread`, `process`, `args`, `data_dict`, `instant`, `sched_slice`, `callstack` and `frame_slice`, then write a catalog listing those exact tables. `run_worker` must invoke `kat_runtime.worker.run` using the same environment pattern as existing runtime contract tests.

Use these exact helpers:

```python
def build_integration_dataset(tmp_path: Path) -> Path:
    dataset = tmp_path / "dataset"
    tables = dataset / "tables"
    tables.mkdir(parents=True)
    ctx = SessionContext()
    definitions = {
        "thread_state": (
            [
                state_row(1, 0, 400, "S"),
                state_row(1, 400, 100, "R"),
                state_row(2, 0, 400, "Running"),
            ],
            STATE_SCHEMA,
        ),
        "thread": (
            [
                {"itid": 1, "tid": 10, "name": "main", "ipid": 100, "is_main_thread": 1},
                {"itid": 2, "tid": 20, "name": "worker", "ipid": 100, "is_main_thread": 0},
            ],
            pa.schema([("itid", pa.int64()), ("tid", pa.int64()), ("name", pa.string()),
                       ("ipid", pa.int64()), ("is_main_thread", pa.int64())]),
        ),
        "process": (
            [{"ipid": 100, "pid": 1000, "name": ".tencent.wechat"}],
            pa.schema([("ipid", pa.int64()), ("pid", pa.int64()), ("name", pa.string())]),
        ),
        "args": (
            [],
            pa.schema([("key", pa.int64()), ("datatype", pa.int64()),
                       ("value", pa.int64()), ("argset", pa.int64())]),
        ),
        "data_dict": (
            [],
            pa.schema([("id", pa.int64()), ("data", pa.string())]),
        ),
        "instant": (
            [{"ts": 400, "name": "sched_wakeup", "ref": 1,
              "wakeup_from": 2, "ref_type": "itid"}],
            pa.schema([("ts", pa.int64()), ("name", pa.string()), ("ref", pa.int64()),
                       ("wakeup_from", pa.int64()), ("ref_type", pa.string())]),
        ),
        "sched_slice": (
            [{"itid": 2, "ts": 0, "dur": 400, "ts_end": 400, "cpu": 1,
              "priority": 120, "end_state": "R"}],
            SCHED_SCHEMA,
        ),
        "callstack": (
            [{"callid": 2, "ts": 0, "dur": 400, "name": "worker_stack"}],
            pa.schema([("callid", pa.int64()), ("ts", pa.int64()),
                       ("dur", pa.int64()), ("name", pa.string())]),
        ),
        "frame_slice": (
            [{"itid": 1, "ipid": 100, "ts": 0, "dur": 500, "type": 0}],
            pa.schema([("itid", pa.int64()), ("ipid", pa.int64()), ("ts", pa.int64()),
                       ("dur", pa.int64()), ("type", pa.int64())]),
        ),
    }
    catalog = []
    for name, (data, schema) in definitions.items():
        path = tables / f"{name}.parquet"
        ctx.from_arrow(pa.Table.from_pylist(data, schema=schema)).write_parquet(str(path))
        catalog.append({"name": name, "path": f"tables/{name}.parquet", "kind": "source"})
    (dataset / "catalog.json").write_text(json.dumps({"tables": catalog}), encoding="utf-8")
    return dataset


def run_worker(tmp_path: Path, dataset: Path, workflow: str, inputs: dict) -> Path:
    run_dir = tmp_path / f"run-{workflow}"
    request = {
        "packRoot": str(PACK_ROOT),
        "workflow": workflow,
        "datasetPath": str(dataset),
        "runDir": str(run_dir),
        "inputs": inputs,
    }
    request_path = tmp_path / f"request-{workflow}.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    env = os.environ.copy()
    env["PYTHONPATH"] = f"{SDK_ROOT}{os.pathsep}{RUNTIME_ROOT}"
    subprocess.run(
        [sys.executable, "-m", "kat_runtime.worker.run", "--request", str(request_path)],
        env=env, text=True, capture_output=True, check=True,
    )
    return run_dir
```

Add a scenario test where no frame matches and assert one nodes row with `termination_reason == "target_not_found"` and zero edges rows.

- [ ] **Step 2: Run workflow tests**

Run:

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "discovers_generic or workflow or target_not_found" -q
```

Expected: FAIL because the generic workflow does not exist and the old first-frame workflow contains SQL and calls the old compute.

- [ ] **Step 3: Implement the shared provider factory and generic workflow**

`workflows/critical_path.py`:

```python
from functools import partial

from kat import workflow

from ..compute.critical_path import extract_critical_path
from ..compute.models import CriticalPathRequest, FactProvider
from ..facts.callstacks import callstack_slices
from ..facts.scheduling import sched_slices, wakeup_edges
from ..facts.threads import thread_metadata, thread_state_segments


def fact_provider(kat) -> FactProvider:
    return FactProvider(
        thread_metadata=partial(thread_metadata, kat),
        thread_state_segments=partial(thread_state_segments, kat),
        wakeup_edges=partial(wakeup_edges, kat),
        sched_slices=partial(sched_slices, kat),
        callstack_slices=partial(callstack_slices, kat),
    )


@workflow(title="Critical path", description="Extract a critical path from a root thread and time window")
def critical_path(
    kat,
    root_itid: int,
    start_ts: int,
    end_ts: int,
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
):
    result = extract_critical_path(
        fact_provider(kat),
        CriticalPathRequest(root_itid, start_ts, end_ts, max_depth, min_segment_ms),
    )
    return {"path_nodes": result.nodes, "path_edges": result.edges}
```

- [ ] **Step 4: Replace the first-frame workflow**

It may collect only the single-row result of `first_frame_window`. It must not contain SQL or state decisions:

```python
@workflow(
    title="WeChat first-frame critical path",
    description="Select the first WeChat frame and run the generic critical-path compute",
)
def wechat_first_frame_critical_path(
    kat,
    app_name: str = ".tencent.wechat",
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
):
    targets = _rows(first_frame_window(kat, app_name))
    if not targets:
        result = target_not_found_result()
    else:
        target = targets[0]
        result = extract_critical_path(
            fact_provider(kat),
            CriticalPathRequest(
                target["root_itid"], target["start_ts"], target["end_ts"],
                max_depth, min_segment_ms,
            ),
        )
    return {"path_nodes": result.nodes, "path_edges": result.edges}
```

Define `_rows` without an algorithm loop because the fact is limited to one row:

```python
def _rows(dataframe) -> list[dict]:
    batches = dataframe.collect()
    return [] if not batches else batches[0].to_pylist()
```

Update `pack.py` to import and expose `extract_critical_path`, `critical_path` and `wechat_first_frame_critical_path`. Discovery ignores reexports and finds definitions in their own modules.

- [ ] **Step 5: Run all automated verification**

Run:

```powershell
python -m pytest python/tests/test_sdk_runtime_contract.py python/tests/test_openharmony_critical_path_pack.py -q
cargo test -p kat-rs-cli --test pack_run_contract
```

Expected: all tests PASS with zero failures.

- [ ] **Step 6: Run real `test/test.db` verification**

```powershell
cargo build -p kat-rs-cli
$tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
$dataset = [IO.Path]::GetFullPath((Join-Path $env:TEMP "kat-rs-critical-path-dataset"))
$genericRun = [IO.Path]::GetFullPath((Join-Path $env:TEMP "kat-rs-critical-path-generic"))
$wechatRun = [IO.Path]::GetFullPath((Join-Path $env:TEMP "kat-rs-critical-path-wechat"))
$targets = @($dataset, $genericRun, $wechatRun)
if (@($targets | Where-Object { -not $_.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0) {
    throw "refusing to clean outside temp root"
}
Remove-Item -Recurse -Force -LiteralPath $targets -ErrorAction SilentlyContinue
.\target\debug\kat-rs.exe dataset materialize sqlite test\test.db $dataset
.\target\debug\kat-rs.exe pack inspect packs\openharmony-critical-path --json
.\target\debug\kat-rs.exe pack run packs\openharmony-critical-path critical_path --dataset $dataset --run-dir $genericRun --param root_itid=405 --param start_ts=0 --param end_ts=1000000
.\target\debug\kat-rs.exe pack run packs\openharmony-critical-path wechat_first_frame_critical_path --dataset $dataset --run-dir $wechatRun
Get-Content -Raw -LiteralPath (Join-Path $genericRun "manifest.json")
Get-Content -Raw -LiteralPath (Join-Path $wechatRun "manifest.json")
```

Expected:

- inspect lists two workflows, six facts and one compute;
- both manifests have `status: success`;
- both runs contain readable `path_nodes.parquet` and `path_edges.parquet`;
- a follow-up DataFusion query joining wakeup edges to `instant` returns zero unmatched edges.

If the generic `0..1000000` window has no state for `itid=405`, obtain the actual first-frame `start_ts/end_ts` with `dataset query` and rerun the generic workflow with those exact values; record both the query and rerun in the PR verification evidence.

- [ ] **Step 7: Verify delivery scope and commit**

Run:

```powershell
git diff --check
git status --short
```

Confirm only the Pack files, focused Python tests, and this task’s optional Rust contract assertion are modified. Do not stage the pre-existing deleted design files, `docs/critical-path.strategy.md`, the two untracked 2026-07-09 specs, or `test/test.db`.

Commit:

```powershell
git add packs/openharmony-critical-path python/tests/test_openharmony_critical_path_pack.py crates/kat-rs-cli/tests/pack_run_contract.rs
git commit -m "feat: 交付通用关键路径示例 Pack"
```

If `crates/kat-rs-cli/tests/pack_run_contract.rs` was not changed, omit it from `git add`.

---

## Final Review Checklist

- [ ] `rg -n "kat\\.sql|while |for " packs/openharmony-critical-path/workflows` shows no SQL or algorithm loop in workflows.
- [ ] `rg -n "thread_state|instant|sched_slice|callstack|frame_slice|data_dict" packs/openharmony-critical-path/compute` returns no source table references.
- [ ] `rg -n "trace_streamer" packs/openharmony-critical-path` returns no matches.
- [ ] Every wakeup edge from both synthetic and real verification resolves to an `instant` row with the same `target_itid`, `waker_itid` and `wakeup_ts`.
- [ ] Missing facts, ambiguous waker, max depth, cycle and `udk-irq` remain successful domain results with explicit uncertainty/termination fields.
- [ ] `path_nodes` and `path_edges` schemas match SDD section 12 exactly.
- [ ] `python -m pytest python/tests/test_sdk_runtime_contract.py python/tests/test_openharmony_critical_path_pack.py -q` passes.
- [ ] `cargo test -p kat-rs-cli --test pack_run_contract` passes.
- [ ] `git diff --check` passes and unrelated working-tree changes remain untouched.
