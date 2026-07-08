from __future__ import annotations

IO_THREAD_NAMES = {
    "fsverity",
    "cdecrypt",
    "erofs_unzipd",
    "fsignature",
    "hmfs",
    "wk:0/0/0",
    "wk:2/1/0",
    "wk:0/-20/0",
}

IO_THREAD_EXCLUDES = {"hmfs_txn"}


def normalized_state(state: str) -> str:
    value = (state or "").strip().lower()
    if value == "running":
        return "running"
    if value == "runnable":
        return "runnable"
    if value in {"d", "uninterruptible", "uninterruptible/d", "d-io", "io_wait"}:
        return "blocked"
    if "io_wait" in value or "d-io" in value:
        return "blocked"
    if "sleep" in value:
        return "sleeping"
    if value:
        return value
    return "unknown"


def is_irq_thread(thread_name: str) -> bool:
    return (thread_name or "").strip().lower().startswith("udk-irq")


def is_io_thread(thread_name: str) -> bool:
    name = (thread_name or "").strip().lower()
    if name in IO_THREAD_EXCLUDES:
        return False
    return name in IO_THREAD_NAMES


def blocked_context(blocked_function: str, final_blocked_caller: str) -> str:
    if blocked_function:
        return blocked_function
    return final_blocked_caller


def classify_state(
    state: str,
    blocked_function: str = "",
    final_blocked_caller: str = "",
    io_wait: bool = False,
) -> tuple[str, str]:
    kind = normalized_state(state)
    context = blocked_context(blocked_function, final_blocked_caller)
    if kind == "running":
        return "self_execution", "thread was running on CPU"
    if kind == "runnable":
        return "scheduler_wait", "thread was runnable but not executing"
    if kind == "blocked" and (io_wait or "io" in (context or "").lower()):
        return "io_block", "thread was blocked in an IO-related state"
    if kind == "blocked" and context:
        return "non_io_block", "thread was blocked with a kernel/blocking caller"
    if kind in {"sleeping", "blocked"}:
        return "waiting_for_wakeup", "thread was waiting for an external event"
    return "uncertain", "thread state is missing or not recognized"
