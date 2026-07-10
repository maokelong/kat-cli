from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import sysconfig
from pathlib import Path

import datafusion
import kat
import kat_runtime
import pyarrow as pa
from datafusion import SessionContext


EXPECTED_REAL_SHA256 = (
    "5f742a759c57bb05fe010e44a1f03aa042e4b7cf6ee53769fa55f7cfd6fe8829"
)
NODE_COLUMNS = [
    "node_id", "depth", "itid", "tid", "thread_name", "pid", "process_name",
    "window_start_ts", "window_end_ts", "segment_start_ts", "segment_end_ts",
    "dur", "state", "classification", "sched_cpu", "sched_priority",
    "callstack_name", "blocked_caller", "blocking_context_node_id",
    "inherited_blocked_caller", "confidence", "uncertainty",
    "termination_reason",
]
EDGE_COLUMNS = [
    "edge_id", "from_node_id", "to_node_id", "from_itid", "to_itid",
    "parent_depth", "child_depth", "wakeup_ts", "edge_type", "confidence",
    "reason",
]
NODE_INTEGER_COLUMNS = {
    "node_id", "depth", "itid", "tid", "pid", "window_start_ts",
    "window_end_ts", "segment_start_ts", "segment_end_ts", "dur",
    "sched_cpu", "sched_priority", "blocking_context_node_id",
}
EDGE_INTEGER_COLUMNS = {
    "edge_id", "from_node_id", "to_node_id", "from_itid", "to_itid",
    "parent_depth", "child_depth", "wakeup_ts",
}
REQUIRED_TABLES = {
    "thread_state", "thread", "process", "args", "data_dict", "instant",
    "sched_slice", "callstack", "frame_slice",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def scalar(ctx: SessionContext, sql: str) -> int:
    columns = ctx.sql(sql).to_pydict()
    assert len(columns) == 1, columns
    return next(iter(columns.values()))[0]


def assert_site_package(module) -> None:
    purelib = Path(sysconfig.get_paths()["purelib"]).resolve()
    module_path = Path(module.__file__).resolve()
    assert module_path.is_relative_to(purelib), (module.__name__, module_path, purelib)


def assert_artifact_schema(schema, columns: list[str], integer_columns: set[str]) -> None:
    assert schema.names == columns
    for name in columns:
        field = schema.field(name)
        assert field.nullable
        if name in integer_columns:
            assert pa.types.is_int64(field.type), (name, field.type)
        else:
            assert (
                pa.types.is_string(field.type)
                or pa.types.is_large_string(field.type)
                or pa.types.is_string_view(field.type)
            ), (name, field.type)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["synthetic", "real"], required=True)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    args = parser.parse_args()

    assert datafusion.__version__ == "54.0.0"
    assert importlib.metadata.version("kat-python-sdk") == "0.1.0"
    assert importlib.metadata.version("kat-python-runtime") == "0.1.0"
    assert_site_package(kat)
    assert_site_package(kat_runtime)
    if args.profile == "real":
        assert args.db.stat().st_size == 61_009_920
        assert sha256(args.db) == EXPECTED_REAL_SHA256

    catalog = json.loads((args.dataset / "catalog.json").read_text(encoding="utf-8"))
    tables = {item["name"]: item for item in catalog["tables"]}
    assert REQUIRED_TABLES <= tables.keys()
    manifest = json.loads((args.run_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["status"] == "success"
    artifacts = {item["name"]: item for item in manifest["artifacts"]}
    assert artifacts.keys() == {"path_nodes", "path_edges"}

    ctx = SessionContext()
    ctx.register_parquet(
        "path_nodes", str(args.run_dir / artifacts["path_nodes"]["path"])
    )
    ctx.register_parquet(
        "path_edges", str(args.run_dir / artifacts["path_edges"]["path"])
    )
    ctx.register_parquet(
        "instant", str(args.dataset / tables["instant"]["path"])
    )
    assert_artifact_schema(
        ctx.table("path_nodes").schema(), NODE_COLUMNS, NODE_INTEGER_COLUMNS
    )
    assert_artifact_schema(
        ctx.table("path_edges").schema(), EDGE_COLUMNS, EDGE_INTEGER_COLUMNS
    )

    expected = {
        "synthetic": {
            "nodes": 3,
            "edges": 2,
            "wakeup": 1,
            "sequence": 1,
            "itid": 1,
            "tid": 10,
            "pid": 1000,
            "thread_name": "main",
            "start": 0,
            "end": 500000,
        },
        "real": {
            "nodes": 333,
            "edges": 331,
            "wakeup": 61,
            "sequence": 270,
            "itid": 405,
            "tid": 15040,
            "pid": 15040,
            "thread_name": ".tencent.wechat",
            "start": 246306873000,
            "end": 246332420000,
        },
    }[args.profile]

    node_count = scalar(ctx, "select count(*) as value from path_nodes")
    distinct_nodes = scalar(
        ctx, "select count(distinct node_id) as value from path_nodes"
    )
    exact_target = scalar(
        ctx,
        f"""
        select count(*) as value from (
          select distinct depth, itid, tid, pid, thread_name, process_name,
                          window_start_ts, window_end_ts
          from path_nodes
          where depth = 0
            and itid = {expected['itid']}
            and tid = {expected['tid']}
            and pid = {expected['pid']}
            and thread_name = '{expected['thread_name']}'
            and process_name = '.tencent.wechat'
            and window_start_ts = {expected['start']}
            and window_end_ts = {expected['end']}
        ) exact_target
        """,
    )
    bad_uncertainty = scalar(
        ctx,
        """
        select count(*) as value
        from path_nodes
        where termination_reason is not null and uncertainty is null
        """,
    )
    assert node_count == expected["nodes"] == distinct_nodes
    assert exact_target == 1
    assert bad_uncertainty == 0

    edge_count = scalar(ctx, "select count(*) as value from path_edges")
    distinct_edges = scalar(
        ctx, "select count(distinct edge_id) as value from path_edges"
    )
    wakeup_count = scalar(
        ctx, "select count(*) as value from path_edges where edge_type = 'wakeup'"
    )
    sequence_count = scalar(
        ctx, "select count(*) as value from path_edges where edge_type = 'sequence'"
    )
    invalid_edges = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges
        where edge_type is null
           or edge_type not in ('wakeup', 'sequence')
           or confidence is null
           or confidence <> 'fact'
           or (edge_type = 'wakeup' and (
                 wakeup_ts is null
                 or reason is null
                 or reason <> 'sched_wakeup'
                 or parent_depth is null
                 or child_depth is null
                 or child_depth <> parent_depth + 1
              ))
           or (edge_type = 'sequence' and (
                 wakeup_ts is not null
                 or reason is null
                 or reason <> 'thread_state_order'
                 or from_itid is null
                 or to_itid is null
                 or from_itid <> to_itid
                 or parent_depth is null
                 or child_depth is null
                 or parent_depth <> child_depth
              ))
        """,
    )
    bad_node_references = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges e
        left join path_nodes source on source.node_id = e.from_node_id
        left join path_nodes target on target.node_id = e.to_node_id
        where source.node_id is null
           or target.node_id is null
           or e.from_itid is null
           or e.to_itid is null
           or e.from_itid <> source.itid
           or e.to_itid <> target.itid
        """,
    )
    unmatched_wakeups = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges e
        where e.edge_type = 'wakeup'
          and not exists (
            select 1
            from instant i
            where i.ref_type = 'itid'
              and i.name like 'sched_wakeup%'
              and i.wakeup_from = e.from_itid
              and i.ref = e.to_itid
              and i.ts = e.wakeup_ts
          )
        """,
    )
    assert edge_count == expected["edges"] == distinct_edges
    assert wakeup_count == expected["wakeup"]
    assert sequence_count == expected["sequence"]
    assert invalid_edges == 0
    assert bad_node_references == 0
    assert unmatched_wakeups == 0

    print(
        json.dumps(
            {
                "profile": args.profile,
                "datafusion": datafusion.__version__,
                "kat": kat.__file__,
                "kat_runtime": kat_runtime.__file__,
                "nodes": node_count,
                "edges": edge_count,
                "wakeup": wakeup_count,
            },
            ensure_ascii=False,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
