from kat import workflow

from compute.critical_path import extract_critical_path, target_not_found_result
from compute.models import CriticalPathRequest
from facts.frames import first_frame_window
from workflows.critical_path import fact_provider


def _rows(dataframe) -> list[dict]:
    batches = dataframe.collect()
    return [] if not batches else batches[0].to_pylist()


@workflow(
    title="WeChat first-frame critical path",
    description="Find the first WeChat window frame and extract its conservative critical path",
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
