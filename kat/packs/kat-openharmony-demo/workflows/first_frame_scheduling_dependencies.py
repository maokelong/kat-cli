import kat

from kat.pack.helpers.scheduling_dependencies import analyze_first_frame


@kat.workflow(
    name="first-frame-scheduling-dependencies",
    title="First-frame Scheduling Dependencies",
    required_tables=[
        "args",
        "data_dict",
        "frame_slice",
        "instant",
        "process",
        "thread",
        "thread_state",
    ],
    parameters={"process_name": "Exact process name to analyze."},
)
def first_frame_scheduling_dependencies(ctx: kat.Context, process_name: str):
    """Analyze observable scheduling dependencies for the earliest completed actual frame."""
    return {"scheduling_dependencies": analyze_first_frame(ctx, process_name)}
