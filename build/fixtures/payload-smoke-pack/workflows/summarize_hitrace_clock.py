from __future__ import annotations

from pathlib import Path

import kat

from kat.pack.datasources.hitrace import HitraceProvider


@kat.workflow(
    name="summarize-hitrace-clock",
    description="Summarize Hitrace clock relations.",
    parameters={"trace_path": "Path to the Hitrace input."},
)
def summarize_hitrace_clock(ctx: kat.Context, trace_path: str):
    """Decode Hitrace, query its Parquet relations, and publish one Run Output."""
    provider = HitraceProvider(
        source=Path(trace_path),
        datasource_root=ctx.datasource_root,
    ).prepare()
    return provider.query(
        """
        SELECT clock_domain, clock_value
        FROM clock_snapshot
        WHERE snapshot_id = 0 AND clock_domain = 'boottime'
        """
    )
