from __future__ import annotations

from pathlib import Path
import shutil
import tempfile

import kat

from kat.pack.datasources.hitrace import HitraceProvider


@kat.workflow(
    name="summarize-hitrace-clock",
    description="Summarize Hitrace clock relations.",
    parameters={"trace_path": "Path to the Hitrace input."},
)
def summarize_hitrace_clock(ctx: kat.Context, trace_path: str):
    """Decode Hitrace, query its Parquet relations, and publish one Run Output."""
    workspace = Path(
        tempfile.mkdtemp(prefix="payload-smoke-", dir=ctx.datasource_root)
    )
    try:
        provider = HitraceProvider(Path(trace_path))
        provider.decode(workspace / "relations")
        return provider.query(
            """
            SELECT clock_domain, clock_value
            FROM clock_snapshot
            WHERE snapshot_id = 0 AND clock_domain = 'boottime'
            """
        )
    finally:
        shutil.rmtree(workspace, ignore_errors=True)
