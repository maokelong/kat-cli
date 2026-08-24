"""基于 `raw_smaps` 来源事实构造可复用的进程内存关系。"""

import kat


def summarize_process_memory(ctx: kat.Context):
    """按快照与 pathname 汇总可归因的 RSS/PSS，保留空 pathname。"""

    return ctx.sql(
        """
        SELECT
            arrow_cast(snapshot_id, 'UInt64') AS snapshot_id,
            arrow_cast(pathname, 'Utf8') AS pathname,
            COALESCE(
                arrow_cast(SUM(CAST(rss_kib AS DECIMAL(38, 0))), 'UInt64'),
                arrow_cast(0, 'UInt64')
            ) AS rss_kib,
            COALESCE(
                arrow_cast(SUM(CAST(pss_kib AS DECIMAL(38, 0))), 'UInt64'),
                arrow_cast(0, 'UInt64')
            ) AS pss_kib
        FROM raw_smaps.mappings
        GROUP BY snapshot_id, pathname
        ORDER BY snapshot_id ASC, pss_kib DESC, rss_kib DESC, pathname ASC
        """
    )
