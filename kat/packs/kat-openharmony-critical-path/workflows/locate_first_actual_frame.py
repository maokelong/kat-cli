import kat

from kat.pack.helpers.critical_path import locate_first_actual_frame


@kat.workflow(
    name="locate-first-actual-frame",
    description="定位指定进程最早完成且持续时间为正的实际帧。",
    parameters={"process_name": "Exact process name to locate."},
)
def locate_first_actual_frame_workflow(ctx: kat.Context, process_name: str):
    """定位指定进程最早完成且持续时间为正的实际帧。"""
    return {"frame_window": locate_first_actual_frame(ctx, process_name)}
